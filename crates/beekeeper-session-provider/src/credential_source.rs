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
}
