//! The canonical execution-claim fold over an accepted 44228 chain.
//!
//! One function, [`fold_current_claim`], answers "who holds this session, and
//! on which body" for the relay's acceptance rules, the provider's fence, the
//! CLI and the Desktop twin. It is pure and takes no events: callers hand it
//! the links they have already verified, in `seq` order, so the same rule
//! applies whether the caller verified them against relay receipts (the
//! provider), read them out of a storage transaction (the relay) or decoded a
//! page (a client).
//!
//! # Three states, never two
//!
//! `Option<claim>` was the first shape of this fold and it was wrong in a way
//! that mattered: it could not tell "no handover ever happened" from "a
//! handover happened and its claimant lost standing". Those two answer
//! opposite questions at the fence — the first means the ordinary rules apply,
//! the second means **nobody** may steer this session until a fresh claim is
//! made — so [`ClaimState`] keeps them apart
//! (`docs/HANDOVER_IMPL.md` §1).
//!
//! The rule that follows from it, and the one this module exists to make
//! unmissable: **a regrant does not resurrect a claim.** Revoking the claimant
//! voids the claim; granting that same pubkey `grant-operator` again restores
//! its standing to steer *other* work, and leaves the session voided. Only a
//! fresh accepted `takeover`/`transfer` — a deliberate, signed, relay-accepted
//! act by the founder or a live operator — returns the session to `Active`.
//! Without that rule a revoke-then-regrant would silently hand the session
//! back to a machine nobody chose.

use serde::{Deserialize, Serialize};

use crate::coding_session_authority_transition::CodingSessionAuthorityTransitionType;

/// One accepted chain link, reduced to the facts the claim fold reads.
///
/// Deliberately not an `Event`: the three callers hold the chain in three
/// different shapes (relay rows, verified receipt/transition pairs, decoded
/// payloads) and none of them should have to rebuild an event to ask this
/// question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClaimLink {
    /// Chain sequence number of the accepted link (starts at 1).
    pub seq: u32,
    /// Event id (lowercase 64-hex) of the accepted 44228 transition.
    pub accepted_event_id: String,
    /// Which transition this link is.
    pub transition_type: CodingSessionAuthorityTransitionType,
    /// The pubkey the link targets — the claimant for `takeover`/`transfer`.
    pub grantee_pubkey: String,
    /// The execution body a claim names; `None` for every other type.
    pub body_pubkey: Option<String>,
}

/// The session's current execution claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurrentClaim {
    /// Pubkey (lowercase 64-hex) of the participant holding the session.
    pub claimant: String,
    /// Provider authority pubkey of the execution body the claimant uses.
    pub body_pubkey: String,
    /// Event id of the accepted `takeover`/`transfer` that set this claim.
    pub accepted_event_id: String,
    /// That link's chain sequence number.
    pub seq: u32,
}

/// What the accepted chain says about this session's claim.
///
/// Consumers must keep the three apart. `NoClaim` is the ordinary session
/// nobody has handed over; `Active` names the holder and the body; `Voided`
/// says a handover happened and its claimant lost standing, which keeps the
/// fence up for **everyone** rather than falling back to the pre-handover
/// rules — the returning machine is still not the one that was chosen.
///
/// Serialized **internally tagged** on `state`, so a persisted or published
/// claim reads as `{"state":"no-claim"}`, `{"state":"active","claimant":…}` or
/// `{"state":"voided","last":{…},"voidedBy":…}` — a shape a TypeScript twin can
/// discriminate on one key rather than on which key happens to be present.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum ClaimState {
    /// No accepted claim has ever been made on this session.
    #[default]
    NoClaim,
    /// A claim is in force.
    Active(CurrentClaim),
    /// A claim was made and its claimant's standing was later removed.
    #[serde(rename_all = "camelCase")]
    Voided {
        /// The claim as it stood when it was voided — who held it, and on what
        /// body. Kept so a surface can say "continued by B until …" rather
        /// than showing an anonymous locked session.
        last: CurrentClaim,
        /// Event id of the accepted `revoke`/`grant-viewer` that voided it.
        voided_by: String,
        /// That link's chain sequence number.
        seq: u32,
    },
}

impl ClaimState {
    /// The claim in force, if any. `Voided` is **not** a claim in force.
    pub const fn active(&self) -> Option<&CurrentClaim> {
        match self {
            Self::Active(claim) => Some(claim),
            Self::NoClaim | Self::Voided { .. } => None,
        }
    }

    /// The last claim this session had, whether it still stands or was voided.
    ///
    /// For surfaces and disclosures only — never for admission. Admission asks
    /// [`Self::active`], which answers `None` for a voided session.
    pub const fn last(&self) -> Option<&CurrentClaim> {
        match self {
            Self::Active(claim) => Some(claim),
            Self::Voided { last, .. } => Some(last),
            Self::NoClaim => None,
        }
    }

    /// Whether a handover ever happened, in force or voided.
    pub const fn is_handed_over(&self) -> bool {
        !matches!(self, Self::NoClaim)
    }
}

/// Fold an accepted authority chain into its current [`ClaimState`].
///
/// `links` must be the **accepted** chain in ascending `seq` order, and the
/// caller guarantees contiguity — this function decides claim state, never
/// chain validity, exactly as
/// [`crate::coding_session_authority_transition`] validates one link and never
/// the chain. Every caller already proves contiguity for its own reasons (the
/// relay in its storage transaction, the provider in
/// `fold_current_authority`), and duplicating the proof here would give two
/// answers to one question.
///
/// The rules, in the order they apply:
///
/// * `takeover`/`transfer` → [`ClaimState::Active`] with the link's grantee as
///   claimant and its `bodyPubkey` as the body. A claim link with no body is
///   ignored and disclosed by nobody: the decoder refuses that shape before it
///   can be accepted, so reaching this fold means the chain carried something
///   this build cannot read, and treating it as a claim would invent a body.
/// * `revoke` or `grant-viewer` naming the current claimant → [`ClaimState::Voided`].
/// * `grant-operator` naming the voided claimant → **still voided**.
/// * everything else → no change.
pub fn fold_current_claim(links: impl Iterator<Item = ClaimLink>) -> ClaimState {
    let mut state = ClaimState::NoClaim;
    for link in links {
        match link.transition_type {
            CodingSessionAuthorityTransitionType::Takeover
            | CodingSessionAuthorityTransitionType::Transfer => {
                // No body, no claim: a claim that cannot name the execution it
                // fences is not one, and guessing a body here would fence the
                // wrong machine.
                let Some(body_pubkey) = link.body_pubkey else {
                    continue;
                };
                state = ClaimState::Active(CurrentClaim {
                    claimant: link.grantee_pubkey,
                    body_pubkey,
                    accepted_event_id: link.accepted_event_id,
                    seq: link.seq,
                });
            }
            CodingSessionAuthorityTransitionType::Revoke
            | CodingSessionAuthorityTransitionType::GrantViewer => {
                if let ClaimState::Active(claim) = &state {
                    if claim.claimant == link.grantee_pubkey {
                        state = ClaimState::Voided {
                            last: claim.clone(),
                            voided_by: link.accepted_event_id,
                            seq: link.seq,
                        };
                    }
                }
            }
            // A regrant restores standing to steer, never the claim itself.
            // A project-action delegation touches neither: it is about one
            // project's actions, not about who is carrying this session.
            CodingSessionAuthorityTransitionType::GrantOperator
            | CodingSessionAuthorityTransitionType::GrantSeat
            | CodingSessionAuthorityTransitionType::RevokeSeat
            | CodingSessionAuthorityTransitionType::GrantProjectActions
            | CodingSessionAuthorityTransitionType::RevokeProjectActions => {}
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(byte: &str) -> String {
        byte.repeat(32)
    }

    fn link(
        seq: u32,
        transition_type: CodingSessionAuthorityTransitionType,
        grantee: &str,
        body: Option<&str>,
    ) -> ClaimLink {
        ClaimLink {
            seq,
            accepted_event_id: format!("{seq:02}").repeat(32),
            transition_type,
            grantee_pubkey: grantee.to_owned(),
            body_pubkey: body.map(str::to_owned),
        }
    }

    #[test]
    fn an_empty_chain_has_no_claim() {
        assert_eq!(fold_current_claim(std::iter::empty()), ClaimState::NoClaim);
    }

    #[test]
    fn a_chain_of_grants_alone_never_claims_anything() {
        let state = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::GrantOperator,
                    &hex("bb"),
                    None,
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::GrantSeat,
                    &hex("cc"),
                    None,
                ),
            ]
            .into_iter(),
        );
        assert_eq!(state, ClaimState::NoClaim);
        assert!(!state.is_handed_over());
    }

    #[test]
    fn a_takeover_sets_the_claim_to_its_signer_and_body() {
        let state = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::GrantOperator,
                    &hex("bb"),
                    None,
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
            ]
            .into_iter(),
        );
        let claim = state.active().expect("an active claim");
        assert_eq!(claim.claimant, hex("bb"));
        assert_eq!(claim.body_pubkey, hex("dd"));
        assert_eq!(claim.seq, 2);
        assert_eq!(claim.accepted_event_id, "02".repeat(32));
    }

    /// A transfer's grantee is the **new** claimant, not its signer — the
    /// signature is checked by the relay, and the fold reads only the link.
    #[test]
    fn a_transfer_moves_the_claim_to_its_grantee() {
        let state = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::Transfer,
                    &hex("cc"),
                    Some(&hex("ee")),
                ),
            ]
            .into_iter(),
        );
        let claim = state.active().expect("an active claim");
        assert_eq!(claim.claimant, hex("cc"));
        assert_eq!(claim.body_pubkey, hex("ee"));
        assert_eq!(claim.seq, 2);
    }

    #[test]
    fn revoking_the_claimant_voids_the_claim() {
        let state = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::Revoke,
                    &hex("bb"),
                    None,
                ),
            ]
            .into_iter(),
        );
        match &state {
            ClaimState::Voided {
                last,
                voided_by,
                seq,
            } => {
                assert_eq!(last.claimant, hex("bb"));
                assert_eq!(last.body_pubkey, hex("dd"));
                assert_eq!(voided_by, &"02".repeat(32));
                assert_eq!(*seq, 2);
            }
            other => panic!("expected a voided claim, got {other:?}"),
        }
        assert!(state.active().is_none());
        assert!(state.is_handed_over());
        assert_eq!(
            state.last().map(|claim| claim.claimant.clone()),
            Some(hex("bb"))
        );
    }

    /// Demotion voids too: a viewer cannot steer, so a claimant demoted to
    /// viewer is no longer holding anything.
    #[test]
    fn demoting_the_claimant_to_viewer_voids_the_claim() {
        let state = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::GrantViewer,
                    &hex("bb"),
                    None,
                ),
            ]
            .into_iter(),
        );
        assert!(matches!(state, ClaimState::Voided { .. }));
    }

    /// A revoke or demotion of somebody who is not the claimant changes
    /// nothing: the claim belongs to one pubkey, and other people's grants
    /// move independently of it.
    #[test]
    fn revoking_anybody_else_leaves_the_claim_standing() {
        let state = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::Revoke,
                    &hex("cc"),
                    None,
                ),
                link(
                    3,
                    CodingSessionAuthorityTransitionType::GrantViewer,
                    &hex("aa"),
                    None,
                ),
            ]
            .into_iter(),
        );
        assert_eq!(
            state.active().map(|claim| claim.claimant.clone()),
            Some(hex("bb"))
        );
    }

    /// The rule this module exists for: revoke, then regrant the same pubkey,
    /// and the session is **still** voided. Nothing but a fresh accepted claim
    /// hands a session back.
    #[test]
    fn a_regrant_does_not_resurrect_a_voided_claim() {
        let voided = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::Revoke,
                    &hex("bb"),
                    None,
                ),
                link(
                    3,
                    CodingSessionAuthorityTransitionType::GrantOperator,
                    &hex("bb"),
                    None,
                ),
            ]
            .into_iter(),
        );
        assert!(
            matches!(&voided, ClaimState::Voided { last, .. } if last.claimant == hex("bb")),
            "a regrant must not restore the claim: {voided:?}"
        );

        // Only a fresh accepted claim returns the session to Active.
        let reclaimed = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::Revoke,
                    &hex("bb"),
                    None,
                ),
                link(
                    3,
                    CodingSessionAuthorityTransitionType::GrantOperator,
                    &hex("bb"),
                    None,
                ),
                link(
                    4,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("ff")),
                ),
            ]
            .into_iter(),
        );
        let claim = reclaimed.active().expect("a fresh claim");
        assert_eq!(claim.body_pubkey, hex("ff"));
        assert_eq!(claim.seq, 4);
    }

    /// A claim link that carries no body cannot be folded into a claim — and
    /// it must not silently clear the claim that stands either.
    #[test]
    fn a_bodyless_claim_link_is_ignored_rather_than_guessed_at() {
        let state = fold_current_claim(
            vec![
                link(
                    1,
                    CodingSessionAuthorityTransitionType::Takeover,
                    &hex("bb"),
                    Some(&hex("dd")),
                ),
                link(
                    2,
                    CodingSessionAuthorityTransitionType::Transfer,
                    &hex("cc"),
                    None,
                ),
            ]
            .into_iter(),
        );
        let claim = state.active().expect("the earlier claim still stands");
        assert_eq!(claim.claimant, hex("bb"));
        assert_eq!(claim.seq, 1);
    }
}
