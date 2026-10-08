//! The opaque slot id, the host-local slot file, and the guard that keeps
//! host-local facts off the relay.
//!
//! A *slot* is one simulator bound to one session on this provider. The wire
//! names it only by [`slot_id`] — the first 16 hex of
//! `sha256(provider pubkey ‖ udid)` — so a watcher can address it without
//! learning the UDID. Everything a seat needs to drive the device (UDID,
//! agent-device daemon config, screenshot directory) lives in a 0600 JSON
//! file under `<CSP state>/device/<session>/`, which a device-enabled seat is
//! granted read access to and nothing else is (brief § 3 "Never on the
//! relay").

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Version of the slot file layout.
pub const SLOT_FILE_VERSION: u32 = 1;

/// The opaque, stable slot id for `udid` on the provider `provider_pubkey`
/// (lowercase 64-hex): first 16 lowercase hex of
/// `sha256(pubkey's 32 raw bytes ‖ udid UTF-8)` (WIRE-C5 § 1,
/// `beekeeper_core::session_device::device_slot_id`). A pubkey that is not
/// 64-hex hashes its text instead, so the id is still opaque and stable.
pub fn slot_id(provider_pubkey: &str, udid: &str) -> String {
    let mut hasher = Sha256::new();
    match hex::decode(provider_pubkey) {
        Ok(raw) if raw.len() == 32 => hasher.update(&raw),
        _ => hasher.update(provider_pubkey.as_bytes()),
    }
    hasher.update(udid.as_bytes());
    hex::encode(hasher.finalize())[..16].to_owned()
}

/// Every host-local path the device feature uses, rooted at
/// `<CSP state>/device`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevicePaths {
    root: PathBuf,
}

impl DevicePaths {
    /// Paths under `<state_dir>/device`.
    pub fn new(state_dir: &Path) -> Self {
        Self {
            root: state_dir.join("device"),
        }
    }

    /// `<CSP state>/device`.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Pinned tool installs: `<root>/tools`.
    pub fn tools_dir(&self) -> PathBuf {
        self.root.join("tools")
    }

    /// agent-device's own state (daemon.json, sessions): `<root>/agent-device`.
    pub fn agent_device_state_dir(&self) -> PathBuf {
        self.root.join("agent-device")
    }

    /// The durable once-only journal: `<root>/journal.json`.
    pub fn journal_file(&self) -> PathBuf {
        self.root.join("journal.json")
    }

    /// One session's directory: `<root>/<session>`. The session id is a
    /// provider-minted UUID; it is a directory name here and nothing more.
    pub fn session_dir(&self, session_id: &str) -> PathBuf {
        self.root.join(sanitize(session_id))
    }

    /// The slot file a seat reads: `<root>/<session>/<slot>.json`.
    pub fn slot_file(&self, session_id: &str, slot: &str) -> PathBuf {
        self.session_dir(session_id)
            .join(format!("{}.json", sanitize(slot)))
    }

    /// The agent-device `--config` file for a slot (daemon URL and token).
    pub fn agent_device_config(&self, session_id: &str, slot: &str) -> PathBuf {
        self.session_dir(session_id)
            .join(format!("{}.agent-device.json", sanitize(slot)))
    }

    /// Where a slot's durable snapshots are written:
    /// `<root>/<session>/shots/<slot>`.
    pub fn shot_dir(&self, session_id: &str, slot: &str) -> PathBuf {
        self.session_dir(session_id)
            .join("shots")
            .join(sanitize(slot))
    }

    /// The per-session shim directory prepended to the seat's PATH.
    pub fn shim_dir(&self, session_id: &str) -> PathBuf {
        self.session_dir(session_id).join("bin")
    }
}

fn sanitize(value: &str) -> String {
    value
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// What the slot file holds. Host-local only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SlotFile {
    /// [`SLOT_FILE_VERSION`].
    pub version: u32,
    /// The opaque slot id the wire uses.
    pub slot: String,
    /// The simulator UDID.
    pub udid: String,
    /// `ios`.
    pub platform: String,
    /// The model name.
    pub model: String,
    /// `iOS 27.0`.
    pub os_version: String,
    /// The agent-device session name the shim passes (`--session`).
    pub session_name: String,
    /// The agent-device `--config` file, when the daemon is running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_device_config: Option<PathBuf>,
    /// The loopback port of the agent-device daemon, when running.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub daemon_port: Option<u16>,
    /// The Node the shim runs agent-device with (read + exec for the seat).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<PathBuf>,
    /// Where `bee device screenshot` finds the PNG the provider wrote.
    pub shot_dir: PathBuf,
}

/// Write `bytes` to `path` atomically with mode 0600, creating parents.
pub fn write_private(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| std::io::Error::other("a private file needs a parent directory"))?;
    fs::create_dir_all(parent)?;
    let tmp = parent.join(format!(
        ".{}.tmp-{}",
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default(),
        std::process::id()
    ));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options.open(&tmp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&tmp, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

/// Write the slot file for `slot` (0600).
pub fn write_slot_file(
    paths: &DevicePaths,
    session_id: &str,
    slot: &SlotFile,
) -> std::io::Result<PathBuf> {
    let path = paths.slot_file(session_id, &slot.slot);
    let json = serde_json::to_vec_pretty(slot).map_err(std::io::Error::other)?;
    write_private(&path, &json)?;
    fs::create_dir_all(&slot.shot_dir)?;
    Ok(path)
}

/// Read a slot file back.
pub fn read_slot_file(
    paths: &DevicePaths,
    session_id: &str,
    slot: &str,
) -> std::io::Result<SlotFile> {
    let bytes = fs::read(paths.slot_file(session_id, slot))?;
    serde_json::from_slice(&bytes).map_err(std::io::Error::other)
}

/// Remove a slot file (on close). Missing is fine.
pub fn remove_slot_file(paths: &DevicePaths, session_id: &str, slot: &str) {
    let _ = fs::remove_file(paths.slot_file(session_id, slot));
    let _ = fs::remove_file(paths.agent_device_config(session_id, slot));
}

/// Whether `text` contains a UUID-shaped run (8-4-4-4-12 hex).
pub fn contains_uuid_shape(text: &str) -> bool {
    let bytes = text.as_bytes();
    const GROUPS: [usize; 5] = [8, 4, 4, 4, 12];
    let total = 36;
    if bytes.len() < total {
        return false;
    }
    (0..=bytes.len() - total).any(|start| {
        if start > 0 && bytes[start - 1].is_ascii_hexdigit() {
            return false;
        }
        let mut at = start;
        for (index, len) in GROUPS.iter().enumerate() {
            if !bytes[at..at + len].iter().all(u8::is_ascii_hexdigit) {
                return false;
            }
            at += len;
            if index < GROUPS.len() - 1 {
                if bytes[at] != b'-' {
                    return false;
                }
                at += 1;
            }
        }
        bytes.get(at).is_none_or(|next| !next.is_ascii_hexdigit())
    })
}

/// The markers WIRE-C5 § 2 refuses anywhere on the six surface kinds, plus
/// `/tmp/` (a provider temp path is no more publishable than a home one).
const HOST_PATH_MARKERS: [&str; 8] = [
    "/Users/",
    "/home/",
    "/var/folders/",
    "/private/var/",
    "DerivedData",
    "CoreSimulator",
    "file://",
    "/tmp/",
];

/// Whether `text` names an absolute home or private path (WIRE-C5 § 2).
pub fn contains_host_path(text: &str) -> bool {
    HOST_PATH_MARKERS.iter().any(|marker| text.contains(marker))
}

/// Replace UUID-shaped runs and absolute paths in a free-text reason, so a
/// tool's error message can be published.
pub fn scrub_host_details(text: &str) -> String {
    text.split(' ')
        .map(|word| {
            if contains_uuid_shape(word) {
                "<device>".to_owned()
            } else if word.contains('/') && (word.starts_with('/') || contains_host_path(word)) {
                "<path>".to_owned()
            } else {
                word.to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Tags whose values are minted identifiers that legitimately look like
/// UUIDs: the channel (`h`), the generation target (`cs-target`, whose
/// session id is a provider-minted UUID), the lifecycle command that minted
/// it (`csl-command`) and the caller's command id (`sdv-cmd`) — the same set
/// `beekeeper_core::session_device` exempts (`MINTED_ID_TAGS`).
const IDENTIFIER_TAGS: [&str; 4] = ["h", "cs-target", "csl-command", "sdv-cmd"];

/// Refuse an event that would publish a host-local fact: a UUID-shaped value
/// outside the identifier tags, or an absolute host path anywhere. Run on
/// every event this module signs, before it is sent.
pub fn assert_publishable(tags: &[Vec<String>], content: &str) -> Result<(), String> {
    if contains_uuid_shape(content) || contains_host_path(content) {
        return Err("device event content carries a host-local value".into());
    }
    for tag in tags {
        let Some(name) = tag.first() else { continue };
        for value in tag.iter().skip(1) {
            if contains_host_path(value) {
                return Err(format!("device event tag {name} carries a host path"));
            }
            if !IDENTIFIER_TAGS.contains(&name.as_str()) && contains_uuid_shape(value) {
                return Err(format!(
                    "device event tag {name} carries a UUID-shaped value"
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slot_ids_are_16_hex_stable_and_never_the_udid() {
        let pk = "a".repeat(64);
        let id = slot_id(&pk, "70E4C638-DE97-4AB1-990D-D0FC30018372");
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(id, slot_id(&pk, "70E4C638-DE97-4AB1-990D-D0FC30018372"));
        assert_ne!(
            id,
            slot_id(&"b".repeat(64), "70E4C638-DE97-4AB1-990D-D0FC30018372")
        );
        assert!(!contains_uuid_shape(&id));
        let keys = nostr::Keys::generate();
        assert_eq!(
            slot_id(
                &keys.public_key().to_hex(),
                "70E4C638-DE97-4AB1-990D-D0FC30018372"
            ),
            beekeeper_core::session_device::device_slot_id(
                &keys.public_key(),
                "70E4C638-DE97-4AB1-990D-D0FC30018372"
            ),
            "the same derivation as lane P's canonical one"
        );
    }

    #[test]
    fn the_slot_file_is_private_and_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let paths = DevicePaths::new(dir.path());
        let slot = SlotFile {
            version: SLOT_FILE_VERSION,
            slot: "0123456789abcdef".into(),
            udid: "70E4C638-DE97-4AB1-990D-D0FC30018372".into(),
            platform: "ios".into(),
            model: "iPhone 17".into(),
            os_version: "iOS 27.0".into(),
            session_name: "bk-0123456789abcdef".into(),
            agent_device_config: None,
            daemon_port: Some(4123),
            node: None,
            shot_dir: paths.shot_dir("sess", "0123456789abcdef"),
        };
        let path = write_slot_file(&paths, "sess", &slot).expect("write");
        assert_eq!(
            read_slot_file(&paths, "sess", &slot.slot).expect("read"),
            slot
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("meta").permissions().mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        assert!(slot.shot_dir.is_dir());
        remove_slot_file(&paths, "sess", &slot.slot);
        assert!(!path.exists());
    }

    #[test]
    fn uuid_shapes_are_found_only_when_whole() {
        assert!(contains_uuid_shape(
            "udid 70E4C638-DE97-4AB1-990D-D0FC30018372 here"
        ));
        assert!(contains_uuid_shape("3f2a1c9e-1111-4222-8333-444455556666"));
        assert!(!contains_uuid_shape("0123456789abcdef"));
        assert!(!contains_uuid_shape("a".repeat(64).as_str()));
        assert!(!contains_uuid_shape("70E4C638-DE97-4AB1-990D-D0FC3001837"));
    }

    #[test]
    fn reasons_are_scrubbed_and_events_are_guarded() {
        let scrubbed = scrub_host_details(
            "Invalid device 70E4C638-DE97-4AB1-990D-D0FC30018372 at /Users/brian/Library",
        );
        assert_eq!(scrubbed, "Invalid device <device> at <path>");
        let channel = "3f2a1c9e-1111-4222-8333-444455556666".to_owned();
        let ok = vec![
            vec!["h".to_owned(), channel.clone()],
            vec!["d".into(), "0123456789abcdef".into()],
        ];
        assert!(assert_publishable(&ok, "{\"type\":\"state\"}").is_ok());
        let bad = vec![vec!["d".to_owned(), channel]];
        assert!(assert_publishable(&bad, "").is_err());
        assert!(assert_publishable(&ok, "/Users/brian/x.png").is_err());
    }
}
