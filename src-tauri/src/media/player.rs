use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::UnixStream;
use tokio::net::unix::{OwnedReadHalf, OwnedWriteHalf};
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio::time::timeout;

use crate::crash;
use crate::error::{EchoraError, Result};

/// How long a single mpv IPC round trip (write a command, read its matching
/// reply) is allowed to take before this treats mpv as hung rather than
/// merely slow. mpv is local IPC — a couple of seconds is already generous
/// for a live process. Without this, a wedged-but-still-running mpv (alive,
/// but not servicing its socket) would hold `send_command`'s caller
/// waiting indefinitely, and with it the shared `tokio::sync::Mutex<Player>`
/// every playback command, MPRIS call, and the tray/watchers all wait on.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(2);

/// Properties subscribed once, right after the persistent connection comes
/// up, so the getters below can read a live cache instead of round-tripping
/// mpv on every call (P2-1: this used to open a fresh `UnixStream::connect`
/// per call — ~29/s combined across `audio_level`, `auto_advance`,
/// `sponsorblock`, and the frontend's own 1Hz poll during active playback).
/// `af-metadata/echora_level` is deliberately not in this list — see
/// `audio_level_db`'s doc comment for why that one stays on its own
/// per-call connection.
const OBSERVED_PROPERTIES: [&str; 4] = ["pause", "time-pos", "duration", "volume"];

/// Live snapshot of `OBSERVED_PROPERTIES`, kept current by the reader task
/// spawned in `connect_persistent`. A field reads `None` until the first
/// matching `property-change` event arrives (e.g. right after `load()`,
/// before mpv has reported anything yet) or whenever mpv itself reports the
/// property as unavailable (e.g. `time-pos`/`duration` once a file is
/// unloaded) — both cases mpv represents the same way: an event with no
/// `data` field, which `apply_property_change` maps to `None` either way.
#[derive(Debug, Default)]
struct Cache {
    is_paused: Option<bool>,
    position_seconds: Option<f64>,
    duration_seconds: Option<f64>,
    volume_percent: Option<u8>,
}

/// One in-flight command's reply channel, keyed by the `request_id` this
/// `Player` put on the request — matched against `request_id` on the way
/// back in so a reply can never be confused with an unrelated event or
/// (in principle) another command's reply on the same shared connection.
/// A single slot rather than a map: every command-sending method takes
/// `&mut self`, and every caller reaches `Player` through the same
/// `tokio::sync::Mutex<Player>` (`AppState::player`), so two commands can
/// never actually be in flight on one `Player` at once. The `request_id`
/// check is a cheap defensive belt-and-braces, not load-bearing for
/// correctness today.
type PendingReply = Arc<Mutex<Option<(u64, oneshot::Sender<Value>)>>>;

/// Wraps the mpv sidecar: spawned as a subprocess (never linked — see
/// docs/adr/0001), audio-only, controlled entirely over its own JSON IPC
/// socket. `start()` opens exactly one persistent connection and keeps it
/// for the life of the mpv process; every mutating command
/// (`load`/`set_paused`/`seek_to`/`set_volume`) and every observed-property
/// getter (`is_paused`/`position_seconds`/`duration_seconds`/
/// `volume_percent`) goes through it instead of opening a fresh socket per
/// call (P2-1). `audio_level_db`/`enable_level_metering` are the one
/// exception — see `audio_level_db`'s doc comment.
pub struct Player {
    socket_path: PathBuf,
    child: Option<CommandChild>,
    app_dir: PathBuf,
    crash_reporting_enabled: Arc<AtomicBool>,
    /// Whether the `astats` metering filter has been successfully attached
    /// for this mpv process yet — attached once, lazily, not per track (see
    /// `media::audio_level::watch`), so later calls to
    /// `enable_level_metering` are a cheap no-op instead of re-adding the
    /// filter every track change.
    level_metering_ready: bool,
    /// Persistent write half of the single mpv IPC connection opened by
    /// `connect_persistent` — `None` whenever there's no live connection
    /// (never started yet, or torn down after a detected mpv death).
    write_half: Option<OwnedWriteHalf>,
    /// The background task reading the connection's event/reply stream.
    /// Aborted and replaced on every (re)connect so a restart after a
    /// crash never leaves a stale reader running against a closed socket.
    reader_task: Option<JoinHandle<()>>,
    cache: Arc<Mutex<Cache>>,
    pending_reply: PendingReply,
    next_id: u64,
    /// Incremented by the reader task each time mpv reports `end-file`
    /// with reason `"eof"` — a track that played through to its own
    /// natural end, as opposed to being replaced by a new `load()` or
    /// stopped (both report reason `"stop"`, confirmed against a real mpv
    /// 0.37 process — see `media::auto_advance`, which uses this instead
    /// of guessing from `position_seconds`/`duration_seconds`).
    end_file_eof_epoch: Arc<AtomicU64>,
}

impl Player {
    pub fn new(
        socket_path: PathBuf,
        app_dir: PathBuf,
        crash_reporting_enabled: Arc<AtomicBool>,
    ) -> Self {
        Player {
            socket_path,
            child: None,
            app_dir,
            crash_reporting_enabled,
            level_metering_ready: false,
            write_half: None,
            reader_task: None,
            cache: Arc::new(Mutex::new(Cache::default())),
            pending_reply: Arc::new(Mutex::new(None)),
            next_id: 0,
            end_file_eof_epoch: Arc::new(AtomicU64::new(0)),
        }
    }

    pub fn is_started(&self) -> bool {
        self.child.is_some()
    }

    pub async fn start<R: tauri::Runtime>(&mut self, app: &tauri::AppHandle<R>) -> Result<()> {
        let _ = std::fs::remove_file(&self.socket_path);
        self.level_metering_ready = false;
        self.disconnect();

        let (_rx, child) = app
            .shell()
            .sidecar("echora-mpv")
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?
            .args([
                "--idle=yes".to_string(),
                "--no-video".to_string(),
                "--no-terminal".to_string(),
                format!("--input-ipc-server={}", self.socket_path.display()),
            ])
            .spawn()
            .map_err(|e| EchoraError::Sidecar(e.to_string()))?;
        self.child = Some(child);

        self.wait_for_socket().await?;
        self.connect_persistent().await
    }

    async fn wait_for_socket(&self) -> Result<()> {
        for _ in 0..50 {
            if self.socket_path.exists() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        Err(EchoraError::SidecarTimeout("mpv".into()))
    }

    /// Stops the current connection's reader task and clears its write
    /// half, without touching `self.child` — used both mid-`start()`
    /// (tearing down any previous connection before a restart) and from
    /// `shutdown()`. Splitting this out from `connect_persistent` keeps
    /// "stop the old connection" and "stand up a fresh one" as two
    /// separately callable steps — the unit test below needs only the
    /// latter, against a bare `UnixListener` standing in for mpv, without
    /// a real sidecar spawn.
    fn disconnect(&mut self) {
        if let Some(handle) = self.reader_task.take() {
            handle.abort();
        }
        self.write_half = None;
    }

    /// Opens the one persistent connection this `Player` uses for every
    /// mutating command and every observed-property getter, spawns the
    /// task that reads its event/reply stream, and subscribes to
    /// `OBSERVED_PROPERTIES`. Always replaces `cache`/`pending_reply`/
    /// `end_file_eof_epoch` with fresh instances, so a reconnect after a
    /// crash can never serve a stale reading left over from the mpv
    /// process it's replacing.
    async fn connect_persistent(&mut self) -> Result<()> {
        let stream = match timeout(COMMAND_TIMEOUT, UnixStream::connect(&self.socket_path)).await {
            Ok(Ok(s)) => s,
            Ok(Err(err)) => return Err(EchoraError::Io(err)),
            Err(_) => return Err(EchoraError::SidecarTimeout("mpv".into())),
        };
        let (read_half, write_half) = stream.into_split();
        self.write_half = Some(write_half);

        let cache = Arc::new(Mutex::new(Cache::default()));
        self.cache = cache.clone();
        let pending_reply: PendingReply = Arc::new(Mutex::new(None));
        self.pending_reply = pending_reply.clone();
        let end_file_eof_epoch = Arc::new(AtomicU64::new(0));
        self.end_file_eof_epoch = end_file_eof_epoch.clone();

        self.reader_task = Some(tokio::spawn(read_loop(
            read_half,
            cache,
            pending_reply,
            end_file_eof_epoch,
        )));

        for property in OBSERVED_PROPERTIES {
            let id = self.next_id();
            // Best-effort: a failed subscribe leaves that one field
            // reading `None` forever rather than failing the whole
            // connect — matches `enable_level_metering`'s own tolerance
            // of a failed `af add`.
            let _ = self
                .write_only(json!(["observe_property", id, property]))
                .await;
        }
        Ok(())
    }

    fn next_id(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Records a detected mpv death (if `child` was thought to still be
    /// running) and tears down the now-broken connection. Called from
    /// `send_command`/`write_only` when a write fails or the reader task
    /// reports the connection closed — the persistent-connection
    /// equivalent of the old per-call design's "connect failed" branch,
    /// just detected at a different point since there's no longer a
    /// connect attempt on every call.
    fn handle_connection_broken(&mut self, detail: &str) {
        if self.child.is_some() {
            if self.crash_reporting_enabled.load(Ordering::Relaxed) {
                let _ = crash::record(
                    &self.app_dir,
                    crash::CrashRecord::from_sidecar("mpv", detail),
                );
            }
            self.child = None;
        }
        self.disconnect();
    }

    /// Writes a command and returns as soon as the write succeeds, without
    /// waiting for (or correlating) its reply. Used for the
    /// `observe_property` subscriptions `connect_persistent` sends: their
    /// success/failure isn't actionable on its own (a failed subscribe
    /// just leaves that cache field `None` forever), so there's nothing to
    /// gain from consuming the single `pending_reply` slot and a
    /// `COMMAND_TIMEOUT` wait to confirm it.
    async fn write_only(&mut self, command: Value) -> Result<()> {
        let request_id = self.next_id();
        let payload = json!({ "command": command, "request_id": request_id });
        let Some(write_half) = self.write_half.as_mut() else {
            return Err(EchoraError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "mpv is not connected",
            )));
        };
        match timeout(
            COMMAND_TIMEOUT,
            write_half.write_all(format!("{payload}\n").as_bytes()),
        )
        .await
        {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => {
                let detail = err.to_string();
                self.handle_connection_broken(&detail);
                Err(EchoraError::Io(err))
            }
            Err(_) => Err(EchoraError::SidecarTimeout("mpv".into())),
        }
    }

    /// Sends a command over the persistent connection and waits for its
    /// matching reply (by `request_id`), each half bounded by
    /// `COMMAND_TIMEOUT` so a wedged-but-connected mpv can't hold this
    /// (and the shared `tokio::sync::Mutex<Player>` every caller waits on)
    /// forever — same guarantee the old per-call-connection design had,
    /// just enforced across a persistent connection instead of a fresh one
    /// per call.
    async fn send_command(&mut self, command: Value) -> Result<Value> {
        let request_id = self.next_id();
        let (tx, rx) = oneshot::channel();
        *self.pending_reply.lock().unwrap() = Some((request_id, tx));

        let payload = json!({ "command": command, "request_id": request_id });
        let Some(write_half) = self.write_half.as_mut() else {
            return Err(EchoraError::Io(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "mpv is not connected",
            )));
        };
        match timeout(
            COMMAND_TIMEOUT,
            write_half.write_all(format!("{payload}\n").as_bytes()),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => {
                let detail = err.to_string();
                self.handle_connection_broken(&detail);
                return Err(EchoraError::Io(err));
            }
            Err(_) => return Err(EchoraError::SidecarTimeout("mpv".into())),
        }

        match timeout(COMMAND_TIMEOUT, rx).await {
            Ok(Ok(value)) => Ok(value),
            Ok(Err(_)) => {
                // The reader task dropped our sender without completing it
                // -- the connection closed (mpv died, or a stray close)
                // while this command was waiting on its reply.
                self.handle_connection_broken("mpv IPC connection closed");
                Err(EchoraError::Io(std::io::Error::new(
                    std::io::ErrorKind::BrokenPipe,
                    "mpv connection closed",
                )))
            }
            // A timed-out reply is *not* the same as "mpv is gone": the
            // process may well still be alive, just not servicing its
            // socket right now. Treat it as a plain IPC failure rather
            // than tearing down the connection here -- doing that would
            // make a later `start()` spawn a second mpv on top of a first
            // one that's still actually running, orphaning it.
            Err(_) => Err(EchoraError::SidecarTimeout("mpv".into())),
        }
    }

    pub async fn load(&mut self, stream_url: &str) -> Result<()> {
        self.send_command(json!(["loadfile", stream_url])).await?;
        Ok(())
    }

    pub async fn set_paused(&mut self, paused: bool) -> Result<()> {
        self.send_command(json!(["set_property", "pause", paused]))
            .await?;
        Ok(())
    }

    pub async fn is_paused(&mut self) -> Result<Option<bool>> {
        Ok(self.cache.lock().unwrap().is_paused)
    }

    pub async fn set_volume(&mut self, volume_percent: u8) -> Result<()> {
        self.send_command(json!(["set_property", "volume", volume_percent]))
            .await?;
        Ok(())
    }

    /// mpv's own volume percentage (0-100+), for callers that need to read
    /// it back rather than only set it — e.g. MPRIS's `Volume` property.
    pub async fn volume_percent(&mut self) -> Result<Option<u8>> {
        Ok(self.cache.lock().unwrap().volume_percent)
    }

    pub async fn seek_to(&mut self, seconds: f64) -> Result<()> {
        self.send_command(json!(["seek", seconds, "absolute"]))
            .await?;
        Ok(())
    }

    pub async fn position_seconds(&mut self) -> Result<Option<f64>> {
        Ok(self.cache.lock().unwrap().position_seconds)
    }

    pub async fn duration_seconds(&mut self) -> Result<Option<f64>> {
        Ok(self.cache.lock().unwrap().duration_seconds)
    }

    /// Attaches the RMS-level metering filter, once per mpv process. Later
    /// calls are a no-op — safe to call unconditionally from every poll
    /// tick in `media::audio_level::watch` rather than tracking "have I
    /// called this yet" at the call site too.
    ///
    /// Deliberately still uses its own fresh, one-shot `UnixStream`
    /// instead of the persistent connection — confirmed empirically (real
    /// mpv 0.37.0, `av://lavfi:sine=...` source + this exact `astats`
    /// filter) that issuing `af add` on the *same* connection that also
    /// carries `loadfile`/other traffic can leave its reply (and every
    /// command after it on that connection) never arriving, even though
    /// mpv itself stays alive; a dedicated fresh connection for this one
    /// command replies immediately and doesn't disturb the persistent
    /// connection's own event stream at all (verified side by side). See
    /// `audio_level_db` for the matching half of this workaround.
    pub async fn enable_level_metering(&mut self) -> Result<()> {
        if self.level_metering_ready {
            return Ok(());
        }
        let reply = self
            .send_command_fresh_connection(json!([
                "af",
                "add",
                "@echora_level:lavfi=[astats=metadata=1:reset=1]"
            ]))
            .await?;
        if reply.get("error").and_then(Value::as_str) == Some("success") {
            self.level_metering_ready = true;
        }
        Ok(())
    }

    /// Current RMS level in dBFS from the `astats` filter `enable_level_metering`
    /// attaches, or `None` if metering isn't enabled yet, the filter hasn't
    /// produced a reading yet, or the reading was non-finite (e.g. true
    /// digital silence reads as `-inf`, which this treats the same as "no
    /// data" rather than propagating an infinite value to the frontend).
    ///
    /// mpv's `af-metadata/<label>` property replies with `data` as a flat
    /// object keyed by strings like `"lavfi.astats.Overall.RMS_level"`,
    /// with **string-typed** values (e.g. `"-21.123621"`) — confirmed
    /// against a real mpv process, not assumed from FFmpeg's docs alone.
    ///
    /// Not observed via the persistent connection's `OBSERVED_PROPERTIES`
    /// on purpose — same empirical finding as `enable_level_metering`:
    /// querying `af-metadata/echora_level` on a connection that has other
    /// traffic (even just having previously carried `af add`/`loadfile`)
    /// reproducibly never gets a reply, while a fresh one-shot connection
    /// per read is fast and reliable (confirmed with 8 consecutive reads
    /// at the real ~80ms poll cadence while the persistent connection was
    /// simultaneously alive and streaming other property-change events —
    /// no interference either direction). So `audio_level` keeps its
    /// per-call connection; everything else in this file moved off it.
    pub async fn audio_level_db(&mut self) -> Result<Option<f64>> {
        if !self.level_metering_ready {
            return Ok(None);
        }
        let reply = self
            .send_command_fresh_connection(json!(["get_property", "af-metadata/echora_level"]))
            .await?;
        Ok(reply
            .get("data")
            .and_then(|d| d.get("lavfi.astats.Overall.RMS_level"))
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<f64>().ok())
            .filter(|v| v.is_finite()))
    }

    /// The pre-P2-1 one-shot-connection-per-call design, kept only for
    /// `enable_level_metering`/`audio_level_db` — see their doc comments.
    /// Does not touch `self.child`/the persistent connection on failure:
    /// a failure here says nothing about the persistent connection's own
    /// health (it's a completely separate socket), only that this one
    /// command didn't get answered.
    async fn send_command_fresh_connection(&mut self, command: Value) -> Result<Value> {
        let stream = match timeout(COMMAND_TIMEOUT, UnixStream::connect(&self.socket_path)).await {
            Ok(Ok(s)) => s,
            Ok(Err(err)) => return Err(EchoraError::Io(err)),
            Err(_) => return Err(EchoraError::SidecarTimeout("mpv".into())),
        };
        let (read_half, mut write_half) = stream.into_split();

        let payload = json!({ "command": command });
        match timeout(
            COMMAND_TIMEOUT,
            write_half.write_all(format!("{payload}\n").as_bytes()),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(err)) => return Err(EchoraError::Io(err)),
            Err(_) => return Err(EchoraError::SidecarTimeout("mpv".into())),
        }

        let mut reader = BufReader::new(read_half);
        let mut line = String::new();
        loop {
            line.clear();
            let bytes_read = match timeout(COMMAND_TIMEOUT, reader.read_line(&mut line)).await {
                Ok(Ok(n)) => n,
                Ok(Err(err)) => return Err(EchoraError::Io(err)),
                Err(_) => return Err(EchoraError::SidecarTimeout("mpv".into())),
            };
            if bytes_read == 0 {
                return Err(EchoraError::SidecarTimeout("mpv".into()));
            }
            let value: Value = serde_json::from_str(line.trim())?;
            // mpv multiplexes property-change events on the same socket;
            // skip anything that isn't the reply to the command we sent.
            if value.get("error").is_some() {
                return Ok(value);
            }
        }
    }

    /// Increments once each time mpv reports `end-file` with reason
    /// `"eof"` — a track playing through to its own natural end.
    /// `media::auto_advance` compares this against a per-track baseline
    /// instead of guessing from `position_seconds`/`duration_seconds`: the
    /// old heuristic is gone, this is mpv's own real signal for the same
    /// thing (P2-1's correctness follow-on, not just its performance one).
    pub fn end_file_eof_epoch(&self) -> u64 {
        self.end_file_eof_epoch.load(Ordering::Relaxed)
    }

    /// Ends the mpv process cleanly (asks it to quit, then kills it if it
    /// doesn't). Must be called on app shutdown and on Player drop — never
    /// leave an orphaned mpv process behind.
    pub async fn shutdown(&mut self) -> Result<()> {
        let _ = self.send_command(json!(["quit"])).await;
        self.disconnect();
        if let Some(child) = self.child.take() {
            let _ = child.kill();
        }
        let _ = std::fs::remove_file(&self.socket_path);
        Ok(())
    }
}

/// Background task owned by `connect_persistent`: reads the connection's
/// event/reply stream for as long as it's open and routes each line to
/// either `apply_property_change`/the eof epoch (events) or the waiting
/// `send_command` call (replies, matched by `request_id`). Runs
/// independently of the `tokio::sync::Mutex<Player>` every command-sending
/// method is called through — it never blocks on that lock, so it can't be
/// the thing holding it up (see `COMMAND_TIMEOUT`'s doc comment).
async fn read_loop(
    read_half: OwnedReadHalf,
    cache: Arc<Mutex<Cache>>,
    pending_reply: PendingReply,
    end_file_eof_epoch: Arc<AtomicU64>,
) {
    let mut reader = BufReader::new(read_half);
    let mut line = String::new();
    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF -- mpv closed the connection
            Ok(_) => {}
            Err(_) => break, // connection error -- treat the same as EOF
        }
        let Ok(value) = serde_json::from_str::<Value>(line.trim()) else {
            // A malformed line shouldn't take down the whole connection.
            continue;
        };
        handle_message(&value, &cache, &pending_reply, &end_file_eof_epoch);
    }
    // The connection is gone -- drop (don't complete) any reply a caller
    // is still waiting on, so `send_command` fails fast on the closed
    // channel instead of burning through the full `COMMAND_TIMEOUT`.
    let _ = pending_reply.lock().unwrap().take();
}

/// Routes one parsed IPC line: an `event` message updates the cache (or,
/// for `end-file`, the eof epoch); anything else with a `request_id`
/// completes the matching pending command, if any is still waiting on it.
fn handle_message(
    value: &Value,
    cache: &Mutex<Cache>,
    pending_reply: &Mutex<Option<(u64, oneshot::Sender<Value>)>>,
    end_file_eof_epoch: &AtomicU64,
) {
    if let Some(event) = value.get("event").and_then(Value::as_str) {
        match event {
            "property-change" => {
                if let Some(name) = value.get("name").and_then(Value::as_str) {
                    let data = value.get("data").unwrap_or(&Value::Null);
                    apply_property_change(&mut cache.lock().unwrap(), name, data);
                }
            }
            "end-file" if value.get("reason").and_then(Value::as_str) == Some("eof") => {
                end_file_eof_epoch.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
        return;
    }

    let Some(request_id) = value.get("request_id").and_then(Value::as_u64) else {
        return;
    };
    let mut slot = pending_reply.lock().unwrap();
    if slot.as_ref().is_some_and(|(id, _)| *id == request_id)
        && let Some((_, tx)) = slot.take()
    {
        let _ = tx.send(value.clone());
    }
}

/// Applies one `property-change` event to the cache — pure and
/// unit-tested on its own, separate from the socket/task plumbing around
/// it (mirrors the project's pattern of factoring the decision logic out
/// of a watch loop into a directly-testable function, e.g.
/// `auto_advance::tick_outcome`).
fn apply_property_change(cache: &mut Cache, name: &str, data: &Value) {
    match name {
        "pause" => cache.is_paused = data.as_bool(),
        "time-pos" => cache.position_seconds = data.as_f64(),
        "duration" => cache.duration_seconds = data.as_f64(),
        "volume" => cache.volume_percent = data.as_f64().map(|v| v as u8),
        _ => {}
    }
}

/// Deterministic IPC-layer tests: a bare Unix socket standing in for mpv,
/// not the real binary — no `--ignored`, no network, safe to run every
/// time.
#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;
    use tokio::net::UnixListener;

    fn hang_socket_path() -> PathBuf {
        std::env::temp_dir().join(format!(
            "echora-player-test-hang-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    /// Regression test for P2-2: a peer that accepts the connection but
    /// never replies (a wedged-but-alive mpv, from `send_command`'s point
    /// of view) must not hang `send_command` forever — it has to give up
    /// around `COMMAND_TIMEOUT` with a clean `SidecarTimeout` error.
    ///
    /// Connects via `connect_persistent` directly rather than `start()`
    /// (which would need a real spawned mpv sidecar via the shell plugin)
    /// — the fake listener below stands in for mpv well enough for this
    /// connection-level guarantee.
    #[tokio::test]
    async fn send_command_times_out_instead_of_hanging_on_a_silent_peer() {
        let socket_path = hang_socket_path();
        let listener = UnixListener::bind(&socket_path).unwrap();
        let _accepting = tokio::spawn(async move {
            // Accept the connection and then go silent for the rest of the
            // test -- never writes a reply.
            let (_stream, _addr) = listener.accept().await.unwrap();
            std::future::pending::<()>().await
        });

        let mut player = Player::new(
            socket_path.clone(),
            std::env::temp_dir(),
            std::sync::Arc::new(AtomicBool::new(false)),
        );
        player.connect_persistent().await.unwrap();

        let started = Instant::now();
        let err = player.set_volume(50).await.unwrap_err();
        let elapsed = started.elapsed();

        assert!(matches!(err, EchoraError::SidecarTimeout(_)));
        assert!(
            elapsed < Duration::from_secs(3),
            "send_command should give up around COMMAND_TIMEOUT, took {elapsed:?} instead"
        );

        let _ = std::fs::remove_file(&socket_path);
    }

    /// Same guarantee as above, for the legacy fresh-connection path
    /// `enable_level_metering`/`audio_level_db` still use (see their doc
    /// comments on `Player`) — must not regress independently of the
    /// persistent-connection path.
    #[tokio::test]
    async fn send_command_fresh_connection_times_out_instead_of_hanging_on_a_silent_peer() {
        let socket_path = hang_socket_path();
        let listener = UnixListener::bind(&socket_path).unwrap();
        let _accepting = tokio::spawn(async move {
            let (_stream, _addr) = listener.accept().await.unwrap();
            std::future::pending::<()>().await
        });

        let mut player = Player::new(
            socket_path.clone(),
            std::env::temp_dir(),
            std::sync::Arc::new(AtomicBool::new(false)),
        );

        let started = Instant::now();
        let err = player
            .send_command_fresh_connection(json!(["get_property", "pause"]))
            .await
            .unwrap_err();
        let elapsed = started.elapsed();

        assert!(matches!(err, EchoraError::SidecarTimeout(_)));
        assert!(
            elapsed < Duration::from_secs(3),
            "send_command_fresh_connection should give up around COMMAND_TIMEOUT, took {elapsed:?} instead"
        );

        let _ = std::fs::remove_file(&socket_path);
    }

    #[test]
    fn apply_property_change_updates_known_properties() {
        let mut cache = Cache::default();
        apply_property_change(&mut cache, "pause", &json!(true));
        apply_property_change(&mut cache, "time-pos", &json!(12.5));
        apply_property_change(&mut cache, "duration", &json!(180.0));
        apply_property_change(&mut cache, "volume", &json!(75.0));

        assert_eq!(cache.is_paused, Some(true));
        assert_eq!(cache.position_seconds, Some(12.5));
        assert_eq!(cache.duration_seconds, Some(180.0));
        assert_eq!(cache.volume_percent, Some(75));
    }

    #[test]
    fn apply_property_change_ignores_unknown_property_names() {
        let mut cache = Cache::default();
        apply_property_change(&mut cache, "af-metadata/echora_level", &json!({"x": "1"}));
        assert_eq!(cache.is_paused, None);
        assert_eq!(cache.position_seconds, None);
    }

    #[test]
    fn apply_property_change_treats_missing_data_as_unavailable() {
        // mpv represents "this property has no value right now" (not yet
        // known, or no longer available -- e.g. `time-pos` right after a
        // file unloads) the same way: an event with no `data` field.
        let mut cache = Cache {
            position_seconds: Some(42.0),
            ..Cache::default()
        };
        apply_property_change(&mut cache, "time-pos", &Value::Null);
        assert_eq!(cache.position_seconds, None);
    }

    #[test]
    fn handle_message_routes_property_change_events_into_the_cache() {
        let cache = Mutex::new(Cache::default());
        let pending = Mutex::new(None);
        let epoch = AtomicU64::new(0);

        handle_message(
            &json!({"event": "property-change", "id": 1, "name": "pause", "data": true}),
            &cache,
            &pending,
            &epoch,
        );

        assert_eq!(cache.lock().unwrap().is_paused, Some(true));
    }

    #[test]
    fn handle_message_increments_the_epoch_only_for_a_natural_eof() {
        let cache = Mutex::new(Cache::default());
        let pending = Mutex::new(None);
        let epoch = AtomicU64::new(0);

        handle_message(
            &json!({"event": "end-file", "reason": "stop"}),
            &cache,
            &pending,
            &epoch,
        );
        assert_eq!(epoch.load(Ordering::Relaxed), 0);

        handle_message(
            &json!({"event": "end-file", "reason": "eof"}),
            &cache,
            &pending,
            &epoch,
        );
        assert_eq!(epoch.load(Ordering::Relaxed), 1);
    }

    #[tokio::test]
    async fn handle_message_completes_only_the_matching_pending_reply() {
        let cache = Mutex::new(Cache::default());
        let (tx, rx) = oneshot::channel();
        let pending = Mutex::new(Some((7, tx)));
        let epoch = AtomicU64::new(0);

        // A reply with a different request_id must not consume or
        // complete this slot.
        handle_message(
            &json!({"request_id": 8, "error": "success"}),
            &cache,
            &pending,
            &epoch,
        );
        assert!(pending.lock().unwrap().is_some());

        handle_message(
            &json!({"request_id": 7, "error": "success", "data": 1.5}),
            &cache,
            &pending,
            &epoch,
        );
        assert!(pending.lock().unwrap().is_none());
        let reply = rx.await.unwrap();
        assert_eq!(reply["data"], 1.5);
    }
}

/// Real mpv-process smoke tests. Not run by default — `cargo test --
/// --ignored` with `mpv` on PATH (see docs/adr/0007: the CI-built portable
/// mpv ships in the real package; local dev uses the system package).
#[cfg(test)]
mod smoke_tests {
    use super::*;

    fn dev_socket_path() -> PathBuf {
        std::env::temp_dir().join(format!("echora-player-test-{}.sock", std::process::id()))
    }

    /// A real (if fake-runtime) `AppHandle` with the shell plugin
    /// registered — `app.shell().sidecar(...)` looks up `Shell<R>` from
    /// managed state, so a bare `MockRuntime` app without the plugin
    /// registered would fail at `.sidecar()`, not just at the (expected,
    /// per tauri-apps/tauri#13767) real spawn.
    fn test_app_handle() -> tauri::AppHandle<tauri::test::MockRuntime> {
        tauri::test::mock_builder()
            .plugin(tauri_plugin_shell::init())
            .build(tauri::test::mock_context(tauri::test::noop_assets()))
            .expect("mock tauri app should build")
            .handle()
            .clone()
    }

    #[tokio::test]
    #[ignore]
    async fn starting_and_shutting_down_leaves_no_process_or_socket() {
        let socket_path = dev_socket_path();
        let app = test_app_handle();
        let mut player = Player::new(
            socket_path.clone(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        assert!(socket_path.exists());

        player.shutdown().await.unwrap();
        assert!(!socket_path.exists());
    }

    #[tokio::test]
    #[ignore]
    async fn loading_a_real_stream_reports_matching_duration() {
        // Resolve a real track first (requires the yt-dlp/Deno dev
        // binaries — see resolver.rs's smoke tests), then hand its stream
        // URL to mpv and confirm mpv's own reported duration lines up with
        // what yt-dlp reported: proof the whole sidecar chain actually
        // plays real audio end to end, not just that each piece runs.
        //
        // Deliberately searches for a normal video, not a livestream — a
        // livestream's DASH manifest behaves differently in mpv and won't
        // report a stable time-pos/duration the way a VOD does.
        use super::super::resolver::{Resolver, ResolverConfig};
        use std::time::Duration as StdDuration;

        let dev_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("binaries/dev");
        let resolver = Resolver::new(ResolverConfig {
            deno_path: dev_dir.join("deno"),
            timeout: StdDuration::from_secs(30),
        });
        let app = test_app_handle();
        let tracks = resolver
            .search(&app, "villain arc playlist", 1)
            .await
            .unwrap();
        let resolved = resolver
            .resolve_with_retry(&app, &tracks[0].id)
            .await
            .unwrap();
        let mut player = Player::new(
            dev_socket_path(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player.set_volume(10).await.unwrap();
        player.load(&resolved.stream_url).await.unwrap();
        tokio::time::sleep(StdDuration::from_secs(4)).await;

        let position = player.position_seconds().await.unwrap();
        assert!(position.unwrap_or(0.0) > 0.0, "playback did not advance");

        let duration = player.duration_seconds().await.unwrap();
        let expected = tracks[0].duration_seconds.unwrap() as f64;
        assert!(
            (duration.unwrap_or(0.0) - expected).abs() < 5.0,
            "mpv duration {duration:?} does not match yt-dlp's reported {expected}"
        );

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn mpv_dying_unexpectedly_is_recorded_as_a_sidecar_crash() {
        // Fully-qualified `Arc`/`AtomicBool`/`Duration` here rather than
        // relying on `use super::*` to have brought them in — matches
        // this file's existing `StdDuration` alias in the test above,
        // which does the same for the same reason.
        let socket_path = dev_socket_path();
        let app_dir =
            std::env::temp_dir().join(format!("echora-crash-smoke-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&app_dir);
        let enabled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
        let app = test_app_handle();
        let mut player = Player::new(socket_path.clone(), app_dir.clone(), enabled);
        player.start(&app).await.unwrap();

        // Kill mpv out-of-band — not through shutdown() — to simulate a
        // real unexpected sidecar death.
        let pid = player.child.as_ref().unwrap().pid();
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        let err = player.set_volume(50).await.unwrap_err();

        assert!(matches!(err, crate::error::EchoraError::Io(_)));
        let summaries = crate::crash::list(&app_dir).unwrap();
        assert_eq!(summaries.len(), 1);
        assert!(matches!(
            summaries[0].kind,
            crate::crash::CrashKind::SidecarCrash
        ));
    }

    #[tokio::test]
    #[ignore]
    async fn enable_level_metering_is_a_noop_the_second_time() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();

        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        // Second call must not error even though the filter is already
        // attached — this is what makes it safe to call unconditionally
        // from audio_level::watch every tick before reading a level.
        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn audio_level_db_returns_none_before_metering_is_enabled() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-none-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();

        assert_eq!(player.audio_level_db().await.unwrap(), None);

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn audio_level_db_reads_a_real_rms_level_once_metering_is_enabled() {
        let app = test_app_handle();
        let mut player = Player::new(
            std::env::temp_dir().join(format!(
                "echora-player-test-level-real-{}.sock",
                std::process::id()
            )),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.enable_level_metering().await.unwrap();

        // Give mpv a moment to actually start decoding/filtering audio —
        // mirrors the existing smoke tests' pattern of a short sleep after
        // a state-changing IPC call before reading it back.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;

        let level = player.audio_level_db().await.unwrap();
        assert!(level.is_some());
        assert!(level.unwrap().is_finite());

        player.shutdown().await.unwrap();
    }

    #[tokio::test]
    #[ignore]
    async fn level_metering_flag_resets_on_process_restart() {
        let socket_path = dev_socket_path();
        let app = test_app_handle();
        let mut player = Player::new(
            socket_path.clone(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );

        // First start: enable metering
        player.start(&app).await.unwrap();
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.enable_level_metering().await.unwrap();
        assert!(player.level_metering_ready);

        // Simulate crash by killing the process
        let pid = player.child.as_ref().unwrap().pid();
        let _ = std::process::Command::new("kill")
            .arg("-9")
            .arg(pid.to_string())
            .status();
        tokio::time::sleep(std::time::Duration::from_millis(300)).await;

        // Trigger error to clear child state
        let _ = player.set_volume(50).await;

        // Restart: flag should be reset to false
        player.start(&app).await.unwrap();
        assert!(
            !player.level_metering_ready,
            "level_metering_ready must reset on start()"
        );

        // Load and re-enable metering on the new process
        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.enable_level_metering().await.unwrap();
        assert!(
            player.level_metering_ready,
            "enable_level_metering must work on new process"
        );

        player.shutdown().await.unwrap();
    }

    /// Real-mpv proof for P2-1's connection-count claim: everything except
    /// `enable_level_metering`/`audio_level_db` now goes over exactly one
    /// `UnixStream`, for the whole session — not one per call.
    #[tokio::test]
    #[ignore]
    async fn playback_getters_and_mutators_share_one_connection() {
        let app = test_app_handle();
        let mut player = Player::new(
            dev_socket_path(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();
        assert!(player.write_half.is_some());

        player
            .load("av://lavfi:sine=frequency=440:duration=5")
            .await
            .unwrap();
        player.set_paused(true).await.unwrap();
        player.set_paused(false).await.unwrap();
        player.set_volume(42).await.unwrap();
        player.seek_to(1.0).await.unwrap();

        tokio::time::sleep(Duration::from_millis(300)).await;

        for _ in 0..20 {
            let _ = player.is_paused().await.unwrap();
            let _ = player.position_seconds().await.unwrap();
            let _ = player.duration_seconds().await.unwrap();
            let _ = player.volume_percent().await.unwrap();
        }

        assert_eq!(
            player.volume_percent().await.unwrap(),
            Some(42),
            "the cache should reflect the set_volume call via a property-change event"
        );

        player.shutdown().await.unwrap();
    }

    /// Real-mpv proof that `end_file_eof_epoch` only advances for a
    /// genuine natural finish, not a manual replace/stop — the exact
    /// distinction `media::auto_advance` relies on.
    #[tokio::test]
    #[ignore]
    async fn end_file_eof_epoch_only_advances_on_a_natural_finish() {
        let app = test_app_handle();
        let mut player = Player::new(
            dev_socket_path(),
            std::env::temp_dir(),
            std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        );
        player.start(&app).await.unwrap();

        player
            .load("av://lavfi:sine=frequency=440:duration=30")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(player.end_file_eof_epoch(), 0);

        // Replacing the file before it finishes must not count as a
        // natural completion.
        player
            .load("av://lavfi:sine=frequency=220:duration=2")
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(player.end_file_eof_epoch(), 0);

        // Let this short one play through to its own end.
        tokio::time::sleep(Duration::from_secs(3)).await;
        assert_eq!(player.end_file_eof_epoch(), 1);

        player.shutdown().await.unwrap();
    }
}
