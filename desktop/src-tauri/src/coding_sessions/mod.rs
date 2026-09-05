//! Host-local coding-session state that never travels.
//!
//! Everything about a coding session that is *signed* lives in relay events.
//! Everything about where it runs on **this machine** lives here, and the two
//! must not meet: a working directory names a person's disk, and putting one
//! into an event would publish it to every member of the channel forever.
//!
//! The working-directory store was the first of these; the git worktree a
//! session may run in, and the model that names it, are the same shape. The
//! module boundary is the point. Anything else that is machine-local and
//! session-shaped belongs beside them, on the same side of that line.

pub(crate) mod naming;
pub(crate) mod workdir_store;
pub(crate) mod worktree;
// L11: what may be done with a worktree once its session is finished.
pub(crate) mod worktree_prune;
// P3: what the host itself does with one when the session closes.
pub(crate) mod worktree_close;

#[cfg(test)]
mod workdir_store_tests;
