//! NIP-CSAT (draft): coding-session authority-chain transitions.
//!
//! Events use [`crate::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION`] and
//! public JSON. A transition is one append-only link in one session's
//! authority chain — the sequence of decisions about who may steer a session
//! after its [`crate::coding_session_genesis`] founded it.
//!
//! # Closed transition types today
//!
//! `grant-operator`, `grant-viewer`, `revoke`, `grant-seat`, `revoke-seat`,
//! `takeover`, `transfer`, `grant-project-actions` and
//! `revoke-project-actions` are implemented — see
//! [`CodingSessionAuthorityTransitionType`]. The type is carried as a string
//! enum precisely so further types are additive later: adding a variant does
//! not change the shape of an existing, already-signed transition, and a relay
//! that only understands the current set correctly rejects any other value as
//! unknown rather than guessing at its meaning. `takeover` and `transfer` were
//! the two names this module reserved from the start; they landed with the
//! absent-participant handover (`docs/HANDOVER_IMPL.md` §1) and each carries
//! one extra field, `bodyPubkey`. The two project-action links landed with
//! ledger 186 and each carries one extra field, `projectRef`; what they
//! delegate is spelled out in [`crate::coding_session_project_action_grant`]
//! and in `docs/nips/NIP-CSAT.md`.
//!
//! # The claim, and why it is not a grant
//!
//! A grant says who *may* steer; a claim says who *is* steering, and on which
//! execution body. `takeover` is a self-claim — the signer names itself as
//! claimant — and `transfer` hands that claim to somebody else. The fold that
//! turns a chain into the current claim is
//! [`crate::coding_session_authority_claim`], shared by the relay's acceptance
//! rules, the provider's fence and the Desktop twin so all three answer "who
//! holds this session" the same way.
//!
//! # The chain, not just the link
//!
//! A legacy grant transition's content is exactly five fields: `genesisRef` (the session's
//! genesis event id — never a `sessionRef` label, for the same reason genesis
//! itself is resolved only by id, see the module doc on
//! [`crate::coding_session_genesis`]), `prevAccepted` (the previous accepted
//! transition's event id, or `null` for the chain's first link), `seq` (a
//! sequence number starting at 1 and incrementing by exactly one per accepted
//! transition), `type`, and `granteePubkey`. Seat transitions add the one
//! required `role` field; legacy payloads keep their original exact shape.
//!
//! This module validates a transition's *self-consistency* only — that its
//! fields are well-formed and that `seq`/`prevAccepted` agree with each other
//! structurally (`seq == 1` if and only if `prevAccepted` is `null`). It
//! cannot and does not validate that a transition actually *extends* the
//! chain: that requires knowing the current accepted head, which is state the
//! relay's storage transaction alone can answer atomically, exactly as
//! genesis's per-`sessionRef` uniqueness does. See
//! `beekeeper_db::event::insert_coding_session_authority_transition_event`.
//!
//! # Why `prevAccepted` is always present, never omitted
//!
//! Unlike genesis's optional `adopts` field, `prevAccepted` is a required key
//! whose *value* is nullable — it is never simply absent. A transition that
//! omitted the key would be ambiguous between "the first link" and "a
//! malformed submission missing a field", and the decoder is written to
//! reject that ambiguity rather than resolve it by treating a missing key the
//! same as an explicit `null` (see [`decode_coding_session_authority_transition`]).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The version tag placed on each coding-session authority-transition event.
pub const CODING_SESSION_AUTHORITY_TRANSITION_TAG_VERSION: &str = "csat1-1";
/// Maximum UTF-8 byte length for the complete signed event content.
///
/// Content is five fixed fields for legacy transitions and six for seat,
/// claim and project-action transitions. The largest realistic encoding is a
/// project-action grant: two 64-hex event ids, a `u32`, the longest
/// transition type, a 64-hex pubkey, and a project coordinate whose `d` tag
/// this crate does not bound — about 390 bytes before the coordinate. The
/// ceiling is 768 rather than the original 512 so a long project slug cannot
/// make a grant unsignable; [`MAX_PROJECT_REF_BYTES`] is what actually bounds
/// the one variable-length field, and the exact-key-set check in
/// [`decode_coding_session_authority_transition`] is what stops the extra
/// room from admitting anything new.
pub const MAX_AUTHORITY_TRANSITION_CONTENT_BYTES: usize = 768;

/// Maximum UTF-8 byte length of a `projectRef` coordinate.
///
/// `30621:<64-hex owner>:<d>` is 71 bytes before the `d` tag, so this leaves
/// 185 bytes of project identifier — far more than any slug a project has
/// carried — while keeping the whole content inside
/// [`MAX_AUTHORITY_TRANSITION_CONTENT_BYTES`] by construction.
pub const MAX_PROJECT_REF_BYTES: usize = 256;

/// One transition type. The enum exists so further types (`transfer`,
/// `takeover`) are additive variants in a future revision rather than a
/// breaking change to this one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionAuthorityTransitionType {
    /// Grants a pubkey standing to steer the session as an operator, without
    /// moving ownership.
    GrantOperator,
    /// Grants a pubkey read access to the session's events (its transport
    /// channel) without any steering authority. Lets a session owner share a
    /// session with someone outside the project.
    GrantViewer,
    /// Removes whatever grant (operator or viewer) `granteePubkey` currently
    /// holds. The relay refuses a revoke naming a pubkey with no live grant —
    /// a no-op link would burn a `seq` for nothing.
    Revoke,
    /// Grants one actor an authoritative team role seat.
    GrantSeat,
    /// Revokes one actor's exact authoritative team role seat.
    RevokeSeat,
    /// One authorized participant claims this session's work for itself, on a
    /// named execution body.
    ///
    /// A **self-claim only**: `granteePubkey` must equal the signer, so nobody
    /// can be volunteered into carrying someone else's work. `bodyPubkey` is
    /// the provider authority pubkey of the body the claimant will use, which
    /// is what every consumer fences against — a returning machine that is not
    /// that body refuses turns rather than silently resuming competing work.
    Takeover,
    /// The current claimant (or the founder) hands the claim to another
    /// authorized participant, naming the body it will run on.
    ///
    /// `granteePubkey` is the **new** claimant, never the signer's own record
    /// of itself; `bodyPubkey` is the new body. Unlike `takeover` this is not a
    /// self-claim, which is exactly why the relay checks the signer against the
    /// folded current claim rather than against the grantee.
    Transfer,
    /// Delegates this project's **actions** to one pubkey — nothing else.
    ///
    /// Signed by the session's owner (a project owner founded the session, so
    /// the chain's owner rule is exactly the "signed by a project owner"
    /// requirement), it lets `granteePubkey` publish the named project's
    /// kind:30620 action definitions and start manual kind:46020 runs of
    /// them. It does **not** approve host steps (kind:46030), does not reach
    /// any other project, and confers no steering, hiring or read authority.
    /// See [`crate::coding_session_project_action_grant`].
    GrantProjectActions,
    /// Withdraws a [`Self::GrantProjectActions`] delegation for the exact
    /// same `(granteePubkey, projectRef)` pair.
    RevokeProjectActions,
}

impl CodingSessionAuthorityTransitionType {
    /// The exact wire token this type is written as, in content and in every
    /// receipt that restates it.
    ///
    /// One function so a consumer that has to print or compare the token —
    /// the relay's receipt, the provider's fence, a refusal message — never
    /// re-spells it by hand.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GrantOperator => "grant-operator",
            Self::GrantViewer => "grant-viewer",
            Self::Revoke => "revoke",
            Self::GrantSeat => "grant-seat",
            Self::RevokeSeat => "revoke-seat",
            Self::Takeover => "takeover",
            Self::Transfer => "transfer",
            Self::GrantProjectActions => "grant-project-actions",
            Self::RevokeProjectActions => "revoke-project-actions",
        }
    }

    /// Whether this type carries `projectRef` and speaks about a project's
    /// actions rather than about the session itself.
    pub const fn is_project_actions(self) -> bool {
        matches!(self, Self::GrantProjectActions | Self::RevokeProjectActions)
    }

    /// Whether this type sets the session's execution claim.
    ///
    /// The two claiming types are the only ones that carry `bodyPubkey`, and
    /// the only ones [`crate::coding_session_authority_claim`] treats as
    /// setting a claim.
    pub const fn is_claim(self) -> bool {
        matches!(self, Self::Takeover | Self::Transfer)
    }
}

/// Durable coding-session authority-transition JSON payload.
///
/// The event's signer is the party claiming to be authorized to extend the
/// chain — for `grant-operator`, the session's current owner. The payload
/// never restates the signer's identity; the signature already settles it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionAuthorityTransitionPayload {
    /// Event id (64-character lowercase hex) of the session's genesis. The
    /// chain is rooted here — never resolved by querying for a `sessionRef`.
    pub genesis_ref: String,
    /// Event id (64-character lowercase hex) of the previous accepted
    /// transition in this chain, or `None` for the chain's first link.
    pub prev_accepted: Option<String>,
    /// Sequence number: 1 for the first transition, incrementing by exactly 1
    /// per accepted transition thereafter.
    pub seq: u32,
    /// Which transition this is.
    #[serde(rename = "type")]
    pub transition_type: CodingSessionAuthorityTransitionType,
    /// Pubkey (64-character lowercase hex) this transition targets: the
    /// grantee for `grant-*`, the pubkey losing its grant for `revoke`.
    pub grantee_pubkey: String,
    /// Required normalized role for seat transitions; absent on legacy grant transitions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    /// The **provider authority pubkey** of the execution body the claimant
    /// will use — required for `takeover` and `transfer`, absent for every
    /// other type.
    ///
    /// This is what makes a claim fenceable. "B has taken over" alone would
    /// leave a returning provider with no way to tell whether *it* is the body
    /// now carrying the work; naming the body turns that into an equality
    /// check against the provider's own key
    /// (`docs/HANDOVER_IMPL.md` §3). It is a pubkey, not a session target: an
    /// execution generation changes, and the claim outlives it.
    ///
    /// Serialized only when present, so every transition signed before this
    /// field existed keeps its exact five- or six-key shape on the wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body_pubkey: Option<String>,
    /// The project coordinate (`30621:<owner>:<d>`) whose actions this
    /// transition grants or revokes — required for
    /// [`CodingSessionAuthorityTransitionType::GrantProjectActions`] and
    /// [`CodingSessionAuthorityTransitionType::RevokeProjectActions`], absent
    /// for every other type.
    ///
    /// A delegation with no project named would be a delegation of every
    /// project the grantee can see, which is the opposite of what this link
    /// is for, so it is required on write *and* on read.
    ///
    /// Serialized only when present, so every transition signed before this
    /// field existed keeps its exact five- or six-key shape on the wire.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_ref: Option<String>,
}

impl CodingSessionAuthorityTransitionPayload {
    /// Build a transition payload of the given type.
    pub fn new(
        transition_type: CodingSessionAuthorityTransitionType,
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_pubkey: impl Into<String>,
    ) -> Self {
        Self {
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            transition_type,
            grantee_pubkey: grantee_pubkey.into(),
            role: None,
            body_pubkey: None,
            project_ref: None,
        }
    }

    /// Build an authoritative role-seat grant.
    pub fn new_grant_seat(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_pubkey: impl Into<String>,
        role: impl Into<String>,
    ) -> Self {
        Self {
            transition_type: CodingSessionAuthorityTransitionType::GrantSeat,
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            grantee_pubkey: grantee_pubkey.into(),
            role: Some(role.into()),
            body_pubkey: None,
            project_ref: None,
        }
    }

    /// Build an authoritative role-seat revocation.
    pub fn new_revoke_seat(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_pubkey: impl Into<String>,
        role: impl Into<String>,
    ) -> Self {
        Self {
            transition_type: CodingSessionAuthorityTransitionType::RevokeSeat,
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            grantee_pubkey: grantee_pubkey.into(),
            role: Some(role.into()),
            body_pubkey: None,
            project_ref: None,
        }
    }

    /// Build a `takeover` transition payload: a self-claim of this session.
    ///
    /// `claimant` must be the signer's own pubkey — the relay refuses any
    /// other, and [`Self::validate`] cannot know the signer, so the caller
    /// passes the same key it will sign with.
    pub fn new_takeover(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        claimant: impl Into<String>,
        body_pubkey: impl Into<String>,
    ) -> Self {
        Self {
            transition_type: CodingSessionAuthorityTransitionType::Takeover,
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            grantee_pubkey: claimant.into(),
            role: None,
            body_pubkey: Some(body_pubkey.into()),
            project_ref: None,
        }
    }

    /// Build a `transfer` transition payload: hand the claim to `claimant`.
    ///
    /// The signer is the current claimant or the founder; that is the relay's
    /// check against the folded chain, not this payload's.
    pub fn new_transfer(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        claimant: impl Into<String>,
        body_pubkey: impl Into<String>,
    ) -> Self {
        Self {
            transition_type: CodingSessionAuthorityTransitionType::Transfer,
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            grantee_pubkey: claimant.into(),
            role: None,
            body_pubkey: Some(body_pubkey.into()),
            project_ref: None,
        }
    }

    /// Build a `grant-operator` transition payload.
    pub fn new_grant_operator(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_pubkey: impl Into<String>,
    ) -> Self {
        Self::new(
            CodingSessionAuthorityTransitionType::GrantOperator,
            genesis_ref,
            prev_accepted,
            seq,
            grantee_pubkey,
        )
    }

    /// Build a `grant-project-actions` transition payload.
    ///
    /// `project_ref` is the coordinate whose actions `grantee_pubkey` may
    /// publish and trigger; the signer must be the session's owner, which the
    /// relay checks against the chain, not this payload.
    pub fn new_grant_project_actions(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_pubkey: impl Into<String>,
        project_ref: impl Into<String>,
    ) -> Self {
        Self {
            transition_type: CodingSessionAuthorityTransitionType::GrantProjectActions,
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            grantee_pubkey: grantee_pubkey.into(),
            role: None,
            body_pubkey: None,
            project_ref: Some(project_ref.into()),
        }
    }

    /// Build a `revoke-project-actions` transition payload.
    pub fn new_revoke_project_actions(
        genesis_ref: impl Into<String>,
        prev_accepted: Option<String>,
        seq: u32,
        grantee_pubkey: impl Into<String>,
        project_ref: impl Into<String>,
    ) -> Self {
        Self {
            transition_type: CodingSessionAuthorityTransitionType::RevokeProjectActions,
            genesis_ref: genesis_ref.into(),
            prev_accepted,
            seq,
            grantee_pubkey: grantee_pubkey.into(),
            role: None,
            body_pubkey: None,
            project_ref: Some(project_ref.into()),
        }
    }

    /// Validate this payload's self-consistency before signing an event.
    ///
    /// This is a structural check only — it cannot know whether `seq` and
    /// `prevAccepted` actually agree with the chain's current head, since
    /// that is state only the relay's storage transaction can answer
    /// atomically. What it does enforce: every event id is well-formed hex,
    /// `seq` is never 0 (sequence numbers start at 1), and `seq == 1` if and
    /// only if `prevAccepted` is `None` — a transition cannot claim to be
    /// both "the first link" and "not the first link" at once.
    pub fn validate(&self) -> Result<(), String> {
        validate_event_id_hex("genesisRef", &self.genesis_ref)?;
        if let Some(prev) = &self.prev_accepted {
            validate_event_id_hex("prevAccepted", prev)?;
        }
        if self.seq == 0 {
            return Err("seq must start at 1 (0 is not a valid sequence number)".into());
        }
        if (self.seq == 1) != self.prev_accepted.is_none() {
            return Err("seq must be exactly 1 if and only if prevAccepted is null".into());
        }
        validate_event_id_hex("granteePubkey", &self.grantee_pubkey)?;
        match self.transition_type {
            CodingSessionAuthorityTransitionType::GrantSeat
            | CodingSessionAuthorityTransitionType::RevokeSeat => {
                let role = self
                    .role
                    .as_deref()
                    .ok_or_else(|| "seat transition requires role".to_owned())?;
                crate::coding_session_lifecycle_command::validate_role_slug(role)
                    .map_err(|error| error.replace("action.role", "role"))?;
            }
            CodingSessionAuthorityTransitionType::GrantOperator
            | CodingSessionAuthorityTransitionType::GrantViewer
            | CodingSessionAuthorityTransitionType::Revoke
            | CodingSessionAuthorityTransitionType::Takeover
            | CodingSessionAuthorityTransitionType::Transfer
            | CodingSessionAuthorityTransitionType::GrantProjectActions
            | CodingSessionAuthorityTransitionType::RevokeProjectActions => {
                if self.role.is_some() {
                    return Err("non-seat authority transition must not carry role".into());
                }
            }
        }
        // A claim names the body it will run on; nothing else may name one.
        // Both halves are refusals rather than tolerations: a `takeover`
        // without a body cannot be fenced against, and a `revoke` carrying one
        // is a producer inventing a shape a consumer would have to guess at.
        match (self.transition_type.is_claim(), self.body_pubkey.as_deref()) {
            (true, Some(body)) => validate_event_id_hex("bodyPubkey", body)?,
            (true, None) => {
                return Err(
                    "takeover and transfer require bodyPubkey: the execution body the claimant \
                     will use is what every consumer fences against"
                        .into(),
                )
            }
            (false, Some(_)) => {
                return Err(
                    "only takeover and transfer may carry bodyPubkey — no other transition \
                     names an execution body"
                        .into(),
                )
            }
            (false, None) => {}
        }
        // A project-action delegation names the project it delegates, and
        // nothing else may name one. Both halves refuse rather than tolerate,
        // for the same reason `bodyPubkey` does: an unscoped delegation and a
        // stray coordinate are each a shape a consumer would have to guess at.
        match (
            self.transition_type.is_project_actions(),
            self.project_ref.as_deref(),
        ) {
            (true, Some(project_ref)) => validate_project_ref(project_ref)?,
            (true, None) => {
                return Err(
                    "grant-project-actions and revoke-project-actions require projectRef: an \
                     unscoped delegation would reach every project the grantee can see"
                        .into(),
                )
            }
            (false, Some(_)) => {
                return Err(
                    "only grant-project-actions and revoke-project-actions may carry projectRef"
                        .into(),
                )
            }
            (false, None) => {}
        }
        Ok(())
    }
}

/// Check that `value` is a project coordinate this transition may delegate:
/// `30621:<64-hex owner>:<non-empty d>`, within [`MAX_PROJECT_REF_BYTES`].
///
/// Kind 30621 is the project record; a coordinate of any other kind is not a
/// project and is refused rather than normalized, so a delegation can never
/// be filed against a repository or a pack coordinate instead.
///
/// Public because the relay's kind:40099 acceptance receipt echoes the same
/// coordinate, and every strict reader of that receipt must hold it to the
/// *same* rule as the signed link it restates. A second hand-written copy is
/// how the reader and the writer drift apart (ledger 204).
pub fn validate_project_ref(value: &str) -> Result<(), String> {
    if value.len() > MAX_PROJECT_REF_BYTES {
        return Err(format!(
            "projectRef must be at most {MAX_PROJECT_REF_BYTES} bytes"
        ));
    }
    let mut parts = value.splitn(3, ':');
    let kind_ok = parts.next() == Some("30621");
    let owner_ok = parts.next().is_some_and(|owner| {
        owner.len() == 64
            && owner
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    let d_ok = parts.next().is_some_and(|d| !d.trim().is_empty());
    if kind_ok && owner_ok && d_ok {
        Ok(())
    } else {
        Err(format!(
            "projectRef must be 30621:<64-hex owner>:<d> (got {value:?})"
        ))
    }
}

/// Check that `value` is a 64-character lowercase-hex Nostr id (event id or
/// pubkey — the two share an encoding).
fn validate_event_id_hex(field: &str, value: &str) -> Result<(), String> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(format!(
            "{field} must be a lowercase 64-hex value (got {value:?})"
        ));
    }
    Ok(())
}

/// Strictly decode and validate signed authority-transition content.
///
/// Legacy transitions accept exactly `{genesisRef, prevAccepted, seq, type,
/// granteePubkey}`; seat transitions accept exactly those keys plus required
/// `role`; claim transitions (`takeover`, `transfer`) accept exactly those
/// keys plus required `bodyPubkey`. Nothing between or beyond is accepted, and
/// `prevAccepted`'s key must be present even though its value may be `null`. A
/// `type` value outside the pinned
/// [`CodingSessionAuthorityTransitionType`] vocabulary fails to decode and is
/// rejected the same as any other malformed field.
pub fn decode_coding_session_authority_transition(
    content: &str,
) -> Result<CodingSessionAuthorityTransitionPayload, String> {
    if content.len() > MAX_AUTHORITY_TRANSITION_CONTENT_BYTES {
        return Err(format!(
            "coding-session authority-transition content exceeds {MAX_AUTHORITY_TRANSITION_CONTENT_BYTES} bytes"
        ));
    }

    let value: Value = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session authority-transition payload".to_string())?;
    let object = value.as_object().ok_or_else(|| {
        "coding-session authority-transition payload must be an object".to_string()
    })?;

    const LEGACY_EXPECTED: [&str; 5] =
        ["genesisRef", "prevAccepted", "seq", "type", "granteePubkey"];
    const SEAT_EXPECTED: [&str; 6] = [
        "genesisRef",
        "prevAccepted",
        "seq",
        "type",
        "granteePubkey",
        "role",
    ];
    /// A claim's exact shape: the five legacy keys plus the body it names.
    /// Required on write *and* on read — a claim with no body could not be
    /// fenced, so there is no legacy shape to stay compatible with.
    const CLAIM_EXPECTED: [&str; 6] = [
        "genesisRef",
        "prevAccepted",
        "seq",
        "type",
        "granteePubkey",
        "bodyPubkey",
    ];
    /// A project-action delegation's exact shape: the five legacy keys plus
    /// the project it is scoped to. Required on write *and* on read — an
    /// unscoped delegation is not a narrower grant, it is a wider one.
    const PROJECT_ACTIONS_EXPECTED: [&str; 6] = [
        "genesisRef",
        "prevAccepted",
        "seq",
        "type",
        "granteePubkey",
        "projectRef",
    ];
    let expected: &[&str] = match object.get("type").and_then(Value::as_str) {
        Some("grant-seat" | "revoke-seat") => &SEAT_EXPECTED,
        Some("takeover" | "transfer") => &CLAIM_EXPECTED,
        Some("grant-project-actions" | "revoke-project-actions") => &PROJECT_ACTIONS_EXPECTED,
        _ => &LEGACY_EXPECTED,
    };
    let complete = expected.iter().all(|key| object.contains_key(*key));
    let recognized = object.keys().all(|key| expected.contains(&key.as_str()));
    if !complete || !recognized {
        return Err(
            "coding-session authority-transition payload has missing or unsupported fields".into(),
        );
    }

    // Decode a second time into the strict serde type: this preserves
    // serde's duplicate-field detection, which the `Value` map above cannot
    // represent (a duplicate JSON key collapses to one entry there), and it
    // is what actually enforces `prevAccepted`'s value being either a
    // 64-hex string or JSON `null` — never any other type.
    let payload: CodingSessionAuthorityTransitionPayload = serde_json::from_str(content)
        .map_err(|_| "malformed coding-session authority-transition payload".to_string())?;
    payload.validate()?;
    Ok(payload)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event_id_hex(byte: &str) -> String {
        byte.repeat(32)
    }

    fn valid_json(genesis_ref: &str, prev: &str, seq: u32, grantee: &str) -> String {
        format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":{prev},"seq":{seq},"type":"grant-operator","granteePubkey":"{grantee}"}}"#
        )
    }

    #[test]
    fn validates_and_strictly_decodes_the_first_transition() {
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_operator(
            event_id_hex("ab"),
            None,
            1,
            event_id_hex("cd"),
        );
        assert!(payload.validate().is_ok());
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            content,
            valid_json(&event_id_hex("ab"), "null", 1, &event_id_hex("cd")),
        );
        assert_eq!(
            decode_coding_session_authority_transition(&content).unwrap(),
            payload
        );
    }

    #[test]
    fn validates_and_strictly_decodes_a_chained_transition() {
        let payload = CodingSessionAuthorityTransitionPayload::new_grant_operator(
            event_id_hex("ab"),
            Some(event_id_hex("11")),
            2,
            event_id_hex("cd"),
        );
        assert!(payload.validate().is_ok());
        let content = serde_json::to_string(&payload).unwrap();
        assert_eq!(
            decode_coding_session_authority_transition(&content).unwrap(),
            payload
        );
    }

    /// The five keys are the whole contract — a sixth (smuggled) field, or a
    /// missing one, must be rejected outright.
    #[test]
    fn rejects_smuggled_and_missing_fields() {
        let base = serde_json::json!({
            "genesisRef": event_id_hex("ab"),
            "prevAccepted": null,
            "seq": 1,
            "type": "grant-operator",
            "granteePubkey": event_id_hex("cd"),
        });

        // Smuggled sixth field.
        let mut smuggled = base.clone();
        smuggled["note"] = serde_json::json!("trust me");
        assert!(decode_coding_session_authority_transition(&smuggled.to_string()).is_err());

        // Missing prevAccepted entirely — must not be treated the same as an
        // explicit null.
        let mut missing_prev = base.clone();
        missing_prev.as_object_mut().unwrap().remove("prevAccepted");
        assert!(decode_coding_session_authority_transition(&missing_prev.to_string()).is_err());

        for key in ["genesisRef", "seq", "type", "granteePubkey"] {
            let mut missing = base.clone();
            missing.as_object_mut().unwrap().remove(key);
            assert!(
                decode_coding_session_authority_transition(&missing.to_string()).is_err(),
                "should reject payload missing {key}"
            );
        }
    }

    /// A duplicate top-level key is caught by the strict second decode, not
    /// by the `Value`-based key-set check (which cannot see it).
    #[test]
    fn rejects_duplicate_fields() {
        let duplicated = format!(
            r#"{{"genesisRef":"{gr}","genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"grant-operator","granteePubkey":"{gp}"}}"#,
            gr = event_id_hex("ab"),
            gp = event_id_hex("cd"),
        );
        assert!(decode_coding_session_authority_transition(&duplicated).is_err());
    }

    #[test]
    fn rejects_malformed_hex_ids() {
        let ok_grantee = event_id_hex("cd");
        for genesis_ref in [
            event_id_hex("ab").to_uppercase(),
            event_id_hex("ab")[..63].to_owned(),
            String::new(),
            "not-hex-at-all-not-hex-at-all-not-hex-at-all-not-hex-at-all-gg".to_owned(),
        ] {
            let content = valid_json(&genesis_ref, "null", 1, &ok_grantee);
            assert!(
                decode_coding_session_authority_transition(&content).is_err(),
                "should reject genesisRef {genesis_ref:?}"
            );
        }

        let ok_genesis = event_id_hex("ab");
        for grantee in [
            event_id_hex("cd").to_uppercase(),
            event_id_hex("cd")[..10].to_owned(),
        ] {
            let content = valid_json(&ok_genesis, "null", 1, &grantee);
            assert!(
                decode_coding_session_authority_transition(&content).is_err(),
                "should reject granteePubkey {grantee:?}"
            );
        }

        // A malformed prevAccepted value.
        let content = valid_json(&ok_genesis, "\"not-hex\"", 2, &ok_grantee);
        assert!(decode_coding_session_authority_transition(&content).is_err());
    }

    /// `seq` must start at 1, and its nullness must agree with
    /// `prevAccepted` — both directions of the mismatch are rejected.
    #[test]
    fn rejects_bad_seq() {
        let genesis_ref = event_id_hex("ab");
        let grantee = event_id_hex("cd");
        let prev = event_id_hex("11");

        // seq = 0 is never valid.
        assert!(decode_coding_session_authority_transition(&valid_json(
            &genesis_ref,
            "null",
            0,
            &grantee
        ))
        .is_err());

        // seq = 1 with a non-null prevAccepted: claims to be first and not-first.
        assert!(decode_coding_session_authority_transition(&format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":"{prev}","seq":1,"type":"grant-operator","granteePubkey":"{grantee}"}}"#
        ))
        .is_err());

        // seq = 2 with a null prevAccepted: claims to extend a chain with no predecessor.
        assert!(decode_coding_session_authority_transition(&valid_json(
            &genesis_ref,
            "null",
            2,
            &grantee
        ))
        .is_err());
    }

    /// Every pinned transition type decodes; the target pubkey field is
    /// shared across them.
    #[test]
    fn decodes_all_pinned_transition_types() {
        let genesis_ref = event_id_hex("ab");
        let grantee = event_id_hex("cd");
        for (name, expected) in [
            (
                "grant-operator",
                CodingSessionAuthorityTransitionType::GrantOperator,
            ),
            (
                "grant-viewer",
                CodingSessionAuthorityTransitionType::GrantViewer,
            ),
            ("revoke", CodingSessionAuthorityTransitionType::Revoke),
        ] {
            let content = format!(
                r#"{{"genesisRef":"{genesis_ref}","prevAccepted":null,"seq":1,"type":"{name}","granteePubkey":"{grantee}"}}"#
            );
            let payload = decode_coding_session_authority_transition(&content)
                .unwrap_or_else(|e| panic!("type {name} should decode: {e}"));
            assert_eq!(payload.transition_type, expected);
        }

        for (name, expected) in [
            (
                "grant-seat",
                CodingSessionAuthorityTransitionType::GrantSeat,
            ),
            (
                "revoke-seat",
                CodingSessionAuthorityTransitionType::RevokeSeat,
            ),
        ] {
            let content = format!(
                r#"{{"genesisRef":"{genesis_ref}","prevAccepted":null,"seq":1,"type":"{name}","granteePubkey":"{grantee}","role":"active-verifier"}}"#
            );
            let payload = decode_coding_session_authority_transition(&content)
                .unwrap_or_else(|e| panic!("type {name} should decode: {e}"));
            assert_eq!(payload.transition_type, expected);
            assert_eq!(payload.role.as_deref(), Some("active-verifier"));
        }
    }

    #[test]
    fn seat_shapes_are_exact_and_legacy_shapes_remain_unchanged() {
        let genesis_ref = event_id_hex("ab");
        let grantee = event_id_hex("cd");
        let seat = format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":null,"seq":1,"type":"grant-seat","granteePubkey":"{grantee}","role":"verifier"}}"#
        );
        assert!(decode_coding_session_authority_transition(&seat).is_ok());

        let mut missing_role: Value = serde_json::from_str(&seat).unwrap();
        missing_role.as_object_mut().unwrap().remove("role");
        assert!(decode_coding_session_authority_transition(&missing_role.to_string()).is_err());

        let mut extra: Value = serde_json::from_str(&seat).unwrap();
        extra["note"] = Value::String("smuggled".into());
        assert!(decode_coding_session_authority_transition(&extra.to_string()).is_err());

        let legacy_with_role = format!(
            r#"{{"genesisRef":"{genesis_ref}","prevAccepted":null,"seq":1,"type":"grant-operator","granteePubkey":"{grantee}","role":"lead"}}"#
        );
        assert!(decode_coding_session_authority_transition(&legacy_with_role).is_err());

        for role in ["Lead", "active_verifier", "", "lead role"] {
            let invalid = format!(
                r#"{{"genesisRef":"{genesis_ref}","prevAccepted":null,"seq":1,"type":"grant-seat","granteePubkey":"{grantee}","role":"{role}"}}"#
            );
            assert!(
                decode_coding_session_authority_transition(&invalid).is_err(),
                "role {role:?} must be rejected"
            );
        }
    }

    /// Types outside the pinned vocabulary, and every non-string, are
    /// rejected the same way an unrecognized enum value should be.
    #[test]
    fn rejects_unknown_transition_types() {
        let genesis_ref = event_id_hex("ab");
        let grantee = event_id_hex("cd");
        for bad_type in [
            "\"handover\"",
            "\"claim\"",
            "\"GRANT-OPERATOR\"",
            "\"grant_operator\"",
            "\"TAKEOVER\"",
            "1",
            "null",
        ] {
            let content = format!(
                r#"{{"genesisRef":"{genesis_ref}","prevAccepted":null,"seq":1,"type":{bad_type},"granteePubkey":"{grantee}"}}"#
            );
            assert!(
                decode_coding_session_authority_transition(&content).is_err(),
                "should reject type {bad_type}"
            );
        }
    }

    #[test]
    fn rejects_content_over_the_byte_ceiling() {
        let content = " ".repeat(MAX_AUTHORITY_TRANSITION_CONTENT_BYTES + 1);
        assert!(decode_coding_session_authority_transition(&content).is_err());
    }

    #[test]
    fn rejects_non_object_and_malformed_json() {
        for rejected in ["not json", "[1,2,3]", "\"a string\"", "42", "null"] {
            assert!(decode_coding_session_authority_transition(rejected).is_err());
        }
    }

    // ── Claims: takeover and transfer (docs/HANDOVER_IMPL.md §1) ────────────

    fn claim_json(kind: &str, body: &str) -> String {
        format!(
            r#"{{"genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"{kind}","granteePubkey":"{gp}","bodyPubkey":"{body}"}}"#,
            gr = event_id_hex("ab"),
            gp = event_id_hex("cd"),
        )
    }

    #[test]
    fn a_takeover_round_trips_through_the_exact_claim_shape() {
        let payload = CodingSessionAuthorityTransitionPayload::new_takeover(
            event_id_hex("ab"),
            None,
            1,
            event_id_hex("cd"),
            event_id_hex("ef"),
        );
        payload.validate().expect("a well-formed takeover");
        let content = serde_json::to_string(&payload).expect("serialize");
        assert_eq!(content, claim_json("takeover", &event_id_hex("ef")));
        assert_eq!(
            decode_coding_session_authority_transition(&content).expect("decode"),
            payload
        );
        assert_eq!(payload.transition_type.as_str(), "takeover");
        assert!(payload.transition_type.is_claim());
    }

    #[test]
    fn a_transfer_round_trips_and_names_the_new_claimant() {
        let payload = CodingSessionAuthorityTransitionPayload::new_transfer(
            event_id_hex("ab"),
            Some(event_id_hex("11")),
            2,
            event_id_hex("cd"),
            event_id_hex("ef"),
        );
        payload.validate().expect("a well-formed transfer");
        let content = serde_json::to_string(&payload).expect("serialize");
        let decoded = decode_coding_session_authority_transition(&content).expect("decode");
        assert_eq!(decoded, payload);
        assert_eq!(decoded.grantee_pubkey, event_id_hex("cd"));
        assert_eq!(
            decoded.body_pubkey.as_deref(),
            Some(event_id_hex("ef").as_str())
        );
        assert_eq!(decoded.transition_type.as_str(), "transfer");
    }

    /// The field is required for a claim, refused everywhere else, and must be
    /// a lowercase 64-hex pubkey.
    #[test]
    fn body_pubkey_is_required_for_claims_and_refused_elsewhere() {
        for kind in ["takeover", "transfer"] {
            // Missing entirely.
            let missing = format!(
                r#"{{"genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"{kind}","granteePubkey":"{gp}"}}"#,
                gr = event_id_hex("ab"),
                gp = event_id_hex("cd"),
            );
            assert!(
                decode_coding_session_authority_transition(&missing).is_err(),
                "{kind} without bodyPubkey must be refused"
            );
            // Present but malformed.
            for bad in [
                event_id_hex("ef").to_uppercase(),
                event_id_hex("ef")[..63].to_owned(),
                String::new(),
            ] {
                assert!(
                    decode_coding_session_authority_transition(&claim_json(kind, &bad)).is_err(),
                    "{kind} with bodyPubkey {bad:?} must be refused"
                );
            }
            // A role alongside a claim is refused: a claim is not a seat.
            let with_role = format!(
                r#"{{"genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"{kind}","granteePubkey":"{gp}","bodyPubkey":"{body}","role":"lead"}}"#,
                gr = event_id_hex("ab"),
                gp = event_id_hex("cd"),
                body = event_id_hex("ef"),
            );
            assert!(decode_coding_session_authority_transition(&with_role).is_err());
        }

        for kind in ["grant-operator", "grant-viewer", "revoke"] {
            let smuggled = format!(
                r#"{{"genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"{kind}","granteePubkey":"{gp}","bodyPubkey":"{body}"}}"#,
                gr = event_id_hex("ab"),
                gp = event_id_hex("cd"),
                body = event_id_hex("ef"),
            );
            assert!(
                decode_coding_session_authority_transition(&smuggled).is_err(),
                "{kind} must not carry bodyPubkey"
            );
        }

        let seat_with_body = format!(
            r#"{{"genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"grant-seat","granteePubkey":"{gp}","role":"builder","bodyPubkey":"{body}"}}"#,
            gr = event_id_hex("ab"),
            gp = event_id_hex("cd"),
            body = event_id_hex("ef"),
        );
        assert!(decode_coding_session_authority_transition(&seat_with_body).is_err());
    }

    /// The one compatibility promise: adding the field changed no existing
    /// transition's bytes. Every legacy and seat shape serializes exactly as
    /// it did before `bodyPubkey` existed.
    #[test]
    fn legacy_and_seat_shapes_serialize_byte_identically_to_before() {
        let legacy = CodingSessionAuthorityTransitionPayload::new_grant_operator(
            event_id_hex("ab"),
            None,
            1,
            event_id_hex("cd"),
        );
        assert_eq!(
            serde_json::to_string(&legacy).expect("serialize"),
            valid_json(&event_id_hex("ab"), "null", 1, &event_id_hex("cd")),
        );
        assert!(!serde_json::to_string(&legacy)
            .expect("serialize")
            .contains("bodyPubkey"));

        let seat = CodingSessionAuthorityTransitionPayload::new_grant_seat(
            event_id_hex("ab"),
            None,
            1,
            event_id_hex("cd"),
            "builder",
        );
        assert_eq!(
            serde_json::to_string(&seat).expect("serialize"),
            format!(
                r#"{{"genesisRef":"{gr}","prevAccepted":null,"seq":1,"type":"grant-seat","granteePubkey":"{gp}","role":"builder"}}"#,
                gr = event_id_hex("ab"),
                gp = event_id_hex("cd"),
            ),
        );
    }

    /// Every pinned token, including the two claims, round-trips through the
    /// enum's own spelling.
    #[test]
    fn as_str_is_the_wire_token_for_every_pinned_type() {
        for (token, variant) in [
            (
                "grant-operator",
                CodingSessionAuthorityTransitionType::GrantOperator,
            ),
            (
                "grant-viewer",
                CodingSessionAuthorityTransitionType::GrantViewer,
            ),
            ("revoke", CodingSessionAuthorityTransitionType::Revoke),
            (
                "grant-seat",
                CodingSessionAuthorityTransitionType::GrantSeat,
            ),
            (
                "revoke-seat",
                CodingSessionAuthorityTransitionType::RevokeSeat,
            ),
            ("takeover", CodingSessionAuthorityTransitionType::Takeover),
            ("transfer", CodingSessionAuthorityTransitionType::Transfer),
        ] {
            assert_eq!(variant.as_str(), token);
            assert_eq!(
                serde_json::to_value(variant).expect("serialize"),
                serde_json::Value::String(token.to_owned())
            );
            assert_eq!(variant.is_claim(), matches!(token, "takeover" | "transfer"));
        }
    }
}

#[cfg(test)]
#[path = "coding_session_authority_transition_project_action_tests.rs"]
mod project_action_tests;
