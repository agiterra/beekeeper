//! Named-key → PTY byte-sequence mapping for the built-in shell.
//!
//! Mirrors the key names the cmux integration accepts (`enter`, `escape`,
//! `ctrl+c`, arrow keys, …) so `buzz session send-key` behaves identically
//! against either backend.

/// Translate a named key into the bytes to write to the PTY. Returns `None`
/// for names this backend does not understand.
pub fn key_to_bytes(key: &str) -> Option<Vec<u8>> {
    let normalized = key.trim().to_lowercase();
    let bytes: Vec<u8> = match normalized.as_str() {
        "enter" | "return" => b"\r".to_vec(),
        "tab" => b"\t".to_vec(),
        "space" => b" ".to_vec(),
        "escape" | "esc" => b"\x1b".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "up" => b"\x1b[A".to_vec(),
        "down" => b"\x1b[B".to_vec(),
        "right" => b"\x1b[C".to_vec(),
        "left" => b"\x1b[D".to_vec(),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" | "page_up" => b"\x1b[5~".to_vec(),
        "pagedown" | "page_down" => b"\x1b[6~".to_vec(),
        other => {
            // ctrl+<letter> → C0 control byte (ctrl+c = 0x03, ctrl+z = 0x1a, …).
            let ctrl = other
                .strip_prefix("ctrl+")
                .or_else(|| other.strip_prefix("ctrl-"))?;
            let mut chars = ctrl.chars();
            let (Some(letter), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let letter = letter.to_ascii_lowercase();
            if !letter.is_ascii_lowercase() {
                return None;
            }
            vec![(letter as u8) - b'a' + 1]
        }
    };
    Some(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_common_keys() {
        assert_eq!(key_to_bytes("enter"), Some(b"\r".to_vec()));
        assert_eq!(key_to_bytes("Escape"), Some(b"\x1b".to_vec()));
        assert_eq!(key_to_bytes("up"), Some(b"\x1b[A".to_vec()));
        assert_eq!(key_to_bytes("ctrl+c"), Some(vec![0x03]));
        assert_eq!(key_to_bytes("ctrl+z"), Some(vec![0x1a]));
    }

    #[test]
    fn rejects_unknown_keys() {
        assert_eq!(key_to_bytes("f13"), None);
        assert_eq!(key_to_bytes("ctrl+enter"), None);
        assert_eq!(key_to_bytes(""), None);
    }
}
