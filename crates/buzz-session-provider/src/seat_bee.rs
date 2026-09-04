//! One `bee` per seat, chosen by the host and disclosed on the wire.
//!
//! # The finding this exists for
//!
//! On 2026-09-01 a seat ran whatever `bee` its harness happened to put on
//! `PATH` — the desktop app's bundled sidecar, three fixes behind — and its
//! transcript read `OPERATION FETCH FAILED … a correction must preserve its
//! logical subject`, a defect that had already been fixed in the checkout. The
//! orchestrator's own shell meanwhile ran
//! `…/beekeeper/target/debug/bee`. One run, two binaries answering about one
//! channel, and neither of them said so. A path handed to a seat in prose was
//! honoured only sometimes, because prose is a request and `PATH` is a fact
//! (`docs/SESSION_STATE.md` item 103 finding 1; `LIVE-RUN-TeamRolesV1.md`
//! finding 13).
//!
//! # What this module does about it
//!
//! Two things, and deliberately not a third.
//!
//! 1. **The host chooses.** [`resolve_seat_bee`] picks exactly one binary —
//!    the sidecar beside the running executable, else the first `bee` on the
//!    inherited `PATH` — and [`compose_seat_path`] prepends that binary's own
//!    directory, once, to the `PATH` the seat runs with. Every other entry
//!    stays reachable, in order, behind it: we choose which `bee` answers, we
//!    do not confiscate the machine's own tools.
//! 2. **The host observes.** [`observe_bee_stamp`] runs `$BEE --version`
//!    itself and parses the answer into the wire's [`BeeStamp`]. Nothing is
//!    ever asked of the agent — an agent's account of which binary it ran is a
//!    claim, and every reporting failure in the live runs was an agent
//!    skipping or mis-stating exactly that kind of step.
//!
//! The third thing it does not do is fall back further. If neither the sidecar
//! nor `PATH` holds a `bee`, the seat gets no `BEE` and no prepended entry, and
//! the wire carries no stamp — an absent fact, not a guessed one.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use buzz_core::coding_session_payload::{BeeStamp, BeeStampSource};

/// The binary name the host resolves. Not configurable: a seat that could be
/// pointed at an arbitrary executable named `bee` is the ambiguity this module
/// removes.
pub const BEE_BINARY: &str = "bee";

/// The variable carrying the absolute path of the chosen binary.
///
/// Deliberately outside the `BUZZ_` prefix, because that is the name the packs
/// and the seat briefing say (`$BEE`). It therefore sits **outside** the
/// environment fence (`crate::agent_fence`), which covers `BUZZ_*` and would
/// otherwise strip it — and, for the same reason, an operator's ambient `BEE`
/// would survive the fence untouched. That is why this is injected
/// **post-fence**, in [`crate::actor_seats::ActorSeat::post_fence_env_with_bee`],
/// where `Command::env` overwrites rather than defers.
pub const BEE_ENV: &str = "BEE";

/// What the build stamps when it could not name a commit. Mirrors
/// `buzz_cli::build_provenance::UNKNOWN_STAMP`; the two must agree, because
/// this is the reader of what that writes.
const UNKNOWN_STAMP: &str = "unknown";

/// The suffix a build from a tree with uncommitted tracked changes carries.
/// Mirrors `buzz_cli::build_provenance::DIRTY_SUFFIX`.
const DIRTY_SUFFIX: &str = "-dirty";

/// Shortest commit abbreviation accepted from `bee --version`.
const MIN_STAMP_HEX: usize = 7;

/// Longest one: a full SHA-1 object name.
const MAX_STAMP_HEX: usize = 40;

/// Ceiling on the bytes read from `bee --version`.
///
/// A version line is tens of bytes. Anything larger is not a version line, and
/// reading it in full would let whatever is on `PATH` decide how much memory
/// the host spends before deciding the answer is unparseable.
const MAX_VERSION_OUTPUT_BYTES: usize = 4096;

/// The `bee` this host chose for its seats, and how it came to choose it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatBee {
    /// Absolute path to the binary.
    pub path: PathBuf,
    /// Which of the two resolution steps produced it.
    pub source: BeeStampSource,
}

impl SeatBee {
    /// The directory the chosen binary lives in — the one entry prepended to
    /// the seat's `PATH`.
    ///
    /// `None` only for a path with no parent, which cannot name an executable.
    pub fn directory(&self) -> Option<&Path> {
        self.path.parent()
    }
}

/// Choose one `bee`, in the order the finding dictates.
///
/// 1. `exe_parent/bee` — the sidecar beside the running executable. This is
///    the directory `build_augmented_path` already puts first
///    (`desktop/src-tauri/src/managed_agents/runtime/path.rs`, *"Why the app's
///    own directory comes first"*, which records the 2026-08-27
///    `~/.local/bin/bee` shadowing). What the app ships is what a seat runs.
/// 2. Failing that, the first `bee` on `inherited_path`, scanned left to
///    right — the same answer the seat's own shell would have reached.
///
/// There is no third step. `None` means this host holds no `bee` to give, and
/// the seat is left with the `PATH` it would have had.
///
/// `is_executable_file` is a parameter so the whole resolution is provable
/// over a directory layout a test owns, with no global `PATH` and no real
/// binaries.
pub fn resolve_seat_bee(
    exe_parent: Option<&Path>,
    inherited_path: Option<&OsString>,
    is_executable_file: &dyn Fn(&Path) -> bool,
) -> Option<SeatBee> {
    if let Some(parent) = exe_parent {
        let candidate = parent.join(BEE_BINARY);
        if is_executable_file(&candidate) {
            return Some(SeatBee {
                path: candidate,
                source: BeeStampSource::Bundled,
            });
        }
    }
    let inherited = inherited_path?;
    for entry in std::env::split_paths(inherited) {
        if entry.as_os_str().is_empty() {
            continue;
        }
        let candidate = entry.join(BEE_BINARY);
        if is_executable_file(&candidate) {
            return Some(SeatBee {
                path: candidate,
                source: BeeStampSource::Path,
            });
        }
    }
    None
}

/// [`resolve_seat_bee`] against this process's own executable and environment.
///
/// The provider is itself resolved the way every other Buzz binary is — a
/// bundled sidecar beside the app in a packaged build, `target/{debug,release}`
/// in a development one — so its own parent directory is exactly where the
/// `bee` this build produced sits, in both cases.
pub fn resolve_host_seat_bee() -> Option<SeatBee> {
    let exe = std::env::current_exe().ok();
    let exe_parent = exe.as_deref().and_then(Path::parent);
    let inherited = std::env::var_os("PATH");
    resolve_seat_bee(exe_parent, inherited.as_ref(), &is_executable_file)
}

/// The one resolution this process makes, and the one `--version` it runs.
///
/// Held in a [`OnceLock`](std::sync::OnceLock) because both halves are facts
/// about *this process* — its own executable's directory and its inherited
/// `PATH` — so re-deriving them per seat could only ever produce the same
/// answer, and re-running `--version` per seat would spawn a process per hire
/// to learn something already known. The work happens on the first seat start
/// and never again.
///
/// `None` means this host holds no `bee`: seats get no `BEE`, no prepended
/// entry, and their 44223 carries no `beeStamp`. An absent fact, not a guess.
///
/// Live-run finding 53 (2026-09-03): a seat's 44223 carried no `beeStamp` on
/// a build that resolves one, and the host log held not one line naming
/// `bee`, `seat_bee`, or `beeStamp` anywhere — the success branch below has
/// always logged which binary a seat gets, but nothing ever logged *this*
/// branch, so "found none" and "never asked" were the same silence on the
/// wire and in the log both. They no longer are.
pub fn host_seat_bee() -> Option<&'static (SeatBee, BeeStamp)> {
    static RESOLVED: std::sync::OnceLock<Option<(SeatBee, BeeStamp)>> = std::sync::OnceLock::new();
    RESOLVED
        .get_or_init(|| {
            let Some(bee) = resolve_host_seat_bee() else {
                tracing::warn!(
                    "no bee binary found beside this process or on its inherited PATH; \
                     seats get no BEE, no prepended PATH entry, and no beeStamp on their \
                     44223 until this host is restarted with one reachable"
                );
                return None;
            };
            let stamp = observe_bee_stamp(&bee);
            tracing::info!(
                path = %bee.path.display(),
                source = bee.source.as_wire(),
                version = stamp.version.as_deref().unwrap_or("unknown"),
                sha = stamp.sha.as_deref().unwrap_or("unknown"),
                "seats on this host run this bee"
            );
            Some((bee, stamp))
        })
        .as_ref()
}

/// Whether `path` is a file this process could execute.
///
/// Existence plus the owner/group/other execute bit on Unix; existence alone
/// elsewhere, where the bit does not decide.
pub fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The `PATH` a seat runs with: the chosen binary's directory, once, then
/// everything the host inherited.
///
/// *Once* is the whole point. Two entries for the same directory restore the
/// ambiguity this module removes — a later reader cannot tell which one the
/// resolution meant — so any further occurrence of the chosen directory is
/// dropped. Every **other** entry keeps its relative order, including a stale
/// `…/target/debug` that happens to hold a `bee` of its own: it simply no
/// longer answers `bee`, while `cargo`, `git` and everything else beside it
/// still resolve. We choose; we do not confiscate.
///
/// Returns `None` when there is nothing to compose — no directory and no
/// inherited `PATH` — so the caller sets no `PATH` at all rather than an empty
/// one, which would strip the child of every system binary.
pub fn compose_seat_path(
    chosen_directory: Option<&Path>,
    inherited_path: Option<&OsString>,
) -> Option<OsString> {
    let inherited: Vec<PathBuf> = inherited_path
        .map(|value| std::env::split_paths(value).collect())
        .unwrap_or_default();
    let mut entries: Vec<PathBuf> = Vec::with_capacity(inherited.len() + 1);
    if let Some(directory) = chosen_directory {
        entries.push(directory.to_path_buf());
    }
    for entry in inherited {
        if chosen_directory.is_some_and(|directory| directory == entry) {
            continue;
        }
        entries.push(entry);
    }
    if entries.is_empty() {
        return None;
    }
    // `join_paths` refuses an entry containing the separator. An inherited
    // `PATH` cannot hold one (it was produced by splitting on it) and the
    // chosen directory comes from `current_exe`, so the error arm is
    // unreachable in practice — and it is *reported* and the composition
    // dropped rather than panicked on, because a provider that aborts while
    // preparing a seat takes every live session with it.
    match std::env::join_paths(entries) {
        Ok(joined) => Some(joined),
        Err(error) => {
            tracing::warn!(
                %error,
                "seat PATH could not be composed; the seat keeps the host's own PATH"
            );
            None
        }
    }
}

/// The two variables a seat is given so that `$BEE` is a fact rather than a
/// request: the absolute path, and that path's directory prepended to `PATH`.
///
/// Both are returned as an overwriting post-fence pair. An operator's ambient
/// `BEE`, which the fence does not cover, is therefore replaced rather than
/// honoured — the host's choice is the seat's `bee`, whatever the shell that
/// launched the host believed.
pub fn seat_bee_env(bee: &SeatBee, inherited_path: Option<&OsString>) -> Vec<(String, String)> {
    let mut env = vec![(BEE_ENV.to_owned(), bee.path.to_string_lossy().into_owned())];
    if let Some(path) = compose_seat_path(bee.directory(), inherited_path) {
        env.push(("PATH".to_owned(), path.to_string_lossy().into_owned()));
    }
    env
}

/// Run `$BEE --version` and record what it said.
///
/// The host runs it, once, and reads the answer. A non-zero exit, a binary
/// that will not start, or output this parser does not recognise all produce
/// the same honest record — the path and the source it was chosen by, with
/// `version`, `sha` and `dirty` all `null` — and never a failed seat. A seat
/// that could not be stamped is still a seat; a seat that was refused because
/// its version string was odd would be a new way to lose a run.
pub fn observe_bee_stamp(bee: &SeatBee) -> BeeStamp {
    let output = Command::new(&bee.path).arg("--version").output();
    let parsed = match output {
        Ok(output) if output.status.success() => {
            let mut text = output.stdout;
            text.truncate(MAX_VERSION_OUTPUT_BYTES);
            String::from_utf8(text)
                .ok()
                .as_deref()
                .and_then(parse_bee_version)
        }
        Ok(output) => {
            tracing::warn!(
                path = %bee.path.display(),
                status = ?output.status.code(),
                "`bee --version` exited non-zero; the seat's build is recorded as unknown"
            );
            None
        }
        Err(error) => {
            tracing::warn!(
                path = %bee.path.display(),
                %error,
                "`bee --version` could not be run; the seat's build is recorded as unknown"
            );
            None
        }
    };
    let (version, sha, dirty) = match parsed {
        Some(parsed) => (Some(parsed.version), parsed.sha, parsed.dirty),
        None => (None, None, None),
    };
    BeeStamp {
        path: bee.path.to_string_lossy().into_owned(),
        source: bee.source,
        version,
        sha,
        dirty,
    }
}

/// What [`parse_bee_version`] recovered from one `--version` line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedBeeVersion {
    /// The crate version, e.g. `0.1.0`.
    pub version: String,
    /// The short commit, lowercase hex, without the `-dirty` suffix — or
    /// `None` when the build stamped `unknown`.
    pub sha: Option<String>,
    /// Whether the build carried uncommitted tracked changes. `None` exactly
    /// when `sha` is `None`: there is no commit for the flag to qualify.
    pub dirty: Option<bool>,
}

/// Parse the one line `bee --version` prints.
///
/// The shape is clap's `{name} {version}` over `buzz_cli::VERSION`, which is
/// `{crate version} ({stamp})` — so `bee 0.1.0 (23728227b)`,
/// `bee 0.1.0 (23728227b-dirty)`, or `bee 0.1.0 (unknown)`.
///
/// `None` means the output was not that. It is never partially believed: a
/// half-read version string is exactly the "comfortable guess" this whole
/// module exists to refuse, so an unrecognised line records unknown and says
/// so on both surfaces.
pub fn parse_bee_version(output: &str) -> Option<ParsedBeeVersion> {
    let line = output.lines().next()?.trim();
    let line = line
        .strip_prefix(BEE_BINARY)
        .map(str::trim_start)
        .unwrap_or(line);
    let (version, rest) = line.split_once(" (")?;
    let stamp = rest.strip_suffix(')')?;
    let version = version.trim();
    // A version is one token. Whitespace here means the line belongs to some
    // other tool that happens to end in a parenthesis — `some other tool 1.2.3
    // (deadbeef1)` would otherwise be read as a `bee` build, which is precisely
    // the wrong binary being believed.
    if version.is_empty() || version.contains(char::is_whitespace) || rest.contains(" (") {
        return None;
    }
    if stamp == UNKNOWN_STAMP {
        return Some(ParsedBeeVersion {
            version: version.to_owned(),
            sha: None,
            dirty: None,
        });
    }
    let (hex, dirty) = match stamp.strip_suffix(DIRTY_SUFFIX) {
        Some(hex) => (hex, true),
        None => (stamp, false),
    };
    if !is_commit_shaped(hex) {
        return None;
    }
    Some(ParsedBeeVersion {
        version: version.to_owned(),
        sha: Some(hex.to_owned()),
        dirty: Some(dirty),
    })
}

/// Whether a stamp names an abbreviated object id.
///
/// The same rule `buzz_cli::build_provenance::is_plausible_stamp` applies to
/// the value it writes. Lowercase only: uppercase is not the form this project
/// produces, and accepting it would mean a stamp that compares unequal to the
/// same commit everywhere else.
fn is_commit_shaped(value: &str) -> bool {
    (MIN_STAMP_HEX..=MAX_STAMP_HEX).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
#[path = "seat_bee_tests.rs"]
mod seat_bee_tests;
