//! The git identity a seat's commits are authored as.
//!
//! # Why this is a host fact, not a question
//!
//! In the control run of 2026-09-22 a builder seat finished its code and then
//! stopped: its worktree carried no `user.name` and no `user.email` at any
//! scope, and the text it had been staged with told it to stop and ask when
//! that field was empty. Finished work sat for 49 minutes waiting for a
//! founder to answer a question that has exactly one correct answer, which the
//! host already knew (ledger 236(a), 239).
//!
//! A commit identity is derived, never negotiated. The seat's own key names
//! the author — the same `<pubkey8>@beekeeper.local` address every other
//! Beekeeper-authored commit on this machine already uses — and the seat's
//! role and project name it in a way a person reading `git log` can place.
//! Deriving it here, with no I/O and no clock, means the host's cut, the
//! host's hook installer and the app's own repository commits cannot end up
//! with three different opinions about what a Beekeeper commit is authored as.
//!
//! # What is deliberately *not* here
//!
//! The operator's own name and address. `~/.nostr/key` is a person's identity,
//! and a seat committing under it is a forged attribution — the same reason
//! [`crate::seat_git_hooks`] refuses to point a seat's `nostr.keyfile` at it.

/// The mail domain every Beekeeper-authored commit is addressed in.
///
/// Not a real mailbox and not meant to become one: it is a stable, obviously
/// local address derived from the *key* that authored the commit, so a commit
/// can be attributed long after a display name has changed.
pub const BEEKEEPER_LOCAL_DOMAIN: &str = "beekeeper.local";

/// The separator between a seat's role and its project in the author name.
///
/// A middle dot rather than a hyphen or a slash: role words and project slugs
/// both contain hyphens, and `git log --author` searches are easier when the
/// join character appears nowhere else in either half.
const NAME_SEPARATOR: &str = " · ";

/// The name a seat with no nameable project commits under.
const UNNAMED_PROJECT_ROLE_SUFFIX: &str = " seat";

/// The role word used when a seat's role is blank or unnameable.
const FALLBACK_ROLE: &str = "agent";

/// The git identity one seat commits under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatCommitIdentity {
    /// The value for `user.name`, e.g. `builder · kettle-control`.
    pub name: String,
    /// The value for `user.email`, always `<pubkey8>@beekeeper.local`.
    pub email: String,
}

/// The address a key authors Beekeeper commits under: `<pubkey8>@beekeeper.local`.
///
/// The **one** definition of that format. Every other site that needs it —
/// the app's own agents-repository commits, the packs repository, a seat's
/// worktree — calls this rather than spelling the format again, because two
/// spellings is how the same key ends up with two authors in one `git log`.
///
/// Total by construction: a hex string shorter than eight characters yields
/// whatever it has, which never happens for a real 64-hex key but keeps a
/// malformed one from panicking in a commit path.
#[must_use]
pub fn beekeeper_local_email(pubkey_hex: &str) -> String {
    let short: String = pubkey_hex.trim().chars().take(8).collect();
    format!("{short}@{BEEKEEPER_LOCAL_DOMAIN}")
}

/// The identity the host configures on a seat's worktree before the seat runs.
///
/// `role` is the seat's role word (`builder`, `verifier`, …) and `project` is
/// the project or session the seat was hired into, when the caller can name
/// one. The name reads `builder · kettle-control`, or `builder seat` when
/// there is no project to name — never a person's name, and never empty.
///
/// # Errors
///
/// Returns the refusal sentence when `pubkey_hex` is not 64 lowercase hex.
/// A seat whose key cannot be named has no author to derive, and guessing one
/// would attribute its commits to nothing.
pub fn seat_commit_identity(
    pubkey_hex: &str,
    role: &str,
    project: Option<&str>,
) -> Result<SeatCommitIdentity, String> {
    let pubkey = pubkey_hex.trim();
    if pubkey.len() != 64
        || !pubkey
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_uppercase())
    {
        return Err("a seat's commit identity needs its 64 lowercase hex pubkey".to_string());
    }
    // Lowercased: the role word is `builder` on the wire but a caller may hand
    // over a display-cased `Builder`, and one key must not end up with two
    // author names in one `git log`. The same normalisation
    // `seat_git_hooks::sanitize_segment` applies to the wip ref.
    let role = collapse(&role.to_lowercase());
    let role = if role.is_empty() {
        FALLBACK_ROLE.to_string()
    } else {
        role
    };
    let project = project.map(collapse).filter(|p| !p.is_empty());
    let name = match project {
        Some(project) => format!("{role}{NAME_SEPARATOR}{project}"),
        None => format!("{role}{UNNAMED_PROJECT_ROLE_SUFFIX}"),
    };
    Ok(SeatCommitIdentity {
        name,
        email: beekeeper_local_email(pubkey),
    })
}

/// One line of text: control characters and runs of whitespace become single
/// spaces, so a name can never carry a newline into a git config file.
fn collapse(value: &str) -> String {
    value
        .split_whitespace()
        .map(|word| word.chars().filter(|c| !c.is_control()).collect::<String>())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

#[cfg(test)]
#[path = "seat_commit_identity_tests.rs"]
mod tests;
