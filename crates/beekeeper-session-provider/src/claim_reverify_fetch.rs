//! The tick's authority-chain re-read, run off the run loop (SV-76).
//!
//! An umbrella whose chain could not be read at startup stays fenced — its
//! executions refuse by name and publish no metadata — until the tick reads it
//! clean ([`Provider::retry_pending_claim_verification`]). That read is paged
//! relay queries plus one lookup per accepted transition, and the tick used to
//! await all of it inline: a slow relay held transcript publication for every
//! session on the provider for as long as the read took, every tick.
//!
//! Now the tick only takes the relay's bytes off the loop
//! ([`Provider::start_claim_reverification`]). The verdict — every rule in
//! `backfill_session_authority` and `verify_umbrella_chain`, unchanged — is
//! reached on the loop when the read arrives
//! ([`Provider::finish_claim_reverification`]), from those bytes, by the same
//! code ([`ChainSource::Read`]).
//!
//! A read is applied only to the state it was taken for. Each umbrella's read
//! records the open records it covered and the chain head each had applied;
//! if any of that moved while the read was out (a live receipt was folded, an
//! execution opened or closed), the read is dropped for that umbrella, which
//! stays fenced and is read again on a later tick. Dropping is always the safe
//! direction: the fence stays up.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::time::Duration;

use beekeeper_acp::relay::RestClient;
use beekeeper_core::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION;
use nostr::{Event, Kind};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::off_loop::OffLoopSlot;
use crate::{read_accepted_chain_as, AcceptedChain, Provider};

/// The longest one off-loop chain re-read may take, every umbrella together.
pub(crate) const CLAIM_REVERIFY_TIMEOUT: Duration = Duration::from_secs(60);

/// What one accepted-transition lookup answered: the event, not visible, or
/// why the lookup failed.
pub(crate) type TransitionLookup = Result<Option<Event>, String>;

/// Where the chain verification reads its bytes from.
pub(crate) enum ChainSource<'a> {
    /// The relay, now. Startup recovery and live receipts read this way.
    Live(&'a RestClient),
    /// A read already taken off the run loop.
    Read(&'a PrefetchedChains),
}

impl ChainSource<'_> {
    pub(crate) async fn chain(
        &self,
        relay_self: Option<String>,
        channel_id: Uuid,
        genesis_ref: &str,
    ) -> AcceptedChain {
        match self {
            Self::Live(rest) => {
                read_accepted_chain_as(relay_self, channel_id, genesis_ref, rest).await
            }
            Self::Read(read) => read.chains.get(&channel_id).cloned().unwrap_or_else(|| {
                AcceptedChain::incomplete(
                    "this channel's chain was not part of the re-read".to_owned(),
                )
            }),
        }
    }

    pub(crate) async fn transition(&self, accepted_event_id: &str) -> TransitionLookup {
        match self {
            Self::Live(rest) => rest
                .query_event_by_id(
                    accepted_event_id,
                    Kind::Custom(KIND_CODING_SESSION_AUTHORITY_TRANSITION as u16),
                )
                .await
                .map_err(|error| error.to_string()),
            // A link the read did not look up is a link this verdict cannot
            // resolve: the chain is not verified, and the fence stays up.
            Self::Read(read) => read
                .transitions
                .get(accepted_event_id)
                .cloned()
                .unwrap_or_else(|| {
                    Err("the accepted transition was not part of the re-read".to_owned())
                }),
        }
    }
}

/// One umbrella's chain bytes, read off the loop.
#[derive(Debug, Default)]
pub(crate) struct PrefetchedChains {
    chains: HashMap<Uuid, AcceptedChain>,
    transitions: HashMap<String, TransitionLookup>,
}

/// The open records an umbrella's read covered, and the head each had applied.
type Basis = BTreeMap<String, u32>;

/// What the read needs to know about one umbrella, taken on the loop.
struct UmbrellaPlan {
    genesis_ref: String,
    basis: Basis,
    /// Each channel its records live in, with the lowest head applied there:
    /// links at or below it are already folded everywhere and need no lookup.
    channels: BTreeMap<Uuid, u32>,
}

/// One umbrella's finished read.
pub(crate) struct UmbrellaRead {
    genesis_ref: String,
    basis: Basis,
    read: PrefetchedChains,
}

/// A finished re-read: every umbrella's bytes, or why none were taken.
pub(crate) type ClaimReverifyAnswer = Result<Vec<UmbrellaRead>, String>;

pub(crate) type ClaimReverifySlot = OffLoopSlot<ClaimReverifyAnswer>;

pub(crate) fn new_slot() -> ClaimReverifySlot {
    OffLoopSlot::new(CLAIM_REVERIFY_TIMEOUT)
}

async fn read_umbrella(rest: &RestClient, relay_self: &str, plan: UmbrellaPlan) -> UmbrellaRead {
    let mut read = PrefetchedChains::default();
    for (channel_id, lowest_applied) in plan.channels {
        let chain = read_accepted_chain_as(
            Some(relay_self.to_owned()),
            channel_id,
            &plan.genesis_ref,
            rest,
        )
        .await;
        if chain.incomplete.is_none() {
            for link in chain.links() {
                if link.seq <= lowest_applied
                    || read.transitions.contains_key(&link.accepted_event_id)
                {
                    continue;
                }
                let lookup = ChainSource::Live(rest)
                    .transition(&link.accepted_event_id)
                    .await;
                read.transitions
                    .insert(link.accepted_event_id.clone(), lookup);
            }
        }
        read.chains.insert(channel_id, chain);
    }
    UmbrellaRead {
        genesis_ref: plan.genesis_ref,
        basis: plan.basis,
        read,
    }
}

#[cfg(test)]
impl UmbrellaRead {
    /// A read of `genesis_ref` holding `chains`, taken against the provider's
    /// records as they stand now.
    pub(crate) fn taken_now(
        provider: &Provider,
        genesis_ref: &str,
        chains: Vec<(Uuid, AcceptedChain)>,
    ) -> Self {
        Self {
            genesis_ref: genesis_ref.to_owned(),
            basis: provider.claim_basis(genesis_ref),
            read: PrefetchedChains {
                chains: chains.into_iter().collect(),
                transitions: HashMap::new(),
            },
        }
    }
}

impl Provider {
    /// The run loop's queue of finished re-reads.
    pub(crate) fn take_claim_reverify_answers(
        &mut self,
    ) -> Option<mpsc::Receiver<ClaimReverifyAnswer>> {
        self.claim_reverify_fetch.take_receiver()
    }

    /// Whether a re-read is in flight.
    #[cfg(test)]
    pub(crate) fn claim_reverify_busy(&self) -> bool {
        self.claim_reverify_fetch.busy()
    }

    /// The open records of `genesis_ref` and the head each has applied.
    fn claim_basis(&self, genesis_ref: &str) -> Basis {
        self.state
            .sessions()
            .filter(|record| !record.closed && record.genesis_ref.as_deref() == Some(genesis_ref))
            .map(|record| (record.session_id.clone(), record.authority_seq))
            .collect()
    }

    /// Start re-reading every umbrella still waiting on its chain, and return
    /// at once. A no-op in the usual case (nothing pending), while a re-read
    /// is out, or before the relay identity is witnessed — the chain cannot
    /// verify without it, so reading would only spend the relay's time.
    pub(crate) fn start_claim_reverification(&mut self) {
        if self.claims_pending_reverification.is_empty() || self.claim_reverify_fetch.busy() {
            return;
        }
        let (Some(rest), Some(relay_self)) = (self.rest_client.clone(), self.relay_self.clone())
        else {
            return;
        };
        let pending: BTreeSet<String> =
            self.claims_pending_reverification.iter().cloned().collect();
        let plans: Vec<UmbrellaPlan> = pending
            .into_iter()
            .map(|genesis_ref| {
                let mut channels: BTreeMap<Uuid, u32> = BTreeMap::new();
                for record in self.state.sessions().filter(|record| {
                    !record.closed && record.genesis_ref.as_deref() == Some(genesis_ref.as_str())
                }) {
                    let lowest = channels
                        .entry(record.channel_id)
                        .or_insert(record.authority_seq);
                    *lowest = (*lowest).min(record.authority_seq);
                }
                UmbrellaPlan {
                    basis: self.claim_basis(&genesis_ref),
                    genesis_ref,
                    channels,
                }
            })
            .collect();
        let timeout = self.claim_reverify_fetch.timeout;
        self.claim_reverify_fetch.spawn(async move {
            let read = async {
                let mut reads = Vec::with_capacity(plans.len());
                for plan in plans {
                    reads.push(read_umbrella(&rest, &relay_self, plan).await);
                }
                reads
            };
            tokio::time::timeout(timeout, read).await.map_err(|_| {
                format!(
                    "the authority chain re-read did not finish within {}s",
                    timeout.as_secs_f64()
                )
            })
        });
    }

    /// Apply a finished re-read on the loop: the same verdict
    /// `retry_pending_claim_verification` reaches, from the bytes the read
    /// took, for each umbrella whose records did not move meanwhile.
    pub(crate) async fn finish_claim_reverification(&mut self, answer: ClaimReverifyAnswer) {
        self.claim_reverify_fetch.settle();
        let reads = match answer {
            Ok(reads) => reads,
            Err(reason) => {
                tracing::warn!(
                    target: "csp::authority",
                    "unverified authority chains stay fenced: {reason}"
                );
                return;
            }
        };
        for umbrella in reads {
            if !self
                .claims_pending_reverification
                .contains(&umbrella.genesis_ref)
            {
                continue; // Settled by another path while the read was out.
            }
            if self.claim_basis(&umbrella.genesis_ref) != umbrella.basis {
                tracing::debug!(
                    target: "csp::authority",
                    genesis_ref = %umbrella.genesis_ref,
                    "the umbrella's records moved while its chain was read; reading again"
                );
                continue;
            }
            if self
                .verify_umbrella_chain_from(
                    &umbrella.genesis_ref,
                    &ChainSource::Read(&umbrella.read),
                )
                .await
            {
                self.publish_reverified_metadata(&umbrella.genesis_ref);
            }
        }
    }
}
