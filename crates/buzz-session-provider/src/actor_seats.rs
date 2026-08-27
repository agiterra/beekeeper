//! Host-local custody of the key material an agent seat runs as.
//!
//! # Why a file, and why this one
//!
//! Plan D6: *key custody never crosses the wire*. A 44221 create names an
//! agent by pubkey (`actor`) and by the role it holds; it carries nothing that
//! could authenticate as that agent. The seat's `nsec` reaches the provider by
//! the same seam every other host-local fact does — a file the desktop writes
//! and the provider reads, keyed by `commandId`, exactly as
//! [`crate::commands::ProjectsFile::pending`] carries the working directory
//! chosen at create time.
//!
//! The two files are siblings on purpose. A working directory and a signing
//! key are the same *kind* of fact — machine-local execution state that a
//! signed event must never carry — so they arrive the same way, are keyed the
//! same way, and are refused the same way when absent. What differs is
//! lifetime and permissions: a projects entry may be re-read forever, while a
//! seat entry is a one-shot secret that is deleted as soon as the child that
//! needed it has been spawned, whether or not the spawn succeeded.
//!
//! # What this module refuses to do
//!
//! - It never puts an `nsec` in a [`std::fmt::Debug`] rendering. [`ActorSeat`]
//!   implements `Debug` by hand for that reason alone.
//! - It never logs a value, only a `commandId` and a path.
//! - It never hands key material to anything that could publish it: the seat
//!   is read inside the create path, converted straight into the child's
//!   environment, and dropped. Nothing derived from it reaches
//!   [`crate::state::SessionRecord`] or any signed payload.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One agent seat's host-local credentials, as the desktop wrote them.
#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActorSeat {
    /// The seat's public key, lowercase 64-hex.
    ///
    /// Checked against the create's `actor` before anything is spawned: an
    /// entry for the right `commandId` naming the wrong identity is a refusal,
    /// not a substitution.
    pub pubkey: String,
    /// The seat's secret key, in whatever form `Keys::parse` accepts.
    ///
    /// Never published, never logged, never persisted by this process.
    pub nsec: String,
    /// The seat's NIP-OA owner attestation, or `None` when it holds none.
    pub auth_tag: Option<String>,
    /// The relay the seat authenticates against.
    pub relay_url: String,
    /// Host-local path to the role pack this seat was launched from, when the
    /// launcher staged one.
    ///
    /// Not a secret and not on the wire: a pack is ordinary repo data, but the
    /// *path to it on this machine* is host-local in exactly the way a working
    /// directory is, so it travels by the same file rather than by the create.
    /// Its only use is [`crate::session::SeatSkills`] — materializing the
    /// pack's skills into the seat's working directory before the child is
    /// spawned.
    #[serde(default)]
    pub pack_dir: Option<PathBuf>,
    /// The persona within [`Self::pack_dir`] this seat runs as.
    ///
    /// Matched against `ResolvedPersona::name`. Absent (or absent alongside
    /// `pack_dir`) means "this seat has no pack" — the seat still runs, it
    /// simply materializes nothing.
    #[serde(default)]
    pub persona_id: Option<String>,
}

// The whole point of this type is that its second field never appears in a log
// line, and `#[derive(Debug)]` on a struct that holds an nsec is one `?seat`
// away from publishing it to the terminal.
impl std::fmt::Debug for ActorSeat {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActorSeat")
            .field("pubkey", &self.pubkey)
            .field("has_auth_tag", &self.auth_tag.is_some())
            .field("relay_url", &self.relay_url)
            .field("has_pack", &self.pack_coordinates().is_some())
            .finish_non_exhaustive()
    }
}

impl ActorSeat {
    /// The role pack this seat runs from, as `(pack directory, persona name)`.
    ///
    /// `None` unless the launcher staged both halves: a pack path with no
    /// persona names nothing, and a persona name with no pack has nowhere to
    /// read from, so a half-staged entry is treated as no pack at all rather
    /// than guessed at.
    pub fn pack_coordinates(&self) -> Option<(&Path, &str)> {
        match (self.pack_dir.as_deref(), self.persona_id.as_deref()) {
            (Some(dir), Some(persona)) if !persona.is_empty() => Some((dir, persona)),
            _ => None,
        }
    }

    /// The environment an actor seat's adapter is given **after** the fence.
    ///
    /// Exactly four variables and never a fifth: the signing key, the relay it
    /// signs against, the owner attestation when one exists, and the
    /// `NOSTR_PRIVATE_KEY` mirror the `bee` CLI and the SDK also read. The
    /// list is closed by design — the fence's value is that everything else in
    /// the `BUZZ_*` namespace stays removed, and an open-ended injection here
    /// would give it back one variable at a time.
    pub fn post_fence_env(&self) -> Vec<(String, String)> {
        let mut env = vec![
            ("BUZZ_PRIVATE_KEY".to_owned(), self.nsec.clone()),
            ("NOSTR_PRIVATE_KEY".to_owned(), self.nsec.clone()),
            ("BUZZ_RELAY_URL".to_owned(), self.relay_url.clone()),
        ];
        // Omitted, not emptied: an empty `BUZZ_AUTH_TAG` is a malformed tag,
        // and a seat with no attestation is a seat with no attestation.
        if let Some(auth_tag) = &self.auth_tag {
            env.push(("BUZZ_AUTH_TAG".to_owned(), auth_tag.clone()));
        }
        env
    }
}

/// The sibling of [`crate::commands::ProjectsFile`] that carries seat custody.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ActorSeatsFile {
    /// Format version. Reserved; unknown versions are still read best-effort.
    pub version: u32,
    /// One-shot seat credentials keyed by the create's `commandId`.
    pub pending: BTreeMap<String, ActorSeat>,
}

impl ActorSeatsFile {
    /// Read the file, treating every failure as "no entries".
    ///
    /// Same failure posture as the projects file, for the same reason: a
    /// missing or malformed custody file must degrade the affected creates
    /// into an `ACTOR_UNAVAILABLE` receipt the operator can act on, never take
    /// the provider down. The parse error is logged without its input, because
    /// the input is a secret.
    pub fn load(path: Option<&Path>) -> Self {
        let Some(path) = path else {
            return Self::default();
        };
        let body = match std::fs::read_to_string(path) {
            Ok(body) => body,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Self::default(),
            Err(error) => {
                tracing::warn!(target: "csp::seats", "cannot read {}: {error}", path.display());
                return Self::default();
            }
        };
        match serde_json::from_str::<Self>(&body) {
            Ok(file) => file,
            Err(_) => {
                // Deliberately no `{error}`: serde's message quotes the
                // offending input, and the offending input is an nsec.
                tracing::warn!(
                    target: "csp::seats",
                    "cannot parse {} — treating it as empty",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// The seat this create names, if this host holds it.
    pub fn seat(&self, command_id: &str) -> Option<&ActorSeat> {
        self.pending.get(command_id)
    }
}

/// Delete one seat entry, rewriting the file with owner-only permissions.
///
/// Called after the spawn attempt, success or failure: a seat entry is a
/// one-shot secret, and leaving it behind turns a create-time hand-off into a
/// credential at rest. Re-reads the file first so a concurrently written entry
/// for another create is preserved.
///
/// Best-effort by contract — the caller has already spawned (or failed to) and
/// must not fail a create because a cleanup write did not land. The error is
/// returned so the caller can log it; nothing about it is fatal.
pub fn consume_seat(path: Option<&Path>, command_id: &str) -> std::io::Result<()> {
    let Some(path) = path else {
        return Ok(());
    };
    let body = match std::fs::read_to_string(path) {
        Ok(body) => body,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    let mut file: ActorSeatsFileOwned = match serde_json::from_str(&body) {
        Ok(file) => file,
        // An unparseable custody file cannot be edited safely, and rewriting
        // it would destroy entries this process never understood.
        Err(_) => return Ok(()),
    };
    if file.pending.remove(command_id).is_none() {
        return Ok(());
    }
    let serialized = serde_json::to_string(&file)?;
    let temporary = path.with_extension("tmp");
    let mut options = OpenOptions::new();
    options.write(true).create(true).truncate(true);
    restrict_new_file(&mut options);
    {
        let mut handle = options.open(&temporary)?;
        handle.write_all(serialized.as_bytes())?;
        handle.sync_all()?;
    }
    restrict_file(&temporary)?;
    std::fs::rename(&temporary, path)?;
    Ok(())
}

/// The same file, re-serializable so [`consume_seat`] can write it back.
///
/// Separate from [`ActorSeatsFile`] because the read type must never gain a
/// `Serialize` impl: a serializable seat is one `serde_json::to_string` away
/// from a log line or a payload.
#[derive(Debug, Clone, Default, serde::Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
struct ActorSeatsFileOwned {
    version: u32,
    pending: BTreeMap<String, serde_json::Value>,
}

#[cfg(unix)]
fn restrict_new_file(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    options.mode(0o600);
}

#[cfg(not(unix))]
fn restrict_new_file(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn restrict_file(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
}

#[cfg(not(unix))]
fn restrict_file(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const NSEC: &str = "nsec1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqq";

    fn write_seats(dir: &Path, body: &str) -> std::path::PathBuf {
        let path = dir.join("actor-seats.json");
        std::fs::write(&path, body).expect("write seats");
        path
    }

    fn seats_body(command_id: &str) -> String {
        format!(
            r#"{{"version":1,"pending":{{"{command_id}":{{"pubkey":"{}","nsec":"{NSEC}","authTag":"[\"auth\"]","relayUrl":"wss://relay.example"}}}}}}"#,
            "cd".repeat(32)
        )
    }

    #[test]
    fn a_seat_is_read_by_command_id_and_yields_exactly_four_variables() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(dir.path(), &seats_body("create-1"));
        let file = ActorSeatsFile::load(Some(&path));
        let seat = file.seat("create-1").expect("the seat is held here");
        assert_eq!(seat.pubkey, "cd".repeat(32));
        assert!(file.seat("create-2").is_none());

        let env = seat.post_fence_env();
        let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "BUZZ_PRIVATE_KEY",
                "NOSTR_PRIVATE_KEY",
                "BUZZ_RELAY_URL",
                "BUZZ_AUTH_TAG"
            ]
        );
        assert_eq!(env[0].1, NSEC);
        assert_eq!(env[1].1, NSEC, "the NOSTR_PRIVATE_KEY mirror");
    }

    /// The launcher's pack coordinates survive the round trip, and a seat that
    /// names no pack simply has none.
    #[test]
    fn pack_coordinates_are_read_from_the_entry() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = format!(
            r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{}","nsec":"{NSEC}","relayUrl":"wss://relay.example","packDir":"/packs/roles","personaId":"builder"}}}}}}"#,
            "cd".repeat(32)
        );
        let path = write_seats(dir.path(), &body);
        let file = ActorSeatsFile::load(Some(&path));
        let seat = file.seat("create-1").expect("the seat is held here");

        let (pack_dir, persona) = seat.pack_coordinates().expect("pack coordinates");
        assert_eq!(pack_dir, Path::new("/packs/roles"));
        assert_eq!(persona, "builder");

        // Pack coordinates never join the closed four-variable environment.
        assert!(
            seat.post_fence_env()
                .iter()
                .all(|(name, value)| !name.contains("PACK") && value != "/packs/roles"),
            "the pack path leaked into the seat environment"
        );
    }

    /// Half a pack — a directory with no persona, or a persona with no
    /// directory — is no pack. Guessing which persona a pack means is how a
    /// seat ends up holding another role's craft.
    #[test]
    fn half_staged_pack_coordinates_are_no_pack() {
        let dir = tempfile::tempdir().expect("tempdir");
        for fragment in [
            r#","packDir":"/packs/roles""#,
            r#","personaId":"builder""#,
            r#","packDir":"/packs/roles","personaId":"""#,
            "",
        ] {
            let body = format!(
                r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{}","nsec":"{NSEC}","relayUrl":"wss://relay.example"{fragment}}}}}}}"#,
                "cd".repeat(32)
            );
            let path = write_seats(dir.path(), &body);
            let file = ActorSeatsFile::load(Some(&path));
            let seat = file.seat("create-1").expect("the seat is held here");
            assert!(
                seat.pack_coordinates().is_none(),
                "half-staged entry {fragment:?} was treated as a pack"
            );
        }
    }

    /// A seat with no attestation omits the variable rather than exporting an
    /// empty one — an empty `BUZZ_AUTH_TAG` is a malformed tag, not "no tag".
    #[test]
    fn a_seat_without_an_attestation_omits_the_variable() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(
            dir.path(),
            &format!(
                r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{}","nsec":"{NSEC}","authTag":null,"relayUrl":"wss://relay.example"}}}}}}"#,
                "cd".repeat(32)
            ),
        );
        let file = ActorSeatsFile::load(Some(&path));
        let env = file.seat("create-1").expect("seat").post_fence_env();
        assert_eq!(env.len(), 3);
        assert!(env.iter().all(|(name, _)| name != "BUZZ_AUTH_TAG"));
    }

    /// The one-shot rule: after the create that used it, the entry is gone and
    /// the file no longer holds the key anywhere.
    #[test]
    fn consuming_a_seat_removes_the_key_and_leaves_the_siblings() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(
            dir.path(),
            &format!(
                r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{pubkey}","nsec":"{NSEC}","authTag":null,"relayUrl":"wss://relay.example"}},"create-2":{{"pubkey":"{pubkey}","nsec":"{NSEC}","authTag":null,"relayUrl":"wss://relay.example"}}}}}}"#,
                pubkey = "cd".repeat(32)
            ),
        );
        consume_seat(Some(&path), "create-1").expect("consume");
        let body = std::fs::read_to_string(&path).expect("read back");
        assert!(!body.contains("create-1"));
        assert!(body.contains("create-2"), "a sibling seat was destroyed");
        assert_eq!(
            body.matches(NSEC).count(),
            1,
            "the consumed seat's key is still in the file"
        );

        let file = ActorSeatsFile::load(Some(&path));
        assert!(file.seat("create-1").is_none());
        assert!(file.seat("create-2").is_some());

        // Idempotent: a redelivery that consumes twice is not an error.
        consume_seat(Some(&path), "create-1").expect("second consume");
    }

    #[cfg(unix)]
    #[test]
    fn a_rewritten_custody_file_stays_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(dir.path(), &seats_body("create-1"));
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).expect("chmod");
        consume_seat(Some(&path), "create-1").expect("consume");
        let mode = std::fs::metadata(&path).expect("stat").permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "custody file mode {mode:o}");
    }

    /// A malformed custody file degrades to "no seats" — the create is refused
    /// with a code the operator can act on, and the provider stays up.
    #[test]
    fn a_malformed_custody_file_reads_as_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(dir.path(), "{not json");
        assert_eq!(ActorSeatsFile::load(Some(&path)), ActorSeatsFile::default());
        assert_eq!(ActorSeatsFile::load(None), ActorSeatsFile::default());
    }

    /// The redaction that keeps an nsec out of every `?seat` in this crate.
    #[test]
    fn a_seats_debug_rendering_never_contains_the_key() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(dir.path(), &seats_body("create-1"));
        let file = ActorSeatsFile::load(Some(&path));
        let rendered = format!("{:?}", file.seat("create-1").expect("seat"));
        assert!(!rendered.contains(NSEC), "{rendered}");
        assert!(!rendered.contains("nsec"), "{rendered}");
        assert!(format!("{file:?}").find(NSEC).is_none());
    }
}
