//! Which credentials a runtime answered with, for the `MODEL_AUTH_FAILED`
//! remedy (HEADLESS_PLUGGABLE_SLICE.md § Step 2, auth failure receipt).
//!
//! The descriptor's `credentialSource` (`RuntimeDescriptor::credential_source`,
//! set by `bee host commission --credential-source`) is the operator's label
//! for the credentials file the host was given. The provider reads it from
//! the runtime descriptor the session runs under — the join point is
//! `Provider::credential_source_label` in `lib.rs` — and echoes it into the
//! session's 44223 metadata. With no label the runtime is assumed to use the
//! machine's ambient login, which is what the pre-existing "Claude's login
//! has expired on this computer" remedy was written for. There is no
//! process-wide override: one config entry, the descriptor, is the plug.

use beekeeper_sdk::coding_session::{CREDENTIAL_SOURCE_AMBIENT_LOGIN, MODEL_AUTH_FAILED};

/// The operator sentence for a recognized authentication failure, chosen by
/// where the credentials came from.
///
/// Ambient login keeps the original remedy (`claude auth login`). A labelled
/// credentials file cannot be fixed by logging in on this computer: the
/// sentence names the label, asks for a fresh file, and says to restart the
/// host so the provider re-reads it.
pub fn auth_failure_sentence(ambient_remedy: &'static str, source: Option<&str>) -> String {
    match source {
        None => ambient_remedy.to_owned(),
        Some(label) => format!(
            "The credentials this host was given for this runtime (source `{label}`) no longer \
             authenticate. Supply a fresh credentials file for `{label}` and run `bee host \
             restart`, then send the next turn."
        ),
    }
}

/// The operator sentence for a credential failure the adapter *declared*
/// with the runtime-neutral `data.reason` signal (`credential_expired`,
/// `credential_invalid`, `credential_missing`) rather than one recognised
/// from a particular CLI's wording.
///
/// With a descriptor label it is the same file-custody remedy as
/// [`auth_failure_sentence`]. Without one nothing here knows where this
/// runtime's credential lives — the Claude ambient-login remedy would be a
/// guess about some other vendor's CLI — so the sentence says the source was
/// not reported, in the words the desktop uses for the same absence.
pub fn declared_auth_failure_sentence(reason: &str, source: Option<&str>) -> String {
    match source {
        Some(label) => format!(
            "The runtime reported `{reason}` for the credentials this host was given for it \
             (source `{label}`). Supply a fresh credentials file for `{label}` and run `bee \
             host restart`, then send the next turn."
        ),
        None => format!(
            "The runtime reported `{reason}`: its credential was refused, and the credential \
             source is not reported, so Beekeeper cannot name which credential to renew. Renew \
             it where this runtime is configured, then send the next turn."
        ),
    }
}

/// Stamp a failed `result` item for a declared credential failure:
/// `MODEL_AUTH_FAILED`, the declared `credentialReason`, and
/// `credentialSource` only when the descriptor names one. An absent source is
/// left absent — never filled with `ambient-login`, which would claim to know
/// how a runtime this build has never seen authenticates.
pub fn stamp_declared_auth_failure(
    item: &mut serde_json::Value,
    reason: &str,
    source: Option<&str>,
) {
    if let Some(object) = item.as_object_mut() {
        object.insert("errorCode".into(), serde_json::json!(MODEL_AUTH_FAILED));
        object.insert("credentialReason".into(), serde_json::json!(reason));
        if let Some(label) = source {
            object.insert("credentialSource".into(), serde_json::json!(label));
        }
    }
}

/// The `credentialSource` value an auth-failure item or receipt carries.
pub fn credential_source_value(source: Option<&str>) -> &str {
    source.unwrap_or(CREDENTIAL_SOURCE_AMBIENT_LOGIN)
}

/// Stamp a failed `result` item with the auth error code and its source.
pub fn stamp_auth_failure(item: &mut serde_json::Value, source: Option<&str>) {
    if let Some(object) = item.as_object_mut() {
        object.insert("errorCode".into(), serde_json::json!(MODEL_AUTH_FAILED));
        object.insert(
            "credentialSource".into(),
            serde_json::json!(credential_source_value(source)),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_sentence_names_the_file_label_and_the_host_restart_only_with_file_custody() {
        let ambient = auth_failure_sentence("ambient remedy", None);
        assert_eq!(ambient, "ambient remedy");
        let file = auth_failure_sentence("ambient remedy", Some("acme-vault"));
        assert!(file.contains("`acme-vault`"), "{file}");
        assert!(file.contains("bee host restart"), "{file}");
        assert!(!file.contains("claude auth login"), "{file}");
        assert_eq!(credential_source_value(None), "ambient-login");
        assert_eq!(credential_source_value(Some("x")), "x");
    }

    #[test]
    fn stamping_adds_the_code_and_source_without_touching_the_rest() {
        let mut item = serde_json::json!({"kind":"result","result":"r"});
        stamp_auth_failure(&mut item, Some("acme-vault"));
        assert_eq!(item["errorCode"], "MODEL_AUTH_FAILED");
        assert_eq!(item["credentialSource"], "acme-vault");
        assert_eq!(item["result"], "r");
        let mut ambient = serde_json::json!({"kind":"result"});
        stamp_auth_failure(&mut ambient, None);
        assert_eq!(ambient["credentialSource"], "ambient-login");
    }

    fn failed(message: &str, reason: Option<&'static str>) -> crate::session::TurnOutcome {
        crate::session::TurnOutcome::Failed {
            message: message.to_owned(),
            agent_gone: false,
            cause: reason.map(crate::session::TurnFailureCause::Credential),
        }
    }

    fn result_of(outcome: &crate::session::TurnOutcome, source: Option<&str>) -> serde_json::Value {
        result_with(outcome, source, false)
    }

    fn result_with(
        outcome: &crate::session::TurnOutcome,
        source: Option<&str>,
        remote: bool,
    ) -> serde_json::Value {
        let identity = crate::ModelIdentity {
            requested: None,
            window_fallback: None,
        };
        crate::turn_result_with_credentials(outcome, 10, None, 0, identity, source, remote).0
    }

    /// A plugged runtime's declared reason is `MODEL_AUTH_FAILED` with the
    /// descriptor's label, though its words match no Claude phrase.
    #[test]
    fn a_declared_credential_reason_is_model_auth_failed_naming_the_label() {
        let raw = "Agent reported error (code -32000): t3 server: token rejected";
        assert!(
            crate::turn_failure_sentence(raw).is_none(),
            "no Claude phrase"
        );
        for reason in beekeeper_acp::acp::CREDENTIAL_FAILURE_REASONS {
            let item = result_of(&failed(raw, Some(reason)), Some("t3-vault"));
            assert_eq!(item["errorCode"], MODEL_AUTH_FAILED, "{item}");
            assert_eq!(item["credentialSource"], "t3-vault", "{item}");
            assert_eq!(item["credentialReason"], reason, "{item}");
            assert_eq!(item["detail"], raw, "{item}");
            let sentence = item["result"].as_str().expect("sentence");
            assert!(sentence.contains("`t3-vault`"), "{sentence}");
            assert!(sentence.contains(reason), "{sentence}");
            assert!(!sentence.contains("claude"), "{sentence}");
        }
    }

    /// With no label the source is disclosed as not reported: absent on the
    /// item, never guessed as `ambient-login`, and no Claude remedy.
    #[test]
    fn a_declared_reason_without_a_label_says_the_source_is_not_reported() {
        let item = result_of(&failed("expired", Some("credential_expired")), None);
        assert_eq!(item["errorCode"], MODEL_AUTH_FAILED, "{item}");
        assert!(item.get("credentialSource").is_none(), "{item}");
        let sentence = item["result"].as_str().expect("sentence");
        assert!(
            sentence.contains("credential source is not reported"),
            "{sentence}"
        );
        assert!(!sentence.contains("claude auth login"), "{sentence}");
    }

    /// The Claude phrase path is unchanged, and an undeclared, unrecognised
    /// failure stays verbatim with no code.
    #[test]
    fn claude_phrases_still_classify_and_other_failures_stay_verbatim() {
        let claude = result_of(
            &failed("Failed to authenticate: OAuth session expired", None),
            None,
        );
        assert_eq!(claude["errorCode"], MODEL_AUTH_FAILED, "{claude}");
        assert_eq!(claude["credentialSource"], "ambient-login", "{claude}");
        assert!(claude.get("credentialReason").is_none(), "{claude}");
        let other = result_of(&failed("tool exploded", None), Some("t3-vault"));
        assert!(other.get("errorCode").is_none(), "{other}");
        assert_eq!(other["result"], "tool exploded", "{other}");
    }

    /// R7: a cancel the adapter did not confirm is an error result with
    /// `CANCEL_NOT_CONFIRMED`, the adapter's words as `detail`, and no
    /// credential stamp even when those words mention authentication.
    #[test]
    fn a_cancel_not_confirmed_is_an_error_result_with_its_code() {
        let raw = "cancel could not be applied; not logged in to T3 thread T-1";
        let outcome = crate::session::TurnOutcome::Failed {
            message: raw.to_owned(),
            agent_gone: false,
            cause: Some(crate::session::TurnFailureCause::CancelNotConfirmed { shutdown: false }),
        };
        let item = result_of(&outcome, Some("t3-vault"));
        assert_eq!(item["subtype"], "error", "{item}");
        assert_eq!(item["errorCode"], "CANCEL_NOT_CONFIRMED", "{item}");
        assert_eq!(item["detail"], raw, "{item}");
        assert_eq!(item["result"], crate::CANCEL_NOT_CONFIRMED_SENTENCE);
        assert!(item.get("credentialSource").is_none(), "{item}");
    }

    /// S2: a stop the provider's shutdown asked for names no operator, and
    /// says the work may continue only when it runs outside the adapter.
    #[test]
    fn a_shutdown_stop_not_confirmed_says_what_happened_to_the_work() {
        let outcome = crate::session::TurnOutcome::Failed {
            message: "stop refused".to_owned(),
            agent_gone: false,
            cause: Some(crate::session::TurnFailureCause::CancelNotConfirmed { shutdown: true }),
        };
        let local = result_with(&outcome, None, false);
        assert_eq!(local["errorCode"], "CANCEL_NOT_CONFIRMED", "{local}");
        assert_eq!(local["result"], crate::SHUTDOWN_STOP_NOT_CONFIRMED_LOCAL);
        let remote = result_with(&outcome, None, true);
        assert_eq!(remote["result"], crate::SHUTDOWN_STOP_NOT_CONFIRMED_REMOTE);
        for item in [&local, &remote] {
            let text = item["result"].as_str().expect("sentence");
            assert!(
                !text.contains("operator") && !text.contains("cancel"),
                "{text}"
            );
        }
        assert!(!local["result"]
            .as_str()
            .expect("sentence")
            .contains("may still be running"));
    }
}
