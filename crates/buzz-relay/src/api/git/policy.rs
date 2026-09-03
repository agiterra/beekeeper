//! Internal policy endpoint — pre-receive hook callback.
//!
//! The pre-receive hook POSTs here with HMAC-signed payload containing
//! the pusher's pubkey, repo ID, and ref updates. This endpoint:
//!
//! 1. Validates HMAC signature + 30s TTL (fail-closed)
//! 2. Resolves kind:30617 → protection rules
//! 3. Grants owner authority to the repo key or its verified managed-agent owner
//! 4. Otherwise resolves the pusher's role through **both** ACLs — the
//!    project roster named by the announcement's `["project", …]` tag and the
//!    channel named by its `buzz-channel` tag — and takes the more permissive
//!    (promoting a channel Bot to Member first)
//! 5. Calls `buzz_core::git_perms::evaluate_ref_update()` per ref, with the
//!    tier that ref calls for (see `ref_is_guarded`)
//! 6. Returns 200 (allow) or 403 (deny with reasons)
//!
//! # Two additive ACLs
//!
//! A repository may be reachable through a project roster, a bound channel,
//! or both. Neither narrows the other: the effective role is the maximum
//! (`buzz_core::git_perms::max_git_role`), and a denial requires *both* to
//! grant nothing. The project mapping lives in
//! `buzz_core::git_perms::git_role_for_project_role` — owner → Owner,
//! collaborator → Member, viewer → no push. `buzz-protect` rules constrain
//! every pusher regardless of which path granted the role.
//!
//! The `no_channel_binding` remediation token is emitted only when the
//! announcement carries *neither* tag; see the const docs in
//! `buzz_core::git_perms`.
//!
//! # Bot Role Model
//!
//! Bots are intentionally added to channels by members/admins. For git push,
//! they're promoted to Member — protection rules still apply. Bot is a
//! designation (what it is), not a permission tier (what it can do). The
//! promotion is scoped to this module; the core `MemberRole::Bot` hierarchy
//! is unchanged.
//!
//! # Security invariants
//!
//! - Endpoint binds to 127.0.0.1 only (enforced at router level)
//! - HMAC binds callback to the specific push operation
//! - Fail-closed: any error → 403

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use axum::{
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use hmac::{Hmac, KeyInit, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use tracing::{error, info, warn};

use uuid::Uuid;

use buzz_core::channel::MemberRole;
use buzz_core::git_perms::{
    evaluate_ref_update, git_role_for_project_role, max_git_role, parse_protection_tags, Denial,
    EffectiveRules, ProtectionRule, RefUpdate, UpdateKind, GIT_NO_CHANNEL_BINDING_BODY,
};
use buzz_db::EventQuery;

use crate::api::git::verdict_admission::{VerdictSearch, VerdictSearchRequest};
use crate::state::AppState;

/// Maximum age of a hook callback (seconds). Push is synchronous so 30s is generous.
const MAX_CALLBACK_AGE_SECS: u64 = 30;

/// Request payload from the pre-receive hook.
#[derive(Debug, Clone, Deserialize)]
pub struct HookCallbackRequest {
    /// Repo identifier (d-tag from kind:30617).
    pub repo_id: String,
    /// Hex-encoded repo owner pubkey (from URL path, verified against kind:30617).
    pub repo_owner: String,
    /// Server-resolved community id from the git HTTP request that spawned the hook.
    /// Internal-only: set by relay env and HMAC-bound by the hook callback.
    pub community_id: String,
    /// Hex-encoded pusher pubkey.
    pub pusher_pubkey: String,
    /// Ref updates from git stdin (old_oid, new_oid, ref_name, is_ancestor).
    pub ref_updates: Vec<HookRefUpdate>,
    /// Unix timestamp when the hook was invoked.
    pub timestamp: u64,
    /// HMAC-SHA256 signature over the canonical payload.
    pub signature: String,
}

/// A single ref update as reported by the pre-receive hook.
#[derive(Debug, Clone, Deserialize)]
pub struct HookRefUpdate {
    /// Old object ID (40 hex chars, zero OID for creates).
    pub old_oid: String,
    /// New object ID (40 hex chars, zero OID for deletes).
    pub new_oid: String,
    /// Full ref name (e.g., "refs/heads/main").
    pub ref_name: String,
    /// Result of `git merge-base --is-ancestor old new`.
    /// For creates/deletes this is false (ignored by classifier).
    pub is_ancestor: bool,
}

/// Response to the hook — either allow or deny.
#[derive(Debug, Serialize)]
pub struct HookCallbackResponse {
    /// Whether the push is allowed.
    pub allowed: bool,
    /// Denial reasons (empty if allowed).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub denials: Vec<DenialResponse>,
}

/// A single denial reason in the hook response.
#[derive(Debug, Serialize)]
pub struct DenialResponse {
    /// The ref that was denied.
    pub ref_name: String,
    /// Human-readable reason for denial.
    pub reason: String,
}

impl From<Denial> for DenialResponse {
    fn from(d: Denial) -> Self {
        Self {
            ref_name: d.ref_name,
            reason: d.reason,
        }
    }
}

/// Compute the canonical HMAC payload.
///
/// Format (length-prefixed, `|`-separated, structurally unambiguous):
/// ```text
/// len(repo_id):repo_id | repo_owner(64) | community_id(36) | pusher(64) | sorted_refs | timestamp
/// ```
/// where each ref is: `old_oid(40) + new_oid(40) + len(ref_name):ref_name + is_ancestor("1"/"0")`
///
/// Fixed-length fields (OIDs=40, pubkeys=64) need no length prefix.
/// Variable-length fields (repo_id, ref_name) are length-prefixed to prevent concatenation ambiguity.
fn compute_hmac(secret: &[u8], req: &HookCallbackRequest) -> Vec<u8> {
    let mut mac = Hmac::<Sha256>::new_from_slice(secret).expect("HMAC can take key of any size");

    // Structurally unambiguous format: length-prefixed fields separated by |.
    // This prevents field confusion attacks (e.g., repo_id="a|b" being parsed differently).
    mac.update(req.repo_id.len().to_string().as_bytes());
    mac.update(b":");
    mac.update(req.repo_id.as_bytes());
    mac.update(b"|");
    mac.update(req.repo_owner.as_bytes()); // Fixed 64 chars, no ambiguity.
    mac.update(b"|");
    mac.update(req.community_id.as_bytes()); // Fixed UUID string from server-resolved tenant.
    mac.update(b"|");
    mac.update(req.pusher_pubkey.as_bytes()); // Fixed 64 chars, no ambiguity.
    mac.update(b"|");
    // Deterministic ref update representation: sorted by ref_name.
    // Each ref is length-prefixed to prevent concatenation ambiguity.
    let mut refs_sorted: Vec<&HookRefUpdate> = req.ref_updates.iter().collect();
    refs_sorted.sort_by_key(|r| r.ref_name.clone());
    for r in &refs_sorted {
        mac.update(r.old_oid.as_bytes()); // Fixed 40 chars.
        mac.update(r.new_oid.as_bytes()); // Fixed 40 chars.
        mac.update(r.ref_name.len().to_string().as_bytes());
        mac.update(b":");
        mac.update(r.ref_name.as_bytes());
        mac.update(if r.is_ancestor { b"1" } else { b"0" });
    }
    mac.update(b"|");
    mac.update(req.timestamp.to_string().as_bytes());

    mac.finalize().into_bytes().to_vec()
}

/// Verify the HMAC signature on a hook callback.
fn verify_hmac(secret: &[u8], req: &HookCallbackRequest) -> bool {
    let expected = compute_hmac(secret, req);
    let provided = match hex::decode(&req.signature) {
        Ok(bytes) => bytes,
        Err(_) => return false,
    };
    // Constant-time comparison.
    use subtle::ConstantTimeEq;
    expected.ct_eq(&provided).into()
}

/// `POST /internal/git/policy` — pre-receive hook callback.
///
/// Fail-closed: ANY error returns 403. The hook script treats non-200 as deny.
pub async fn hook_policy_check(
    State(state): State<Arc<AppState>>,
    Json(req): Json<HookCallbackRequest>,
) -> Response {
    // 1. Validate input fields (cheap structural checks before expensive HMAC).
    // This prevents wasting CPU on malformed payloads.
    if req.repo_id.is_empty() || req.repo_id.len() > 64 {
        return (StatusCode::FORBIDDEN, "invalid repo_id").into_response();
    }
    if req.repo_owner.len() != 64
        || !req
            .repo_owner
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return (StatusCode::FORBIDDEN, "invalid repo_owner").into_response();
    }
    if req.pusher_pubkey.len() != 64
        || !req
            .pusher_pubkey
            .chars()
            .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())
    {
        return (StatusCode::FORBIDDEN, "invalid pusher_pubkey").into_response();
    }
    let community_uuid = match Uuid::parse_str(&req.community_id) {
        Ok(id) => id,
        Err(_) => return (StatusCode::FORBIDDEN, "invalid community_id").into_response(),
    };
    let community = buzz_core::CommunityId::from_uuid(community_uuid);
    if req.ref_updates.is_empty() || req.ref_updates.len() > 500 {
        return (StatusCode::FORBIDDEN, "invalid ref_updates count").into_response();
    }
    for r in &req.ref_updates {
        if r.old_oid.len() != 40 || !r.old_oid.chars().all(|c| c.is_ascii_hexdigit()) {
            return (StatusCode::FORBIDDEN, "invalid old_oid").into_response();
        }
        if r.new_oid.len() != 40 || !r.new_oid.chars().all(|c| c.is_ascii_hexdigit()) {
            return (StatusCode::FORBIDDEN, "invalid new_oid").into_response();
        }
        if r.ref_name.is_empty()
            || r.ref_name.len() > 256
            || !r.ref_name.starts_with("refs/")
            || r.ref_name.contains("..")
            || r.ref_name.bytes().any(|b| b <= 0x20 || b == 0x7f)
        {
            return (StatusCode::FORBIDDEN, "invalid ref_name").into_response();
        }
    }

    // 2. Verify HMAC signature (now that we know the payload is structurally valid).
    let secret = state.config.git_hook_hmac_secret.as_bytes();
    if !verify_hmac(secret, &req) {
        warn!(repo = %req.repo_id, "hook callback: HMAC verification failed");
        return (StatusCode::FORBIDDEN, "signature verification failed").into_response();
    }

    // 3. Validate timestamp (30s TTL, max 5s future tolerance).
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now.saturating_sub(req.timestamp) > MAX_CALLBACK_AGE_SECS {
        warn!(repo = %req.repo_id, age = now.saturating_sub(req.timestamp), "hook callback: expired");
        return (StatusCode::FORBIDDEN, "callback expired").into_response();
    }
    if req.timestamp.saturating_sub(now) > 5 {
        warn!(repo = %req.repo_id, "hook callback: timestamp too far in future");
        return (StatusCode::FORBIDDEN, "callback timestamp invalid").into_response();
    }

    // 4. Decide, then log every ref update with the decision before rendering.
    //
    // The decision is computed by `decide_push` rather than returned from here
    // so that EVERY authenticated outcome passes one logging point. Run 3
    // could only answer "who pushed main" from the derived kind:30618 event,
    // because the relay logged nothing at the moment it allowed the push.
    let outcome = decide_push(&state, &req, community).await;
    log_ref_updates(&req, &outcome);
    outcome.into_response()
}

/// The decision one authenticated hook callback reaches.
///
/// A borrowed static body for every fail-closed refusal keeps those responses
/// byte-identical to the ones shipped before this lane; only the
/// policy-engine denials change shape (plain text plus a structured header).
enum PolicyOutcome {
    /// The push may proceed.
    Allowed,
    /// Refused before the policy engine ran, with the generic body.
    Refused(&'static str),
    /// Refused by the policy engine, per ref.
    Denied(Vec<Denial>),
}

impl PolicyOutcome {
    /// A stable word per outcome for the ref-update log line.
    fn decision(&self) -> &'static str {
        match self {
            Self::Allowed => "allowed",
            Self::Refused(_) | Self::Denied(_) => "denied",
        }
    }

    /// The refusal's own words **for this ref**, for the log line only.
    ///
    /// A push is atomic, so every ref in a denied push is `denied` — but only
    /// the ref that was actually refused gets the reason. Joining all reasons
    /// onto every line made the log say `refs/heads/topic` was refused for
    /// something only `refs/heads/main` did, which is the kind of borrowed
    /// attribution this batch exists to remove.
    fn detail_for(&self, ref_name: &str) -> String {
        match self {
            Self::Allowed => String::new(),
            Self::Refused(body) => (*body).to_string(),
            Self::Denied(denials) => denials
                .iter()
                .filter(|denial| denial.ref_name == ref_name)
                .map(|denial| denial.reason.clone())
                .collect::<Vec<_>>()
                .join("; "),
        }
    }
}

impl IntoResponse for PolicyOutcome {
    fn into_response(self) -> Response {
        match self {
            Self::Allowed => Json(HookCallbackResponse {
                allowed: true,
                denials: vec![],
            })
            .into_response(),
            Self::Refused(body) => (StatusCode::FORBIDDEN, body).into_response(),
            Self::Denied(denials) => {
                let structured = serde_json::to_string(&HookCallbackResponse {
                    allowed: false,
                    denials: denials.iter().cloned().map(DenialResponse::from).collect(),
                })
                .ok();
                crate::api::git::verdict_admission::denial_response(&denials, structured)
            }
        }
    }
}

/// One line per ref update, at the moment the decision is made.
///
/// Carries the repository, the ref, both object ids, the authenticated pusher
/// and the decision — the facts finding 27 had to reconstruct from kind:30618
/// after the fact. `ref` is a Rust keyword, so the field is `ref_name`. The
/// reason is the one that names *this* ref; a ref carried down by an atomic
/// push logs `denied` with an empty detail rather than borrowing another
/// ref's words.
fn log_ref_updates(req: &HookCallbackRequest, outcome: &PolicyOutcome) {
    let decision = outcome.decision();
    for update in &req.ref_updates {
        let detail = outcome.detail_for(&update.ref_name);
        info!(
            repo = %req.repo_id,
            ref_name = %update.ref_name,
            old = %update.old_oid,
            new = %update.new_oid,
            pusher = %req.pusher_pubkey,
            decision,
            detail = %detail,
            "git ref update"
        );
    }
}

/// The default branch, capped for an inherited grant whether or not anyone
/// wrote a `buzz-protect` rule for it.
///
/// A repository that never set a rule is the common case, and it is the case
/// live run 3 happened in. Naming it here is what stops "nobody configured
/// anything" from meaning "a seat may rewrite the trunk".
const ALWAYS_GUARDED_REF: &str = "refs/heads/main";

/// Whether a ref is one an *inherited* grant is capped on.
///
/// **Scoped, not blanket.** Two kinds of ref are guarded: the default branch,
/// and any ref an operator explicitly wrote a `buzz-protect` rule for — the
/// two places a person has said "this history is shared". Everywhere else
/// (`lane/*`, `wip/*`, a seat's own topic branch) an inherited grant keeps its
/// full tier, because rebase-then-force-push and delete-the-branch-after are
/// the *normal* operations this repository asks of a lane
/// (`CLAUDE.md`, `docs/CREW_SESSIONS_PLAN.md`, `docs/INTEGRATION.md`
/// § Landing a batch), and a blanket cap silently broke all three.
fn ref_is_guarded(ref_name: &str, rules: &[ProtectionRule]) -> bool {
    ref_name == ALWAYS_GUARDED_REF || EffectiveRules::for_ref(ref_name, rules).has_explicit_match
}

/// Cap a role a pusher holds only by attestation to someone else.
///
/// **Not a `buzz-protect` rule, and it has no default to set.** A per-repo
/// opt-in would leave every repository that never set one with an inherited
/// Owner grant over its own trunk, which is how a hired lead came to hold
/// owner authority over this repository in live run 3.
///
/// A seat's *own* roster row is untouched — a seat added to the channel as
/// Admin is Admin by its own grant, not by inheritance. What an inherited
/// grant can no longer do **on a guarded ref** ([`ref_is_guarded`]) is
/// force-push, delete a ref, or overwrite a tag
/// ([`buzz_core::git_perms::default_min_role`] puts all three at Admin).
///
/// **What this does not close.** Run 3's push was a *fast-forward*
/// (`1dd98e876..07c470be0`), and a fast-forward to `refs/heads/*` needs only
/// Member — which a capped seat still holds. The rule that refuses that push
/// is `require-verdict`, and it is opt-in: until someone sets it on a
/// repository, this cap closes the inherited-Owner escalation and nothing
/// else.
fn cap_inherited(role: MemberRole, inherited: bool) -> MemberRole {
    if inherited && role.permission_level() > MemberRole::Member.permission_level() {
        MemberRole::Member
    } else {
        role
    }
}

/// Everything after HMAC and TTL validation: rules, roles, and the verdict
/// gate. Returns a decision rather than a `Response` so the caller can log it.
async fn decide_push(
    state: &Arc<AppState>,
    req: &HookCallbackRequest,
    community: buzz_core::CommunityId,
) -> PolicyOutcome {
    // 4. Validate and resolve kind:30617 for this repo.
    // Query by (community_id, kind=30617, pubkey=owner, d_tag=repo_id) to
    // prevent spoofing and keep the localhost hook callback on the same
    // server-resolved tenant as the git HTTP request that spawned it.
    let owner_bytes = match hex::decode(&req.repo_owner) {
        Ok(b) if b.len() == 32 => b,
        _ => {
            return PolicyOutcome::Refused("invalid repo owner");
        }
    };
    let query = EventQuery {
        kinds: Some(vec![30617]),
        pubkey: Some(owner_bytes.clone()),
        d_tag: Some(req.repo_id.clone()),
        global_only: true,
        limit: Some(1),
        ..EventQuery::for_community(community)
    };
    let repo_event = match state.db.query_events(&query).await {
        Ok(mut events) => {
            if let Some(event) = events.pop() {
                event
            } else {
                warn!(repo = %req.repo_id, "hook callback: kind:30617 not found");
                return PolicyOutcome::Refused("repository not found");
            }
        }
        Err(e) => {
            error!(repo = %req.repo_id, error = %e, "hook callback: DB error");
            return PolicyOutcome::Refused("internal error");
        }
    };

    // 5. Parse protection rules from kind:30617 tags.
    let tags: Vec<Vec<String>> = repo_event
        .event
        .tags
        .iter()
        .map(|t| t.as_slice().to_vec())
        .collect();

    let rules = match parse_protection_tags(&tags) {
        Ok(parsed) => {
            // Log unknown rules as warnings (helps catch typos).
            for unknown in &parsed.unknown_rules {
                warn!(repo = %req.repo_id, rule = %unknown, "unknown buzz-protect rule (skipped)");
            }
            parsed.rules
        }
        Err(e) => {
            warn!(repo = %req.repo_id, error = %e, "hook callback: malformed protection tags");
            // Fail-closed: malformed rules = deny.
            return PolicyOutcome::Refused("malformed protection rules");
        }
    };

    // 6. Resolve channel binding via the shared resolver (same first-tag,
    // fail-closed semantics as the read gate) and check archived state
    // (applies to ALL pushers including owner).
    //
    // `Broken` denies HERE, before owner resolution: a malformed or
    // ambiguous first binding fails closed for *everyone*, exactly like the
    // read gate. Letting it fall through as "unbound" would hand the owner
    // short-circuit below a push path through a binding the read gate
    // refuses to honor — the tri-state exists precisely so Broken and
    // NotBound cannot collapse. Only genuinely-NotBound repos proceed, and
    // only they may earn the remediation-token denial.
    let channel_id = match crate::api::git::binding::resolve_repo_binding(&repo_event.event) {
        crate::api::git::binding::RepoBinding::Bound(id) => Some(id),
        crate::api::git::binding::RepoBinding::NotBound => None,
        crate::api::git::binding::RepoBinding::Broken => {
            warn!(repo = %req.repo_id, "hook callback: broken buzz-channel binding");
            // Deliberately NOT the no_channel_binding token body: the
            // remediation contract is NotBound-only. A broken binding is
            // ambiguity, and ambiguity gets a generic denial (matching the
            // read gate's posture for the same announcement).
            return PolicyOutcome::Refused("invalid channel binding");
        }
    };

    if let Some(ch_id) = channel_id {
        match state.db.get_channel(community, ch_id).await {
            Ok(ch) if ch.archived_at.is_some() => {
                return PolicyOutcome::Refused("channel is archived (read-only)");
            }
            Err(e) => {
                error!(error = %e, "hook callback: channel lookup failed");
                return PolicyOutcome::Refused("internal error");
            }
            _ => {} // Channel exists and is not archived.
        }
    }

    // 7. Resolve pusher's role. A cryptographically verified managed-agent
    // owner has the same repository authority as the agent key itself.
    let repo_owner_hex = hex::encode(repo_event.event.pubkey.to_bytes());
    let pusher_bytes = match hex::decode(&req.pusher_pubkey) {
        Ok(bytes) if bytes.len() == 32 => bytes,
        _ => return PolicyOutcome::Refused("invalid pusher pubkey"),
    };
    let is_repo_owner = req.pusher_pubkey == repo_owner_hex;
    let is_managed_agent_owner = if is_repo_owner {
        false
    } else {
        match state
            .db
            .is_agent_owner(community, &owner_bytes, &pusher_bytes)
            .await
        {
            Ok(is_owner) => is_owner,
            Err(error) => {
                error!(
                    repo = %req.repo_id,
                    error = %error,
                    "hook callback: managed-agent owner lookup failed"
                );
                return PolicyOutcome::Refused("internal error");
            }
        }
    };

    // The pusher's own NIP-OA owner, if it is a managed seat. The hook
    // callback carries only the pusher pubkey — it cannot see this push's
    // attestation — so the mapping the git request extractor materialized from
    // that attestation is what a seat's grant is resolved through. A seat signs
    // git as itself (the fence forbids signing as its operator), so without
    // this it holds no roster row anywhere and every hired seat's push is
    // refused after the read gate already let it in.
    let seat_owner_bytes = match state
        .db
        .get_agent_channel_policy(community, &pusher_bytes)
        .await
    {
        Ok(Some((_, owner))) => owner,
        Ok(None) => None,
        Err(error) => {
            error!(
                repo = %req.repo_id,
                error = %error,
                "hook callback: seat owner lookup failed"
            );
            return PolicyOutcome::Refused("internal error");
        }
    };
    // A seat of the repo owner pushes on INHERITED authority. On a guarded
    // ref (`ref_is_guarded`) that authority is capped at Member; everywhere
    // else it is the operator's own tier, exactly as before batch 3. Until
    // batch 3 it was the operator's tier everywhere, which is how a hired lead
    // came to hold owner authority over this repository's trunk in live run 3.
    let pushes_for_repo_owner = seat_owner_bytes.as_deref() == Some(owner_bytes.as_slice());

    // The repo's own `["project", …]` back-reference, if any. Read from the
    // announcement rather than the `git_repo_names.project_ref` projection so
    // the gate agrees with the signed event even if the projection is stale.
    let project_ref = buzz_core::kind::repo_project_ref(&repo_event.event);

    // Principals a grant may be found under, each flagged with whether the
    // grant would be INHERITED: the signing key itself (never inherited),
    // then the owner it is attested to (always inherited). Inheritance, never
    // a bypass — an owner with no grant grants nothing, and the denial copy is
    // unchanged.
    let principals: Vec<(&[u8], bool)> = std::iter::once((pusher_bytes.as_slice(), false))
        .chain(
            seat_owner_bytes
                .as_deref()
                .filter(|owner| *owner != pusher_bytes.as_slice())
                .map(|owner| (owner, true)),
        )
        .collect();

    // Two tiers, because the cap is scoped to guarded refs (`ref_is_guarded`).
    // `git_role` is what this key holds on an ordinary topic branch — for
    // everyone but a seat, and for a seat off a guarded ref, that is exactly
    // the tier resolved before batch 3. `guarded_role` is the same resolution
    // with inherited grants capped at Member, and is used on `refs/heads/main`
    // and on any ref an operator wrote a `buzz-protect` rule for.
    let mut guarded_role: Option<MemberRole> = None;
    let git_role = if is_repo_owner || is_managed_agent_owner {
        // Not inheritance from a *seat*: the announcement's own author, or the
        // human who owns the managed agent that authored it. Nothing to cap.
        MemberRole::Owner
    } else {
        // Two ACLs, both additive: the project's curated roster and the bound
        // channel's membership. Resolve each independently and take the more
        // permissive grant — a channel Admin must not be demoted for also
        // being a project Collaborator, nor a project Owner for also being a
        // channel Guest. Neither granting is what denies.
        let mut project_role = None;
        let mut project_role_guarded = None;
        if let Some(coordinate) = &project_ref {
            for (principal, inherited) in &principals {
                match state
                    .db
                    .get_project_role_by_coordinate(community, coordinate, principal)
                    .await
                {
                    Ok(role) => {
                        if let Some(resolved) = role.and_then(git_role_for_project_role) {
                            project_role = Some(match project_role {
                                Some(current) => max_git_role(current, resolved),
                                None => resolved,
                            });
                            let capped = cap_inherited(resolved, *inherited);
                            project_role_guarded = Some(match project_role_guarded {
                                Some(current) => max_git_role(current, capped),
                                None => capped,
                            });
                        }
                    }
                    Err(e) => {
                        error!(repo = %req.repo_id, error = %e, "hook callback: project role lookup failed");
                        return PolicyOutcome::Refused("internal error");
                    }
                }
            }
        }

        let mut channel_role = None;
        let mut channel_role_guarded = None;
        if let Some(ch_id) = channel_id {
            for (principal, inherited) in &principals {
                let resolved = match state.db.get_member_role(community, ch_id, principal).await {
                    Ok(Some(role_str)) => match role_str.parse::<MemberRole>() {
                        // Bots are intentionally added to channels by members
                        // and admins; for git push they are ordinary members.
                        // Protection rules still apply. Bot is a designation
                        // (what it is), not a permission tier (what it can do).
                        // Normalized here rather than after ranking so an
                        // out-of-hierarchy Bot never loses a max() it should
                        // win.
                        Ok(MemberRole::Bot) => Some(MemberRole::Member),
                        Ok(role) => Some(role),
                        Err(_) => {
                            error!(role = %role_str, "hook callback: unknown role");
                            return PolicyOutcome::Refused("internal error");
                        }
                    },
                    Ok(None) => None,
                    Err(e) => {
                        error!(error = %e, "hook callback: role lookup failed");
                        return PolicyOutcome::Refused("internal error");
                    }
                };
                if let Some(resolved) = resolved {
                    channel_role = Some(match channel_role {
                        Some(current) => max_git_role(current, resolved),
                        None => resolved,
                    });
                    let capped = cap_inherited(resolved, *inherited);
                    channel_role_guarded = Some(match channel_role_guarded {
                        Some(current) => max_git_role(current, capped),
                        None => capped,
                    });
                }
            }
        }

        // A seat of the repo owner holds the owner's authority by attestation
        // alone, with no roster row anywhere. That path survives — a seat must
        // still be able to push, rebase and delete a topic branch — as the
        // owner's own tier off a guarded ref, and as Member on one.
        let inherited_owner_seat = pushes_for_repo_owner.then_some(MemberRole::Owner);
        let inherited_owner_seat_guarded = pushes_for_repo_owner.then_some(MemberRole::Member);
        let combine = |a: Option<MemberRole>, b: Option<MemberRole>| match (a, b) {
            (Some(a), Some(b)) => Some(max_git_role(a, b)),
            (Some(role), None) | (None, Some(role)) => Some(role),
            (None, None) => None,
        };
        let roster_role = combine(project_role, channel_role);
        guarded_role = combine(
            combine(project_role_guarded, channel_role_guarded),
            inherited_owner_seat_guarded,
        );
        match combine(roster_role, inherited_owner_seat) {
            Some(role) => role,
            None => {
                // Denial copy tells the pusher which door to knock on, and
                // must not invent one that does not exist.
                return match (&project_ref, channel_id) {
                    // No door at all. Declared cross-component contract —
                    // see the const docs in buzz-core::git_perms for who
                    // consumes the token and why the body also repeats the
                    // legacy phrase. Emitted ONLY here: a repo inside a
                    // project is legitimately unbound, and telling its
                    // pusher to bind a channel would be advice for a problem
                    // they do not have.
                    (None, None) => {
                        warn!(repo = %req.repo_id, "hook callback: repo has neither a channel binding nor a project");
                        PolicyOutcome::Refused(GIT_NO_CHANNEL_BINDING_BODY)
                    }
                    (Some(_), None) => PolicyOutcome::Refused("not a project member"),
                    (None, Some(_)) => PolicyOutcome::Refused("not a channel member"),
                    (Some(_), Some(_)) => {
                        PolicyOutcome::Refused("not a project member or channel member")
                    }
                };
            }
        }
    };

    // 8. Classify ref updates and evaluate policy.
    let updates: Vec<RefUpdate> = req
        .ref_updates
        .iter()
        .map(|r| RefUpdate {
            ref_name: r.ref_name.clone(),
            kind: UpdateKind::classify(&r.old_oid, &r.new_oid, r.is_ancestor),
            old_oid: r.old_oid.clone(),
            new_oid: r.new_oid.clone(),
        })
        .collect();

    // The role check, per ref, with the tier that ref calls for. `git_role`
    // off a guarded ref keeps a seat able to rebase-and-force-push its own
    // lane branch; `guarded_role` caps an inherited grant on the trunk and on
    // anything explicitly protected. They are equal for every pusher whose
    // authority is not inherited, so this is a no-op for a human.
    let guarded_role = guarded_role.unwrap_or(git_role);
    let mut denials: Vec<Denial> = updates
        .iter()
        .filter_map(|update| {
            let role = if ref_is_guarded(&update.ref_name, &rules) {
                guarded_role
            } else {
                git_role
            };
            evaluate_ref_update(update, role, &rules).err()
        })
        .collect();

    // 9. The verdict gate. It runs only for updates whose effective rules set
    // `require-verdict`, and only for updates the role check already allowed —
    // the rule subtracts, never adds. An ordinary push therefore issues no
    // session query at all.
    let gated: Vec<RefUpdate> =
        crate::api::git::verdict_admission::refs_requiring_verdict(&updates, &rules, &denials)
            .into_iter()
            .cloned()
            .collect();
    for update in &gated {
        match crate::api::git::verdict_admission::search_verdict_admission(
            state,
            &VerdictSearchRequest {
                community,
                channel_id,
                repo_owner_hex: &repo_owner_hex,
                repo_owner_bytes: &owner_bytes,
                ref_name: &update.ref_name,
                new_oid: &update.new_oid,
                pusher_pubkey: &req.pusher_pubkey,
            },
        )
        .await
        {
            VerdictSearch::Admitted => {}
            VerdictSearch::Refused(refusal) => denials.push(Denial {
                ref_name: update.ref_name.clone(),
                reason: refusal.reason(),
            }),
            // Fail closed with the handler's generic body: a storage failure
            // is not evidence that a verdict exists.
            VerdictSearch::Unavailable => return PolicyOutcome::Refused("internal error"),
        }
    }

    if denials.is_empty() {
        PolicyOutcome::Allowed
    } else {
        PolicyOutcome::Denied(denials)
    }
}

/// Generate the HMAC signature for a hook callback payload.
///
/// Called by the relay when setting up the pre-receive hook environment.
pub fn generate_hook_hmac(
    secret: &[u8],
    repo_id: &str,
    repo_owner: &str,
    community_id: &str,
    pusher_pubkey: &str,
    ref_updates: &[HookRefUpdate],
    timestamp: u64,
) -> String {
    let req = HookCallbackRequest {
        repo_id: repo_id.to_string(),
        repo_owner: repo_owner.to_string(),
        community_id: community_id.to_string(),
        pusher_pubkey: pusher_pubkey.to_string(),
        ref_updates: ref_updates.to_vec(),
        timestamp,
        signature: String::new(), // Not used in computation.
    };
    let mac_bytes = compute_hmac(secret, &req);
    hex::encode(mac_bytes)
}

#[cfg(test)]
#[path = "policy_tests.rs"]
pub(crate) mod tests;
