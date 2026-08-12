//! Host-local coding-session state that never travels.
//!
//! Everything about a coding session that is *signed* lives in relay events.
//! Everything about where it runs on **this machine** lives here, and the two
//! must not meet: a working directory names a person's disk, and putting one
//! into an event would publish it to every member of the channel forever.
//!
//! Today that is exactly one thing — the working-directory store — but the
//! module boundary is the point. Anything else that is machine-local and
//! session-shaped belongs beside it, on the same side of that line.

pub(crate) mod workdir_store;

#[cfg(test)]
mod workdir_store_tests;
