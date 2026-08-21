//! Wire protocol between the detached shell host and its attaching clients.
//!
//! Newline framing would force base64 on the hot output path, so frames are
//! length-prefixed binary: `[u8 kind][u32 be len][payload len bytes]`. The
//! payload is raw for byte streams (`Output`/`Input`) and JSON for the small
//! control frames (`Resize`/`Hello`). One frame is one logical message.
//!
//! Directions:
//! - host → client: `Hello` (once, on connect), `Output` (scrollback replay
//!   then live), `Exit` (shell ended).
//! - client → host: `Input` (keystrokes), `Resize` (terminal geometry), `Kill`
//!   (terminate the shell — an explicit close, distinct from just detaching),
//!   `SetTitle` (rename the session; the host owns the receipt + persisted
//!   metadata, so a rename has to reach it to survive a restart).

use std::io::{self, Read, Write};

use serde::{Deserialize, Serialize};

/// Frame kind tags. Stable on the wire — only append.
pub mod kind {
    pub const OUTPUT: u8 = 0;
    pub const INPUT: u8 = 1;
    pub const RESIZE: u8 = 2;
    pub const KILL: u8 = 3;
    pub const EXIT: u8 = 4;
    pub const HELLO: u8 = 5;
    pub const SYNCED: u8 = 6;
    pub const SET_TITLE: u8 = 7;
}

/// A decoded frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Frame {
    /// Raw PTY bytes (host → client): scrollback replay, then live output.
    Output(Vec<u8>),
    /// Raw keystrokes to feed the PTY (client → host).
    Input(Vec<u8>),
    /// New terminal geometry (client → host).
    Resize { rows: u16, cols: u16 },
    /// Terminate the shell (client → host) — an explicit close, not a detach.
    Kill,
    /// Rename the session (client → host). The host updates its reattach
    /// receipt and persisted metadata so the new title survives an app restart.
    SetTitle(String),
    /// The shell exited (host → client).
    Exit,
    /// Sent once when a client attaches, before any output (host → client).
    Hello(Hello),
    /// Sent after the scrollback replay, before live output (host → client).
    /// Marks the boundary so the client can seed its cursor from `Hello.total`
    /// and begin surfacing live output as events.
    Synced,
}

/// The greeting a host sends a freshly-attached client, before scrollback.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hello {
    /// Total bytes the shell has ever produced (monotonic; survives the app
    /// restarting). Lets the client keep its output cursor continuous.
    pub total: u64,
    /// The shell's current working directory.
    pub cwd: String,
    /// The shell process id, for the client's own cwd probing later.
    pub shell_pid: Option<u32>,
}

/// Cap on a single frame's payload, so a corrupt length can't trigger a huge
/// allocation. Comfortably above the 1 MiB scrollback replayed in one frame is
/// avoided — replay is chunked below this.
pub const MAX_FRAME_LEN: usize = 2 * 1024 * 1024;

impl Frame {
    fn tag(&self) -> u8 {
        match self {
            Frame::Output(_) => kind::OUTPUT,
            Frame::Input(_) => kind::INPUT,
            Frame::Resize { .. } => kind::RESIZE,
            Frame::Kill => kind::KILL,
            Frame::Exit => kind::EXIT,
            Frame::Hello(_) => kind::HELLO,
            Frame::Synced => kind::SYNCED,
            Frame::SetTitle(_) => kind::SET_TITLE,
        }
    }

    fn payload(&self) -> Vec<u8> {
        match self {
            Frame::Output(bytes) | Frame::Input(bytes) => bytes.clone(),
            Frame::Resize { rows, cols } => {
                let mut v = Vec::with_capacity(4);
                v.extend_from_slice(&rows.to_be_bytes());
                v.extend_from_slice(&cols.to_be_bytes());
                v
            }
            Frame::Kill | Frame::Exit | Frame::Synced => Vec::new(),
            Frame::SetTitle(title) => title.as_bytes().to_vec(),
            Frame::Hello(hello) => serde_json::to_vec(hello).unwrap_or_default(),
        }
    }

    /// Encode `self` and write it to `w` as one length-prefixed frame.
    pub fn write_to<W: Write>(&self, w: &mut W) -> io::Result<()> {
        let payload = self.payload();
        let mut header = [0u8; 5];
        header[0] = self.tag();
        header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
        w.write_all(&header)?;
        w.write_all(&payload)?;
        w.flush()
    }

    /// Read one frame from `r`. Returns `Ok(None)` on clean EOF at a frame
    /// boundary (the peer closed the connection).
    pub fn read_from<R: Read>(r: &mut R) -> io::Result<Option<Frame>> {
        let mut header = [0u8; 5];
        if !read_exact_or_eof(r, &mut header)? {
            return Ok(None);
        }
        let tag = header[0];
        let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
        if len > MAX_FRAME_LEN {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("shell-host frame too large: {len} bytes"),
            ));
        }
        let mut payload = vec![0u8; len];
        r.read_exact(&mut payload)?;
        decode(tag, payload)
            .map(Some)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unknown shell-host frame"))
    }
}

fn decode(tag: u8, payload: Vec<u8>) -> Option<Frame> {
    match tag {
        kind::OUTPUT => Some(Frame::Output(payload)),
        kind::INPUT => Some(Frame::Input(payload)),
        kind::RESIZE => {
            if payload.len() != 4 {
                return None;
            }
            let rows = u16::from_be_bytes([payload[0], payload[1]]);
            let cols = u16::from_be_bytes([payload[2], payload[3]]);
            Some(Frame::Resize { rows, cols })
        }
        kind::KILL => Some(Frame::Kill),
        kind::EXIT => Some(Frame::Exit),
        kind::SYNCED => Some(Frame::Synced),
        kind::SET_TITLE => String::from_utf8(payload).ok().map(Frame::SetTitle),
        kind::HELLO => serde_json::from_slice::<Hello>(&payload)
            .ok()
            .map(Frame::Hello),
        _ => None,
    }
}

/// Read exactly `buf.len()` bytes, or return `Ok(false)` if EOF happens before
/// any byte is read (clean close). A partial read then EOF is an error.
fn read_exact_or_eof<R: Read>(r: &mut R, buf: &mut [u8]) -> io::Result<bool> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..]) {
            Ok(0) => {
                if filled == 0 {
                    return Ok(false);
                }
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "shell-host frame truncated",
                ));
            }
            Ok(n) => filled += n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip(frame: Frame) {
        let mut buf = Vec::new();
        frame.write_to(&mut buf).expect("encode");
        let mut cursor = std::io::Cursor::new(buf);
        let decoded = Frame::read_from(&mut cursor)
            .expect("decode")
            .expect("some");
        assert_eq!(decoded, frame);
    }

    #[test]
    fn frames_round_trip() {
        round_trip(Frame::Output(b"\x1b[32mhi\x1b[0m".to_vec()));
        round_trip(Frame::Input(b"ls -la\r".to_vec()));
        round_trip(Frame::Resize {
            rows: 40,
            cols: 120,
        });
        round_trip(Frame::Kill);
        round_trip(Frame::Exit);
        round_trip(Frame::Synced);
        round_trip(Frame::SetTitle("my project".to_string()));
        round_trip(Frame::SetTitle(String::new()));
        round_trip(Frame::Hello(Hello {
            total: 4096,
            cwd: "/Users/andy/Code".to_string(),
            shell_pid: Some(1234),
        }));
    }

    #[test]
    fn clean_eof_at_boundary_is_none() {
        let mut cursor = std::io::Cursor::new(Vec::new());
        assert_eq!(Frame::read_from(&mut cursor).expect("ok"), None);
    }

    #[test]
    fn oversized_length_is_rejected() {
        let mut buf = Vec::new();
        buf.push(kind::OUTPUT);
        buf.extend_from_slice(&((MAX_FRAME_LEN as u32) + 1).to_be_bytes());
        let mut cursor = std::io::Cursor::new(buf);
        assert!(Frame::read_from(&mut cursor).is_err());
    }

    #[test]
    fn empty_stream_frames_decode() {
        round_trip(Frame::Output(Vec::new()));
    }
}
