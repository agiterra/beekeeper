//! **Where** a verdict-gated push looks for a mission (finding 56).
//!
//! Live run 4 (2026-09-03, 17:40) refused a seat's push of a commit its own
//! mission had watched three gates pass on, with *"Searched 0 mission(s) — the
//! newest 16 on this channel"*. The founder set was right; the lookup was
//! wrong. Every coding session lives in **its own** channel, and
//! [`super::search_verdict_admission`] only ever read the channel the
//! repository's `buzz-channel` tag names, so arms (B) and (C) could not find a
//! real mission however green it was.
//!
//! The gate now discovers candidates by **who is pushing**, in three steps,
//! and says in the refusal which one ran.
//!
//! # 0. The channels this repository grants (finding 91)
//!
//! Every step below is intersected with one set first: the transport channels
//! of the project the announcement back-references, plus the channel its
//! `buzz-channel` tag binds. That set is what this repository has explicitly
//! granted, and a mission outside it is not this repository's mission however
//! green it is. Before finding 91 step 1 returned the pusher's seats
//! **community-wide**, so a key seated on a mission of R1 offered that
//! mission's rows to a push of R2 whenever one founder founded both. The
//! narrowing is disclosed in the scope's own `source` sentence
//! ([`VerdictAdmissionCandidateSource::SeatOfMissionInScope`]), because a
//! seat count that has been narrowed must not read as every seat the key
//! holds.
//!
//! A repository that grants no channel at all — no project, no binding — has
//! nowhere to look, and the search returns `None` rather than the community.
//!
//! # 1. The pusher's own seats
//!
//! The authoritative record that a key is seated is the kind:44228
//! `grant-seat` transition its mission's founder signed — the same chain
//! [`beekeeper_db::coding_session_acl::session_authority_for_hire`] folds into the
//! seat list the rule already checks. So the lookup is: the newest
//! [`VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS`] kind-44228 events in this
//! community, kept where the grantee is the pusher, reduced to the newest
//! [`VERDICT_ADMISSION_MAX_PUSHER_SEATS`] genesis references whose highest-seq
//! transition for that key is a grant rather than a revocation.
//!
//! **Why 44228 and not the 44221 hire / 44224 receipt.** Both would work as
//! evidence, and neither is cheaper: no coding-session kind carries the seat's
//! pubkey in a `p` tag, so none of them is reachable through the indexed
//! `event_mentions` join, and the hire's grantee lives in a JSON body exactly
//! as the transition's does. 44228 is chosen because it is the record the
//! *relay itself* accepted into a serialized chain — a hire the relay refused
//! still exists as an event, and a receipt is the host's word. One indexed
//! read serves it: `idx_events_community_kind_created` is
//! `(community_id, kind, created_at DESC)`, which is exactly this query.
//!
//! # 2. The project's session channels
//!
//! A pusher holding no seat at all — a human collaborator, a CI key — falls
//! back to the project the announcement's `["project", …]` back-reference
//! names, and to the transport channels that project owns (bounded by
//! [`VERDICT_ADMISSION_MAX_PROJECT_CHANNELS`]).
//!
//! # 3. The bound channel
//!
//! What the gate used to do, kept last for a repository in no project.
//!
//! Every bound here fails in the **refusing** direction: a lookup that read
//! less than exists can only miss a ruling and deny a push it might have
//! admitted. None of them can manufacture an admission, because the rule still
//! requires the mission's founder to be a founder of this repository and the
//! pusher to hold an active seat of the mission that admits it.

use std::collections::HashMap;
use std::sync::Arc;

use uuid::Uuid;

use beekeeper_core::coding_session_authority_transition::{
    decode_coding_session_authority_transition, CodingSessionAuthorityTransitionType,
};
use beekeeper_core::coding_session_verdict_admission::{
    VerdictAdmissionCandidateSource, VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS,
    VERDICT_ADMISSION_MAX_PROJECT_CHANNELS, VERDICT_ADMISSION_MAX_PUSHER_SEATS,
};
use beekeeper_core::kind::KIND_CODING_SESSION_AUTHORITY_TRANSITION;
use beekeeper_db::EventQuery;

use crate::state::AppState;

/// The missions one gated ref update may be judged by, and how they were found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerdictSearchScope {
    /// Channels whose stored events the search may read. Never empty, and
    /// always a subset of [`VerdictSearchScope::granted_channels`].
    pub channels: Vec<Uuid>,
    /// Every channel this repository grants: its project's transport
    /// channels, plus the channel its `buzz-channel` tag binds (finding 91).
    ///
    /// Resolved once per push and carried out of here because the candidate
    /// binding needs the same set: a mission published in one of these
    /// channels is a mission of this repository's own project, and that is
    /// half of what [`beekeeper_core::coding_session_verdict_admission::VerdictAdmissionCandidate::bound_repositories`]
    /// means. Never empty when the scope is `Some`.
    pub granted_channels: Vec<Uuid>,
    /// The exact genesis event ids to consider, when the seats lookup found
    /// them. Empty means "every genesis on `channels` a founder signed" — the
    /// project and bound-channel lookups, which have no id list of their own.
    pub genesis_ids: Vec<Vec<u8>>,
    /// Which lookup produced this scope, for the refusal's own sentence.
    pub source: VerdictAdmissionCandidateSource,
}

/// Resolve the scope for one push, or `None` when there is nowhere to look.
///
/// `Err(())` is a storage failure and fails the push closed: a page the relay
/// could not read is not evidence that a mission does not exist.
pub async fn resolve_candidate_scope(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    channel_id: Option<Uuid>,
    project_ref: Option<&str>,
    pusher_pubkey: &str,
) -> Result<Option<VerdictSearchScope>, ()> {
    // Step 0 (finding 91). Everything below is intersected with this set, so
    // no branch can return the community.
    let project_channels = match project_ref {
        Some(project) => match state
            .db
            .project_session_channel_ids(
                community,
                project,
                VERDICT_ADMISSION_MAX_PROJECT_CHANNELS as i64,
            )
            .await
        {
            Ok(channels) => channels,
            Err(error) => {
                tracing::error!(error = %error, "verdict admission: project session channels failed");
                return Err(());
            }
        },
        None => Vec::new(),
    };
    let mut granted: Vec<Uuid> = project_channels.clone();
    if let Some(bound) = channel_id {
        if !granted.contains(&bound) {
            granted.push(bound);
        }
    }
    if granted.is_empty() {
        // No project and no binding: this repository has granted no channel,
        // so there is nowhere a mission of *this* repository could live.
        return Ok(None);
    }

    let seated = pusher_seat_missions(state, community, pusher_pubkey).await?;
    let held = seated.len();
    let seated: Vec<(Uuid, Vec<u8>)> = seated
        .into_iter()
        .filter(|(channel, _)| granted.contains(channel))
        .collect();
    if !seated.is_empty() {
        let seats = seated.len();
        let mut channels: Vec<Uuid> = Vec::with_capacity(seats);
        for (channel, _) in &seated {
            if !channels.contains(channel) {
                channels.push(*channel);
            }
        }
        return Ok(Some(VerdictSearchScope {
            channels,
            granted_channels: granted,
            genesis_ids: seated.into_iter().map(|(_, genesis)| genesis).collect(),
            source: VerdictAdmissionCandidateSource::SeatOfMissionInScope {
                seat: short_key(pusher_pubkey),
                seats,
                held,
                within: narrowing_clause(project_ref, project_channels.len(), channel_id),
            },
        }));
    }
    if held > 0 {
        // The key is seated — somewhere else. The two fall-backs below say
        // "this key holds no seat", which would be false here, so the search
        // reads this repository's own channels under the narrowed source and
        // the sentence says exactly what happened.
        return Ok(Some(VerdictSearchScope {
            channels: granted.clone(),
            granted_channels: granted,
            genesis_ids: Vec::new(),
            source: VerdictAdmissionCandidateSource::SeatOfMissionInScope {
                seat: short_key(pusher_pubkey),
                seats: 0,
                held,
                within: narrowing_clause(project_ref, project_channels.len(), channel_id),
            },
        }));
    }

    if let Some(project) = project_ref {
        if !project_channels.is_empty() {
            return Ok(Some(VerdictSearchScope {
                source: VerdictAdmissionCandidateSource::ProjectSessions {
                    project: project.to_owned(),
                    channels: project_channels.len(),
                },
                channels: project_channels,
                granted_channels: granted,
                genesis_ids: Vec::new(),
            }));
        }
    }

    Ok(channel_id.map(|channel| VerdictSearchScope {
        channels: vec![channel],
        granted_channels: granted,
        genesis_ids: Vec::new(),
        source: VerdictAdmissionCandidateSource::BoundChannel,
    }))
}

/// How the seat lookup was narrowed, as the refusal sentence prints it.
///
/// Names both halves of the grant when both exist, because a reader who is
/// told only about the project cannot tell whether the bound channel was
/// searched.
fn narrowing_clause(
    project_ref: Option<&str>,
    project_channels: usize,
    channel_id: Option<Uuid>,
) -> String {
    match (project_ref, channel_id) {
        (Some(project), Some(channel)) => format!(
            "the {project_channels} session channel(s) of {project} and the channel this \
             repository binds ({channel})"
        ),
        (Some(project), None) => {
            format!("the {project_channels} session channel(s) of {project}")
        }
        (None, Some(channel)) => {
            format!("the channel this repository binds ({channel})")
        }
        // Unreachable: an empty grant returned `None` above.
        (None, None) => "no channel this repository grants".to_owned(),
    }
}

/// The missions whose accepted authority chain currently seats `pusher`,
/// newest first, bounded by [`VERDICT_ADMISSION_MAX_PUSHER_SEATS`].
///
/// Returns `(channel, genesis event id)` pairs. A key whose newest transition
/// for a chain is a `revoke-seat` is not seated by it and its mission is not
/// searched — which is the same conclusion the relay's own seat projection
/// reaches, one step earlier.
async fn pusher_seat_missions(
    state: &Arc<AppState>,
    community: beekeeper_core::CommunityId,
    pusher_pubkey: &str,
) -> Result<Vec<(Uuid, Vec<u8>)>, ()> {
    let transitions = match state
        .db
        .query_events(&EventQuery {
            kinds: Some(vec![KIND_CODING_SESSION_AUTHORITY_TRANSITION as i32]),
            limit: Some(VERDICT_ADMISSION_MAX_AUTHORITY_TRANSITIONS as i64),
            ..EventQuery::for_community(community)
        })
        .await
    {
        Ok(events) => events,
        Err(error) => {
            tracing::error!(error = %error, "verdict admission: authority-transition query failed");
            return Err(());
        }
    };

    // Newest-first order is the page's order, and the *seat* question is
    // answered by the highest `seq` this key holds on each chain — a grant
    // that a later revocation superseded seats nobody.
    let mut order: Vec<(Uuid, Vec<u8>)> = Vec::new();
    let mut newest: HashMap<Vec<u8>, (u32, bool)> = HashMap::new();
    for stored in &transitions {
        let Some(channel) = stored.channel_id else {
            continue;
        };
        let Ok(payload) = decode_coding_session_authority_transition(&stored.event.content) else {
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
        let Ok(genesis) = hex::decode(&payload.genesis_ref) else {
            continue;
        };
        if genesis.len() != 32 {
            continue;
        }
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

/// The first 8 hex characters of a key, lowercased — how every refusal in this
/// module names one.
fn short_key(pubkey: &str) -> String {
    pubkey.chars().take(8).collect::<String>().to_lowercase()
}

#[cfg(test)]
#[path = "verdict_admission_scope_tests.rs"]
mod tests;
