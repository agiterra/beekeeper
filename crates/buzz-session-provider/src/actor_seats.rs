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

/// The wire's account of the role pack a seat was staged with — the kind:44223
/// `packRef` object, declared once in `buzz-core` and re-exported here so the
/// seat file this crate reads and the metadata it publishes cannot drift apart.
pub use buzz_core::coding_session_payload::PackRef;

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
    /// The agent's display name, when the launcher staged one.
    ///
    /// Not a secret and not on the wire — the desktop already publishes it as
    /// the agent's profile name. It is here for one purpose: a seated
    /// execution's `git` identity ([`Self::post_fence_env`]). Absent means the
    /// launcher had no name to give, and the seat commits under its role or,
    /// failing that, under its own pubkey.
    #[serde(default)]
    pub display_name: Option<String>,
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
    /// The wire's account of the pack, when the launcher staged one out of a
    /// project's packs repository.
    ///
    /// Unlike `pack_dir` this is *not* host-local: it names a repository, a
    /// commit, a role and a path, all of which mean the same thing on every
    /// machine. It is republished verbatim as the seat's kind:44223 `packRef`
    /// so a reader can answer "which pack ran" from the wire.
    ///
    /// Absent for a pack installed on the launching computer — there is no
    /// repository that can vouch for it, and naming one would be a proof of
    /// something that did not happen — and absent, of course, for every seat
    /// staged before this key existed.
    #[serde(default)]
    pub pack_ref: Option<PackRef>,
    /// This host's clone of the project's agents repository cut for this
    /// seat, and whether it may write there (spec § 4.11). Host-local, like
    /// `pack_dir`; it reaches the seat as a briefing paragraph and, on
    /// Claude, as a write-fence rule when the access is `read`.
    #[serde(default)]
    pub agents_checkout: Option<SeatAgentsCheckout>,
}

/// A seat's clone of the project's agents repository.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeatAgentsCheckout {
    /// Absolute path of the clone.
    pub path: PathBuf,
    /// `read` or `write`, as the role's `team.yml` entry granted it.
    pub access: String,
}

impl SeatAgentsCheckout {
    /// Whether the seat may commit and push there.
    pub fn writable(&self) -> bool {
        self.access.trim().eq_ignore_ascii_case("write")
    }
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
            .field("display_name", &self.display_name)
            .field("has_pack", &self.pack_coordinates().is_some())
            .finish_non_exhaustive()
    }
}

/// Domain of the address a seated execution commits from.
///
/// Deliberately not a mail host and deliberately not a domain this project
/// owns a mailbox on: a seat's commits must be attributable to the seat's
/// Nostr identity, and a plausible-looking address that silently bounces is
/// worse than one that is obviously synthetic. `git` requires *an* address;
/// this is the one that claims nothing.
pub const SEAT_EMAIL_DOMAIN: &str = "agents.beekeeper";

/// The variable `bee pulse` reads when `--project` is not passed.
///
/// Named here rather than inlined because the seat side and the CLI side must
/// agree exactly: `crates/buzz-cli/src/lib.rs` declares it as clap's
/// `env = "BUZZ_PULSE_PROJECT"` on every `pulse` subcommand.
pub const PULSE_PROJECT_ENV: &str = "BUZZ_PULSE_PROJECT";

/// How much of the seat's pubkey the address carries.
///
/// Long enough that two seats on one machine cannot collide by accident, short
/// enough to read in `git log`. The full key is on the wire in the session's
/// 44223 metadata; this is a handle for it, not a substitute.
const SEAT_EMAIL_PUBKEY_PREFIX: usize = 16;

impl ActorSeat {
    /// The `git` author/committer identity this seat commits under.
    ///
    /// Returns `(name, email)`. The name is the agent's display name when the
    /// launcher staged one, else the role it was seated with, else
    /// `agent-<pubkey-prefix>` — never the operator's, which is what an
    /// inherited `~/.gitconfig` would otherwise supply (ledger 77, *Fence*
    /// (b)). The address is `<pubkey-prefix>@`[`SEAT_EMAIL_DOMAIN`].
    pub fn git_identity(&self, role: Option<&str>) -> (String, String) {
        let prefix: String = self.pubkey.chars().take(SEAT_EMAIL_PUBKEY_PREFIX).collect();
        let name = [self.display_name.as_deref(), role]
            .into_iter()
            .flatten()
            .map(str::trim)
            .find(|candidate| !candidate.is_empty())
            .map(str::to_owned)
            .unwrap_or_else(|| format!("agent-{prefix}"));
        (name, format!("{prefix}@{SEAT_EMAIL_DOMAIN}"))
    }

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
    /// A closed list, in two halves.
    ///
    /// *Identity on the relay*: the signing key, the relay it signs against,
    /// the owner attestation when one exists, and the `NOSTR_PRIVATE_KEY`
    /// mirror the `bee` CLI and the SDK also read. The fence's value is that
    /// everything else in the `BUZZ_*` namespace stays removed, and an
    /// open-ended injection here would give it back one variable at a time.
    ///
    /// *Identity in the checkout*: the four `GIT_*` variables from
    /// [`Self::git_identity`]. `git` resolves author and committer from the
    /// environment before `~/.gitconfig`, so without them a seat's commits are
    /// signed off by whoever owns the home directory — the operator. That is
    /// the same class of untruth as a forged transcript: a diff attributed to
    /// a person who did not write it.
    ///
    /// `role` is the role this execution was seated with, used as the name
    /// when the launcher staged no display name.
    pub fn post_fence_env(&self, role: Option<&str>) -> Vec<(String, String)> {
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
        let (name, email) = self.git_identity(role);
        env.push(("GIT_AUTHOR_NAME".to_owned(), name.clone()));
        env.push(("GIT_AUTHOR_EMAIL".to_owned(), email.clone()));
        env.push(("GIT_COMMITTER_NAME".to_owned(), name));
        env.push(("GIT_COMMITTER_EMAIL".to_owned(), email));
        env
    }

    /// [`Self::post_fence_env`] plus the coordinate of the project this
    /// execution belongs to, when the umbrella named one.
    ///
    /// Ledger 80 (d): a seat had no project coordinate, so `bee pulse update`
    /// had nowhere to write and minted a private project of its own. Six
    /// entries the operator could never see is the same class of untruth as a
    /// status that reads Idle over a dead process — the writes appeared to
    /// land, and did, somewhere nobody was looking.
    ///
    /// `BUZZ_PULSE_PROJECT` is the variable `bee pulse` already reads for its
    /// `--project` flag (`buzz_cli::PulseCmd`), so passing it here targets the
    /// operator's project without teaching the seat a new flag. It rides the
    /// post-fence list because the fence strips the whole `BUZZ_*` namespace;
    /// an unseated execution never gets it, and neither does a seat whose
    /// umbrella has no project — an absent coordinate is absent, never
    /// guessed.
    ///
    /// A blank or whitespace-only `project_ref` is treated as no project: an
    /// empty `BUZZ_PULSE_PROJECT` would fill clap's `--project` with a
    /// coordinate that cannot resolve, which reads as a broken project rather
    /// than as no project at all.
    pub fn post_fence_env_in_project(
        &self,
        role: Option<&str>,
        project_ref: Option<&str>,
    ) -> Vec<(String, String)> {
        let mut env = self.post_fence_env(role);
        if let Some(project_ref) = project_ref.map(str::trim).filter(|it| !it.is_empty()) {
            env.push((PULSE_PROJECT_ENV.to_owned(), project_ref.to_owned()));
        }
        env
    }

    /// [`Self::post_fence_env_in_project`] plus the `bee` this host chose for
    /// its seats: `BEE` naming it absolutely, and its own directory prepended
    /// once to `PATH` ([`crate::seat_bee::seat_bee_env`]).
    ///
    /// Ledger item 103 finding 1: a seat ran whichever `bee` its harness put
    /// on `PATH` — on 2026-09-01 the app's bundled sidecar, three fixes
    /// behind — and a path handed to it in prose was honoured only sometimes,
    /// because prose is a request and `PATH` is a fact. The host resolves one
    /// binary and states it twice, so `$BEE` and a bare `bee` are the same
    /// binary and neither depends on the seat reading its instructions.
    ///
    /// It rides the **post-fence** list for a reason the fence itself makes
    /// necessary: `BEE` sits outside the `BUZZ_` prefix
    /// (`crate::agent_fence`), so an operator's ambient `BEE` survives the
    /// fence untouched. Post-fence injection overwrites it; an exemption
    /// would merely have widened the fence.
    ///
    /// `bee` of `None` — a host holding no `bee` at all — adds nothing, and
    /// the seat keeps exactly the environment it has today.
    pub fn post_fence_env_with_bee(
        &self,
        role: Option<&str>,
        project_ref: Option<&str>,
        bee: Option<&crate::seat_bee::SeatBee>,
        inherited_path: Option<&std::ffi::OsString>,
    ) -> Vec<(String, String)> {
        let mut env = self.post_fence_env_in_project(role, project_ref);
        if let Some(bee) = bee {
            env.extend(crate::seat_bee::seat_bee_env(bee, inherited_path));
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
    // The host stages/restages under this same stable sibling lock. Reading
    // before acquiring it could resurrect consumed entries or lose new seats.
    let mut lock_options = OpenOptions::new();
    lock_options
        .read(true)
        .write(true)
        .create(true)
        .truncate(false);
    restrict_new_file(&mut lock_options);
    let lock = match lock_options.open(path.with_extension("lock")) {
        Ok(lock) => lock,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    lock.lock()?;
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
    fn concurrent_consumers_do_not_resurrect_other_consumed_seats() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut body: serde_json::Value = serde_json::from_str(&seats_body("seed")).expect("json");
        let entry = body["pending"]["seed"].take();
        let pending = body["pending"].as_object_mut().expect("pending");
        pending.clear();
        for index in 0..16 {
            pending.insert(format!("command-{index}"), entry.clone());
        }
        let path = write_seats(dir.path(), &body.to_string());
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(16));
        let workers: Vec<_> = (0..16)
            .map(|index| {
                let path = path.clone();
                let barrier = barrier.clone();
                std::thread::spawn(move || {
                    barrier.wait();
                    consume_seat(Some(&path), &format!("command-{index}")).expect("consume");
                })
            })
            .collect();
        for worker in workers {
            worker.join().expect("worker");
        }
        assert!(ActorSeatsFile::load(Some(&path)).pending.is_empty());
    }

    #[test]
    fn a_seat_is_read_by_command_id_and_yields_exactly_the_closed_list() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(dir.path(), &seats_body("create-1"));
        let file = ActorSeatsFile::load(Some(&path));
        let seat = file.seat("create-1").expect("the seat is held here");
        assert_eq!(seat.pubkey, "cd".repeat(32));
        assert!(file.seat("create-2").is_none());

        let env = seat.post_fence_env(Some("builder"));
        let names: Vec<&str> = env.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            vec![
                "BUZZ_PRIVATE_KEY",
                "NOSTR_PRIVATE_KEY",
                "BUZZ_RELAY_URL",
                "BUZZ_AUTH_TAG",
                "GIT_AUTHOR_NAME",
                "GIT_AUTHOR_EMAIL",
                "GIT_COMMITTER_NAME",
                "GIT_COMMITTER_EMAIL",
            ]
        );
        assert_eq!(env[0].1, NSEC);
        assert_eq!(env[1].1, NSEC, "the NOSTR_PRIVATE_KEY mirror");
    }

    /// The `packRef` reaches the provider off the host-local file, and a seat
    /// staged before the key existed still decodes — the finding-31 rule: every
    /// reader accepts absence, and a `null` is refused like `beeStamp`'s.
    #[test]
    fn a_pack_ref_is_read_off_the_seat_and_its_absence_is_legal() {
        let dir = tempfile::tempdir().expect("tempdir");
        let with_pack = format!(
            r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{pubkey}","nsec":"{NSEC}","authTag":null,"relayUrl":"wss://relay.example","packDir":"/packs/aa-bb/personas/roles/builder","personaId":"builder","packRef":{{"repo":"30617:{pubkey}:packs","sha":"{sha}","role":"builder","path":"personas/roles/builder"}}}}}}}}"#,
            pubkey = "cd".repeat(32),
            sha = "ab".repeat(20),
        );
        let path = write_seats(dir.path(), &with_pack);
        let file = ActorSeatsFile::load(Some(&path));
        let seat = file.seat("create-1").expect("seat");
        let pack_ref = seat.pack_ref.clone().expect("the launcher staged one");
        assert_eq!(pack_ref.sha, "ab".repeat(20));
        assert_eq!(pack_ref.role, "builder");
        assert_eq!(pack_ref.path, "personas/roles/builder");
        assert_eq!(
            seat.pack_coordinates()
                .map(|(dir, persona)| (dir.to_string_lossy().into_owned(), persona.to_owned())),
            Some((
                "/packs/aa-bb/personas/roles/builder".to_owned(),
                "builder".to_owned()
            ))
        );

        // Signed before the key existed: absence is a legal seat, not an error.
        let older = write_seats(dir.path(), &seats_body("create-1"));
        let file = ActorSeatsFile::load(Some(&older));
        assert_eq!(file.seat("create-1").expect("seat").pack_ref, None);
    }

    /// The name falls back role → pubkey handle, and never to the operator's.
    #[test]
    fn a_seat_without_a_display_name_commits_under_its_role() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(dir.path(), &seats_body("create-1"));
        let seat_file = ActorSeatsFile::load(Some(&path));
        let seat = seat_file.seat("create-1").expect("seat");

        assert_eq!(seat.git_identity(Some("lead")).0, "lead");
        assert_eq!(
            seat.git_identity(Some("  ")).0,
            format!("agent-{}", "cd".repeat(8))
        );
        assert_eq!(
            seat.git_identity(None).0,
            format!("agent-{}", "cd".repeat(8))
        );
        assert_eq!(
            seat.git_identity(None).1,
            format!("{}@agents.beekeeper", "cd".repeat(8))
        );
    }

    /// Ledger 77 (Fence, b): a seat used to commit as the operator, because
    /// `git` fell back to the inherited `~/.gitconfig`. A seated execution
    /// gets its own four `GIT_*` variables — the agent's display name (or the
    /// role it holds) and a `<pubkey-prefix>@agents.beekeeper` address that is
    /// deliberately not a mailbox.
    #[test]
    fn a_seat_commits_as_itself_not_as_the_operator() {
        let dir = tempfile::tempdir().expect("tempdir");
        let body = format!(
            r#"{{"version":1,"pending":{{"create-1":{{"pubkey":"{pubkey}","nsec":"{NSEC}","relayUrl":"wss://relay.example","displayName":"Levain"}}}}}}"#,
            pubkey = "cd".repeat(32)
        );
        let path = write_seats(dir.path(), &body);
        let file = ActorSeatsFile::load(Some(&path));
        let seat = file.seat("create-1").expect("the seat is held here");

        let env = seat.post_fence_env(Some("builder"));
        let lookup = |name: &str| -> String {
            env.iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.clone())
                .unwrap_or_else(|| panic!("{name} was not exported for a seat"))
        };
        assert_eq!(lookup("GIT_AUTHOR_NAME"), "Levain");
        assert_eq!(lookup("GIT_COMMITTER_NAME"), "Levain");
        let expected_email = format!("{}@agents.beekeeper", "cd".repeat(8));
        assert_eq!(lookup("GIT_AUTHOR_EMAIL"), expected_email);
        assert_eq!(lookup("GIT_COMMITTER_EMAIL"), expected_email);
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
            seat.post_fence_env(Some("builder"))
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

    /// Ledger 80 (d): a seated execution used to reach `bee pulse update` with
    /// no project coordinate at all, so the lead minted a private project of
    /// its own and wrote six entries the operator could not see. The
    /// umbrella's `projectRef` now rides the post-fence list — and only when
    /// the umbrella actually has one.
    #[test]
    fn a_seated_execution_targets_the_umbrellas_project() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = write_seats(dir.path(), &seats_body("create-1"));
        let file = ActorSeatsFile::load(Some(&path));
        let seat = file.seat("create-1").expect("the seat is held here");
        let coordinate = format!("30621:{}:beekeeper", "ab".repeat(32));

        let targeted = seat.post_fence_env_in_project(Some("lead"), Some(&coordinate));
        assert_eq!(
            targeted
                .iter()
                .find(|(name, _)| name == PULSE_PROJECT_ENV)
                .map(|(_, value)| value.as_str()),
            Some(coordinate.as_str()),
            "a seat in a project must be able to write that project's pulse"
        );
        // Everything the fence-era list carried is still there, in order.
        assert_eq!(
            targeted[..targeted.len() - 1],
            seat.post_fence_env(Some("lead"))[..]
        );

        for absent in [None, Some(""), Some("   ")] {
            assert!(
                seat.post_fence_env_in_project(Some("lead"), absent)
                    .iter()
                    .all(|(name, _)| name != PULSE_PROJECT_ENV),
                "an umbrella with no project ({absent:?}) exported a coordinate anyway"
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
        let env = file
            .seat("create-1")
            .expect("seat")
            .post_fence_env(Some("builder"));
        assert!(env.iter().all(|(name, _)| name != "BUZZ_AUTH_TAG"));
        assert_eq!(
            env.iter()
                .filter(|(name, _)| name.starts_with("BUZZ_"))
                .count(),
            2,
            "only the key and the relay survive without an attestation"
        );
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
