//! Who **provides** one mission — the lifecycle half of the admission rule.
//!
//! Split out of [`super::observed`] so neither file passes 1,000 lines, and
//! because the question is its own: arm (B) asks whether the required gates
//! were watched green, and this asks whose word "watched" is. The fold that
//! answers the first
//! ([`crate::coding_session_observation::fold_coding_session_observations`])
//! downgrades an `observed` claim from a signer no provider backs, so the set
//! this module resolves is what decides whether a row is evidence or a claim.
//!
//! Two rules, both of them about *proof rather than assertion*:
//!
//! * **finding 90** — a provider is named by an accepted kind 44221 command
//!   and confirmed by the kind 44224 receipt **that key** signed. Publishing
//!   kind 44223 metadata about a mission makes nobody its provider;
//! * **the 2026-09-05 refuter's B1, completed 2026-09-06** — and the command
//!   itself only counts when somebody entitled to steer the mission signed it.
//!   A receipt proves who answered a command; it says nothing about who was
//!   entitled to issue one. A public `hireRef` attributes why a create exists;
//!   it is not authority an unrelated seat can borrow to commission itself and
//!   then sign the `observed` rows arm (B) lands.

use nostr::Event;

use crate::coding_session_command::CodingSessionTarget;
use crate::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
    CodingSessionLifecycleCommandPayload,
};
use crate::coding_session_payload::{
    decode_coding_session_lifecycle_receipt, LifecycleReceipt, ReceiptStatus,
};

/// Newest kind 44221 lifecycle commands, and separately the newest kind 44224
/// receipts, one verdict-gated ref update may read to learn a mission's
/// providers.
///
/// Finding 90: the provider set is proven by an accepted lifecycle command
/// and the receipt its named provider signed
/// ([`mission_provider_pubkeys_from_lifecycle`]), not by who published
/// metadata. Past this bound a provider is simply not found, so its rows fold
/// down to `declared` and admit nothing — the failure direction is a refusal,
/// never a wrong admission.
pub const VERDICT_ADMISSION_MAX_LIFECYCLE_RECORDS: usize = 256;

/// One accepted lifecycle pair, as the provider rule keeps it.
struct LifecyclePair<'a> {
    command: &'a Event,
    action: CodingSessionLifecycleAction,
    receipt_target: CodingSessionTarget,
}

/// The provider identities behind one mission, proven by its lifecycle
/// (finding 90).
///
/// A key is a provider of this mission when a kind 44221 command **names it**
/// as `providerAuthorityPubkey` and a kind 44224 receipt **signed by that
/// key** confirms the command succeeded — the rule the desktop coordination
/// fold applies (`sessionCoordinationFold.ts`, `acceptedGenerations`) and
/// the Pulse fold mirrors, here without the per-project scoping neither
/// admission needs:
///
/// * a `session.create` counts when its `sessionRef` **and** `genesisRef`
///   name this mission (the provider's own context projector requires both)
///   and its receipt reports `created` or `created_with_failed_initial_turn`
///   with a generation-1 target;
/// * a `session.resume` or `session.restart` counts when it targets an
///   execution an accepted create (or an accepted earlier resume or
///   restart) of this mission produced, at
///   exactly that execution's accepted generation, and its receipt reports
///   `resumed` or `resumed_without_context` for the next generation;
/// * a `session.hire` names no provider and contributes no provider authority;
///   a create's `hireRef` is attribution only. The create still needs a signer
///   entitled to steer the mission, and it is that create which names the key;
/// * a `session.stop` confirms nothing.
///
/// # Who may commission (2026-09-05 refuter, B1)
///
/// A receipt signed by the provider the command names proves that *that key*
/// answered *that command*. It proves nothing about who was entitled to issue
/// the command, and until this parameter existed nobody asked: any channel
/// member could publish a `session.create` naming **itself** as
/// `providerAuthorityPubkey`, answer it with its own receipt, and become a
/// provider of a mission it has nothing to do with — and then sign its own
/// `observed` gate rows, which arm (B) lands.
///
/// So a create or a resume counts only when **its signer may steer the
/// mission**: `commissioners` is the mission's founder together with the keys
/// its accepted authority chain grants `operator` (NIP-CSP § Validation
/// boundary), resolved by the caller because this crate cannot read that
/// chain. A create signed by anybody else names no provider, even when its
/// public `hireRef` names a genuine hire. In the current hire path a lead may
/// request a seat and the founder's host signs the fulfillment create, so lead
/// hiring remains valid without making the hire event id a bearer capability.
///
/// An empty `commissioners` therefore yields an empty provider set, and an
/// empty provider set folds every `observed` claim down to `declared` — the
/// fail-closed direction, and the honest one for a caller that could not
/// resolve who steers.
///
/// A command answered by two receipts, or a command id carried by two
/// commands, is ambiguous and accepted as nothing, exactly as the desktop
/// fold treats it. Commands and receipts pair within one channel (`h`) and
/// under one `csl-command`; the receipt's `csl-command` must equal its own
/// `commandId`.
///
/// The answer is a `Vec` and never an `Option`: an empty set is "no lifecycle
/// record proves a provider on this page", and folding observations with
/// `Some(empty)` folds every `observed` claim down to `declared` — the
/// fail-closed direction.
pub fn mission_provider_pubkeys_from_lifecycle(
    session_ref: &str,
    genesis_ref: &str,
    commissioners: &[String],
    commands: &[Event],
    receipts: &[Event],
) -> Vec<String> {
    let pairs: Vec<LifecyclePair<'_>> = accepted_lifecycle_pairs(commands, receipts)
        .into_iter()
        .filter(|pair| commissioned(pair, commissioners))
        .collect();
    let mut providers: Vec<String> = Vec::new();
    // (channel, driver, instance, session) → the accepted generation.
    let mut executions: Vec<(String, u64)> = Vec::new();
    let add = |providers: &mut Vec<String>, pubkey: &str| {
        let pubkey = pubkey.to_ascii_lowercase();
        if !providers.iter().any(|held| held == &pubkey) {
            providers.push(pubkey);
        }
    };

    for pair in &pairs {
        let CodingSessionLifecycleAction::SessionCreate {
            session_ref: named_session,
            genesis_ref: named_genesis,
            provider_authority_pubkey,
            ..
        } = &pair.action
        else {
            continue;
        };
        if named_session.as_deref() != Some(session_ref)
            || named_genesis.as_deref() != Some(genesis_ref)
            || pair.receipt_target.generation != 1
        {
            continue;
        }
        let key = execution_key(pair.command, &pair.receipt_target);
        if executions.iter().any(|(held, _)| held == &key) {
            // Two accepted creates for one execution: neither is proof.
            executions.retain(|(held, _)| held != &key);
            continue;
        }
        executions.push((key, 1));
        add(&mut providers, provider_authority_pubkey);
    }

    let mut changed = true;
    while changed {
        changed = false;
        for pair in &pairs {
            // A restart or a rewind mints the next generation exactly as a resume does.
            let (CodingSessionLifecycleAction::SessionResume {
                session: previous,
                provider_authority_pubkey,
            }
            | CodingSessionLifecycleAction::SessionRestart {
                session: previous,
                provider_authority_pubkey,
            }
            | CodingSessionLifecycleAction::SessionRewind {
                session: previous,
                provider_authority_pubkey,
                ..
            }) = &pair.action
            else {
                continue;
            };
            let key = execution_key(pair.command, previous);
            let Some(slot) = executions.iter_mut().find(|(held, _)| held == &key) else {
                continue;
            };
            if slot.1 != previous.generation
                || execution_key(pair.command, &pair.receipt_target) != key
                || pair.receipt_target.generation != previous.generation + 1
            {
                continue;
            }
            slot.1 = pair.receipt_target.generation;
            add(&mut providers, provider_authority_pubkey);
            changed = true;
        }
    }
    providers
}

/// Every command/receipt pair that is unambiguous, signed by the command's
/// named provider, and reports success for its action.
fn accepted_lifecycle_pairs<'a>(
    commands: &'a [Event],
    receipts: &'a [Event],
) -> Vec<LifecyclePair<'a>> {
    let mut decoded: Vec<(&'a Event, CodingSessionLifecycleCommandPayload, String)> = Vec::new();
    for event in commands {
        let Ok(payload) = decode_coding_session_lifecycle_command(&event.content) else {
            continue;
        };
        let Some(channel) = tag_value(event, "h") else {
            continue;
        };
        if !super::has_exact_tag(event, "csl-command", &payload.command_id) {
            continue;
        }
        decoded.push((event, payload, channel.to_owned()));
    }
    let mut answers: Vec<(&'a Event, LifecycleReceipt, String)> = Vec::new();
    for event in receipts {
        let Ok(payload) = decode_coding_session_lifecycle_receipt(&event.content) else {
            continue;
        };
        let Some(channel) = tag_value(event, "h") else {
            continue;
        };
        if !super::has_exact_tag(event, "csl-command", &payload.command_id) {
            continue;
        }
        answers.push((event, payload, channel.to_owned()));
    }

    let mut pairs = Vec::new();
    for (command, payload, channel) in &decoded {
        let same_id = |held_channel: &str, held_id: &str| {
            held_channel == channel && held_id == payload.command_id
        };
        if decoded
            .iter()
            .filter(|(_, other, other_channel)| same_id(other_channel, &other.command_id))
            .count()
            != 1
        {
            continue;
        }
        let mut matching = answers
            .iter()
            .filter(|(_, receipt, receipt_channel)| same_id(receipt_channel, &receipt.command_id));
        let (Some((receipt, receipt_payload, _)), None) = (matching.next(), matching.next()) else {
            continue;
        };
        let succeeded = match &payload.action {
            CodingSessionLifecycleAction::SessionCreate { .. } => matches!(
                receipt_payload.status,
                ReceiptStatus::Created | ReceiptStatus::CreatedWithFailedInitialTurn
            ),
            // A restart (and a rewind) is a resume that detached first; the
            // provider answers each with the resume receipts.
            CodingSessionLifecycleAction::SessionResume { .. }
            | CodingSessionLifecycleAction::SessionRestart { .. }
            | CodingSessionLifecycleAction::SessionRewind { .. } => matches!(
                receipt_payload.status,
                ReceiptStatus::Resumed | ReceiptStatus::ResumedWithoutContext
            ),
            CodingSessionLifecycleAction::SessionHire { .. }
            | CodingSessionLifecycleAction::SessionStop { .. } => false,
        };
        let Some(authority) = lifecycle_authority(&payload.action) else {
            continue;
        };
        if !succeeded || !receipt.pubkey.to_hex().eq_ignore_ascii_case(authority) {
            continue;
        }
        let Some(target) = receipt_payload.session.clone() else {
            continue;
        };
        pairs.push(LifecyclePair {
            command,
            action: payload.action.clone(),
            receipt_target: target,
        });
    }
    pairs
}

/// Whether this pair's **command** was issued by someone entitled to issue it.
///
/// One way only: the command's signer may steer the mission. `hireRef` is a
/// public attribution link, not an authorization token. The named provider's
/// distinct receipt signature is still required by [`accepted_lifecycle_pairs`].
fn commissioned(pair: &LifecyclePair<'_>, commissioners: &[String]) -> bool {
    may_commission(&pair.command.pubkey.to_hex(), commissioners)
}

/// Whether `signer` is one of the keys that may steer this mission.
fn may_commission(signer: &str, commissioners: &[String]) -> bool {
    commissioners
        .iter()
        .any(|held| held.eq_ignore_ascii_case(signer))
}

/// The provider a lifecycle command names, or `None` for a hire.
fn lifecycle_authority(action: &CodingSessionLifecycleAction) -> Option<&str> {
    match action {
        CodingSessionLifecycleAction::SessionCreate {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionResume {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionRestart {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionRewind {
            provider_authority_pubkey,
            ..
        }
        | CodingSessionLifecycleAction::SessionStop {
            provider_authority_pubkey,
            ..
        } => Some(provider_authority_pubkey),
        CodingSessionLifecycleAction::SessionHire { .. } => None,
    }
}

/// One execution, keyed the way the desktop fold keys it: channel, driver,
/// instance and session id — the generation is what changes along the chain.
fn execution_key(command: &Event, target: &CodingSessionTarget) -> String {
    format!(
        "{}|{}:{}|{}:{}|{}:{}",
        tag_value(command, "h").unwrap_or_default(),
        target.driver.len(),
        target.driver,
        target.instance_id.len(),
        target.instance_id,
        target.session_id.len(),
        target.session_id
    )
}

/// The value of the first two-field tag named `name`.
fn tag_value<'a>(event: &'a Event, name: &str) -> Option<&'a str> {
    event.tags.iter().find_map(|tag| match tag.as_slice() {
        [tag_name, value] if tag_name == name => Some(value.as_str()),
        _ => None,
    })
}
