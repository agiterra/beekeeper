//! The storage half of the verdict-gated push rule, plus how a refusal
//! reaches the person who typed `git push`.
//!
//! [`buzz_core::coding_session_verdict_admission`] holds the rule itself and
//! has no I/O. This module supplies it with candidates: the bounded set of
//! missions in the scope [`super::verdict_admission_scope`] resolves — the
//! pusher's own seats first, then the project's session channels, then the
//! repository's bound channel — whose founder is **a founder of the
//! repository**, each folded from stored events.
//!
//! # Who founds a repository
//!
//! Finding 33: keying that to the kind:30617 signer alone made a two-human
//! repository unlandable by one of them. [`resolve_repository_founders`] is
//! the one function the push path asks, and it composes three sources — the
//! announcement's signer, its NIP-34 `maintainers` tag, and every project
//! roster row whose git tier is Owner under
//! [`buzz_core::git_perms::git_role_for_project_role`] (commit `a56ad5d01`:
//! the roster is a first-class git ACL). `buzz-core` cannot reach the roster,
//! so the set is resolved here and passed to the predicate.
//!
//! # Cost
//!
//! One indexed query per kind over the resolved scope, plus the scope lookup
//! itself (one page of kind 44228 on `(community, kind, created_at)`), plus one
//! authority lookup per mission that published anything. Each page is split
//! into missions in memory because none of these kinds is addressable and the
//! umbrella label is therefore not a queryable column. A SHA→session
//! projection would make this O(1); it costs a migration whose numbering
//! collides with `vanilla/main`, so the caps are disclosed in the refusal
//! instead.
//!
//! **Nothing here runs unless a matching `buzz-protect` rule sets
//! `require-verdict`.** An ordinary push issues exactly the queries it issued
//! before this module existed — the founder resolution included, which is why
//! `policy.rs` calls [`resolve_repository_founders`] inside the gated branch
//! and not beside the role check.

use std::sync::Arc;

use axum::http::{header::CONTENT_TYPE, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use tracing::warn;
use uuid::Uuid;

use buzz_core::coding_session_genesis::decode_coding_session_genesis;
use buzz_core::coding_session_observation::{
    fold_coding_session_observation_page, CodingSessionObservationFoldContext,
};
use buzz_core::coding_session_team_transaction::CodingSessionTeamActiveSeat;
use buzz_core::coding_session_verdict_admission::{
    evaluate_verdict_admission, fold_candidate_records, mission_gate_policy, mission_observations,
    mission_provider_pubkeys, mission_transactions, verdict_admission_fold_context,
    VerdictAdmission, VerdictAdmissionCandidate, VerdictAdmissionCandidateSource,
    VerdictAdmissionQuery, VerdictAdmissionRefusal, VERDICT_ADMISSION_MAX_OBSERVATIONS,
    VERDICT_ADMISSION_MAX_POLICIES, VERDICT_ADMISSION_MAX_PROVIDER_METADATA,
    VERDICT_ADMISSION_MAX_SESSIONS, VERDICT_ADMISSION_MAX_TRANSACTIONS,
};
use buzz_core::git_perms::{Denial, EffectiveRules, ProtectionRule, RefUpdate};
use buzz_core::kind::{
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_OBSERVATION,
    KIND_CODING_SESSION_POLICY, KIND_CODING_SESSION_TEAM_TRANSACTION,
};
use buzz_core::repository_founders::RepositoryFounders;
use buzz_db::EventQuery;

use crate::api::git::verdict_admission_scope::resolve_candidate_scope;
use crate::state::AppState;

/// Header carrying the structured denial list a machine reader wants.
///
/// The 403 **body** is plain text so an unmodified pre-receive hook — the one
/// already installed in every repository — prints readable lines when it
/// `cat`s the response. The JSON that body used to be moves here verbatim, so
/// nothing that parsed it loses the shape; it is omitted above
/// [`MAX_DENIAL_HEADER_BYTES`], the body being complete on its own.
pub const GIT_DENIALS_HEADER: &str = "x-buzz-git-denials";

/// Above this, the structured header is dropped rather than truncated.
pub const MAX_DENIAL_HEADER_BYTES: usize = 8 * 1024;

/// What the storage-backed search concluded for one ref update.
#[derive(Debug, Clone)]
pub enum VerdictSearch {
    /// A canonical ruling admits the update.
    Admitted,
    /// It does not; this is the exact sentence the pusher sees.
    Refused(VerdictAdmissionRefusal),
    /// Storage failed. Fail closed, with the handler's generic body.
    Unavailable,
}

/// One ref update, and the repository facts the policy handler already
/// resolved for it.
pub struct VerdictSearchRequest<'a> {
    /// Server-resolved tenant.
    pub community: buzz_core::CommunityId,
    /// The repository's resolved `buzz-channel` binding. Since finding 56
    /// this is the **last** place the gate looks, not the first.
    pub channel_id: Option<Uuid>,
    /// The repository announcement's `["project", …]` back-reference, already
    /// normalized by [`buzz_core::kind::repo_project_ref`]. The fallback
    /// lookup for a pusher who holds no seat.
    pub project_ref: Option<&'a str>,
    /// Every founder of the repository, resolved once per push by
    /// [`resolve_repository_founders`].
    pub founders: &'a RepositoryFounders,
    /// Full ref name being updated.
    pub ref_name: &'a str,
    /// The object id the update would leave on it.
    pub new_oid: &'a str,
    /// The authenticated pusher, hex.
    pub pusher_pubkey: &'a str,
}

/// The updates in one push that the verdict gate must search for.
///
/// The gate's cost claim rests on this function: it is the **only** thing that
/// decides whether a session query happens at all, and it returns nothing
/// unless a matching `buzz-protect` rule sets `require-verdict`. An update the
/// role check already denied is excluded too — the rule subtracts, and there
/// is nothing left to subtract from.
pub fn refs_requiring_verdict<'a>(
    updates: &'a [RefUpdate],
    rules: &[ProtectionRule],
    denials: &[Denial],
) -> Vec<&'a RefUpdate> {
    updates
        .iter()
        .filter(|update| {
            !denials
                .iter()
                .any(|denial| denial.ref_name == update.ref_name)
                && EffectiveRules::for_ref(&update.ref_name, rules).require_verdict
        })
        .collect()
}

/// Who founds this repository: signer ∪ NIP-34 `maintainers` ∪ roster Owners.
///
/// The single function the push path asks "who speaks for this repository",
/// so the verdict gate and any future caller cannot answer it two ways. The
/// announcement half is [`RepositoryFounders::from_announcement`]; the roster
/// half is the project the announcement's `["project", …]` back-reference
/// names, read through [`buzz_db::Db::get_project_roster`] — the same rows
/// `get_project_role_by_coordinate` authorizes pushes against (`a56ad5d01`).
///
/// A repository with no `project` tag has no roster to read, and the set is
/// still marked **read**: there is nothing missing from it. A roster the
/// coordinate does not resolve is likewise read-and-empty — an unknown project
/// grants nobody. Only a storage failure is an error, and it fails the push
/// closed rather than silently narrowing the set to the signer.
pub async fn resolve_repository_founders(
    state: &Arc<AppState>,
    community: buzz_core::CommunityId,
    announcement: &nostr::Event,
) -> Result<RepositoryFounders, ()> {
    let founders = RepositoryFounders::from_announcement(announcement);
    let Some(coordinate) = buzz_core::kind::repo_project_ref(announcement) else {
        return Ok(founders.with_roster_owners(Vec::new()));
    };
    let roster = match state.db.get_project_roster(community, &coordinate).await {
        Ok(roster) => roster,
        Err(error) => {
            tracing::error!(error = %error, "verdict admission: project roster lookup failed");
            return Err(());
        }
    };
    let Some(roster) = roster else {
        return Ok(founders.with_roster_owners(Vec::new()));
    };
    // The creator holds no `project_acl_members` row — a membership op naming
    // them is refused outright — and is an implicit Owner everywhere else that
    // reads this roster. Omitting them here would drop the one founder the
    // coordinate itself names.
    let mut rows: Vec<(String, buzz_core::channel::ProjectRole)> = vec![(
        hex::encode(&roster.owner),
        buzz_core::channel::ProjectRole::Owner,
    )];
    rows.extend(
        roster
            .members
            .into_iter()
            .map(|(pubkey, role)| (hex::encode(pubkey), role)),
    );
    Ok(founders.with_roster_roles(rows))
}

/// Search for a ruling that admits `new_oid`, in the scope the pusher earns.
///
/// The scope itself is [`super::verdict_admission_scope`]'s job (finding 56);
/// this function turns it into candidates and hands them to the pure rule.
pub async fn search_verdict_admission(
    state: &Arc<AppState>,
    request: &VerdictSearchRequest<'_>,
) -> VerdictSearch {
    let VerdictSearchRequest {
        community,
        channel_id,
        project_ref,
        founders,
        pusher_pubkey,
        ..
    } = *request;
    // Arm (A), before anything is fetched. A founder's push is admitted with
    // no verdict, so reading missions for one would be three queries for an
    // answer that cannot change — and worse, a repository bound to no channel
    // would refuse a founder over a fact arm (A) does not consult. `decide`
    // reaches the same conclusion; short-circuiting here is what keeps the
    // unbound and unreadable cases from overtaking it.
    if founders
        .pubkeys()
        .iter()
        .any(|founder| founder.eq_ignore_ascii_case(pusher_pubkey))
    {
        return VerdictSearch::Admitted;
    }

    let scope =
        match resolve_candidate_scope(state, community, channel_id, project_ref, pusher_pubkey)
            .await
        {
            Ok(Some(scope)) => scope,
            // Nowhere to look at all: no seat, no project, no binding.
            Ok(None) => return VerdictSearch::Refused(VerdictAdmissionRefusal::RepositoryUnbound),
            Err(()) => return VerdictSearch::Unavailable,
        };

    // Every founder's geneses, not only the signer's. A founder whose hex the
    // announcement mangled is already absent from the set (counted, not
    // guessed at), so `hex::decode` here cannot fail on a founder that
    // `RepositoryFounders` admitted; a value that somehow does is skipped
    // rather than turned into a wildcard author filter.
    let founder_bytes: Vec<Vec<u8>> = founders
        .pubkeys()
        .iter()
        .filter_map(|founder| hex::decode(founder).ok())
        .filter(|bytes| bytes.len() == 32)
        .collect();
    if founder_bytes.is_empty() {
        return decide(&[], request, &scope.source);
    }

    // The author filter is what keeps the seats lookup from becoming a way
    // in: a mission that seats the pusher but was founded by somebody who
    // founds no part of *this* repository is not a candidate at all.
    let mut genesis_query = EventQuery {
        kinds: Some(vec![KIND_CODING_SESSION_GENESIS as i32]),
        authors: Some(founder_bytes.clone()),
        channel_ids: Some(scope.channels.clone()),
        channel_ids_include_global: false,
        limit: Some(VERDICT_ADMISSION_MAX_SESSIONS as i64),
        ..EventQuery::for_community(community)
    };
    if !scope.genesis_ids.is_empty() {
        // The seats lookup already knows exactly which geneses to read.
        genesis_query.ids = Some(scope.genesis_ids.clone());
    }
    let geneses = match state.db.query_events(&genesis_query).await {
        Ok(events) => events,
        Err(error) => {
            tracing::error!(error = %error, "verdict admission: genesis query failed");
            return VerdictSearch::Unavailable;
        }
    };
    if geneses.is_empty() {
        return decide(&[], request, &scope.source);
    }

    // One page per kind across the whole scope, not one per session: none of
    // these kinds is addressable, so the umbrella label is not a queryable
    // column and the split into missions happens in memory. The bound is
    // therefore "the newest N on these channels", which the refusal's own
    // sentence discloses. Every bound fails in the refusing direction — fewer
    // rows, fewer providers, an unread policy — so a page that missed
    // something can only deny a push it might have admitted.
    let transactions = match read_page(
        state,
        community,
        &scope.channels,
        KIND_CODING_SESSION_TEAM_TRANSACTION,
        None,
        VERDICT_ADMISSION_MAX_TRANSACTIONS,
    )
    .await
    {
        Ok(events) => events,
        Err(()) => return VerdictSearch::Unavailable,
    };
    let observations = match read_page(
        state,
        community,
        &scope.channels,
        KIND_CODING_SESSION_OBSERVATION,
        None,
        VERDICT_ADMISSION_MAX_OBSERVATIONS,
    )
    .await
    {
        Ok(events) => events,
        Err(()) => return VerdictSearch::Unavailable,
    };
    // Founder-signed only: `gates.verifierRequired` decides whether a seat may
    // land its own work, so a policy anyone else signed must not be able to
    // turn the requirement off — or on.
    let policies = match read_page(
        state,
        community,
        &scope.channels,
        KIND_CODING_SESSION_POLICY,
        Some(founder_bytes),
        VERDICT_ADMISSION_MAX_POLICIES,
    )
    .await
    {
        Ok(events) => events,
        Err(()) => return VerdictSearch::Unavailable,
    };
    let session_metadata = match read_page(
        state,
        community,
        &scope.channels,
        KIND_CODING_SESSION_METADATA,
        None,
        VERDICT_ADMISSION_MAX_PROVIDER_METADATA,
    )
    .await
    {
        Ok(events) => events,
        Err(()) => return VerdictSearch::Unavailable,
    };

    let mut candidates: Vec<VerdictAdmissionCandidate> = Vec::with_capacity(geneses.len());
    for stored in &geneses {
        // A mission is read in the channel it was founded in, and nowhere
        // else. With a multi-channel scope the pages carry other missions'
        // events, and a 44244 published somewhere else naming this umbrella's
        // refs is not this mission speaking.
        let Some(mission_channel) = stored.channel_id else {
            continue;
        };
        let page = in_channel(&transactions, mission_channel);
        let genesis_ref = stored.event.id.to_hex();
        // The mission's founder is whoever signed **this** genesis, not the
        // announcement's signer. Before finding 33 the query was scoped to one
        // author so the two were the same string; with a founder set they are
        // not, and writing the wrong one here would make a co-founder's own
        // ruling fail the fold's `founder_pubkey` check.
        let founder_pubkey = stored.event.pubkey.to_hex();
        let Ok(payload) = decode_coding_session_genesis(&stored.event.content) else {
            continue;
        };
        let events: Vec<nostr::Event> =
            mission_transactions(&payload.session_ref, &genesis_ref, &page)
                .into_iter()
                .cloned()
                .collect();
        // Arm (B) needs neither an assignment nor a report, so a mission that
        // published no team transaction at all can still admit a push on its
        // gate rows. The observations are therefore resolved before the
        // early-out below, not after it.
        //
        // `read_page` returns storage's canonical order, newest first, and
        // `mission_observations` keeps it. The page fold reverses it: folding
        // it as read crowned a seat's *first* row per gate — one red, dirty
        // `cargo fmt` at the base commit outranked four later green rows on
        // the pushed commit, and every push was refused (finding 79).
        let observed_gates = fold_coding_session_observation_page(
            &mission_observations(
                &payload.session_ref,
                &genesis_ref,
                &in_channel(&observations, mission_channel),
            )
            .into_iter()
            .cloned()
            .collect::<Vec<nostr::Event>>(),
            &CodingSessionObservationFoldContext {
                session_ref: payload.session_ref.clone(),
                genesis_ref: genesis_ref.clone(),
                // The gate resolves no assignments, and an `assignmentRef` it
                // cannot resolve excludes nothing anywhere. Supplying none is
                // the honest input, not a shortcut.
                known_assignment_refs: Vec::new(),
                provider_pubkeys: Some(mission_provider_pubkeys(
                    &payload.session_ref,
                    &in_channel(&session_metadata, mission_channel),
                )),
            },
        )
        .gates;
        let gate_policy = mission_gate_policy(
            &payload.session_ref,
            &genesis_ref,
            &in_channel(&policies, mission_channel),
        );

        if events.is_empty() && observed_gates.is_empty() {
            // Nothing published under this genesis: it admits nothing, and
            // asking storage for its seats would be a query for no answer.
            candidates.push(VerdictAdmissionCandidate {
                session_ref: payload.session_ref.clone(),
                genesis_ref,
                founder_pubkey,
                canonical: Vec::new(),
                active_seats: Vec::new(),
                observed_gates,
                gate_policy,
            });
            continue;
        }

        let seats = match state
            .db
            .session_authority_for_hire(
                community,
                mission_channel,
                &genesis_ref,
                &payload.session_ref,
            )
            .await
        {
            Ok(Some(authority)) => authority
                .seats
                .into_iter()
                .map(|seat| CodingSessionTeamActiveSeat {
                    actor_pubkey: hex::encode(seat.actor),
                    role: seat.role,
                })
                .collect::<Vec<_>>(),
            // A genesis with no resolvable authority projection seats nobody;
            // the fold then keeps only what the founder signed.
            Ok(None) => Vec::new(),
            Err(error) => {
                tracing::error!(error = %error, "verdict admission: authority lookup failed");
                return VerdictSearch::Unavailable;
            }
        };

        let context = verdict_admission_fold_context(
            mission_channel.to_string(),
            payload.session_ref.clone(),
            genesis_ref.clone(),
            founder_pubkey.clone(),
            seats.clone(),
        );
        // One malformed mission must not deny every ref update: a fold that
        // errors contributes no records, so it admits nothing and refuses
        // nothing else.
        let canonical = match fold_candidate_records(&events, &context) {
            Ok(records) => records,
            Err(error) => {
                warn!(
                    session = %payload.session_ref,
                    error = %error,
                    "verdict admission: session did not fold; it admits nothing"
                );
                Vec::new()
            }
        };
        candidates.push(VerdictAdmissionCandidate {
            session_ref: payload.session_ref.clone(),
            genesis_ref,
            founder_pubkey,
            canonical,
            active_seats: seats,
            observed_gates,
            gate_policy,
        });
    }

    decide(&candidates, request, &scope.source)
}

/// One bounded page of `kind` across every channel in the scope.
///
/// `Err(())` is a storage failure; the caller fails the push closed.
async fn read_page(
    state: &Arc<AppState>,
    community: buzz_core::CommunityId,
    channels: &[Uuid],
    kind: u32,
    authors: Option<Vec<Vec<u8>>>,
    limit: usize,
) -> Result<Vec<buzz_core::StoredEvent>, ()> {
    state
        .db
        .query_events(&EventQuery {
            kinds: Some(vec![kind as i32]),
            authors,
            channel_ids: Some(channels.to_vec()),
            channel_ids_include_global: false,
            limit: Some(limit as i64),
            ..EventQuery::for_community(community)
        })
        .await
        .map_err(|error| {
            tracing::error!(error = %error, kind, "verdict admission: page query failed");
        })
}

/// The events of one page that were published in `channel`.
fn in_channel(page: &[buzz_core::StoredEvent], channel: Uuid) -> Vec<nostr::Event> {
    page.iter()
        .filter(|stored| stored.channel_id == Some(channel))
        .map(|stored| stored.event.clone())
        .collect()
}

/// Run the pure rule over the resolved candidates.
///
/// Named for what it usually does: the gate exists to subtract, and an
/// admission is the case where it finds a ruling that says so.
fn decide(
    candidates: &[VerdictAdmissionCandidate],
    request: &VerdictSearchRequest<'_>,
    source: &VerdictAdmissionCandidateSource,
) -> VerdictSearch {
    let query = VerdictAdmissionQuery {
        ref_name: request.ref_name,
        new_oid: request.new_oid,
        pusher_pubkey: request.pusher_pubkey,
        repo_founders: request.founders.pubkeys(),
        candidate_source: source,
    };
    match evaluate_verdict_admission(candidates, &query) {
        VerdictAdmission::Admitted(_) => VerdictSearch::Admitted,
        VerdictAdmission::Refused(refusal) => VerdictSearch::Refused(refusal),
    }
}

/// Render a denied push as the hook can print it.
///
/// One `{ref}: {reason}` line per denial, `text/plain`. The pre-receive hook
/// `cat`s this to stderr unchanged, so `git push` shows
/// `remote: refs/heads/main: …` on a client nobody upgraded.
pub fn denial_response(denials: &[Denial], structured: Option<String>) -> Response {
    let body = denials
        .iter()
        .map(|denial| format!("{}: {}", denial.ref_name, denial.reason))
        .collect::<Vec<_>>()
        .join("\n");
    let mut response = (StatusCode::FORBIDDEN, body).into_response();
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    if let Some(json) = structured {
        if json.len() <= MAX_DENIAL_HEADER_BYTES {
            // A header value must be visible ASCII. A ref name may legally
            // carry other bytes, and the body already says everything, so a
            // value that cannot be represented is dropped rather than mangled.
            if let Ok(value) = HeaderValue::from_str(&json) {
                response.headers_mut().insert(GIT_DENIALS_HEADER, value);
            }
        }
    }
    response
}

#[cfg(test)]
#[path = "verdict_admission_tests.rs"]
mod tests;

/// Arm (B) against Postgres, in its own file: its fixtures are observations,
/// a policy and provider metadata rather than a transaction chain.
#[cfg(test)]
#[path = "verdict_admission_observed_tests.rs"]
mod observed_tests;

/// Arm (C) after the 2026-09-03 follow-up ruling, against Postgres: its
/// fixtures need the transaction chain **and** the observations **and** the
/// policy at once, which neither sibling assembles.
#[cfg(test)]
#[path = "verdict_admission_verified_tests.rs"]
mod verified_tests;

/// Finding 56 — *where* the gate looks for a mission — in its own file, because
/// every case here binds the repository somewhere other than the mission's
/// channel and reuses [`observed_tests`]'s watched-mission fixture.
#[cfg(test)]
#[path = "verdict_admission_lookup_tests.rs"]
mod lookup_tests;
