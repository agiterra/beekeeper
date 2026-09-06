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
    evaluate_verdict_admission, fold_candidate_records, mission_observations,
    mission_provider_pubkeys_from_lifecycle, mission_transactions, resolve_mission_gate_policy,
    verdict_admission_fold_context, GatePolicyResolution, VerdictAdmission,
    VerdictAdmissionCandidate, VerdictAdmissionCandidateSource, VerdictAdmissionEvidence,
    VerdictAdmissionQuery, VerdictAdmissionRefusal, VERDICT_ADMISSION_MAX_LIFECYCLE_RECORDS,
    VERDICT_ADMISSION_MAX_OBSERVATIONS, VERDICT_ADMISSION_MAX_POLICIES,
    VERDICT_ADMISSION_MAX_PROVIDER_METADATA, VERDICT_ADMISSION_MAX_SESSIONS,
    VERDICT_ADMISSION_MAX_TRANSACTIONS,
};
use buzz_core::git_perms::{Denial, EffectiveRules, ProtectionRule, RefUpdate};
use buzz_core::kind::{
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT, KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_OBSERVATION, KIND_CODING_SESSION_POLICY,
    KIND_CODING_SESSION_TEAM_TRANSACTION, KIND_GIT_REPO_ANNOUNCEMENT,
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
    /// A canonical ruling admits the update, and this is what it stood on.
    ///
    /// The evidence used to be dropped here (the audit's Q1 disclosure
    /// finding): the caller learned only *that* a push was admitted, never by
    /// which arm or under which policy, so a founder's exception and a
    /// verifier's clearance left the same trace — none. It is carried now and
    /// written to the push audit by [`super::policy`].
    Admitted(Box<VerdictAdmissionEvidence>),
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
    /// The kind:30617 coordinate of the repository being pushed, as
    /// [`repository_coordinate`] builds it (finding 91).
    ///
    /// A candidate mission that is not bound to it is refused before either
    /// arm reads it: founder overlap is not a binding.
    pub repository: &'a str,
}

/// The `30617:<owner-hex>:<d>` coordinate an announcement addresses.
///
/// `None` when the announcement carries no `d` tag, which is not a repository
/// this relay could have stored: the caller fails the gated push closed
/// rather than judging a mission against a coordinate it invented.
pub fn repository_coordinate(announcement: &nostr::Event) -> Option<String> {
    let d = nostr::SingleLetterTag::lowercase(nostr::Alphabet::D);
    let name = announcement
        .tags
        .filter(nostr::TagKind::SingleLetter(d))
        .find_map(|tag| tag.content())?;
    if name.is_empty() {
        return None;
    }
    Some(format!(
        "{KIND_GIT_REPO_ANNOUNCEMENT}:{}:{name}",
        announcement.pubkey.to_hex()
    ))
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
        // The receipt the audit asked for: a founder's landing is recorded as
        // the exception it is, never as something a verifier approved.
        return VerdictSearch::Admitted(Box::new(VerdictAdmissionEvidence::FounderPush {
            pusher_pubkey: pusher_pubkey.to_ascii_lowercase(),
            policy_not_evaluated:
                buzz_core::coding_session_verdict_admission::VerdictAdmissionPolicyNotEvaluated::FounderException,
        }));
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
        authors: Some(founder_bytes),
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
    // Finding 90: the provider set is proven by the lifecycle, not by who
    // published metadata. These two pages are what proves it — a kind 44221
    // command naming a `providerAuthorityPubkey`, and the kind 44224 receipt
    // that key signed. A page that missed the pair finds no provider, every
    // `observed` claim folds to `declared`, and the push is refused: the
    // failure direction is a refusal, never a wrong admission.
    let lifecycle_commands = match read_page(
        state,
        community,
        &scope.channels,
        KIND_CODING_SESSION_LIFECYCLE_COMMAND,
        None,
        VERDICT_ADMISSION_MAX_LIFECYCLE_RECORDS,
    )
    .await
    {
        Ok(events) => events,
        Err(()) => return VerdictSearch::Unavailable,
    };
    let lifecycle_receipts = match read_page(
        state,
        community,
        &scope.channels,
        KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
        None,
        VERDICT_ADMISSION_MAX_LIFECYCLE_RECORDS,
    )
    .await
    {
        Ok(events) => events,
        Err(()) => return VerdictSearch::Unavailable,
    };
    // Kind 44223 is read for **one** thing now: the `repoRef` a mission's own
    // provider or founder published for it, which is half of finding 91's
    // binding. It contributes no provider authority (finding 90) and no
    // signer of it gains any; a `repoRef` from anybody else is ignored,
    // because a binding a stranger can widen is not a binding.
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
        // The authority projection is resolved for **every** candidate, and
        // before anything reads the lifecycle: it names the keys that may
        // steer this mission, and since the 2026-09-05 refuter's B1 that set
        // is what decides which `session.create` commissions a provider at
        // all. It also names the keys that may sign this mission's policy, and
        // finding 89 made an unreadable policy refuse the push whether the
        // mission holds anything else or not.
        let authority = match state
            .db
            .session_authority_for_hire(
                community,
                mission_channel,
                &genesis_ref,
                &payload.session_ref,
            )
            .await
        {
            Ok(authority) => authority,
            Err(error) => {
                tracing::error!(error = %error, "verdict admission: authority lookup failed");
                return VerdictSearch::Unavailable;
            }
        };
        // A genesis with no resolvable authority projection seats nobody; the
        // fold then keeps only what the founder signed.
        let seats = authority
            .as_ref()
            .map(|authority| {
                authority
                    .seats
                    .iter()
                    .map(|seat| CodingSessionTeamActiveSeat {
                        actor_pubkey: hex::encode(&seat.actor),
                        role: seat.role.clone(),
                    })
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        // NIP-CSP § Validation boundary: the founder, or a key holding an
        // operator grant. Applied here and not in `buzz-core`, which cannot
        // read the authority chain. One set, two uses — who may commission an
        // execution (B1) and who may set this mission's policy (finding 89) —
        // because they are the same question: who steers this mission.
        let mut steering_signers: Vec<String> = vec![founder_pubkey.to_ascii_lowercase()];
        if let Some(authority) = authority.as_ref() {
            for operator in &authority.operators {
                steering_signers.push(hex::encode(operator));
            }
        }
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
        // Finding 90: the provider set comes from the mission's own accepted
        // lifecycle, scoped to the channel it was founded in. A 44221 named
        // this genesis from somewhere else is not this mission's hire. B1: and
        // a 44221 signed by a key that may not steer this mission commissions
        // nothing, however well its own receipt answers it.
        let providers = mission_provider_pubkeys_from_lifecycle(
            &payload.session_ref,
            &genesis_ref,
            &steering_signers,
            &in_channel(&lifecycle_commands, mission_channel),
            &in_channel(&lifecycle_receipts, mission_channel),
        );
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
                provider_pubkeys: Some(providers.clone()),
            },
        )
        .gates;

        let (gate_policy, excluded_unauthorized_policies) = match mission_gate_policy(
            state,
            community,
            mission_channel,
            &payload.session_ref,
            &genesis_ref,
            &steering_signers,
        )
        .await
        {
            Ok(resolved) => resolved,
            Err(()) => return VerdictSearch::Unavailable,
        };
        // Finding 91: what this mission may prove commits for.
        let bound_repositories = bound_repositories(
            &payload.session_ref,
            request.repository,
            mission_channel,
            &scope.granted_channels,
            &founder_pubkey,
            &providers,
            &in_channel(&session_metadata, mission_channel),
        );

        if events.is_empty() && observed_gates.is_empty() {
            // Nothing published under this genesis: it admits nothing.
            candidates.push(VerdictAdmissionCandidate {
                session_ref: payload.session_ref.clone(),
                genesis_ref,
                founder_pubkey,
                canonical: Vec::new(),
                active_seats: Vec::new(),
                observed_gates,
                gate_policy,
                bound_repositories,
                excluded_unauthorized_policies,
            });
            continue;
        }

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
            bound_repositories,
            excluded_unauthorized_policies,
        });
    }

    decide(&candidates, request, &scope.source)
}

/// Resolve one mission's kind 44245 policy, read for **that mission**
/// (finding 89).
///
/// Before this the gate read one page of 44245 across the whole scope and
/// took whatever of it named the mission: a mission whose policy fell outside
/// that page resolved to "no policy" and was judged with the default gates, so
/// a `verifierRequired: true` the page missed opened arm (B). The query is now
/// keyed on the mission's own `d` tag and its channel, newest first, so the
/// authoritative record is the first row and the bound only limits how many
/// superseded records travel with it.
///
/// `signers` is the NIP-CSP standing rule applied where it can be: the
/// mission's founder and the keys the relay's own accepted chain currently
/// grants `operator`. A record by anyone else is **excluded with its reason**
/// rather than promoted or treated as unreadable — NIP-CSP § Validation
/// boundary rule 3 — and the newest record that remains is the policy.
///
/// The signer set is in the **query** (2026-09-05 refuter, B2). Filtering it
/// in memory after a page read was attacker-triggerable in the one direction
/// that matters: kind 44245 ingest is structure-only, so any channel member
/// could publish 64 newer structurally valid records carrying this mission's
/// `d` and `csp-genesis`, evict the founder's authorized record from the page,
/// and leave the filter with an empty slice — `Absent`, defaults, and a
/// `verifierRequired: true` mission admitted under arm (B). Flooding every
/// other page here only refuses; flooding this one failed **open**. With
/// `authors` in the query the flood cannot displace anything, because the
/// unauthorized records were never in the page to begin with.
///
/// The second return value is how many records naming this mission the signer
/// rule excluded, for disclosure only (2026-09-05 refuter, S2) — it decides
/// nothing. It is **exact**, not a page count: two `COUNT`s over the same
/// per-mission filter, one of them keyed on the signer set, so neither is
/// bounded by [`VERDICT_ADMISSION_MAX_POLICIES`]. Subtracting the *page*
/// length instead over-stated the exclusions whenever a mission held more
/// authorized records than the bound — it would have reported a founder's own
/// superseded policies as records somebody was not entitled to sign.
///
/// `Err(())` is a storage failure and fails the push closed: a page the relay
/// could not read is not evidence that no policy exists.
async fn mission_gate_policy(
    state: &Arc<AppState>,
    community: buzz_core::CommunityId,
    channel: Uuid,
    session_ref: &str,
    genesis_ref: &str,
    signers: &[String],
) -> Result<(GatePolicyResolution, u32), ()> {
    let mission_records = EventQuery {
        kinds: Some(vec![KIND_CODING_SESSION_POLICY as i32]),
        channel_ids: Some(vec![channel]),
        channel_ids_include_global: false,
        // `d_tag` is the wrong column: it is materialized only for
        // addressable kinds (30000–39999), and 44245 is not one — every
        // coding-session record's `d` lives in the `tags` JSONB. The
        // containment filter is what makes this a *per-mission* read
        // rather than a page of the channel (finding 89).
        tags_containing: Some(vec![
            ("d".to_owned(), session_ref.to_owned()),
            ("csp-genesis".to_owned(), genesis_ref.to_owned()),
        ]),
        limit: Some(VERDICT_ADMISSION_MAX_POLICIES as i64),
        ..EventQuery::for_community(community)
    };
    let author_bytes: Vec<Vec<u8>> = signers
        .iter()
        .filter_map(|signer| hex::decode(signer).ok())
        .collect();
    let page = state
        .db
        .query_events(&EventQuery {
            authors: Some(author_bytes.clone()),
            ..mission_records.clone()
        })
        .await
        .map_err(|error| {
            tracing::error!(error = %error, session = %session_ref, "verdict admission: policy query failed");
        })?;
    // The same read counted twice — once as it stands, once keyed on the
    // signer set — so the difference is exactly "n records name this mission
    // that nobody entitled to set its policy signed". Counting against the
    // *page* instead would have called a founder's own superseded records
    // unauthorized as soon as they outnumbered the bound, and a surface that
    // stayed silent would be hiding the very records a flood is made of.
    let count = |authors: Option<Vec<Vec<u8>>>| {
        let query = EventQuery {
            authors,
            limit: None,
            ..mission_records.clone()
        };
        async move {
            state.db.count_events(&query).await.map_err(|error| {
                tracing::error!(error = %error, session = %session_ref, "verdict admission: policy count failed");
            })
        }
    };
    let all = count(None).await?;
    let authorized_records = count(Some(author_bytes.clone())).await?;
    let excluded =
        u32::try_from(all.max(0).saturating_sub(authorized_records.max(0))).unwrap_or(u32::MAX);
    // The in-memory filter stays: a record whose author bytes the query
    // matched is still checked against the hex set the rule was written in, so
    // the disclosure and the decision cannot come from two different rules.
    let authorized: Vec<nostr::Event> = page
        .into_iter()
        .filter(|stored| {
            let signer = stored.event.pubkey.to_hex();
            signers
                .iter()
                .any(|held| held.eq_ignore_ascii_case(&signer))
        })
        .map(|stored| stored.event)
        .collect();
    Ok((
        resolve_mission_gate_policy(session_ref, genesis_ref, &authorized),
        excluded,
    ))
}

/// The kind:30617 coordinates one mission may prove commits for (finding 91).
///
/// Two sources, and since the 2026-09-05 refuter's S3 they are **not** unioned
/// — the first one, when it says anything, is the whole answer:
///
/// 1. every `repoRef` on a kind 44223 naming this mission that its **founder
///    or a lifecycle-proven provider** signed. A stranger's metadata widens
///    nothing — that is the same conversion finding 90 closed, arriving by a
///    different door. When any such record names a repository, *that set* is
///    the binding: a mission whose own authority said "this work is for R1"
///    has said which repository its rows prove commits for, and adding R2
///    because both hang off one project would let R1's green rows land a push
///    to R2 — two repositories in one project, one seat, and the binding
///    check silently satisfied;
/// 2. the repository being pushed, when this mission was founded in a channel
///    that repository **grants** — one of its project's transport channels,
///    or the channel its own `buzz-channel` tag binds. That is the project
///    grant the audit asked for, read from the repository's side so no second
///    lookup is needed, and it is the **fallback** for the missions that name
///    no repository at all.
///
/// The set can be empty, and an empty set binds the mission to nothing: it
/// admits no push anywhere, which is the direction this must fail in.
///
/// With the fallback narrowed to missions that name nothing,
/// `MissionNotBoundToRepository` is reachable through the relay for the first
/// time: a mission that named R1 and is pushed to R2 is refused *by the rule*,
/// not merely undiscovered by the scope.
fn bound_repositories(
    session_ref: &str,
    repository: &str,
    mission_channel: Uuid,
    granted_channels: &[Uuid],
    founder_pubkey: &str,
    providers: &[String],
    metadata: &[nostr::Event],
) -> Vec<String> {
    let mut bound: Vec<String> = Vec::new();
    for event in metadata {
        let signer = event.pubkey.to_hex();
        let may_speak = signer.eq_ignore_ascii_case(founder_pubkey)
            || providers
                .iter()
                .any(|provider| provider.eq_ignore_ascii_case(&signer));
        if !may_speak {
            continue;
        }
        let Ok(payload) =
            buzz_core::coding_session_payload::decode_coding_session_metadata(&event.content)
        else {
            continue;
        };
        if payload.session_ref.as_deref() != Some(session_ref) {
            continue;
        }
        let Some(repo_ref) = payload.repo_ref else {
            continue;
        };
        if !bound.iter().any(|held| held == &repo_ref) {
            bound.push(repo_ref);
        }
    }
    // The project grant is the fallback, never an addition: a mission its own
    // authority bound to a repository is bound to that one (S3).
    if bound.is_empty() && granted_channels.contains(&mission_channel) {
        bound.push(repository.to_owned());
    }
    bound
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
        repository: request.repository,
    };
    match evaluate_verdict_admission(candidates, &query) {
        VerdictAdmission::Admitted(evidence) => VerdictSearch::Admitted(Box::new(evidence)),
        VerdictAdmission::Refused(refusal) => VerdictSearch::Refused(refusal),
    }
}

/// One admitted ref update, as the push audit records it.
///
/// **Not a wire kind.** The audit's Q1 asked for a durable per-push receipt
/// naming the arm and, for a founder, `policy_not_evaluated:
/// founder_exception`. The comments in this module mention kind 30618; that
/// event is the relay's *ref state* — parameterized-replaceable, "where this
/// ref stands now" — and equating a record that a ref moved with proof of
/// which arm permitted it is exactly the confusion the audit warned against.
/// No admission-receipt kind exists, and inventing one is not this lane's
/// call, so the receipt is written to the relay's structured log with every
/// field a later wire kind would carry.
pub fn record_admission(
    repository: &str,
    ref_name: &str,
    new_oid: &str,
    pusher_pubkey: &str,
    founders: &RepositoryFounders,
    evidence: &VerdictAdmissionEvidence,
) {
    match evidence {
        VerdictAdmissionEvidence::FounderPush {
            pusher_pubkey: founder,
            policy_not_evaluated,
        } => tracing::info!(
            audit = "git_verdict_admission",
            arm = "founder",
            repository,
            ref_name,
            commit = new_oid,
            pusher = %founder,
            policy_not_evaluated = policy_not_evaluated.as_str(),
            // 2026-09-05 refuter, F5: *which* founder source admitted the key
            // — the announcement's signer, one of its NIP-34 maintainers, or a
            // project-roster Owner. Three grants, revoked in three places; an
            // audit line that says only "founder" cannot answer "by what
            // authority" afterwards. `unknown` is a disclosed non-answer for a
            // set that no longer names the key, never a guess.
            founder_basis = founders
                .basis(founder)
                .map(|basis| basis.as_str())
                .unwrap_or("unknown"),
            "verdict admission: admitted"
        ),
        VerdictAdmissionEvidence::ObservedGates {
            session_ref,
            head_sha,
            gates,
            row_event_ids,
            policy,
        } => tracing::info!(
            audit = "git_verdict_admission",
            arm = "observed-gates",
            repository,
            ref_name,
            commit = new_oid,
            pusher = pusher_pubkey,
            session = %session_ref,
            head_sha = %head_sha,
            gates = %gates.join(","),
            rows = %row_event_ids.join(","),
            policy_resolution = policy.resolution.as_str(),
            policy_event_id = policy.event_id.as_deref().unwrap_or(""),
            // S2: how many records naming this mission the signer rule
            // dropped. Zero is the ordinary case; anything else is worth
            // looking at, and it used to be dropped in silence.
            policy_excluded_unauthorized = policy.excluded_unauthorized,
            "verdict admission: admitted"
        ),
        VerdictAdmissionEvidence::VerifierVerdict {
            session_ref,
            disposition_event_id,
            refutation_event_id,
            report_event_id,
            head_sha,
            verifier_pubkey,
            gates,
            row_event_ids,
            policy,
        } => tracing::info!(
            audit = "git_verdict_admission",
            arm = "verifier-verdict",
            repository,
            ref_name,
            commit = new_oid,
            pusher = pusher_pubkey,
            session = %session_ref,
            disposition = %disposition_event_id,
            refutation = %refutation_event_id,
            report = %report_event_id,
            verifier = %verifier_pubkey,
            head_sha = %head_sha,
            gates = %gates.join(","),
            rows = %row_event_ids.join(","),
            policy_resolution = policy.resolution.as_str(),
            policy_event_id = policy.event_id.as_deref().unwrap_or(""),
            // S2: how many records naming this mission the signer rule
            // dropped. Zero is the ordinary case; anything else is worth
            // looking at, and it used to be dropped in silence.
            policy_excluded_unauthorized = policy.excluded_unauthorized,
            "verdict admission: admitted"
        ),
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

/// Findings 89, 90 and 91 — the 2026-09-05 admission audit — in production
/// composition, because what those defects lived in is *which events this
/// module fetches*, which no pure-rule test can reach.
#[cfg(test)]
#[path = "verdict_admission_hardening_tests.rs"]
mod hardening_tests;

/// Finding 56 — *where* the gate looks for a mission — in its own file, because
/// every case here binds the repository somewhere other than the mission's
/// channel and reuses [`observed_tests`]'s watched-mission fixture.
#[cfg(test)]
#[path = "verdict_admission_lookup_tests.rs"]
mod lookup_tests;
