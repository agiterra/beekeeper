//! Accepted whole-session deletion, consumed into durable local retirement.
//!
//! # The failure this exists to end
//!
//! A founder deleted an umbrella. The relay applied it: the genesis, the
//! transcript and every event the kind 5 named stopped being returned. Then a
//! provider that had been offline came back, ran
//! [`Provider::recover`](crate::Provider::recover), found its local records
//! still open, and published fresh `disconnected` metadata for all of them.
//! Those 44223s carried **new** event ids the deletion had never named, so
//! nothing was deleting them, and a session whose whole history was gone read
//! on every surface as an execution waiting to be resumed
//! (`review-2026-09-08-release-candidate/continuity-diagnosis.md`).
//!
//! Deletion was not the bug. The bug was that the provider had no way to
//! *witness* one: it subscribed to commands, team transactions, closures and
//! system messages, and a kind 5 is none of those.
//!
//! # What counts as authority, and what deliberately does not
//!
//! Two proofs, either sufficient, both requiring an authenticated read
//! (`docs/HANDOVER_IMPL.md` §3.2):
//!
//! 1. **A relay-signed deletion receipt** — kind 40099,
//!    `coding_session_deletion_accepted`, verified against the NIP-11 `self`
//!    key this provider witnessed at connect time, exactly as authority
//!    acceptance receipts are ([`crate::authority`]). This covers every
//!    deleter the relay authorizes, including a project owner who is not the
//!    founder.
//! 2. **A founder-signed kind 5 plus a confirmed absence** — a deletion
//!    request signed by *this record's own* `founder_pubkey`, naming the
//!    record's immutable `genesis_ref` in an `e` tag, **and** an exact-id read
//!    of that genesis returning zero rows. The signature says who asked; the
//!    absence says the relay agreed.
//!
//! Three things are never authority, and each has a test:
//!
//! - **A missing or failed read.** "I could not check" and "it was deleted"
//!   are different answers and only one of them stops a session. A query
//!   error, or a genesis that still reads back, leaves the record untouched.
//! - **An unaccepted signed request.** Anyone who can sign can publish a kind
//!   5 naming anything; a request by a non-founder, or a founder's request the
//!   relay did not apply, retires nothing.
//! - **Content text.** A kind 5's `content` is free-form prose. Nothing here
//!   reads it.
//!
//! # What retirement does
//!
//! Persists [`crate::state::Retirement`] on every local record of that
//! genesis, interrupts any open turn and releases the process the way closure
//! cleanup does, and stops there. No metadata is published, no seat request is
//! written, no restore is attempted, and every later command answers
//! [`crate::payload::SESSION_RETIRED`]. Nothing is republished, and no grant,
//! transcript or genesis is reconstructed to make a deleted session resumable.
//! It is idempotent: a replayed deletion or receipt finds the record already
//! retired and does nothing a second time.

use buzz_acp::relay::RestClient;
use buzz_core::coding_session_lease::CodingSessionLeaseState;
use buzz_core::kind::{KIND_CODING_SESSION_GENESIS, KIND_DELETION, KIND_SYSTEM_MESSAGE};
use nostr::{Event, Kind};
use uuid::Uuid;

use crate::{authority, now_ms, Provider};

/// Ceiling on rows read per channel when scanning for deletions.
///
/// Deletions are rare and a channel's kind 5 history is short, so this is a
/// generous bound rather than a paging cursor. It matches
/// the authority backfill's reasoning: one bounded read at
/// startup, not a crawl.
const DELETION_QUERY_LIMIT: usize = 500;

impl Provider {
    /// Reconcile accepted deletions into durable retirement, before anything
    /// is republished.
    ///
    /// Called from [`Provider::recover`] **ahead of** claim derivation and the
    /// stranded loop, which is the whole ordering guarantee: a record that has
    /// been deleted must be retired before the loop that publishes metadata
    /// can reach it. Best-effort by construction — a channel this provider
    /// cannot read leaves its records exactly as they were, because a failed
    /// read is not a deletion.
    pub(crate) async fn reconcile_retirements(&mut self, rest: &RestClient) {
        let channels: Vec<Uuid> = self
            .state
            .sessions()
            .filter(|record| !record.closed && !record.is_retired() && record.genesis_ref.is_some())
            .map(|record| record.channel_id)
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for channel_id in channels {
            if let Err(error) = self.reconcile_channel_retirements(channel_id, rest).await {
                tracing::warn!(
                    target: "csp::retirement",
                    %channel_id,
                    "deletion reconciliation failed, leaving this channel's records \
                     untouched: {error}"
                );
            }
        }
    }

    /// Reconcile one channel: relay receipts first, then founder requests.
    async fn reconcile_channel_retirements(
        &mut self,
        channel_id: Uuid,
        rest: &RestClient,
    ) -> anyhow::Result<()> {
        let genesis_refs: Vec<String> = self
            .state
            .sessions()
            .filter(|record| {
                record.channel_id == channel_id && !record.closed && !record.is_retired()
            })
            .filter_map(|record| record.genesis_ref.clone())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        if genesis_refs.is_empty() {
            return Ok(());
        }

        // (a) Relay-signed receipts. Read first because they are the stronger
        //     proof and the only one that covers a project-owner deletion.
        for (receipt_event_id, accepted) in self.accepted_deletion_receipts(channel_id, rest).await
        {
            if genesis_refs.contains(&accepted.genesis_ref) {
                self.retire_genesis(
                    &accepted.genesis_ref,
                    &accepted.deletion_event_id,
                    Some(&receipt_event_id),
                )?;
            }
        }

        // (b) Founder-signed requests, each confirmed by an absence.
        for genesis_ref in genesis_refs {
            if self.genesis_is_retired(&genesis_ref) {
                continue;
            }
            let Some(deletion) = self
                .founder_deletion_for(channel_id, &genesis_ref, rest)
                .await
            else {
                continue;
            };
            match rest
                .query_event_by_id(
                    &genesis_ref,
                    Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
                )
                .await
            {
                // Zero rows for an exact id, over an authenticated read: the
                // relay applied the request.
                Ok(None) => {
                    self.retire_genesis(&genesis_ref, &deletion, None)?;
                }
                // The genesis still reads back. The request exists; the relay
                // did not act on it. Nothing is retired.
                Ok(Some(_)) => tracing::info!(
                    target: "csp::retirement",
                    %genesis_ref,
                    deletion_event_id = %deletion,
                    "a founder-signed deletion names this genesis but the genesis still \
                     reads back, so nothing is retired"
                ),
                // "I could not check" is not "it was deleted".
                Err(error) => tracing::warn!(
                    target: "csp::retirement",
                    %genesis_ref,
                    "could not confirm whether the deleted genesis is absent, so nothing \
                     is retired: {error}"
                ),
            }
        }
        Ok(())
    }

    /// Every verified deletion receipt this channel holds.
    ///
    /// Discovery by query, authority by verification: the query surfaces
    /// candidate 40099s from the witnessed relay identity, and each one is
    /// checked against that identity's signature before any fact is read out
    /// of it. Returns nothing at all when no relay identity is witnessed —
    /// unverifiable receipts fail closed, exactly as authority receipts do.
    async fn accepted_deletion_receipts(
        &self,
        channel_id: Uuid,
        rest: &RestClient,
    ) -> Vec<(String, authority::AcceptedDeletion)> {
        use nostr::{Alphabet, SingleLetterTag};

        let Some(relay_self) = self.relay_self.clone() else {
            tracing::debug!(
                target: "csp::retirement",
                "no relay identity witnessed — deletion receipts cannot be verified"
            );
            return Vec::new();
        };
        let Ok(relay_author) = nostr::PublicKey::from_hex(&relay_self) else {
            return Vec::new();
        };
        let filter = nostr::Filter::new()
            .kind(Kind::Custom(KIND_SYSTEM_MESSAGE as u16))
            .author(relay_author)
            .custom_tags(
                SingleLetterTag::lowercase(Alphabet::H),
                [channel_id.to_string()],
            )
            .limit(DELETION_QUERY_LIMIT);
        let rows = match rest.query(&[filter]).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    target: "csp::retirement",
                    %channel_id,
                    "deletion-receipt query failed: {error}"
                );
                return Vec::new();
            }
        };
        let Some(rows) = rows.as_array() else {
            return Vec::new();
        };
        rows.iter()
            .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
            .filter_map(|event| {
                let accepted = authority::verify_deletion_receipt(&event, &relay_self).ok()?;
                (accepted.channel_id == channel_id).then(|| (event.id.to_hex(), accepted))
            })
            .collect()
    }

    /// The id of a kind 5, signed by this record's own founder, naming
    /// `genesis_ref` in an `e` tag — or `None`.
    ///
    /// Selection is by the locally recorded founder, never by whoever the
    /// event says it is: the record's `founder_pubkey` was resolved from the
    /// genesis when the execution was created, and it is the only signer whose
    /// deletion request this half of the rule accepts.
    async fn founder_deletion_for(
        &self,
        channel_id: Uuid,
        genesis_ref: &str,
        rest: &RestClient,
    ) -> Option<String> {
        use nostr::{Alphabet, SingleLetterTag};

        let founder = self
            .state
            .sessions()
            .find(|record| {
                record.channel_id == channel_id
                    && record.genesis_ref.as_deref() == Some(genesis_ref)
            })
            .and_then(|record| record.founder_pubkey.clone())?;
        let author = nostr::PublicKey::from_hex(&founder).ok()?;
        let filter = nostr::Filter::new()
            .kind(Kind::Custom(KIND_DELETION as u16))
            .author(author)
            .custom_tags(
                SingleLetterTag::lowercase(Alphabet::H),
                [channel_id.to_string()],
            )
            .limit(DELETION_QUERY_LIMIT);
        let rows = match rest.query(&[filter]).await {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(
                    target: "csp::retirement",
                    %channel_id,
                    "deletion-request query failed: {error}"
                );
                return None;
            }
        };
        rows.as_array()?
            .iter()
            .filter_map(|row| serde_json::from_value::<Event>(row.clone()).ok())
            .find(|event| deletion_names_genesis(event, &founder, genesis_ref))
            .map(|event| event.id.to_hex())
    }

    /// Whether any local record of this genesis is already retired.
    fn genesis_is_retired(&self, genesis_ref: &str) -> bool {
        self.state
            .sessions()
            .any(|record| record.genesis_ref.as_deref() == Some(genesis_ref) && record.is_retired())
    }

    /// Retire every local record rooted at `genesis_ref`.
    ///
    /// Umbrella-wide, like the claim: a whole-session deletion names the
    /// genesis, and every execution under it goes with it. Idempotent — a
    /// record that already carries a retirement keeps the one it has, so a
    /// replayed deletion writes nothing and releases nothing twice.
    pub(crate) fn retire_genesis(
        &mut self,
        genesis_ref: &str,
        deletion_event_id: &str,
        receipt_event_id: Option<&str>,
    ) -> anyhow::Result<()> {
        let session_ids: Vec<String> = self
            .state
            .sessions()
            .filter(|record| {
                record.genesis_ref.as_deref() == Some(genesis_ref) && !record.is_retired()
            })
            .map(|record| record.session_id.clone())
            .collect();
        if session_ids.is_empty() {
            return Ok(());
        }
        let retirement = crate::state::Retirement {
            deletion_event_id: deletion_event_id.to_owned(),
            receipt_event_id: receipt_event_id.map(str::to_owned),
            at: now_ms(),
        };
        for session_id in &session_ids {
            // Durable intent first, exactly as `release_settled_umbrella`
            // orders it: if this write fails the actor stays live and nothing
            // contradictory has escaped. `closed` is deliberately *not* set —
            // a retired record is not a stopped one, and conflating them would
            // let it answer `SESSION_CLOSED` instead of naming the deletion.
            self.state.update_session(session_id, |record| {
                record.retired = Some(retirement.clone());
                record.open_turn = None;
            })?;
            if self.sessions.handle(session_id).is_some() {
                // The lease is ephemeral (24223), so retracting a liveness
                // claim publishes nothing durable into a channel whose session
                // has been deleted; leaving it to its TTL would read as live
                // for three more minutes.
                if let Err(error) = self.queue_lease(session_id, CodingSessionLeaseState::Released)
                {
                    tracing::warn!(
                        target: "csp::retirement",
                        %session_id,
                        "retired session's lease release could not be queued, falling back \
                         to TTL expiry: {error}"
                    );
                }
                self.sessions.shutdown(session_id);
                self.discard_context_packages(session_id);
                self.forget_redactions(session_id);
            }
            tracing::warn!(
                target: "csp::retirement",
                %session_id,
                %genesis_ref,
                %deletion_event_id,
                receipt = receipt_event_id.unwrap_or("none"),
                "session retired by an accepted whole-session deletion; it publishes \
                 nothing further and every command answers SESSION_RETIRED"
            );
        }
        // A retired umbrella needs no claim re-verification: `SESSION_RETIRED`
        // is the stricter answer and it is already durable, so holding its
        // metadata behind a chain read would only delay a publish that is
        // never going to happen.
        self.claims_pending_reverification.remove(genesis_ref);
        // Those generations ask for no custody now, and a stale row would have
        // the host stage a key for a deleted session.
        self.publish_seat_requests();
        Ok(())
    }

    /// Consume one live event that might be a deletion or a deletion receipt.
    ///
    /// The live half of §3.2, verified exactly as the startup half is. A kind
    /// 5 still needs its absence confirmed by an authenticated read, because a
    /// request arriving on the subscription says only that it was published.
    pub(crate) async fn on_possible_deletion(
        &mut self,
        channel_id: Uuid,
        event: &Event,
    ) -> anyhow::Result<()> {
        let kind = u32::from(event.kind.as_u16());
        if kind == KIND_SYSTEM_MESSAGE {
            let Some(relay_self) = self.relay_self.clone() else {
                return Ok(());
            };
            let Ok(accepted) = authority::verify_deletion_receipt(event, &relay_self) else {
                return Ok(());
            };
            if accepted.channel_id != channel_id {
                return Ok(());
            }
            return self.retire_genesis(
                &accepted.genesis_ref,
                &accepted.deletion_event_id,
                Some(&event.id.to_hex()),
            );
        }
        if kind != KIND_DELETION {
            return Ok(());
        }
        let Some(rest) = self.rest_client.clone() else {
            tracing::debug!(
                target: "csp::retirement",
                "a deletion arrived with no relay reader, so its effect cannot be \
                 confirmed; startup reconciliation will read it"
            );
            return Ok(());
        };
        // Which of this channel's genesis refs this signer could be deleting.
        // Selection is by the *record's* founder, so a kind 5 from anybody
        // else matches nothing and is never read further.
        let signer = event.pubkey.to_hex();
        let candidates: Vec<String> = self
            .state
            .sessions()
            .filter(|record| {
                record.channel_id == channel_id
                    && !record.is_retired()
                    && record.founder_pubkey.as_deref() == Some(signer.as_str())
            })
            .filter_map(|record| record.genesis_ref.clone())
            .filter(|genesis_ref| deletion_names_genesis(event, &signer, genesis_ref))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        for genesis_ref in candidates {
            match rest
                .query_event_by_id(
                    &genesis_ref,
                    Kind::Custom(KIND_CODING_SESSION_GENESIS as u16),
                )
                .await
            {
                Ok(None) => self.retire_genesis(&genesis_ref, &event.id.to_hex(), None)?,
                Ok(Some(_)) => tracing::info!(
                    target: "csp::retirement",
                    %genesis_ref,
                    "a founder's deletion request arrived but the genesis still reads \
                     back, so nothing is retired"
                ),
                Err(error) => tracing::warn!(
                    target: "csp::retirement",
                    %genesis_ref,
                    "could not confirm the deletion took effect, so nothing is \
                     retired: {error}"
                ),
            }
        }
        Ok(())
    }
}

/// Whether `event` is a valid kind 5, signed by `founder`, naming
/// `genesis_ref` in an `e` tag.
///
/// The signature is checked here rather than trusted from the query: rows come
/// back as JSON and `RestClient::query` verifies nothing, so a relay (or
/// anything between) could otherwise hand this provider a forged request for
/// the one event id that stops a session.
fn deletion_names_genesis(event: &Event, founder: &str, genesis_ref: &str) -> bool {
    if u32::from(event.kind.as_u16()) != KIND_DELETION || event.pubkey.to_hex() != founder {
        return false;
    }
    if event.verify().is_err() {
        return false;
    }
    event.tags.iter().any(|tag| {
        let tag = tag.as_slice();
        tag.len() >= 2 && tag[0] == "e" && tag[1] == genesis_ref
    })
}
