//! NIP-42 AUTH handler — verify challenge response, transition auth state.
//!
//! Relay membership enforcement uses the shared
//! [`crate::api::relay_members::enforce_relay_membership`] helper, which supports
//! NIP-OA owner-delegation fallback on closed relays. On open relays, the auth
//! handler calls [`crate::api::relay_members::extract_nip_oa_owner`] directly to
//! extract the owner pubkey for agent→owner backfill (observer frame auth).
//!
//! For WebSocket auth, the NIP-OA `auth` tag is extracted from the signed AUTH
//! event itself (the tag is integrity-protected by the event signature).
//!
//! A login admitted *through* its NIP-OA owner (closed relay, agent not a
//! member, owner is) must retain that owner for the life of the connection
//! (NIP-AA step 6). If the agent→owner relationship cannot be put on record,
//! the login is refused — never authenticated ownerless (finding 92). The
//! decision is [`decide_owner_retention`], a pure function, so the fail-closed
//! table is testable without a database.

use std::sync::Arc;

use axum::extract::ws::Message as WsMessage;
use tracing::{debug, info, warn};

use crate::api::relay_members::OwnerMaterialization;
use crate::connection::{AuthState, ConnectionState};
use crate::protocol::RelayMessage;
use crate::state::AppState;

/// Metric reason (`buzz_auth_failures_total{reason=…}`) when a virtual login's
/// owner relationship could not be recorded because the store did not answer.
pub(crate) const NIP_OA_OWNER_UNRECORDED_REASON: &str = "nip_oa_owner_unrecorded";
/// The OK-frame sentence for [`NIP_OA_OWNER_UNRECORDED_REASON`]. NIP-42 gives a
/// connection one AUTH, so "try again" means reconnect.
pub(crate) const NIP_OA_OWNER_UNRECORDED_MESSAGE: &str =
    "error: owner relationship could not be recorded; try again";
/// Metric reason when the agent is already bound to a different owner than the
/// one whose membership admitted it. Retrying the same credential cannot fix
/// this, so it gets its own sentence rather than "try again".
pub(crate) const NIP_OA_OWNER_CONFLICT_REASON: &str = "nip_oa_owner_conflict";
/// The OK-frame sentence for [`NIP_OA_OWNER_CONFLICT_REASON`].
pub(crate) const NIP_OA_OWNER_CONFLICT_MESSAGE: &str =
    "restricted: agent is already bound to a different owner";

/// How the relay-membership gate admitted a login, which decides whether the
/// NIP-OA owner *must* be retained or is merely recorded when it can be.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum LoginAdmission {
    /// A direct relay member, or a login with no usable credential. There is
    /// no owner to retain.
    Direct,
    /// Admitted only because this owner is a relay member (closed relay,
    /// NIP-OA fallback). NIP-AA step 6: the owner MUST be retained.
    ViaOwner(nostr::PublicKey),
    /// Open relay: the login was admitted on its own, and the credential's
    /// owner is recorded opportunistically for agent→owner backfill. Dropping
    /// the tag would admit the same login, so failing closed here would
    /// enforce nothing.
    OpenRelayCredential(nostr::PublicKey),
}

impl LoginAdmission {
    /// The owner named by the credential, if the login carried one.
    pub(crate) fn owner(&self) -> Option<nostr::PublicKey> {
        match self {
            Self::Direct => None,
            Self::ViaOwner(owner) | Self::OpenRelayCredential(owner) => Some(*owner),
        }
    }
}

/// What the AUTH handler does after the owner relationship was (or was not)
/// recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OwnerRetention {
    /// Authenticate the connection with this owner in its auth context.
    Authenticate { owner: Option<nostr::PublicKey> },
    /// Refuse the AUTH: the login depended on an owner the relay cannot
    /// retain. `reason` is the metric label, `message` the OK-frame text.
    Refuse {
        reason: &'static str,
        message: &'static str,
    },
}

/// The fail-closed table for NIP-OA owner retention (finding 92).
///
/// `outcome` is `None` when nothing was recorded — no credential, or the
/// caller never attempted it. For a [`LoginAdmission::ViaOwner`] login that is
/// still a refusal: there is no path from "admitted through the owner" to
/// "authenticated without one".
pub(crate) fn decide_owner_retention(
    admission: &LoginAdmission,
    outcome: Option<OwnerMaterialization>,
) -> OwnerRetention {
    match admission {
        LoginAdmission::Direct => OwnerRetention::Authenticate { owner: None },
        LoginAdmission::ViaOwner(owner) => match outcome {
            Some(OwnerMaterialization::Recorded) => OwnerRetention::Authenticate {
                owner: Some(*owner),
            },
            Some(OwnerMaterialization::Conflict) => OwnerRetention::Refuse {
                reason: NIP_OA_OWNER_CONFLICT_REASON,
                message: NIP_OA_OWNER_CONFLICT_MESSAGE,
            },
            Some(OwnerMaterialization::Unavailable) | None => OwnerRetention::Refuse {
                reason: NIP_OA_OWNER_UNRECORDED_REASON,
                message: NIP_OA_OWNER_UNRECORDED_MESSAGE,
            },
        },
        LoginAdmission::OpenRelayCredential(owner) => match outcome {
            Some(OwnerMaterialization::Recorded) => OwnerRetention::Authenticate {
                owner: Some(*owner),
            },
            _ => OwnerRetention::Authenticate { owner: None },
        },
    }
}

/// Extract a NIP-OA `auth` tag from a verified AUTH event and serialize it as
/// the JSON-array string that [`buzz_sdk::nip_oa::verify_auth_tag`] expects.
///
/// Returns `None` if no `auth` tag is present (direct-member auth path) or if
/// more than one `auth` tag exists (per NIP-OA spec: >1 auth tag ⇒ no valid tag).
pub fn extract_auth_tag_json(event: &nostr::Event) -> Option<String> {
    let mut iter = event
        .tags
        .iter()
        .filter(|t| t.as_slice().first().map(|s| s.as_str()) == Some("auth"));
    let first = iter.next()?;
    if iter.next().is_some() {
        return None; // NIP-OA spec: treat >1 auth tag as no valid auth tag
    }
    serde_json::to_string(first.as_slice()).ok()
}

/// Handle a NIP-42 AUTH message: verify the challenge response and transition
/// the connection to authenticated state.
///
/// Pure crypto verification — no API tokens, no JWT, no DB token lookups.
#[tracing::instrument(skip_all, fields(event_id, conn_id))]
pub async fn handle_auth(event: nostr::Event, conn: Arc<ConnectionState>, state: Arc<AppState>) {
    let event_id_hex = event.id.to_hex();
    let (challenge, conn_id) = {
        let auth = conn.auth_state.read().await;
        match &*auth {
            AuthState::Pending { challenge } => (challenge.clone(), conn.conn_id),
            AuthState::Authenticated(_) => {
                debug!(conn_id = %conn.conn_id, "AUTH received but already authenticated");
                conn.send(RelayMessage::ok(
                    &event_id_hex,
                    false,
                    "auth-required: already authenticated",
                ));
                return;
            }
            AuthState::Failed => {
                debug!(conn_id = %conn.conn_id, "AUTH received after failed auth");
                conn.send(RelayMessage::ok(
                    &event_id_hex,
                    false,
                    "auth-required: authentication already failed",
                ));
                return;
            }
        }
    };

    // Record the declared span fields now that we have the values.
    tracing::Span::current()
        .record("event_id", event_id_hex.as_str())
        .record("conn_id", conn_id.to_string().as_str());

    // Extract the NIP-OA auth tag before verification consumes the event.
    // The tag is integrity-protected by the event's Schnorr signature — if
    // tampered, NIP-42 verification will fail before we ever inspect it.
    let auth_tag_json = extract_auth_tag_json(&event);
    let signed_auth_created_at = event.created_at.as_secs();

    let relay_url =
        crate::api::bridge::nip42_expected_relay_url(&state.config.relay_url, &conn.tenant);
    let auth_svc = Arc::clone(&state.auth);

    metrics::counter!("buzz_auth_attempts_total", "method" => "nip42").increment(1);

    // Pure NIP-42 verification — crypto only, no DB lookups.
    match auth_svc
        .verify_auth_event(event, &challenge, &relay_url)
        .await
    {
        Ok(mut auth_ctx) => {
            let pubkey = auth_ctx.pubkey;

            // Community ban gate (NIP-42 seam). Runs immediately after auth
            // verification succeeds and before the allowlist and relay-membership
            // gates, per COMMUNITY_MODERATION_PLAN.md §0 decision 4 and the
            // MOD-7/M20 invariant (a ban must block connection auth even for open
            // channels — enforcement is structural, not filtered later). A banned
            // principal gets the standard protocol denial and the connection is
            // dropped with zero further processing.
            //
            // NIP-OA cascade: a ban on the authenticated pubkey blocks it directly;
            // a ban on its cryptographically-proven owner cascades to the agent
            // (owner ban ⇒ agents banned; agent ban is agent-only). The owner is
            // extracted from the self-proving auth tag with no DB round-trip.
            {
                // Fail closed on a DB error, but distinguish it from a real ban:
                // a transient blip must deny (never let a banned principal
                // through) without telling an innocent user they are banned and
                // pinning `Failed` for the connection's life on a false premise.
                // `Banned` claims the ban; `DbError` denies with `error: internal`
                // (mirrors the ingest write-path gate).
                enum BanOutcome {
                    Clear,
                    Banned,
                    DbError,
                }

                let mut outcome = match state
                    .db
                    .moderation_restriction_state(conn.tenant.community(), pubkey.as_bytes())
                    .await
                {
                    Ok(state) if state.banned => BanOutcome::Banned,
                    Ok(_) => BanOutcome::Clear,
                    Err(e) => {
                        warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), error = %e,
                              "ban-state DB lookup failed, denying (fail-closed)");
                        BanOutcome::DbError
                    }
                };

                // Cascade: check the proven NIP-OA owner only if the agent itself
                // is clear (a DB error already denies; a direct ban already blocks
                // — both skip the needless second DB read).
                if matches!(outcome, BanOutcome::Clear) {
                    if let Some(owner) = crate::api::relay_members::extract_nip_oa_owner(
                        pubkey.as_bytes(),
                        auth_tag_json.as_deref(),
                        Some(signed_auth_created_at),
                    ) {
                        outcome = match state
                            .db
                            .moderation_restriction_state(conn.tenant.community(), owner.as_bytes())
                            .await
                        {
                            Ok(state) if state.banned => BanOutcome::Banned,
                            Ok(_) => BanOutcome::Clear,
                            Err(e) => {
                                warn!(conn_id = %conn_id, owner = %owner.to_hex(), error = %e,
                                      "owner ban-state DB lookup failed, denying (fail-closed)");
                                BanOutcome::DbError
                            }
                        };
                    }
                }

                let denial: Option<(&str, &str)> = match outcome {
                    BanOutcome::Clear => None,
                    BanOutcome::Banned => {
                        Some(("banned", "blocked: you are banned from this community"))
                    }
                    BanOutcome::DbError => Some((
                        "ban_check_error",
                        "error: internal error checking restriction state",
                    )),
                };

                if let Some((metric_reason, deny_reason)) = denial {
                    warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), reason = deny_reason, "principal denied at ban seam");
                    metrics::counter!("buzz_auth_failures_total", "reason" => metric_reason)
                        .increment(1);
                    *conn.auth_state.write().await = AuthState::Failed;
                    // Decision 4: banned ⇒ OK false + immediate WebSocket close.
                    // Route the reason frame on the control channel (not `send`,
                    // which uses the data channel and would race the cancel), so
                    // the send loop drains it ahead of the Close it emits on
                    // cancel. Then cancel to close the socket immediately.
                    let _ = conn.ctrl_tx.try_send(WsMessage::Text(
                        RelayMessage::ok(&event_id_hex, false, deny_reason).into(),
                    ));
                    conn.cancel.cancel();
                    return;
                }
            }

            // Pubkey allowlist gate — only for pubkey-only auth.
            if state.config.pubkey_allowlist_enabled
                && auth_ctx.auth_method == buzz_auth::AuthMethod::Nip42
            {
                let allowed = match state
                    .db
                    .is_pubkey_allowed(conn.tenant.community(), pubkey.as_bytes())
                    .await
                {
                    Ok(v) => v,
                    Err(e) => {
                        warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), error = %e,
                              "allowlist DB lookup failed, denying (fail-closed)");
                        false
                    }
                };
                if !allowed {
                    warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), "pubkey not in allowlist");
                    metrics::counter!("buzz_auth_failures_total", "reason" => "allowlist_denied")
                        .increment(1);
                    *conn.auth_state.write().await = AuthState::Failed;
                    conn.send(RelayMessage::ok(
                        &event_id_hex,
                        false,
                        "auth-required: verification failed",
                    ));
                    return;
                }
            }

            // Relay membership gate — uses the shared helper with NIP-OA fallback.
            let nip_oa_owner = match crate::api::relay_members::enforce_relay_membership(
                &state,
                conn.tenant.community(),
                pubkey.as_bytes(),
                auth_tag_json.as_deref(),
                Some(signed_auth_created_at),
            )
            .await
            {
                Ok(owner) => owner,
                Err(e) => {
                    warn!(conn_id = %conn_id, pubkey = %pubkey.to_hex(), error = ?e, "not a relay member");
                    metrics::counter!("buzz_auth_failures_total", "reason" => "not_relay_member")
                        .increment(1);
                    *conn.auth_state.write().await = AuthState::Failed;
                    conn.send(RelayMessage::ok(
                        &event_id_hex,
                        false,
                        "restricted: not a relay member",
                    ));
                    return;
                }
            };

            // Classify the admission. `enforce_relay_membership` returns
            // `Some(owner)` only when the login was admitted *through* that
            // owner (closed relay, agent not a member). On an open relay the
            // login stands on its own and the credential's owner is extracted
            // opportunistically for agent→owner backfill (observer frame auth);
            // NIP-OA is cryptographically self-proving, so no feature flag.
            let admission = match nip_oa_owner {
                Some(owner) => LoginAdmission::ViaOwner(owner),
                None if !state.config.require_relay_membership && auth_tag_json.is_some() => {
                    match crate::api::relay_members::extract_nip_oa_owner(
                        pubkey.as_bytes(),
                        auth_tag_json.as_deref(),
                        Some(signed_auth_created_at),
                    ) {
                        Some(owner) => LoginAdmission::OpenRelayCredential(owner),
                        None => LoginAdmission::Direct,
                    }
                }
                None => LoginAdmission::Direct,
            };

            // Put the first-write-wins relationship on record, then apply the
            // fail-closed table: a login admitted through its owner is never
            // authenticated without that owner (NIP-AA step 6, finding 92).
            let outcome = match admission.owner() {
                Some(owner) => Some(
                    crate::api::relay_members::materialize_nip_oa_owner_outcome(
                        &state,
                        &conn.tenant,
                        &pubkey,
                        &owner,
                    )
                    .await,
                ),
                None => None,
            };
            match decide_owner_retention(&admission, outcome) {
                OwnerRetention::Authenticate { owner } => {
                    if let (Some(named), None) = (admission.owner(), owner) {
                        warn!(
                            conn_id = %conn_id,
                            agent = %pubkey.to_hex(),
                            nip_oa_owner = %named.to_hex(),
                            outcome = ?outcome,
                            "open relay: NIP-OA owner not recorded; authenticating without owner context"
                        );
                    }
                    auth_ctx.agent_owner_pubkey = owner;
                }
                OwnerRetention::Refuse { reason, message } => {
                    warn!(
                        conn_id = %conn_id,
                        agent = %pubkey.to_hex(),
                        nip_oa_owner = ?admission.owner().map(|o| o.to_hex()),
                        outcome = ?outcome,
                        reason,
                        "virtual login refused: owner relationship not retained"
                    );
                    metrics::counter!("buzz_auth_failures_total", "reason" => reason).increment(1);
                    *conn.auth_state.write().await = AuthState::Failed;
                    conn.send(RelayMessage::ok(&event_id_hex, false, message));
                    return;
                }
            }

            info!(
                conn_id = %conn_id,
                pubkey = %pubkey.to_hex(),
                nip_oa_owner = ?auth_ctx.agent_owner_pubkey.map(|o| o.to_hex()),
                "NIP-42 auth successful"
            );
            // Record the key first so the count below includes this socket,
            // then refuse the one over the cap. Two sockets racing here may
            // both count nine and both be refused; that errs on the side
            // the cap exists for (the per-connection budgets multiply by
            // socket count, and this is what bounds them).
            let pubkey_bytes = pubkey.to_bytes().to_vec();
            state
                .conn_manager
                .set_authenticated_pubkey(conn_id, pubkey_bytes.clone());
            let live = state
                .conn_manager
                .connection_ids_for_pubkey_in_community(conn.tenant.community(), &pubkey_bytes)
                .len();
            let max = state
                .auth
                .config()
                .rate_limits
                .max_ws_connections_per_pubkey;
            if crate::admission::connection_cap_exceeded(live, max) {
                warn!(
                    conn_id = %conn_id,
                    pubkey = %pubkey.to_hex(),
                    live,
                    max,
                    "refusing socket: too many connections for this key"
                );
                metrics::counter!(
                    "buzz_admission_rejections_total",
                    "transport" => "websocket",
                    "reason" => "connections",
                    "budget" => "connections",
                    "scope" => "key"
                )
                .increment(1);
                *conn.auth_state.write().await = AuthState::Failed;
                // Control channel: drained ahead of queued data and the
                // cancel branch, so the client learns why before the close.
                if conn
                    .ctrl_tx
                    .try_send(WsMessage::Text(
                        RelayMessage::notice(crate::admission::TOO_MANY_CONNECTIONS).into(),
                    ))
                    .is_err()
                {
                    tracing::warn!(
                        conn_id = %conn.conn_id,
                        "connection cap NOTICE could not be queued; closing without it"
                    );
                }
                conn.cancel.cancel();
                return;
            }
            *conn.auth_state.write().await = AuthState::Authenticated(auth_ctx);
            conn.send(RelayMessage::ok(&event_id_hex, true, ""));
        }
        Err(e) => {
            warn!(conn_id = %conn_id, error = %e, "NIP-42 auth failed");
            metrics::counter!("buzz_auth_failures_total", "reason" => "nip42_invalid").increment(1);
            *conn.auth_state.write().await = AuthState::Failed;
            conn.send(RelayMessage::ok(
                &event_id_hex,
                false,
                "auth-required: verification failed",
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::extract_auth_tag_json;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    /// Build a signed NIP-98 (kind 27235) event carrying the given tags. The
    /// `auth` tag lives inside the signed event exactly as the git and
    /// WebSocket auth paths receive it.
    fn signed_event_with_tags(tags: Vec<Tag>) -> nostr::Event {
        EventBuilder::new(Kind::HttpAuth, "")
            .tags(tags)
            .sign_with_keys(&Keys::generate())
            .expect("sign auth event")
    }

    /// A single `auth` tag is extracted verbatim as its JSON-array string —
    /// this is the exact value fed to `verify_auth_tag` on the git path.
    #[test]
    fn single_auth_tag_extracted_verbatim() {
        let owner = Keys::generate().public_key().to_hex();
        let sig = "00".repeat(64);
        let event = signed_event_with_tags(vec![
            Tag::parse(["u", "https://relay/git/x/y"]).unwrap(),
            Tag::parse(["auth", owner.as_str(), "", sig.as_str()]).unwrap(),
        ]);

        let extracted = extract_auth_tag_json(&event).expect("auth tag present");
        let expected = serde_json::to_string(&["auth", owner.as_str(), "", sig.as_str()]).unwrap();
        assert_eq!(extracted, expected);
    }

    /// No `auth` tag → `None` (the direct-member path, tag absent).
    #[test]
    fn no_auth_tag_returns_none() {
        let event =
            signed_event_with_tags(vec![Tag::parse(["u", "https://relay/git/x/y"]).unwrap()]);
        assert_eq!(extract_auth_tag_json(&event), None);
    }

    /// More than one `auth` tag → `None`. Per NIP-OA, an ambiguous set of
    /// attestations is treated as no valid attestation (fail-closed), so a
    /// second forged tag cannot smuggle an alternate delegation past the gate.
    #[test]
    fn duplicate_auth_tags_return_none() {
        let a = Keys::generate().public_key().to_hex();
        let b = Keys::generate().public_key().to_hex();
        let sig = "00".repeat(64);
        let event = signed_event_with_tags(vec![
            Tag::parse(["auth", a.as_str(), "", sig.as_str()]).unwrap(),
            Tag::parse(["auth", b.as_str(), "", sig.as_str()]).unwrap(),
        ]);
        assert_eq!(extract_auth_tag_json(&event), None);
    }

    /// The fail-closed table (finding 92). Every branch of
    /// `decide_owner_retention` is enumerated here so the refuter constraint —
    /// no `Authenticated` with `agent_owner_pubkey == None` for a login
    /// admitted through NIP-OA — is a checked table, not a reading of the
    /// handler.
    mod owner_retention {
        use super::super::{
            decide_owner_retention, LoginAdmission, OwnerRetention, NIP_OA_OWNER_CONFLICT_MESSAGE,
            NIP_OA_OWNER_CONFLICT_REASON, NIP_OA_OWNER_UNRECORDED_MESSAGE,
            NIP_OA_OWNER_UNRECORDED_REASON,
        };
        use crate::api::relay_members::OwnerMaterialization;
        use nostr::Keys;

        const ALL_OUTCOMES: [Option<OwnerMaterialization>; 4] = [
            Some(OwnerMaterialization::Recorded),
            Some(OwnerMaterialization::Conflict),
            Some(OwnerMaterialization::Unavailable),
            None,
        ];

        #[test]
        fn via_owner_recorded_retains_owner() {
            let owner = Keys::generate().public_key();
            assert_eq!(
                decide_owner_retention(
                    &LoginAdmission::ViaOwner(owner),
                    Some(OwnerMaterialization::Recorded)
                ),
                OwnerRetention::Authenticate { owner: Some(owner) }
            );
        }

        #[test]
        fn via_owner_unavailable_refuses_with_the_unrecorded_sentence() {
            let owner = Keys::generate().public_key();
            assert_eq!(
                decide_owner_retention(
                    &LoginAdmission::ViaOwner(owner),
                    Some(OwnerMaterialization::Unavailable)
                ),
                OwnerRetention::Refuse {
                    reason: NIP_OA_OWNER_UNRECORDED_REASON,
                    message: NIP_OA_OWNER_UNRECORDED_MESSAGE,
                }
            );
            assert_eq!(NIP_OA_OWNER_UNRECORDED_REASON, "nip_oa_owner_unrecorded");
            assert!(
                NIP_OA_OWNER_UNRECORDED_MESSAGE
                    .ends_with("owner relationship could not be recorded; try again"),
                "the brief's sentence is the one on the wire"
            );
        }

        /// The caller never even attempted to record the relationship: still
        /// a refusal. Forgetting the materialization step cannot open a path.
        #[test]
        fn via_owner_with_no_attempt_refuses() {
            let owner = Keys::generate().public_key();
            assert_eq!(
                decide_owner_retention(&LoginAdmission::ViaOwner(owner), None),
                OwnerRetention::Refuse {
                    reason: NIP_OA_OWNER_UNRECORDED_REASON,
                    message: NIP_OA_OWNER_UNRECORDED_MESSAGE,
                }
            );
        }

        #[test]
        fn via_owner_conflict_refuses_with_its_own_sentence() {
            let owner = Keys::generate().public_key();
            let decision = decide_owner_retention(
                &LoginAdmission::ViaOwner(owner),
                Some(OwnerMaterialization::Conflict),
            );
            assert_eq!(
                decision,
                OwnerRetention::Refuse {
                    reason: NIP_OA_OWNER_CONFLICT_REASON,
                    message: NIP_OA_OWNER_CONFLICT_MESSAGE,
                }
            );
            assert_ne!(
                NIP_OA_OWNER_CONFLICT_MESSAGE, NIP_OA_OWNER_UNRECORDED_MESSAGE,
                "a conflict is not transient; it must not say 'try again'"
            );
        }

        /// The refuter constraint, exhaustively: for a NIP-OA-admitted login
        /// there is no outcome that yields an ownerless authentication.
        #[test]
        fn via_owner_never_authenticates_ownerless() {
            let owner = Keys::generate().public_key();
            for outcome in ALL_OUTCOMES {
                match decide_owner_retention(&LoginAdmission::ViaOwner(owner), outcome) {
                    OwnerRetention::Authenticate { owner: None } => {
                        panic!("ownerless authentication for ViaOwner with {outcome:?}")
                    }
                    OwnerRetention::Authenticate { owner: Some(o) } => {
                        assert_eq!(o, owner);
                        assert_eq!(outcome, Some(OwnerMaterialization::Recorded));
                    }
                    OwnerRetention::Refuse { .. } => {
                        assert_ne!(outcome, Some(OwnerMaterialization::Recorded));
                    }
                }
            }
        }

        /// A direct relay member is unaffected by anything the owner store does.
        #[test]
        fn direct_member_is_unaffected() {
            for outcome in ALL_OUTCOMES {
                assert_eq!(
                    decide_owner_retention(&LoginAdmission::Direct, outcome),
                    OwnerRetention::Authenticate { owner: None },
                    "direct member with {outcome:?}"
                );
            }
        }

        /// On an open relay the credential is opportunistic: the owner is
        /// retained when recorded and dropped (disclosed by the handler's warn)
        /// otherwise — never a refusal, since omitting the tag would admit the
        /// same login anyway.
        #[test]
        fn open_relay_credential_is_best_effort() {
            let owner = Keys::generate().public_key();
            for outcome in ALL_OUTCOMES {
                let expected = if outcome == Some(OwnerMaterialization::Recorded) {
                    Some(owner)
                } else {
                    None
                };
                assert_eq!(
                    decide_owner_retention(&LoginAdmission::OpenRelayCredential(owner), outcome),
                    OwnerRetention::Authenticate { owner: expected },
                    "open relay with {outcome:?}"
                );
            }
        }

        #[test]
        fn admission_names_its_owner() {
            let owner = Keys::generate().public_key();
            assert_eq!(LoginAdmission::Direct.owner(), None);
            assert_eq!(LoginAdmission::ViaOwner(owner).owner(), Some(owner));
            assert_eq!(
                LoginAdmission::OpenRelayCredential(owner).owner(),
                Some(owner)
            );
        }
    }

    /// `handle_auth` end to end against a real Postgres on a closed relay.
    /// These prove the wiring: the positive virtual login retains its owner,
    /// the conflict refusal reaches the wire, and a direct member is untouched.
    /// The `Unavailable` refusal cannot be injected against a live store
    /// without corrupting it; it is covered by the pure table above and by
    /// `relay_members::tests::unreachable_store_is_unavailable_not_recorded`.
    mod live {
        use std::collections::HashMap;
        use std::sync::atomic::AtomicU8;
        use std::sync::Arc;

        use axum::extract::ws::Message as WsMessage;
        use buzz_core::{tenant::CommunityId, TenantContext};
        use buzz_sdk::nip_oa::compute_auth_tag;
        use nostr::{EventBuilder, Keys, RelayUrl, Tag};
        use tokio::sync::{mpsc, Mutex, RwLock};
        use tokio_util::sync::CancellationToken;

        use super::super::{
            handle_auth, NIP_OA_OWNER_CONFLICT_MESSAGE, NIP_OA_OWNER_UNRECORDED_MESSAGE,
        };
        use crate::connection::{AuthState, ConnectionState};
        use crate::state::AppState;

        const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

        /// A closed relay with NIP-OA delegation on, against the test Postgres.
        /// Redis is deliberately unreachable: the AUTH path never touches it.
        async fn closed_relay_state() -> Arc<AppState> {
            let mut config = crate::config::Config::from_env().expect("default config loads");
            config.require_relay_membership = true;
            config.allow_nip_oa_auth = true;
            config.pubkey_allowlist_enabled = false;
            config.redis_url = "redis://127.0.0.1:1".to_string();
            config.database_url = std::env::var("BUZZ_TEST_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .unwrap_or_else(|_| TEST_DB_URL.to_string());
            let pool = sqlx::PgPool::connect(&config.database_url)
                .await
                .expect("connect test DB");
            let db = buzz_db::Db::from_pool(pool.clone());
            let redis_pool = deadpool_redis::Config::from_url(&config.redis_url)
                .create_pool(Some(deadpool_redis::Runtime::Tokio1))
                .expect("redis pool");
            let pubsub = Arc::new(
                buzz_pubsub::PubSubManager::new(&config.redis_url, redis_pool.clone())
                    .await
                    .expect("pubsub manager"),
            );
            let audit = buzz_audit::AuditService::new(pool.clone());
            let auth = buzz_auth::AuthService::new(config.auth.clone());
            let search = buzz_search::SearchService::new(pool.clone());
            let workflow_engine = Arc::new(buzz_workflow::WorkflowEngine::new(
                db.clone(),
                buzz_workflow::WorkflowConfig::default(),
            ));
            let media_storage =
                buzz_media::MediaStorage::new(&config.media).expect("media storage");
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

        async fn fresh_community(state: &AppState) -> (CommunityId, String) {
            let host = format!("adm-c-{}.example", uuid::Uuid::new_v4().simple());
            let community = state
                .db
                .ensure_configured_community(&host)
                .await
                .expect("community")
                .id;
            (community, host)
        }

        fn pending_connection(
            community: CommunityId,
            host: &str,
            challenge: &str,
        ) -> (Arc<ConnectionState>, mpsc::Receiver<WsMessage>) {
            let (send_tx, send_rx) = mpsc::channel(8);
            let (ctrl_tx, _ctrl_rx) = mpsc::channel(8);
            let conn = Arc::new(ConnectionState {
                conn_id: uuid::Uuid::new_v4(),
                tenant: TenantContext::resolved(community, host),
                remote_addr: "127.0.0.1:1234".parse().expect("socket addr"),
                auth_state: RwLock::new(AuthState::Pending {
                    challenge: challenge.to_string(),
                }),
                subscriptions: Arc::new(Mutex::new(HashMap::new())),
                send_tx,
                ctrl_tx,
                cancel: CancellationToken::new(),
                backpressure_count: Arc::new(AtomicU8::new(0)),
                grace_limit: 3,
                budgets: Default::default(),
            });
            (conn, send_rx)
        }

        /// A signed kind:22242 AUTH event for `host`, optionally carrying a
        /// NIP-OA `auth` tag issued by `owner` for the signing agent.
        fn auth_event(
            agent: &Keys,
            challenge: &str,
            host: &str,
            owner: Option<&Keys>,
        ) -> nostr::Event {
            let url = RelayUrl::parse(&format!("ws://{host}")).expect("relay url");
            let mut tags = Vec::new();
            if let Some(owner) = owner {
                let tag_json =
                    compute_auth_tag(owner, &agent.public_key(), "").expect("compute auth tag");
                let parts: Vec<String> = serde_json::from_str(&tag_json).expect("auth tag array");
                tags.push(Tag::parse(parts).expect("auth tag"));
            }
            EventBuilder::auth(challenge, url)
                .tags(tags)
                .sign_with_keys(agent)
                .expect("sign auth event")
        }

        /// The `["OK", id, accepted, message]` frame the handler sent.
        fn ok_frame(rx: &mut mpsc::Receiver<WsMessage>) -> (bool, String) {
            let WsMessage::Text(text) = rx.try_recv().expect("an OK frame was sent") else {
                panic!("expected a text frame");
            };
            let frame: serde_json::Value = serde_json::from_str(&text).expect("relay frame JSON");
            assert_eq!(frame[0], "OK");
            (
                frame[2].as_bool().expect("accepted flag"),
                frame[3].as_str().expect("message").to_string(),
            )
        }

        #[tokio::test]
        #[ignore = "requires Postgres"]
        async fn virtual_login_retains_its_owner() {
            let state = closed_relay_state().await;
            let (community, host) = fresh_community(&state).await;
            let owner = Keys::generate();
            let agent = Keys::generate();
            state
                .db
                .add_relay_member(community, &owner.public_key().to_hex(), "member", None)
                .await
                .expect("owner joins");

            let challenge = uuid::Uuid::new_v4().to_string();
            let (conn, mut rx) = pending_connection(community, &host, &challenge);
            handle_auth(
                auth_event(&agent, &challenge, &host, Some(&owner)),
                Arc::clone(&conn),
                Arc::clone(&state),
            )
            .await;

            let (accepted, message) = ok_frame(&mut rx);
            assert!(accepted, "virtual login admitted: {message}");
            let auth_state = conn.auth_state.read().await;
            match &*auth_state {
                AuthState::Authenticated(ctx) => {
                    assert_eq!(ctx.pubkey, agent.public_key());
                    assert_eq!(
                        ctx.agent_owner_pubkey,
                        Some(owner.public_key()),
                        "NIP-AA step 6: the admitting owner is retained"
                    );
                }
                other => panic!("expected Authenticated, got {other:?}"),
            }
            assert!(
                state
                    .db
                    .is_agent_owner(
                        community,
                        &agent.public_key().to_bytes(),
                        &owner.public_key().to_bytes()
                    )
                    .await
                    .expect("read mapping"),
                "the relationship is on record"
            );
        }

        /// The agent was first minted to owner A; owner B (also a member)
        /// issues it a credential. First-write-wins cannot record A→B, so the
        /// login is refused with the conflict sentence — not authenticated
        /// ownerless, which is what the relay did before finding 92.
        #[tokio::test]
        #[ignore = "requires Postgres"]
        async fn virtual_login_refused_when_owner_conflicts() {
            let state = closed_relay_state().await;
            let (community, host) = fresh_community(&state).await;
            let first_owner = Keys::generate();
            let second_owner = Keys::generate();
            let agent = Keys::generate();
            for pk in [first_owner.public_key(), agent.public_key()] {
                state
                    .db
                    .ensure_user(community, &pk.to_bytes())
                    .await
                    .expect("ensure user");
            }
            assert!(state
                .db
                .set_agent_owner(
                    community,
                    &agent.public_key().to_bytes(),
                    &first_owner.public_key().to_bytes()
                )
                .await
                .expect("first mint"));
            state
                .db
                .add_relay_member(
                    community,
                    &second_owner.public_key().to_hex(),
                    "member",
                    None,
                )
                .await
                .expect("second owner joins");

            let challenge = uuid::Uuid::new_v4().to_string();
            let (conn, mut rx) = pending_connection(community, &host, &challenge);
            handle_auth(
                auth_event(&agent, &challenge, &host, Some(&second_owner)),
                Arc::clone(&conn),
                Arc::clone(&state),
            )
            .await;

            let (accepted, message) = ok_frame(&mut rx);
            assert!(!accepted, "conflicting owner must be refused");
            assert_eq!(message, NIP_OA_OWNER_CONFLICT_MESSAGE);
            assert_ne!(message, NIP_OA_OWNER_UNRECORDED_MESSAGE);
            assert!(
                matches!(&*conn.auth_state.read().await, AuthState::Failed),
                "no Authenticated state for a refused virtual login"
            );
        }

        /// A direct relay member presenting no credential is untouched by the
        /// owner-retention gate.
        #[tokio::test]
        #[ignore = "requires Postgres"]
        async fn direct_member_is_unaffected() {
            let state = closed_relay_state().await;
            let (community, host) = fresh_community(&state).await;
            let member = Keys::generate();
            state
                .db
                .add_relay_member(community, &member.public_key().to_hex(), "member", None)
                .await
                .expect("member joins");

            let challenge = uuid::Uuid::new_v4().to_string();
            let (conn, mut rx) = pending_connection(community, &host, &challenge);
            handle_auth(
                auth_event(&member, &challenge, &host, None),
                Arc::clone(&conn),
                Arc::clone(&state),
            )
            .await;

            let (accepted, message) = ok_frame(&mut rx);
            assert!(accepted, "direct member admitted: {message}");
            let auth_state = conn.auth_state.read().await;
            match &*auth_state {
                AuthState::Authenticated(ctx) => {
                    assert_eq!(ctx.pubkey, member.public_key());
                    assert_eq!(ctx.agent_owner_pubkey, None);
                }
                other => panic!("expected Authenticated, got {other:?}"),
            }
        }
    }
}
