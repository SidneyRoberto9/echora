//! Discord Rich Presence integration. See
//! docs/superpowers/specs/2026-09-09-discord-rich-presence-design.md and
//! docs/adr/0010-discord-rich-presence-hand-rolled-ipc.md. Hand-rolled
//! against Discord's local IPC protocol -- a Unix socket handshake
//! followed by length-prefixed JSON frames -- using only `tokio`
//! (already a dependency; `net`/`io-util`/`time` features already
//! enabled) and `serde_json`. Opt-in, off by default: see
//! `Settings::discord_presence_enabled`.

const MAX_FIELD_BYTES: usize = 128;

/// Discord's `details`/`state` activity fields are untrusted-YouTube-
/// metadata boundaries (CLAUDE.md: normalize/validate external metadata
/// before it's used). Strips control characters, collapses whitespace
/// runs, truncates at a UTF-8 char boundary within `max_bytes`, and
/// falls back to "Echora" if nothing printable is left.
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
}
