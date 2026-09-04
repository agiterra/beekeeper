//! Where `bee git check --ref` looks for a mission (finding 56).
//!
//! The prediction has to run the relay's lookup, not a lookup of its own, or
//! it predicts a refusal the relay would never give — which is how live run 4
//! ended with "Searched 0 mission(s)" for a mission that was sitting there,
//! green, in its own channel.
//!
//! The three steps are
//! [`buzz_relay`'s](../../../buzz_relay/api/git/verdict_admission_scope/index.html)
//! in the same order: the pusher's own seats, then the project's session
//! channels, then the repository's bound channel. What differs is the
//! **inputs**, and both differences under-promise rather than over-promise:
//!
//! - seats are read from the kind:44228 chain on the wire, where the relay
//!   folds its own accepted projection (the same prediction-grade gap
//!   `active_seats_from_authority_transitions` already discloses);
//! - a project's session channels are read from the relay-signed kind:39000
//!   channel metadata, where the relay reads the `channels` table. A channel
//!   whose 39000 has not been emitted is invisible here, so the prediction can
//!   only miss a mission and predict a refusal that does not come.

use buzz_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use buzz_core::coding_session_verdict_admission::{
    VerdictAdmissionCandidateSource, VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS,
    VERDICT_ADMISSION_MAX_PROJECT_CHANNELS, VERDICT_ADMISSION_MAX_PUSHER_SEATS,
};
use buzz_core::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION;
use std::collections::HashMap;

use crate::client::BuzzClient;
use crate::error::CliError;

/// How many kind:39000 channel-metadata events one prediction may page through
/// looking for a project's session channels.
///
/// The `project` back-reference is not a single-letter tag, so it is not a
/// Nostr filter; the projection is paged and filtered here. Sixteen pages'
/// worth of the per-project bound is the disclosed ceiling, and reading fewer
/// can only shrink the search.
const PREDICTION_MAX_CHANNEL_METADATA: u32 = 512;

/// Kind of the NIP-29 group-metadata event the relay signs per channel.
const KIND_CHANNEL_METADATA: u32 = 39000;

/// The missions one prediction may be judged by, and how they were found.
pub(crate) struct PredictionScope {
    /// Channel ids to search, as the `#h` filter wants them.
    pub(crate) channels: Vec<String>,
    /// Exact genesis event ids, when the seats lookup found them; empty for
    /// the two channel-shaped lookups.
    pub(crate) genesis_ids: Vec<String>,
    /// Which lookup ran, for the refusal sentence and `prediction.lookup`.
    pub(crate) source: VerdictAdmissionCandidateSource,
}

/// Resolve the scope, or `None` when there is nowhere to look at all.
pub(crate) async fn resolve_prediction_scope(
    client: &BuzzClient,
    bound_channel: Option<&str>,
    project_ref: Option<&str>,
    pusher_pubkey: &str,
) -> Result<Option<PredictionScope>, CliError> {
    let seated = pusher_seat_missions(client, pusher_pubkey).await?;
    if !seated.is_empty() {
        let seats = seated.len();
        let mut channels: Vec<String> = Vec::with_capacity(seats);
        for (channel, _) in &seated {
            if !channels.contains(channel) {
                channels.push(channel.clone());
            }
        }
        return Ok(Some(PredictionScope {
            channels,
            genesis_ids: seated.into_iter().map(|(_, genesis)| genesis).collect(),
            source: VerdictAdmissionCandidateSource::SeatOfMission {
                seat: short_key(pusher_pubkey),
                seats,
            },
        }));
    }

    if let Some(project) = project_ref {
        let channels = project_session_channels(client, project).await?;
        if !channels.is_empty() {
            return Ok(Some(PredictionScope {
                source: VerdictAdmissionCandidateSource::ProjectSessions {
                    project: project.to_owned(),
                    channels: channels.len(),
                },
                channels,
                genesis_ids: Vec::new(),
            }));
        }
    }

    Ok(bound_channel.map(|channel| PredictionScope {
        channels: vec![channel.to_owned()],
        genesis_ids: Vec::new(),
        source: VerdictAdmissionCandidateSource::BoundChannel,
    }))
}

/// `(channel, genesis event id)` for each mission that currently seats
/// `pusher`, newest first and bounded like the relay's own lookup.
async fn pusher_seat_missions(
    client: &BuzzClient,
    pusher_pubkey: &str,
) -> Result<Vec<(String, String)>, CliError> {
    let rows = client
        .query_paginated(
            serde_json::json!({ "kinds": [KIND_CODING_SESSION_AUTHORITY_TRANSITION] }),
            VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS as u32,
        )
        .await?;

    let mut order: Vec<(String, String)> = Vec::new();
    let mut newest: HashMap<String, (u32, bool)> = HashMap::new();
    for row in rows {
        let Ok(event) = serde_json::from_value::<nostr::Event>(row) else {
            continue;
        };
        let Ok(payload) = decode_coding_session_authority_transition(&event.content) else {
            continue;
        };
        let granted = match payload.transition_type {
            CodingSessionAuthorityTransitionType::GrantSeat => true,
            CodingSessionAuthorityTransitionType::RevokeSeat => false,
            _ => continue,
        };
        if !payload.grantee_pubkey.eq_ignore_ascii_case(pusher_pubkey) {
            continue;
        }
        let Some(channel) = h_tag(&event) else {
            continue;
        };
        let genesis = payload.genesis_ref.clone();
        match newest.get(&genesis) {
            Some((seq, _)) if *seq >= payload.seq => {}
            _ => {
                newest.insert(genesis.clone(), (payload.seq, granted));
            }
        }
        if !order.iter().any(|(_, id)| id == &genesis) {
            order.push((channel, genesis));
        }
    }

    Ok(order
        .into_iter()
        .filter(|(_, genesis)| newest.get(genesis).map(|(_, granted)| *granted) == Some(true))
        .take(VERDICT_ADMISSION_MAX_PUSHER_SEATS)
        .collect())
}

/// The session (transport) channels of one project, from the relay-signed
/// kind:39000 projection.
async fn project_session_channels(
    client: &BuzzClient,
    project_ref: &str,
) -> Result<Vec<String>, CliError> {
    let rows = client
        .query_paginated(
            serde_json::json!({ "kinds": [KIND_CHANNEL_METADATA] }),
            PREDICTION_MAX_CHANNEL_METADATA,
        )
        .await?;
    let mut channels = Vec::new();
    for row in rows {
        let Ok(event) = serde_json::from_value::<nostr::Event>(row) else {
            continue;
        };
        let mut names_project = false;
        let mut is_transport = false;
        let mut channel: Option<String> = None;
        for tag in event.tags.iter() {
            match tag.as_slice() {
                [name, value] if name == "project" && value == project_ref => names_project = true,
                [name, value] if name == "t" && value == "transport" => is_transport = true,
                [name, value] if name == "d" => channel = Some(value.clone()),
                _ => {}
            }
        }
        if !names_project || !is_transport {
            continue;
        }
        if let Some(channel) = channel {
            if !channels.contains(&channel) {
                channels.push(channel);
            }
        }
        if channels.len() >= VERDICT_ADMISSION_MAX_PROJECT_CHANNELS {
            break;
        }
    }
    Ok(channels)
}

/// The channel an event was published in, from its NIP-29 `h` tag.
pub(crate) fn h_tag(event: &nostr::Event) -> Option<String> {
    event.tags.iter().find_map(|tag| match tag.as_slice() {
        [name, value] if name == "h" => Some(value.clone()),
        _ => None,
    })
}

/// The first 8 hex characters of a key, lowercased — how the refusal names one.
fn short_key(pubkey: &str) -> String {
    pubkey.chars().take(8).collect::<String>().to_lowercase()
}
