//! Provider-authored wire payloads: receipts (44224), metadata (44223), and
//! transcript envelopes (44225).
//!
//! The definitions live in [`buzz_core::coding_session_payload`] so the readers
//! of these facts — `buzz sessions`, and anything else that has to agree with
//! the consumer's exact key sets — share them without depending on this crate
//! and the ACP stack behind it. This module stays as the provider's name for
//! them.

pub use buzz_core::coding_session_payload::*;
