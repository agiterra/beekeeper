//! The git hooks a checkout gets so its local commits reach Project Pulse
//! without anybody being asked to report them.
//!
//! Brian's ruling for this lane: **nothing in Pulse's data path may depend on
//! anyone being asked to report.** So the hire host installs two hooks into the
//! seat's own checkout and the agent does nothing but `git commit`. The commit
//! lands on a `refs/heads/wip/<role>/<slug>` ref, and Pulse reads the ref.
//!
//! This module is the single source of both hooks' bytes and of every config
//! line that arms them. It performs no I/O: it plans, and the caller writes.
//! Two installers use it — the Desktop hire host
//! (`desktop/src-tauri/src/commands/coding_session_seat_hooks.rs`) and, for
//! people, `lefthook.yml`'s `post-commit` section running the same scripts from
//! `scripts/`. The `WIP_POST_COMMIT_HOOK` / `WIP_PREPARE_COMMIT_MSG_HOOK`
//! constants are `include_str!`s of those very files, so the two installers
//! cannot drift apart.
//!
//! Two rules run through everything here:
//!
//! * **The ref name is derived, never read from agent text.** It comes from the
//!   seat's role and the assignment id the host already holds, sanitised to
//!   `[a-z0-9-]`. An agent cannot name the ref its work is force-pushed to.
//! Pruning wip refs is **not** here: `bee pulse prune-wip`
//! (`crates/beekeeper-cli/src/commands/wip_refs.rs`) owns that plan, so there is one
//! answer to "which refs go" rather than two.
//!
//! * **No config line ever names a remote.** CLAUDE.md is explicit: never
//!   hard-code a remote name in tooling — two pre-push guards did and both
//!   broke silently the day the remote names moved. The hook resolves the
//!   remote at run time from git's own push configuration.

/// The exact bytes of `scripts/wip-post-commit.sh`.
///
/// Compiled in rather than read at run time so the seat installer and lefthook
/// provably write the same script; `seat_git_hooks_tests.rs` asserts the
/// identity so a future edit to either side fails the build's tests.
pub const WIP_POST_COMMIT_HOOK: &str = include_str!("../../../scripts/wip-post-commit.sh");

/// The exact bytes of `scripts/wip-prepare-commit-msg.sh`.
///
/// See [`WIP_POST_COMMIT_HOOK`] for why this is compiled in.
pub const WIP_PREPARE_COMMIT_MSG_HOOK: &str =
    include_str!("../../../scripts/wip-prepare-commit-msg.sh");

// The namespace, the retention window and the namespace predicate have one
// definition, in `pulse_mission`, because Pulse's renderer, the CLI's prune
// planner (`crates/beekeeper-cli/src/commands/wip_refs.rs`) and this module must not
// be able to disagree about what a wip ref is. Re-exported here so an installer
// needs only this module.
pub use crate::pulse_mission::{is_wip_ref, WIP_REF_PREFIX, WIP_REF_RETENTION_DAYS};

/// Longest a single derived ref segment may be, in bytes.
const MAX_SEGMENT_BYTES: usize = 40;

/// The file name of the post-commit hook, as git spells it.
const POST_COMMIT_HOOK_NAME: &str = "post-commit";

/// The file name of the prepare-commit-msg hook, as git spells it.
const PREPARE_COMMIT_MSG_HOOK_NAME: &str = "prepare-commit-msg";

/// Mode every installed hook file gets: readable and executable by the owner's
/// git, writable by nobody else.
const HOOK_MODE: u32 = 0o755;

/// One `git config` line the installer should apply to the seat's own scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatGitHookConfigLine {
    /// The config key, e.g. `buzz.wipRef`.
    pub key: String,
    /// The value to set, applied verbatim as a single argv value.
    pub value: String,
}

/// One hook file to write into a checkout's hooks directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatGitHookFile {
    /// The file name git dispatches on, e.g. `post-commit`.
    pub name: &'static str,
    /// The complete file contents.
    pub contents: &'static str,
    /// The POSIX mode the file needs to be executable (`0o755`).
    pub mode: u32,
}

/// Everything an installer must write for a checkout to share its commits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatGitHookPlan {
    /// The hook files, in the order they should be written.
    pub hooks: Vec<SeatGitHookFile>,
    /// The config lines, in the order they should be applied.
    pub config: Vec<SeatGitHookConfigLine>,
}

/// What the hire host knows about a seat when it cuts the seat's worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatGitHookRequest {
    /// The seat's role word, e.g. `refuter`. Sanitised into the ref name.
    pub seat_role: String,
    /// The seat's own pubkey, 64 lowercase hex — what it signs commits as.
    pub seat_pubkey: String,
    /// Path to the seat's Nostr key file, for `nostr.keyfile`.
    /// Where the seat's own key lives, when it lives in a file at all.
    ///
    /// `None` is the ordinary case today, and it is not a degradation: the
    /// Desktop hire host writes the seat's secret into a transient
    /// `actor-seats.json` the provider consumes and deletes, and the provider
    /// then injects it as `$NOSTR_PRIVATE_KEY`, which both `git-sign-nostr` and
    /// `git-credential-nostr` read **before** any `nostr.keyfile`. There is no
    /// stable file to point at, and the only one on the machine holds the
    /// *operator's* identity — a seat signing with that would be a forged
    /// attribution.
    ///
    /// With `None` the plan omits all five signing lines rather than pointing
    /// `nostr.keyfile` at a path that does not exist: `commit.gpgsign = true`
    /// over an unreachable key fails **every** commit the seat makes, which is
    /// strictly worse than an unsigned shared commit.
    pub keyfile_path: Option<String>,
    /// The signing program for `gpg.x509.program`, normally `git-sign-nostr`.
    pub signer_program: String,
    /// The hire event this seat answers, 64 lowercase hex, when there is one.
    pub assignment_id: Option<String>,
    /// The coding session this seat belongs to, for the wip checkpoint.
    pub session_ref: Option<String>,
    /// The session genesis, for the wip checkpoint.
    pub genesis_ref: Option<String>,
    /// The channel the checkpoint is published to.
    pub channel_id: Option<String>,
    /// The branch the seat's worktree was cut on, used only as a fallback name.
    pub branch: Option<String>,
}

/// True when `value` is exactly 64 lowercase hex characters.
fn is_lower_hex_64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Reduce arbitrary text to one ref segment: lowercase `[a-z0-9-]`, runs of
/// `-` collapsed, trimmed at both ends, bounded to [`MAX_SEGMENT_BYTES`].
///
/// This is the whole of the "derived, never agent text" guarantee: whatever
/// reaches the ref name goes through here first, so no input can introduce a
/// `/`, a `..`, a leading `-`, or a name of unbounded length.
fn sanitize_segment(value: &str) -> String {
    let mut out = String::with_capacity(value.len().min(MAX_SEGMENT_BYTES * 2));
    for byte in value.bytes() {
        let lowered = byte.to_ascii_lowercase();
        let mapped = if lowered.is_ascii_lowercase() || lowered.is_ascii_digit() {
            lowered as char
        } else {
            '-'
        };
        if mapped == '-' && (out.is_empty() || out.ends_with('-')) {
            continue;
        }
        out.push(mapped);
    }
    out.truncate(MAX_SEGMENT_BYTES);
    while out.ends_with('-') {
        out.pop();
    }
    out
}

/// Build `refs/heads/wip/<role>/<slug>` from a role and an assignment slug.
///
/// **The ref name is derived, never read from agent text.** Both segments are
/// sanitised to `[a-z0-9-]` — lowercased, every other byte turned into `-`,
/// runs collapsed, ends trimmed, each bounded to 40 bytes — so nothing an agent
/// can write reaches git as a ref name. A segment that sanitises to nothing is
/// an error rather than a ref with an empty component.
pub fn wip_ref_name(role: &str, assignment_slug: &str) -> Result<String, String> {
    let role = sanitize_segment(role);
    if role.is_empty() {
        return Err("a wip ref role is empty once sanitised".to_string());
    }
    let slug = sanitize_segment(assignment_slug);
    if slug.is_empty() {
        return Err("a wip ref assignment slug is empty once sanitised".to_string());
    }
    Ok(format!("{WIP_REF_PREFIX}{role}/{slug}"))
}

/// The slug half of a wip ref: the first 8 characters of a lowercase 64-hex
/// assignment id, else the sanitised branch name.
///
/// A malformed id is not an id — it falls through to the branch rather than
/// failing, which is what the shell hook does too. With neither, there is
/// nothing to name a ref after and this is an error.
pub fn wip_assignment_slug(
    assignment_id: Option<&str>,
    branch: Option<&str>,
) -> Result<String, String> {
    if let Some(id) = assignment_id {
        if is_lower_hex_64(id) {
            return Ok(id[..8].to_string());
        }
    }
    if let Some(branch) = branch {
        let slug = sanitize_segment(branch);
        if !slug.is_empty() {
            return Ok(slug);
        }
    }
    Err("no assignment id and no branch to derive a wip ref from".to_string())
}

/// Build one config line.
fn line(key: &str, value: &str) -> SeatGitHookConfigLine {
    SeatGitHookConfigLine {
        key: key.to_string(),
        value: value.to_string(),
    }
}

/// A non-empty trimmed value, or nothing. An `Some("")` is treated as unset:
/// a config line whose value is the empty string tells a reader something is
/// configured when nothing is.
fn present(value: Option<&String>) -> Option<&str> {
    value.map(|value| value.trim()).filter(|v| !v.is_empty())
}

/// Everything a seat's checkout needs so its commits reach Pulse on their own.
///
/// Validates the request, derives the wip ref, and returns both hook files plus
/// the config lines that arm them — signing identity first, then the `buzz.*`
/// lines the hooks read. Optional lines appear only when the corresponding
/// request field is set: a request with no assignment id yields **no**
/// `buzz.assignmentId` line, so `wip-prepare-commit-msg.sh` invents no trailer.
///
/// **No line names a remote.** The post-commit hook resolves the remote from
/// git's own push configuration when it runs, because a remote name written
/// down today is a guard that breaks silently the day the names move.
///
/// # Errors
///
/// Returns the refusal sentence when the seat pubkey or a present assignment id
/// is not 64 lowercase hex, when the keyfile path or signer program is blank,
/// or when neither the assignment id nor the branch can name a ref.
pub fn plan_seat_git_hooks(request: &SeatGitHookRequest) -> Result<SeatGitHookPlan, String> {
    if !is_lower_hex_64(&request.seat_pubkey) {
        return Err("seat pubkey must be 64 lowercase hex characters".to_string());
    }
    if let Some(id) = present(request.assignment_id.as_ref()) {
        if !is_lower_hex_64(id) {
            return Err("assignment id must be 64 lowercase hex characters".to_string());
        }
    }
    let keyfile = request
        .keyfile_path
        .as_deref()
        .map(str::trim)
        .filter(|path| !path.is_empty());
    if keyfile.is_some() && request.signer_program.trim().is_empty() {
        return Err("a seat needs a signer program to sign with".to_string());
    }

    let role = sanitize_segment(&request.seat_role);
    if role.is_empty() {
        return Err("a wip ref role is empty once sanitised".to_string());
    }
    let slug = wip_assignment_slug(
        present(request.assignment_id.as_ref()),
        present(request.branch.as_ref()),
    )?;
    let ref_name = wip_ref_name(&role, &slug)?;

    // Signing is all-or-nothing: five lines together, or none of them. A
    // partial set is the shape that breaks every commit.
    let mut config = Vec::new();
    if let Some(keyfile) = keyfile {
        config.extend([
            line("gpg.format", "x509"),
            line("gpg.x509.program", request.signer_program.trim()),
            line("commit.gpgsign", "true"),
            line("user.signingkey", &request.seat_pubkey),
            line("nostr.keyfile", keyfile),
        ]);
    }
    config.extend([
        line("buzz.wipShare", "true"),
        line("buzz.seatRole", &role),
        line("buzz.wipRef", &ref_name),
    ]);
    for (key, value) in [
        ("buzz.assignmentId", present(request.assignment_id.as_ref())),
        ("buzz.sessionRef", present(request.session_ref.as_ref())),
        ("buzz.genesisRef", present(request.genesis_ref.as_ref())),
        ("buzz.channel", present(request.channel_id.as_ref())),
    ] {
        if let Some(value) = value {
            config.push(line(key, value));
        }
    }

    Ok(SeatGitHookPlan {
        hooks: vec![
            SeatGitHookFile {
                name: POST_COMMIT_HOOK_NAME,
                contents: WIP_POST_COMMIT_HOOK,
                mode: HOOK_MODE,
            },
            SeatGitHookFile {
                name: PREPARE_COMMIT_MSG_HOOK_NAME,
                contents: WIP_PREPARE_COMMIT_MSG_HOOK,
                mode: HOOK_MODE,
            },
        ],
        config,
    })
}

#[cfg(test)]
#[path = "seat_git_hooks_tests.rs"]
mod tests;
