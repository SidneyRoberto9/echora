//! Discord Rich Presence integration. See
//! docs/superpowers/specs/2026-09-09-discord-rich-presence-design.md and
//! docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md. Hand-rolled
//! against Discord's local IPC protocol -- a Unix socket handshake
//! followed by length-prefixed JSON frames -- using only `tokio`
//! (already a dependency; `net`/`io-util`/`time` features already
//! enabled) and `serde_json`. Opt-in, off by default: see
//! `Settings::discord_presence_enabled`.

use std::path::PathBuf;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const OP_HANDSHAKE: i32 = 0;
#[allow(dead_code)]
const OP_FRAME: i32 = 1;
#[allow(dead_code)]
const DISCORD_CLIENT_ID: &str = "1547402077863542914";

#[allow(dead_code)]
const MAX_FIELD_BYTES: usize = 128;

/// Discord's `details`/`state` activity fields are untrusted-YouTube-
/// metadata boundaries (CLAUDE.md: normalize/validate external metadata
/// before it's used). Strips control characters, collapses whitespace
/// runs, truncates at a UTF-8 char boundary within `max_bytes`, and
/// falls back to "Echora" if nothing printable is left.
#[allow(dead_code)]
pub(crate) fn sanitize_field(input: &str, max_bytes: usize) -> String {
    let cleaned: String = input.chars().filter(|c| !c.is_control() || c.is_whitespace()).collect();
    let collapsed = cleaned.split_whitespace().collect::<Vec<_>>().join(" ");
    let truncated = truncate_utf8(&collapsed, max_bytes);
    if truncated.is_empty() {
        "Echora".to_string()
    } else {
        truncated
    }
}

#[allow(dead_code)]
fn truncate_utf8(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    s[..end].to_string()
}

/// One Discord IPC frame: an opcode, then a little-endian `u32` byte
/// length, then the JSON payload -- no other framing exists in the
/// protocol.
fn encode_frame(opcode: i32, payload: &serde_json::Value) -> Vec<u8> {
    let body = serde_json::to_vec(payload).expect("activity payload is always valid JSON");
    let mut buf = Vec::with_capacity(8 + body.len());
    buf.extend_from_slice(&opcode.to_le_bytes());
    buf.extend_from_slice(&(body.len() as u32).to_le_bytes());
    buf.extend_from_slice(&body);
    buf
}

/// Discord's IPC socket lives at `<base>/discord-ipc-<0..9>`, `base`
/// being `$XDG_RUNTIME_DIR` (or `$TMPDIR`, or `/tmp` if neither is
/// set). Multiple indices exist for multiple concurrently-running
/// Discord clients (stable/PTB/canary); trying all ten and taking the
/// first that accepts a connection is standard practice for RPC
/// clients.
fn candidate_paths(base: &str) -> Vec<PathBuf> {
    (0..10)
        .map(|n| PathBuf::from(base).join(format!("discord-ipc-{n}")))
        .collect()
}

fn runtime_dir() -> String {
    std::env::var("XDG_RUNTIME_DIR")
        .or_else(|_| std::env::var("TMPDIR"))
        .unwrap_or_else(|_| "/tmp".to_string())
}

async fn read_frame(stream: &mut UnixStream) -> std::io::Result<(i32, Vec<u8>)> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header).await?;
    let opcode = i32::from_le_bytes(header[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    Ok((opcode, body))
}

#[allow(dead_code)]
async fn write_frame(
    stream: &mut UnixStream,
    opcode: i32,
    payload: &serde_json::Value,
) -> std::io::Result<()> {
    stream.write_all(&encode_frame(opcode, payload)).await
}

/// Tries every candidate socket path in order, handshaking with each
/// connection that accepts -- the first that completes the handshake
/// wins. Returns `None` (never an error) if nothing is listening
/// anywhere, which just means Discord isn't running right now; the
/// caller's reconnect loop tries again later.
#[allow(dead_code)]
async fn connect() -> Option<UnixStream> {
    let base = runtime_dir();
    for path in candidate_paths(&base) {
        let Ok(mut stream) = UnixStream::connect(&path).await else {
            continue;
        };
        let handshake = serde_json::json!({ "v": 1, "client_id": DISCORD_CLIENT_ID });
        if write_frame(&mut stream, OP_HANDSHAKE, &handshake).await.is_err() {
            continue;
        }
        if read_frame(&mut stream).await.is_err() {
            continue;
        }
        return Some(stream);
    }
    None
}

/// What's currently true about playback, mapped from the same
/// queue/player state `mpris::notify` already reads. `Idle` covers both
/// "nothing loaded" and "the feature is enabled but disconnected" —
/// either way, Discord shows no activity.
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum PresenceState {
    Idle,
    Playing {
        title: String,
        artist: Option<String>,
        position_secs: f64,
    },
    Paused {
        title: String,
        artist: Option<String>,
    },
}

#[allow(dead_code)]
fn activity_value(state: &PresenceState, now_unix: f64) -> serde_json::Value {
    match state {
        PresenceState::Idle => serde_json::Value::Null,
        PresenceState::Playing {
            title,
            artist,
            position_secs,
        } => {
            let start = (now_unix - position_secs).max(0.0) as i64;
            serde_json::json!({
                "details": sanitize_field(title, MAX_FIELD_BYTES),
                "state": sanitize_field(artist.as_deref().unwrap_or(""), MAX_FIELD_BYTES),
                "timestamps": { "start": start },
            })
        }
        PresenceState::Paused { title, artist } => {
            let label = format!("Paused — {}", artist.as_deref().unwrap_or(""));
            serde_json::json!({
                "details": sanitize_field(title, MAX_FIELD_BYTES),
                "state": sanitize_field(&label, MAX_FIELD_BYTES),
            })
        }
    }
}

#[allow(dead_code)]
fn now_unix() -> f64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64()
}

static NONCE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// Builds the full `SET_ACTIVITY` IPC command for the given state.
/// `nonce` only needs to be unique per outgoing request for this
/// process's lifetime, per the protocol — a monotonic counter is
/// simpler than pulling in a UUID dependency for it.
#[allow(dead_code)]
fn set_activity_frame(state: &PresenceState) -> serde_json::Value {
    let nonce = NONCE
        .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        .to_string();
    serde_json::json!({
        "cmd": "SET_ACTIVITY",
        "args": {
            "pid": std::process::id(),
            "activity": activity_value(state, now_unix()),
        },
        "nonce": nonce,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_field_strips_control_characters() {
        assert_eq!(sanitize_field("Song\u{0007}Title", MAX_FIELD_BYTES), "SongTitle");
    }

    #[test]
    fn sanitize_field_collapses_whitespace_runs() {
        assert_eq!(
            sanitize_field("Song   Title\n\nHere", MAX_FIELD_BYTES),
            "Song Title Here"
        );
    }

    #[test]
    fn sanitize_field_falls_back_to_echora_when_empty() {
        assert_eq!(sanitize_field("   ", MAX_FIELD_BYTES), "Echora");
        assert_eq!(sanitize_field("", MAX_FIELD_BYTES), "Echora");
    }

    #[test]
    fn sanitize_field_truncates_at_a_utf8_char_boundary() {
        // Each "é" is 2 bytes -- a naive byte-index truncation at an odd
        // offset would split the character and panic or produce garbage.
        let input = "é".repeat(100); // 200 bytes
        let result = sanitize_field(&input, 11); // odd byte budget
        assert!(result.len() <= 11);
        assert!(String::from_utf8(result.into_bytes()).is_ok());
    }

    #[test]
    fn encode_frame_header_matches_opcode_and_payload_length() {
        let payload = serde_json::json!({"a": 1});
        let body = serde_json::to_vec(&payload).unwrap();
        let frame = encode_frame(1, &payload);

        assert_eq!(&frame[0..4], &1i32.to_le_bytes());
        assert_eq!(&frame[4..8], &(body.len() as u32).to_le_bytes());
        assert_eq!(&frame[8..], body.as_slice());
    }

    #[test]
    fn candidate_paths_covers_all_ten_discord_ipc_indices() {
        let paths = candidate_paths("/run/user/1000");
        assert_eq!(paths.len(), 10);
        assert_eq!(paths[0], std::path::PathBuf::from("/run/user/1000/discord-ipc-0"));
        assert_eq!(paths[9], std::path::PathBuf::from("/run/user/1000/discord-ipc-9"));
    }

    #[test]
    fn activity_value_for_idle_is_null() {
        assert_eq!(activity_value(&PresenceState::Idle, 1000.0), serde_json::Value::Null);
    }

    #[test]
    fn activity_value_for_playing_sets_start_timestamp_from_position() {
        let state = PresenceState::Playing {
            title: "Song".into(),
            artist: Some("Artist".into()),
            position_secs: 30.0,
        };
        let value = activity_value(&state, 1000.0);
        assert_eq!(value["details"], "Song");
        assert_eq!(value["state"], "Artist");
        assert_eq!(value["timestamps"]["start"], 970);
    }

    #[test]
    fn activity_value_for_paused_omits_timestamps_and_labels_state() {
        let state = PresenceState::Paused {
            title: "Song".into(),
            artist: Some("Artist".into()),
        };
        let value = activity_value(&state, 1000.0);
        assert_eq!(value["details"], "Song");
        assert_eq!(value["state"], "Paused — Artist");
        assert!(value.get("timestamps").is_none());
    }

    #[test]
    fn activity_value_sanitizes_title_and_artist() {
        let state = PresenceState::Playing {
            title: "Song\u{0007}".into(),
            artist: Some("  Art   ist  ".into()),
            position_secs: 0.0,
        };
        let value = activity_value(&state, 1000.0);
        assert_eq!(value["details"], "Song");
        assert_eq!(value["state"], "Art ist");
    }
}
