//! HTTP API — media, git, NIP-05, and the Nostr HTTP bridge.

pub mod admin;
pub mod bridge;
pub mod events;
pub mod git;
pub mod invites;
pub mod media;
pub mod mesh_demo;
pub mod nip05;
pub mod operator;
pub mod system_health;
pub mod workflows;

// Re-export imeta helpers used by ingest pipeline.
pub use crate::handlers::imeta::{validate_imeta_tags, verify_imeta_blobs};

use axum::{http::StatusCode, response::Json};

/// Standard error envelope.
pub(crate) fn api_error(status: StatusCode, msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    (status, Json(serde_json::json!({ "error": msg })))
}

pub(crate) fn internal_error(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    tracing::error!("Internal error: {msg}");
    api_error(StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
}

#[allow(dead_code)]
pub(crate) fn not_found(msg: &str) -> (StatusCode, Json<serde_json::Value>) {
    api_error(StatusCode::NOT_FOUND, msg)
}

/// Relay membership enforcement — single gate for all authenticated entry points.
///
/// Moved here from the deleted `relay_members` module. Called by `media.rs`, `bridge.rs`,
/// `git/transport.rs`, and `audio/handler.rs`.
pub mod relay_members {
    use axum::{
        http::{HeaderMap, StatusCode},
        response::Json,
    };
    use beekeeper_core::{tenant::CommunityId, TenantContext};
    use tracing::{debug, info};

    use crate::state::AppState;

    /// Transport-neutral outcome of a relay-membership check.
    #[derive(Debug, Clone, PartialEq, Eq)]
    pub enum MembershipDecision {
        /// Relay membership enforcement is disabled.
        OpenRelay,
        /// Caller is directly present in `relay_members`.
        Member,
        /// Caller is admitted through a NIP-OA owner that is a relay member.
        ViaOwner(nostr::PublicKey),
        /// Caller is not admitted.
        Denied,
    }

    /// Return the sole NIP-OA credential header, if one was supplied.
    ///
    /// Repeated security-sensitive headers are ambiguous across HTTP stacks,
    /// so they are treated as no credential instead of silently selecting one.
    pub fn extract_auth_tag_header(headers: &HeaderMap) -> Option<&str> {
        let mut values = headers.get_all("x-auth-tag").iter();
        let (Some(value), None) = (values.next(), values.next()) else {
            return None;
        };
        value.to_str().ok()
    }

    /// Check relay membership without committing to an HTTP response shape.
    ///
    /// `community` is the server-resolved tenant of the request; membership is
    /// scoped to it so admitting a pubkey to community A never admits it to B.
    /// A NIP-OA credential is usable only when `signed_auth_created_at` came
    /// from the already-verified authentication event carrying that request.
    pub async fn check_relay_membership(
        state: &AppState,
        community: CommunityId,
        pubkey_bytes: &[u8],
        auth_tag_header: Option<&str>,
        signed_auth_created_at: Option<u64>,
    ) -> Result<MembershipDecision, String> {
        if !state.config.require_relay_membership {
            return Ok(MembershipDecision::OpenRelay);
        }

        let pubkey_hex = hex::encode(pubkey_bytes);
        let is_member = state
            .db
            .is_relay_member(community, &pubkey_hex)
            .await
            .map_err(|e| format!("relay membership check failed: {e}"))?;
        if is_member {
            return Ok(MembershipDecision::Member);
        }

        if state.config.allow_nip_oa_auth {
            if let Some(tag_json) = auth_tag_header {
                let agent_pubkey = nostr::PublicKey::from_slice(pubkey_bytes)
                    .map_err(|e| format!("invalid agent pubkey for NIP-OA check: {e}"))?;
                let Some(auth_created_at) = signed_auth_created_at else {
                    info!(agent = %pubkey_hex, "NIP-OA auth tag has no verified signed auth timestamp");
                    return Ok(MembershipDecision::Denied);
                };

                match beekeeper_sdk::nip_oa::verify_auth_tag_for_auth_event(
                    tag_json,
                    &agent_pubkey,
                    auth_created_at,
                ) {
                    Ok(owner_pubkey) => {
                        let owner_hex = owner_pubkey.to_hex();
                        let owner_is_member = state
                            .db
                            .is_relay_member(community, &owner_hex)
                            .await
                            .map_err(|e| format!("relay membership check (owner) failed: {e}"))?;
                        if owner_is_member {
                            debug!(
                                agent = %pubkey_hex,
                                owner = %owner_hex,
                                "NIP-OA membership granted via owner"
                            );
                            return Ok(MembershipDecision::ViaOwner(owner_pubkey));
                        }
                    }
                    Err(e) => {
                        info!(agent = %pubkey_hex, "NIP-OA auth tag invalid: {e}");
                    }
                }
            }
        }

        Ok(MembershipDecision::Denied)
    }

    /// Enforce relay membership for a pubkey, with NIP-OA agent delegation fallback.
    ///
    /// Returns `Ok(Some(owner_pubkey))` when the agent is not a direct member but
    /// its NIP-OA owner *is* — access is granted via delegation.
    ///
    /// On open relays (`require_relay_membership = false`), returns `Ok(None)`
    /// immediately — no membership check is performed. Callers that need NIP-OA
    /// owner extraction on open relays should call [`extract_nip_oa_owner`] directly.
    ///
    /// Returns `Ok(None)` when the caller is a direct member (closed relay) or when
    /// no NIP-OA tag is present/applicable (open relay without auth tag).
    pub async fn enforce_relay_membership(
        state: &AppState,
        community: CommunityId,
        pubkey_bytes: &[u8],
        auth_tag_header: Option<&str>,
        signed_auth_created_at: Option<u64>,
    ) -> Result<Option<nostr::PublicKey>, (StatusCode, Json<serde_json::Value>)> {
        match check_relay_membership(
            state,
            community,
            pubkey_bytes,
            auth_tag_header,
            signed_auth_created_at,
        )
        .await
        {
            Ok(MembershipDecision::OpenRelay) | Ok(MembershipDecision::Member) => Ok(None),
            Ok(MembershipDecision::ViaOwner(owner)) => Ok(Some(owner)),
            Ok(MembershipDecision::Denied) => Err((
                StatusCode::FORBIDDEN,
                Json(serde_json::json!({
                    "error": "relay_membership_required",
                    "message": "You must be a relay member to access this relay"
                })),
            )),
            Err(e) => {
                tracing::error!("relay membership check errored: {e}");
                Err(super::internal_error(&e))
            }
        }
    }

    /// Extract NIP-OA owner from an auth tag without membership enforcement.
    ///
    /// Used on open relays (`require_relay_membership = false`) to opportunistically
    /// extract the owner pubkey for agent→owner backfill. The NIP-OA signature is
    /// cryptographically self-proving, so no feature flag is needed. Temporal
    /// conditions are evaluated against `signed_auth_created_at`. Returns
    /// `None` if the tag, timestamp, or conditions are absent or invalid.
    pub fn extract_nip_oa_owner(
        pubkey_bytes: &[u8],
        auth_tag_header: Option<&str>,
        signed_auth_created_at: Option<u64>,
    ) -> Option<nostr::PublicKey> {
        let tag_json = auth_tag_header?;
        let auth_created_at = signed_auth_created_at?;
        let agent_pubkey = nostr::PublicKey::from_slice(pubkey_bytes).ok()?;
        match beekeeper_sdk::nip_oa::verify_auth_tag_for_auth_event(
            tag_json,
            &agent_pubkey,
            auth_created_at,
        ) {
            Ok(owner) => Some(owner),
            Err(e) => {
                info!("extract_nip_oa_owner: invalid auth tag: {e}");
                None
            }
        }
    }

    /// What became of an attempt to put a NIP-OA agent→owner relationship on
    /// record. Callers that admitted a login *through* that owner (NIP-AA
    /// step 6) must retain the owner and therefore need to know the difference
    /// between "the store refused" and "the store could not answer" — the
    /// first is a rule, the second is a transient — instead of one `false`.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub enum OwnerMaterialization {
        /// The relationship is on record: written now, or already present
        /// naming this same owner.
        Recorded,
        /// The agent is already bound to a *different* owner. The mapping is
        /// first-write-wins, so this credential's owner cannot be recorded
        /// and retrying with the same credential will not change that.
        Conflict,
        /// The store could not say: `ensure_user`, the mapping write, or the
        /// mapping read errored. Nothing is known about the relationship.
        Unavailable,
    }

    /// Persist a cryptographically verified NIP-OA agent→owner relationship.
    ///
    /// Both principals are ensured first because `agent_owner_pubkey` has a
    /// community-scoped foreign key. The mapping is first-write-wins; an
    /// existing mapping is accepted only when it names the same owner.
    ///
    /// Collapses [`materialize_nip_oa_owner_outcome`] to a bool for callers
    /// that only record the owner opportunistically (HTTP bridge, git
    /// transport). The WebSocket AUTH path, which may have admitted the
    /// login *because of* this owner, uses the outcome directly so it can
    /// refuse with the right sentence.
    pub async fn materialize_nip_oa_owner(
        state: &AppState,
        tenant: &TenantContext,
        agent: &nostr::PublicKey,
        owner: &nostr::PublicKey,
    ) -> bool {
        materialize_nip_oa_owner_outcome(state, tenant, agent, owner).await
            == OwnerMaterialization::Recorded
    }

    /// Record a NIP-OA owner and **fail closed** when a login that depended on
    /// it cannot keep it (finding 92; 2026-09-05 refuter, F1).
    ///
    /// The one step `POST /events` and git smart-HTTP share with the WebSocket
    /// AUTH path, so the three surfaces cannot disagree about what NIP-AA:113's
    /// "retain O" obliges. `admitted_via_owner` is the whole question: a login
    /// the membership gate admitted *because* this owner is a member must not
    /// proceed with the relationship unrecorded, while on an open relay the
    /// same credential would be admitted with no tag at all, so refusing there
    /// would enforce nothing.
    ///
    /// `Err` carries the sentence to return — the same two
    /// [`crate::handlers::auth`] prints, since a person hitting the transient
    /// and a person hitting the conflict need different next steps.
    pub async fn retain_nip_oa_owner(
        state: &AppState,
        tenant: &TenantContext,
        agent: &nostr::PublicKey,
        owner: &nostr::PublicKey,
        admitted_via_owner: bool,
    ) -> Result<(), (&'static str, &'static str)> {
        let admission = if admitted_via_owner {
            crate::handlers::auth::LoginAdmission::ViaOwner(*owner)
        } else {
            crate::handlers::auth::LoginAdmission::OpenRelayCredential(*owner)
        };
        let outcome = materialize_nip_oa_owner_outcome(state, tenant, agent, owner).await;
        match crate::handlers::auth::decide_owner_retention(&admission, Some(outcome)) {
            crate::handlers::auth::OwnerRetention::Authenticate { .. } => Ok(()),
            crate::handlers::auth::OwnerRetention::Refuse { reason, message } => {
                Err((reason, message))
            }
        }
    }

    /// [`materialize_nip_oa_owner`] with the failure kind preserved.
    pub async fn materialize_nip_oa_owner_outcome(
        state: &AppState,
        tenant: &TenantContext,
        agent: &nostr::PublicKey,
        owner: &nostr::PublicKey,
    ) -> OwnerMaterialization {
        for (role, pubkey) in [("agent", agent), ("owner", owner)] {
            match state
                .db
                .ensure_user(tenant.community(), pubkey.as_bytes())
                .await
            {
                Ok(true) => {
                    metrics::counter!(
                        "buzz_users_created_total",
                        "community" => tenant.host().to_owned()
                    )
                    .increment(1);
                }
                Ok(false) => {}
                Err(e) => {
                    tracing::warn!(%role, error = %e, "ensure_user failed during NIP-OA backfill");
                    return OwnerMaterialization::Unavailable;
                }
            }
        }

        let outcome = match state
            .db
            .set_agent_owner(tenant.community(), agent.as_bytes(), owner.as_bytes())
            .await
        {
            Ok(true) => OwnerMaterialization::Recorded,
            Ok(false) => match state
                .db
                .is_agent_owner(tenant.community(), agent.as_bytes(), owner.as_bytes())
                .await
            {
                Ok(true) => OwnerMaterialization::Recorded,
                Ok(false) => OwnerMaterialization::Conflict,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to read back agent_owner_pubkey");
                    OwnerMaterialization::Unavailable
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "failed to backfill agent_owner_pubkey");
                OwnerMaterialization::Unavailable
            }
        };

        if outcome == OwnerMaterialization::Recorded {
            state
                .author_type_cache
                .insert((tenant.community(), agent.to_bytes().to_vec()), true);
            state.observer_owner_cache.insert(
                (
                    tenant.community(),
                    agent.to_bytes().to_vec(),
                    owner.to_bytes().to_vec(),
                ),
                true,
            );
        }
        outcome
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use axum::http::{HeaderMap, HeaderValue};
        use beekeeper_sdk::nip_oa::compute_auth_tag;
        use nostr::Keys;
        use std::sync::Arc;

        /// A relay state whose Postgres is a lazy pool at a port nothing
        /// listens on, so the first `ensure_user` errors without any live
        /// infrastructure. Mirrors `crate::state::tests::test_state`, which
        /// points at the real `DATABASE_URL` and so cannot stand in here.
        async fn unreachable_db_state() -> Arc<AppState> {
            let mut config = crate::config::Config::from_env().expect("default config loads");
            config.require_relay_membership = true;
            config.allow_nip_oa_auth = true;
            config.redis_url = "redis://127.0.0.1:1".to_string();
            config.database_url = "postgres://buzz:unreachable@127.0.0.1:1/buzz".to_string(); // sadscan:disable np.postgres.1

            // A refused connect is retried until `acquire_timeout` (30s by
            // default); cap it so the unit gate does not stall on a port
            // nothing listens on.
            let pool = sqlx::postgres::PgPoolOptions::new()
                .acquire_timeout(std::time::Duration::from_secs(2))
                .connect_lazy(&config.database_url)
                .expect("lazy pg pool");
            let db = beekeeper_db::Db::from_pool(pool.clone());
            let redis_pool = deadpool_redis::Config::from_url(&config.redis_url)
                .create_pool(Some(deadpool_redis::Runtime::Tokio1))
                .expect("redis pool");
            let pubsub = Arc::new(
                beekeeper_pubsub::PubSubManager::new(&config.redis_url, redis_pool.clone())
                    .await
                    .expect("pubsub manager"),
            );
            let audit = beekeeper_audit::AuditService::new(pool.clone());
            let auth = beekeeper_auth::AuthService::new(config.auth.clone());
            let search = beekeeper_search::SearchService::new(pool.clone());
            let workflow_engine = Arc::new(beekeeper_workflow::WorkflowEngine::new(
                db.clone(),
                beekeeper_workflow::WorkflowConfig::default(),
            ));
            let media_storage =
                beekeeper_media::MediaStorage::new(&config.media).expect("media storage");
            let (state, _audit_shutdown) = AppState::new(
                config,
                db,
                redis_pool,
                audit,
                pubsub,
                auth,
                search,
                workflow_engine,
                nostr::Keys::generate(),
                media_storage,
            );
            Arc::new(state)
        }

        /// F1: the same injected failure, through the step `POST /events` and
        /// git smart-HTTP now share. A login admitted **through** its owner is
        /// refused with the transient's own sentence; a login that carried the
        /// credential but did not need it proceeds, because dropping the tag
        /// would have admitted it anyway.
        #[tokio::test]
        async fn a_login_admitted_through_its_owner_is_refused_when_the_store_cannot_answer() {
            let state = unreachable_db_state().await;
            let tenant = beekeeper_core::TenantContext::resolved(
                beekeeper_core::tenant::CommunityId::from_uuid(uuid::Uuid::new_v4()),
                "adm-f1.example",
            );
            let agent = Keys::generate().public_key();
            let owner = Keys::generate().public_key();

            assert_eq!(
                retain_nip_oa_owner(&state, &tenant, &agent, &owner, true).await,
                Err((
                    crate::handlers::auth::NIP_OA_OWNER_UNRECORDED_REASON,
                    crate::handlers::auth::NIP_OA_OWNER_UNRECORDED_MESSAGE,
                ))
            );
            assert_eq!(
                retain_nip_oa_owner(&state, &tenant, &agent, &owner, false).await,
                Ok(()),
                "an open-relay credential enforces nothing, so an unrecordable owner is not fatal"
            );
        }

        /// Finding 92, the injected failure: when `ensure_user` cannot reach
        /// the store, the outcome is `Unavailable` — and the bool wrapper the
        /// opportunistic callers use reads it as "not recorded". This is the
        /// `false` that used to authenticate an ownerless virtual login.
        #[tokio::test]
        async fn unreachable_store_is_unavailable_not_recorded() {
            let state = unreachable_db_state().await;
            let tenant = beekeeper_core::TenantContext::resolved(
                beekeeper_core::tenant::CommunityId::from_uuid(uuid::Uuid::new_v4()),
                "adm-c.example",
            );
            let agent = Keys::generate().public_key();
            let owner = Keys::generate().public_key();

            assert_eq!(
                materialize_nip_oa_owner_outcome(&state, &tenant, &agent, &owner).await,
                OwnerMaterialization::Unavailable
            );
            assert!(
                !materialize_nip_oa_owner(&state, &tenant, &agent, &owner).await,
                "the bool wrapper must not read an unreachable store as recorded"
            );
        }

        #[test]
        fn auth_tag_header_must_be_unique() {
            let mut headers = HeaderMap::new();
            assert_eq!(extract_auth_tag_header(&headers), None);

            headers.insert("x-auth-tag", HeaderValue::from_static("credential-one"));
            assert_eq!(extract_auth_tag_header(&headers), Some("credential-one"));

            headers.append("x-auth-tag", HeaderValue::from_static("credential-two"));
            assert_eq!(extract_auth_tag_header(&headers), None);
        }

        /// Valid NIP-OA auth tag → returns Some(owner_pubkey).
        #[test]
        fn valid_nip_oa_returns_owner() {
            let owner_keys = Keys::generate();
            let agent_keys = Keys::generate();
            let agent_pubkey = agent_keys.public_key();

            let tag_json = compute_auth_tag(&owner_keys, &agent_pubkey, "")
                .expect("compute_auth_tag must succeed");

            let result = extract_nip_oa_owner(
                &agent_pubkey.to_bytes(),
                Some(&tag_json),
                Some(nostr::Timestamp::now().as_secs()),
            );

            assert_eq!(result, Some(owner_keys.public_key()));
        }

        #[test]
        fn nip_oa_time_conditions_use_signed_auth_event_time() {
            let owner_keys = Keys::generate();
            let agent_pubkey = Keys::generate().public_key();

            let expired = compute_auth_tag(&owner_keys, &agent_pubkey, "created_at<200")
                .expect("sign expired credential");
            assert_eq!(
                extract_nip_oa_owner(&agent_pubkey.to_bytes(), Some(&expired), Some(200)),
                None
            );

            let future = compute_auth_tag(&owner_keys, &agent_pubkey, "created_at>200")
                .expect("sign future credential");
            assert_eq!(
                extract_nip_oa_owner(&agent_pubkey.to_bytes(), Some(&future), Some(200)),
                None
            );

            let in_window = compute_auth_tag(
                &owner_keys,
                &agent_pubkey,
                "kind=9&created_at>199&created_at<201",
            )
            .expect("sign in-window credential");
            assert_eq!(
                extract_nip_oa_owner(&agent_pubkey.to_bytes(), Some(&in_window), Some(200)),
                Some(owner_keys.public_key())
            );
            assert_eq!(
                extract_nip_oa_owner(&agent_pubkey.to_bytes(), Some(&in_window), None),
                None,
                "a credential without a verified signed auth timestamp must fail closed"
            );
        }

        /// No auth tag → returns None.
        #[test]
        fn no_auth_tag_returns_none() {
            let agent_keys = Keys::generate();
            let agent_pubkey = agent_keys.public_key();

            let result = extract_nip_oa_owner(
                &agent_pubkey.to_bytes(),
                None,
                Some(nostr::Timestamp::now().as_secs()),
            );

            assert_eq!(result, None);
        }

        /// Invalid auth tag → returns None.
        #[test]
        fn invalid_auth_tag_returns_none() {
            let agent_keys = Keys::generate();
            let agent_pubkey = agent_keys.public_key();

            let result = extract_nip_oa_owner(
                &agent_pubkey.to_bytes(),
                Some("not valid json"),
                Some(nostr::Timestamp::now().as_secs()),
            );

            assert_eq!(result, None);
        }
    }
}
