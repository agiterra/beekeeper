//! A hired seat's create hint, staged from the seat's own worktree record.
//!
//! Control run 7 (2026-09-24, session `beb51e9d…`): the hire cut and recorded
//! `…-wt-coding-session-builder-1`, staged its path as a one-shot hint, and the
//! builder still started in the project's own checkout. The hint map was at its
//! 64-entry cap and evicted the key that sorts first — which was the hint just
//! staged — so the provider fell through to `projects[projectRef]`. The seat
//! then shared the operator's tree, the verifier hire after it was refused
//! `SEAT_CWD_SHARED`, and the write fence written into the root denied the
//! builder's own worktree.
//!
//! The rule here is the host half of the invariant: a hired seat's cwd is the
//! path of its own `worktrees/<sessionRef>/<Label>` record, never the project
//! root and never another seat's tree, and a hire with no record is refused by
//! name rather than sent anywhere else. The provider enforces the other half.

use std::path::{Path, PathBuf};

use super::{seat_worktree_key, CodingSessionWorkdirStore};

/// A hired seat has no worktree record on this host to be staged into.
pub(crate) const SEAT_CWD_UNRECORDED: &str = "SEAT_CWD_UNRECORDED";

/// A hired seat's recorded tree is a project's (or channel's) own checkout.
pub(crate) const SEAT_CWD_PROJECT_ROOT: &str = "SEAT_CWD_PROJECT_ROOT";

/// A hired seat's recorded tree is another seat's recorded tree. Same code the
/// provider raises for the same condition (`beekeeper-session-provider`
/// `session::SEAT_CWD_SHARED`).
pub(crate) const SEAT_CWD_SHARED: &str = "SEAT_CWD_SHARED";

impl CodingSessionWorkdirStore {
    /// Stage `command_id`'s hint at the seat's own recorded worktree.
    ///
    /// Answers the staged path, or `"<CODE>: <reason>"` with nothing staged:
    /// [`SEAT_CWD_UNRECORDED`] when there is no record for
    /// `<session_ref>/<seat_label>`, [`SEAT_CWD_PROJECT_ROOT`] when the record
    /// names the repository root or any remembered project/channel checkout,
    /// and [`SEAT_CWD_SHARED`] when another seat's record names the same tree.
    pub(crate) fn stage_seat_hint_from_record(
        &mut self,
        command_id: &str,
        session_ref: &str,
        seat_label: &str,
        project_ref: Option<&str>,
    ) -> Result<PathBuf, String> {
        let key = seat_worktree_key(session_ref, seat_label);
        let Some(entry) = self.worktrees.get(&key) else {
            return Err(format!(
                "{SEAT_CWD_UNRECORDED}: this host has no worktree recorded for {key}, so the \
                 seat has no tree of its own to start in; refusing rather than starting it in \
                 the project's checkout"
            ));
        };
        let path = entry.path.clone();
        let project_root = project_ref
            .and_then(|project_ref| self.by_project.get(project_ref))
            .map(|entry| entry.path.as_path());
        let is_a_checkout = same_dir(&path, &entry.repo_root)
            || project_root.is_some_and(|root| same_dir(&path, root))
            || self
                .by_project
                .values()
                .chain(self.by_channel.values())
                .any(|remembered| same_dir(&path, &remembered.path));
        if is_a_checkout {
            return Err(format!(
                "{SEAT_CWD_PROJECT_ROOT}: the worktree recorded for {key} is {}, which is a \
                 project's own checkout, not a tree cut for this seat",
                path.display()
            ));
        }
        if let Some((other, _)) = self
            .worktrees
            .iter()
            .find(|(other, record)| **other != key && same_dir(&record.path, &path))
        {
            return Err(format!(
                "{SEAT_CWD_SHARED}: the worktree recorded for {key} is {}, which is also \
                 recorded for {other}",
                path.display()
            ));
        }
        self.stage_hint(command_id, path.clone());
        Ok(path)
    }
}

/// Component-wise, so `/a/b/` and `/a/b` are one directory, and canonical when
/// both exist, so a symlinked spelling of the same tree is caught too.
fn same_dir(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

#[cfg(test)]
#[path = "workdir_store_seat_hint_tests.rs"]
mod tests;
