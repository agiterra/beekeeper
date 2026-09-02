//! One name per thing: the four coding-session identity words.
//!
//! Four different strings name a provider execution, and three of the four
//! pairs of them look interchangeable to a reader and to `String`'s
//! `PartialEq`. They are not:
//!
//! | Type | What it is | The name it is **not** |
//! |---|---|---|
//! | [`ProviderInstanceAlias`] | the human-facing `providerInstanceRef` (`claude-primary`) | the instance id |
//! | [`ProviderInstanceId`] | the short id in every `cs-target` (`1958c6c448e05eed`) | the alias |
//! | [`RuntimeWord`] | `runtime` on kind 44223 (`claude`, `codex`) | the driver slug |
//! | [`DriverSlug`] | `driver` in a `cs-target` (`claude-agent-acp`, `codex-acp`) | the runtime word |
//!
//! The cost of not having these types is on the record. Ledger item 102: the
//! Desktop hire path compared a provider receipt's cryptographic
//! `cs-target.instanceId` against the human-facing `providerInstanceRef`
//! alias, the comparison could never be true, and a receipt-backed hire was
//! silently classified unbound. Both values are `String`, so nothing at all
//! objected. Here it is a type error, checked by the compile-fail doc tests on
//! [`ProviderInstanceAlias`] and [`RuntimeWord`].
//!
//! # These types do not tighten the wire
//!
//! Every [`from_wire`](ProviderInstanceAlias::from_wire) here accepts exactly
//! what the field it names already accepted — non-blank and bounded by the same
//! constant the hand-written validator used, and **nothing more** — so adopting
//! a newtype can never reject a signed event that decoded yesterday. That is
//! not a stylistic preference: `providerInstanceRef = "claude\tprimary"` and a
//! kind 44223 whose `provider` carries a tab are both signed shapes that decode
//! today, and a constructor that refused them would turn B2's field flip into a
//! wire-breaking change disguised as a refactor.
//!
//! Where a *canonical* shape exists but is not guaranteed on the wire, it is
//! exposed as a predicate — [`is_canonical`](ProviderInstanceAlias::is_canonical)
//! on all four, and [`ProviderInstanceId::is_short_pubkey_prefix`] — rather than
//! smuggled into the constructor. A provider whose operator set
//! `BUZZ_CSP_INSTANCE_ID` to `workstation-a` publishes exactly that, and a type
//! that refused to hold it would be lying about the wire rather than validating
//! it. **Report the odd shape; never refuse it.**

use std::fmt;

use serde::{Deserialize, Serialize};

use crate::coding_session_command::MAX_IDENTIFIER_BYTES;

/// Maximum bytes in a [`ProviderInstanceAlias`] or a [`RuntimeWord`].
///
/// The same 2 KiB the lifecycle command's `MAX_LIFECYCLE_REFERENCE_BYTES` and
/// the metadata payload's `MAX_METADATA_REFERENCE_BYTES` already enforce on
/// these exact fields, restated here so the newtype's bound and the field's
/// bound cannot drift apart.
pub const MAX_CODING_SESSION_REFERENCE_BYTES: usize = 2 * 1024;

/// Maximum bytes in a [`ProviderInstanceId`] or a [`DriverSlug`].
///
/// Both ride inside a `cs-target`, whose every string field is already bounded
/// by [`MAX_IDENTIFIER_BYTES`].
pub const MAX_CODING_SESSION_TARGET_IDENTIFIER_BYTES: usize = MAX_IDENTIFIER_BYTES;

/// Length in characters of the canonical short instance id: the first 16 hex
/// characters of the provider's own signing pubkey.
pub const SHORT_PUBKEY_PREFIX_LEN: usize = 16;

/// Shared shape rule for every identity word: `bounded_nonblank`, and nothing
/// more.
///
/// Non-blank after trimming, within `max` bytes. **Exactly** the two rules
/// `validate_required` (`coding_session_lifecycle_command.rs`), the metadata
/// validator (`coding_session_payload.rs`) and `bounded_nonblank`
/// (`coding_session_catalog.rs`) already apply to these fields — no third rule.
///
/// An earlier draft added a control-character check here. It was removed:
/// `providerInstanceRef = "claude\tprimary"` is a signed shape that decodes
/// today, and a newtype that refused it would have made B2's field flip a
/// wire-breaking change disguised as a refactor. The stricter shape is
/// *reported* by [`is_canonical`](ProviderInstanceAlias::is_canonical), never
/// refused.
fn validate_identity(field: &str, value: &str, max: usize) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be blank"));
    }
    if value.len() > max {
        return Err(format!(
            "{field} exceeds {max} bytes (got {} bytes)",
            value.len()
        ));
    }
    Ok(())
}

/// Generate one identity newtype with its constructor, accessors, and traits.
macro_rules! identity_newtype {
    (
        $(#[$meta:meta])*
        $name:ident, $field:literal, $max:expr
    ) => {
        $(#[$meta])*
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Validate one wire string and take ownership of it.
            ///
            /// Accepts exactly what the field this type names already
            /// accepted: non-blank and bounded, and nothing more. Adopting
            /// this newtype can therefore never reject a signed event that
            /// decoded yesterday.
            pub fn from_wire(value: impl Into<String>) -> Result<Self, String> {
                let value = value.into();
                validate_identity($field, &value, $max)?;
                Ok(Self(value))
            }

            /// Whether this value has the *canonical* shape for its field:
            /// free of control characters and of leading or trailing
            /// whitespace.
            ///
            /// A **disclosure, never a gate**. `false` means "the wire carries
            /// something odd here and a surface should say so"; it never means
            /// the value is invalid, because the wire has always accepted it.
            /// Refusing it in [`from_wire`](Self::from_wire) would break every
            /// signed event that already carries it.
            pub fn is_canonical(&self) -> bool {
                !self.0.chars().any(char::is_control) && self.0.trim() == self.0
            }

            /// The exact wire string, borrowed.
            pub fn as_str(&self) -> &str {
                &self.0
            }

            /// The exact wire string, owned.
            pub fn into_inner(self) -> String {
                self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
                formatter.write_str(&self.0)
            }
        }

        impl AsRef<str> for $name {
            fn as_ref(&self) -> &str {
                &self.0
            }
        }

        impl TryFrom<&str> for $name {
            type Error = String;

            fn try_from(value: &str) -> Result<Self, Self::Error> {
                Self::from_wire(value)
            }
        }

        impl TryFrom<String> for $name {
            type Error = String;

            fn try_from(value: String) -> Result<Self, Self::Error> {
                Self::from_wire(value)
            }
        }

        impl From<$name> for String {
            fn from(value: $name) -> Self {
                value.0
            }
        }
    };
}

identity_newtype!(
    /// The human-facing `providerInstanceRef` — `claude-primary`.
    ///
    /// **Not the instance id.** This is the alias an operator types, a catalog
    /// advertises (`coding_session_catalog::CatalogProvider::provider_instance_ref`),
    /// and a create or hire names. The value that identifies the same provider
    /// *cryptographically*, inside a `cs-target`, is a
    /// [`ProviderInstanceId`], and the two are never equal by accident:
    ///
    /// ```compile_fail
    /// use buzz_core::coding_session_identity::{ProviderInstanceAlias, ProviderInstanceId};
    /// let alias = ProviderInstanceAlias::from_wire("claude-primary").unwrap();
    /// let id = ProviderInstanceId::from_wire("1958c6c448e05eed").unwrap();
    /// // The comparison ledger item 102 shipped: rejected by the compiler here.
    /// let _ = alias == id;
    /// ```
    ProviderInstanceAlias,
    "providerInstanceRef",
    MAX_CODING_SESSION_REFERENCE_BYTES
);

identity_newtype!(
    /// The provider instance id carried by every `cs-target` —
    /// `1958c6c448e05eed`.
    ///
    /// **Not the alias.** By default it is the first
    /// [`SHORT_PUBKEY_PREFIX_LEN`] hex characters of the provider's own
    /// signing pubkey, which is why it looks cryptographic; an operator who
    /// sets `BUZZ_CSP_INSTANCE_ID` publishes whatever they set instead. Use
    /// [`is_short_pubkey_prefix`](Self::is_short_pubkey_prefix) to ask which
    /// of the two a given value is, and never assume.
    ProviderInstanceId,
    "cs-target.instanceId",
    MAX_CODING_SESSION_TARGET_IDENTIFIER_BYTES
);

identity_newtype!(
    /// The `runtime` word on kind 44223 metadata and in a catalog entry —
    /// `claude`, `codex`, `goose`.
    ///
    /// **Not the driver slug.** A runtime word names the agent product; the
    /// [`DriverSlug`] names the ACP adapter that drives it, and one runtime
    /// can sit behind more than one driver:
    ///
    /// ```compile_fail
    /// use buzz_core::coding_session_identity::{DriverSlug, RuntimeWord};
    /// let runtime = RuntimeWord::from_wire("claude").unwrap();
    /// let driver = DriverSlug::from_wire("claude-agent-acp").unwrap();
    /// let _ = runtime == driver;
    /// ```
    RuntimeWord,
    "runtime",
    MAX_CODING_SESSION_REFERENCE_BYTES
);

identity_newtype!(
    /// The `driver` slug in a `cs-target` — `claude-agent-acp`, `codex-acp`.
    ///
    /// **Not the runtime word.** The driver is the ACP adapter binary the
    /// provider runs; the [`RuntimeWord`] is the product behind it. A command
    /// is routed to a provider by driver, never by runtime.
    DriverSlug,
    "cs-target.driver",
    MAX_CODING_SESSION_TARGET_IDENTIFIER_BYTES
);

impl ProviderInstanceId {
    /// Whether this id has the canonical shape a provider mints for itself:
    /// exactly [`SHORT_PUBKEY_PREFIX_LEN`] lowercase hex characters.
    ///
    /// A disclosure, not a gate. `false` means "an operator named this
    /// instance", never "this id is invalid".
    pub fn is_short_pubkey_prefix(&self) -> bool {
        self.0.len() == SHORT_PUBKEY_PREFIX_LEN
            && self
                .0
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    }

    /// Whether this id is the short prefix of `pubkey_hex`.
    ///
    /// The check the hire path needed and did not have: a `cs-target`'s
    /// instance id is bound to a provider by its *signing key*, never by
    /// string-comparing it with the human-facing alias.
    ///
    /// **The argument is validated.** `pubkey_hex` must be a canonical
    /// lowercase 64-hex public key; anything else answers `false`. Without
    /// that check, `matches_pubkey` handed *another instance id* returned an
    /// unqualified `true` — the item-102 failure mode one level removed, in
    /// the very function offered as its cure.
    pub fn matches_pubkey(&self, pubkey_hex: &str) -> bool {
        let is_pubkey = pubkey_hex.len() == 64
            && pubkey_hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
        is_pubkey && self.is_short_pubkey_prefix() && pubkey_hex.starts_with(self.as_str())
    }
}

impl crate::coding_session_command::CodingSessionTarget {
    /// This target's **driver slug**, typed.
    ///
    /// The accessor the lane spec calls for instead of retyping the field: a
    /// `cs-target` is constructed in every crate in the workspace, and
    /// tightening the struct would put a construction-time failure inside code
    /// paths this lane cannot test. Reading through the accessor gives the
    /// same guarantee at every comparison.
    pub fn driver_slug(&self) -> Result<DriverSlug, String> {
        DriverSlug::from_wire(self.driver.as_str())
    }

    /// This target's **provider instance id**, typed so it can never be
    /// compared with a [`ProviderInstanceAlias`].
    ///
    /// Ask [`ProviderInstanceId::is_short_pubkey_prefix`] whether the value is
    /// the canonical minted form; ask
    /// [`ProviderInstanceId::matches_pubkey`] whether it belongs to a given
    /// signer. Never ask whether it equals an alias — that question has no
    /// true answer, and asking it is what item 102 shipped.
    pub fn provider_instance_id(&self) -> Result<ProviderInstanceId, String> {
        ProviderInstanceId::from_wire(self.instance_id.as_str())
    }
}

#[cfg(test)]
#[path = "coding_session_identity_tests.rs"]
mod tests;
