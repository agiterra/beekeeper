//! Where `bee preview snapshot` writes its PNG and aria text (ledger 371(h)).
//!
//! Without `--out`, never into the working tree: files there show up in the
//! session's Diff and are swept into rewinds. The default is a per-session
//! directory in the app's state dir, found the way this command already
//! finds the app: beside the broker socket (`$BEEKEEPER_SESSION_BROKER_SOCK`,
//! else `~/.local/state/buzz/session-broker.sock`), so a dev instance's
//! snapshots land beside the dev instance. The session is the one the grant
//! names (read unverified, for a directory name only; the broker is what
//! authorizes). An explicit `--out` is still honoured, relative to the cwd.

use std::path::{Path, PathBuf};

use base64::Engine as _;
use serde_json::{json, Map, Value};

use beekeeper_core::preview_grant::decode_preview_grant_unverified;

use super::{PreviewContext, PreviewFailure, DEFAULT_SOCKET_REL};

/// The directory under the app's state dir that holds snapshots, one
/// subdirectory per session.
pub const SNAPSHOT_DIR_NAME: &str = "preview-snapshots";

/// The directory name used when the grant names no readable session.
const UNKNOWN_SESSION: &str = "unknown-session";

/// Longest session directory name kept.
const MAX_SESSION_DIR_LEN: usize = 96;

/// The default snapshot directory for the broker at `socket` and the
/// session `grant` names: `<state dir>/preview-snapshots/<session id>`.
pub fn default_snapshot_dir(socket: &Path, grant: Option<&str>) -> PathBuf {
    let state_dir = socket
        .parent()
        .filter(|parent| parent.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| {
            std::env::var_os("HOME").map(|home| {
                let socket = PathBuf::from(home).join(DEFAULT_SOCKET_REL);
                socket.parent().map(Path::to_path_buf).unwrap_or(socket)
            })
        })
        .unwrap_or_else(std::env::temp_dir);
    let session = grant
        .and_then(|token| decode_preview_grant_unverified(token).ok())
        .map(|claims| session_dir_name(&claims.target.session_id))
        .unwrap_or_else(|| UNKNOWN_SESSION.to_owned());
    state_dir.join(SNAPSHOT_DIR_NAME).join(session)
}

/// A session id as one safe path component: `[A-Za-z0-9._-]` kept, anything
/// else `_`, bounded, and never `.`/`..`/empty.
pub fn session_dir_name(session_id: &str) -> String {
    let name: String = session_id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-') {
                c
            } else {
                '_'
            }
        })
        .take(MAX_SESSION_DIR_LEN)
        .collect();
    if name.is_empty() || name.chars().all(|c| c == '.') {
        UNKNOWN_SESSION.to_owned()
    } else {
        name
    }
}

/// Decode the snapshot's PNG (never printed) and write it and the aria text,
/// replacing `png.base64` with the absolute paths.
pub fn write_snapshot(
    result: &mut Map<String, Value>,
    out: Option<&Path>,
    context: &PreviewContext,
) -> Result<(), PreviewFailure> {
    let generation = result
        .get("generation")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let png_path = match out {
        Some(out) if out.is_absolute() => out.to_path_buf(),
        Some(out) => context.cwd.join(out),
        None => context
            .snapshot_dir
            .join(format!("snapshot-{generation}-{}.png", context.now_ms)),
    };
    let aria_path = png_path.with_extension("aria.yaml");
    let io = |path: &Path, error: std::io::Error| {
        PreviewFailure::new(
            "preview_io_error",
            format!("cannot write {}: {error}", path.display()),
        )
    };
    if let Some(parent) = png_path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| io(parent, error))?;
    }

    let mut written_png = None;
    if let Some(Value::Object(png)) = result.get_mut("png") {
        if let Some(Value::String(encoded)) = png.remove("base64") {
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(encoded.as_bytes())
                .map_err(|error| {
                    PreviewFailure::new(
                        "preview_bad_response",
                        format!("the snapshot's PNG is not base64: {error}"),
                    )
                })?;
            std::fs::write(&png_path, &bytes).map_err(|error| io(&png_path, error))?;
            written_png = Some(png_path.clone());
        }
    }
    let aria = result.get("aria").and_then(Value::as_str).unwrap_or("");
    std::fs::write(&aria_path, aria).map_err(|error| io(&aria_path, error))?;
    result.insert(
        "pngPath".into(),
        written_png.map_or(Value::Null, |path| json!(path.display().to_string())),
    );
    result.insert("ariaPath".into(), json!(aria_path.display().to_string()));
    Ok(())
}
