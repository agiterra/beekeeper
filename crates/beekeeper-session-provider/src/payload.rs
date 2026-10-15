//! Provider-authored wire payloads: receipts (44224), metadata (44223), and
//! transcript envelopes (44225).
//!
//! The definitions live in [`beekeeper_core::coding_session_payload`] so the readers
//! of these facts — `bee sessions`, and anything else that has to agree with
//! the consumer's exact key sets — share them without depending on this crate
//! and the ACP stack behind it. This module stays as the provider's name for
//! them.

/// A same-owner sibling's NIP-OA attestation does not hold at the input's
/// authorization time (expired or not yet in force); the input is refused with
/// a receipt rather than dropped, so "not allowed" never reads as "not running".
pub const OWNER_ATTESTATION_EXPIRED: &str = "OWNER_ATTESTATION_EXPIRED";

pub use beekeeper_core::coding_session_payload::*;
