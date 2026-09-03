use super::*;

use crate::coding_session_lifecycle_command::MAX_LIFECYCLE_REFERENCE_BYTES;
use crate::coding_session_payload::MAX_METADATA_REFERENCE_BYTES;

#[test]
fn each_word_accepts_the_value_its_field_carries_today() {
    assert_eq!(
        ProviderInstanceAlias::from_wire("claude-primary")
            .expect("alias")
            .as_str(),
        "claude-primary"
    );
    assert_eq!(
        ProviderInstanceId::from_wire("1958c6c448e05eed")
            .expect("instance id")
            .as_str(),
        "1958c6c448e05eed"
    );
    assert_eq!(
        RuntimeWord::from_wire("claude").expect("runtime").as_str(),
        "claude"
    );
    assert_eq!(
        DriverSlug::from_wire("claude-agent-acp")
            .expect("driver")
            .as_str(),
        "claude-agent-acp"
    );
}

/// The newtype must never be stricter than the hand-written validator it
/// replaces, or a signed event that decoded yesterday stops decoding today.
#[test]
fn bounds_equal_the_field_bounds_they_replace() {
    assert_eq!(
        MAX_CODING_SESSION_REFERENCE_BYTES,
        MAX_LIFECYCLE_REFERENCE_BYTES
    );
    assert_eq!(
        MAX_CODING_SESSION_REFERENCE_BYTES,
        MAX_METADATA_REFERENCE_BYTES
    );
    assert_eq!(
        MAX_CODING_SESSION_TARGET_IDENTIFIER_BYTES,
        crate::coding_session_command::MAX_IDENTIFIER_BYTES
    );

    let at_limit = "a".repeat(MAX_CODING_SESSION_REFERENCE_BYTES);
    assert!(ProviderInstanceAlias::from_wire(at_limit.clone()).is_ok());
    assert!(ProviderInstanceAlias::from_wire(format!("{at_limit}a")).is_err());

    let target_limit = "b".repeat(MAX_CODING_SESSION_TARGET_IDENTIFIER_BYTES);
    assert!(DriverSlug::from_wire(target_limit.clone()).is_ok());
    assert!(DriverSlug::from_wire(format!("{target_limit}b")).is_err());
}

#[test]
fn only_blank_and_oversize_are_refused_and_the_refusal_names_the_field() {
    assert_eq!(
        ProviderInstanceAlias::from_wire("   ").unwrap_err(),
        "providerInstanceRef must not be blank"
    );
    assert!(RuntimeWord::from_wire("").unwrap_err().contains("blank"));
    assert!(
        DriverSlug::from_wire("a".repeat(MAX_CODING_SESSION_TARGET_IDENTIFIER_BYTES + 1))
            .unwrap_err()
            .contains("exceeds")
    );
}

/// **REVIEW-B1 F3.** A control character is a shape the wire has always
/// accepted, so the newtype holds it. The two cases the reviewer built are
/// asserted end to end: the signed event decodes **and** the typed accessor
/// answers `Ok`. Only `is_canonical()` reports the oddity.
///
/// If this ever goes red, B2's field flip has silently become a wire-breaking
/// change: two signed events that decode today would stop decoding.
#[test]
fn a_control_character_decodes_and_the_accessor_answers_ok() {
    use crate::coding_session_payload::decode_coding_session_metadata;

    // (1) A `session.create` whose providerInstanceRef carries a tab.
    let create = serde_json::json!({
        "schema": crate::coding_session_lifecycle_command::CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
        "commandId": "create-1",
        "action": {
            "type": "session.create",
            "projectRef": serde_json::Value::Null,
            "repoRef": serde_json::Value::Null,
            "providerInstanceRef": "claude\tprimary",
            "providerAuthorityPubkey": "ab".repeat(32),
            "model": serde_json::Value::Null,
            "title": serde_json::Value::Null,
            "initialTurn": serde_json::Value::Null,
        },
    })
    .to_string();
    let decoded =
        crate::coding_session_lifecycle_command::decode_coding_session_lifecycle_command(&create)
            .expect("a tab in providerInstanceRef decodes today and must keep decoding");
    let alias = decoded
        .provider_instance_alias()
        .expect("the accessor must not refuse what the wire accepts")
        .expect("the create names an alias");
    assert_eq!(alias.as_str(), "claude\tprimary");
    assert!(
        !alias.is_canonical(),
        "the odd shape is reported, not refused"
    );

    // (2) A kind 44223 whose `provider` carries a tab.
    let metadata = serde_json::json!({
        "schema": crate::coding_session_payload::METADATA_SCHEMA,
        "session": {
            "driver": "claude-agent-acp",
            "instanceId": "1958c6c448e05eed",
            "sessionId": "session-1",
            "generation": 1,
        },
        "projectRef": serde_json::Value::Null,
        "repoRef": serde_json::Value::Null,
        "title": serde_json::Value::Null,
        "agentRef": serde_json::Value::Null,
        "provider": "claude\tprimary",
        "runtime": "cla\tude",
        "model": serde_json::Value::Null,
        "status": "running",
        "branch": serde_json::Value::Null,
        "capabilities": serde_json::to_value(
            crate::coding_session_payload::Capabilities::v1_for_runtime("claude"),
        )
        .expect("capabilities"),
    })
    .to_string();
    let decoded = decode_coding_session_metadata(&metadata)
        .expect("a tab in metadata provider decodes today and must keep decoding");
    let alias = decoded
        .provider_alias()
        .expect("the accessor must not refuse what the wire accepts")
        .expect("metadata names a provider");
    assert_eq!(alias.as_str(), "claude\tprimary");
    let runtime = decoded
        .runtime_word()
        .expect("the accessor must not refuse what the wire accepts")
        .expect("metadata names a runtime");
    assert_eq!(runtime.as_str(), "cla\tude");
    assert!(!alias.is_canonical());
    assert!(!runtime.is_canonical());

    // The ordinary values are canonical, so the predicate discriminates.
    assert!(ProviderInstanceAlias::from_wire("claude-primary")
        .expect("alias")
        .is_canonical());
    assert!(!ProviderInstanceAlias::from_wire(" claude-primary")
        .expect("leading space is held")
        .is_canonical());
}

/// An operator who sets `BUZZ_CSP_INSTANCE_ID` publishes exactly that, and the
/// type holds it. The canonical shape is disclosed, never assumed.
#[test]
fn an_operator_named_instance_is_held_and_disclosed_as_not_a_pubkey_prefix() {
    let named = ProviderInstanceId::from_wire("workstation-a").expect("operator-named instance");
    assert!(!named.is_short_pubkey_prefix());
    assert!(!named.matches_pubkey("workstation-a-and-more"));

    let minted = ProviderInstanceId::from_wire("1958c6c448e05eed").expect("minted instance");
    assert!(minted.is_short_pubkey_prefix());
    assert!(
        minted.matches_pubkey("1958c6c448e05eed7f0f2a4b6c8d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f70819")
    );
    assert!(
        !minted.matches_pubkey("deadbeefdeadbeef00112233445566778899aabbccddeeff0011223344556677")
    );

    let uppercase = ProviderInstanceId::from_wire("1958C6C448E05EED").expect("uppercase is held");
    assert!(
        !uppercase.is_short_pubkey_prefix(),
        "canonical form is lowercase hex; an uppercase copy is disclosed as not canonical"
    );
}

/// **REVIEW-B1 F6.** `matches_pubkey` validates its argument. Handed another
/// instance id — a 16-character string that is not a pubkey — it used to
/// answer an unqualified `true`: the item-102 failure mode inside the function
/// offered as its cure.
#[test]
fn matches_pubkey_refuses_an_argument_that_is_not_a_pubkey() {
    let minted = ProviderInstanceId::from_wire("1958c6c448e05eed").expect("minted instance");
    assert!(
        !minted.matches_pubkey("1958c6c448e05eed"),
        "another instance id is not a pubkey and must never answer true"
    );
    for not_a_pubkey in [
        "",
        "1958c6c448e05eed7f0f2a4b6c8d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f7081",
        "1958c6c448e05eed7f0f2a4b6c8d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f708190",
        "1958C6C448E05EED7F0F2A4B6C8D0E1F2A3B4C5D6E7F8091A2B3C4D5E6F70819",
        "1958c6c448e05eedzzzz2a4b6c8d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f70819",
    ] {
        assert!(
            !minted.matches_pubkey(not_a_pubkey),
            "{not_a_pubkey:?} is not a canonical pubkey"
        );
    }
    assert!(
        minted.matches_pubkey("1958c6c448e05eed7f0f2a4b6c8d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f70819"),
        "the one true answer still holds"
    );
}

/// Serde is transparent: the wire bytes of every existing kind are unchanged
/// when a field's Rust type becomes one of these words.
#[test]
fn serde_is_transparent_in_both_directions() {
    let alias = ProviderInstanceAlias::from_wire("claude-primary").expect("alias");
    assert_eq!(
        serde_json::to_string(&alias).expect("serialize"),
        "\"claude-primary\""
    );
    assert_eq!(
        serde_json::from_str::<ProviderInstanceAlias>("\"claude-primary\"").expect("deserialize"),
        alias
    );

    #[derive(serde::Serialize, serde::Deserialize)]
    struct Carrier {
        provider: Option<ProviderInstanceAlias>,
        runtime: Option<RuntimeWord>,
    }
    let carrier = Carrier {
        provider: Some(ProviderInstanceAlias::from_wire("claude-primary").expect("alias")),
        runtime: None,
    };
    assert_eq!(
        serde_json::to_string(&carrier).expect("serialize carrier"),
        r#"{"provider":"claude-primary","runtime":null}"#
    );
}

/// The accessors hand back the four words from the three wire structs that
/// carry them, so a consumer never has to touch the raw `String` to get the
/// type-level guarantee.
#[test]
fn the_accessors_type_the_reads_on_every_carrier() {
    use crate::coding_session_command::CodingSessionTarget;
    use crate::coding_session_lifecycle_command::{
        CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
        CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
    };
    use crate::coding_session_payload::{
        Capabilities, SessionMetadata, SessionStatus, METADATA_SCHEMA,
    };

    let target = CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "1958c6c448e05eed".into(),
        session_id: "session-1".into(),
        generation: 1,
    };
    assert_eq!(
        target.driver_slug().expect("driver"),
        DriverSlug::from_wire("claude-agent-acp").expect("driver")
    );
    let instance = target.provider_instance_id().expect("instance id");
    assert!(instance.is_short_pubkey_prefix());

    let metadata = SessionMetadata {
        schema: METADATA_SCHEMA.to_owned(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: None,
        agent_ref: None,
        role: None,
        provider: Some("claude-primary".try_into().expect("alias")),
        runtime: Some("claude".try_into().expect("runtime")),
        model: None,
        status: SessionStatus::Running,
        branch: None,
        capabilities: Capabilities::v1_for_runtime("claude"),
        session_ref: None,
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
    };
    let alias = metadata
        .provider_alias()
        .expect("alias decodes")
        .expect("an alias is published");
    assert_eq!(alias.as_str(), "claude-primary");
    assert_eq!(
        metadata
            .runtime_word()
            .expect("runtime decodes")
            .expect("a runtime is published")
            .as_str(),
        "claude"
    );

    // The exact ledger item 102 comparison. `alias` is a ProviderInstanceAlias
    // and `instance` a ProviderInstanceId, so `alias == instance` does not
    // compile — see the compile_fail doc test on ProviderInstanceAlias. The
    // question with a true answer is asked of the signer instead.
    assert!(
        instance.matches_pubkey("1958c6c448e05eed7f0f2a4b6c8d0e1f2a3b4c5d6e7f8091a2b3c4d5e6f70819")
    );

    let hire = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: "hire-1".to_owned(),
        action: CodingSessionLifecycleAction::SessionHire {
            session_ref: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10".to_owned(),
            genesis_ref: "12".repeat(32),
            role: "builder".to_owned(),
            provider_instance_ref: Some("claude-primary".try_into().expect("alias")),
            model: None,
            brief: "Rebase the lane and run the gate.".to_owned(),
            requested_by: None,
            routing: None,
        },
    };
    assert_eq!(
        hire.provider_instance_alias().expect("alias decodes"),
        Some(ProviderInstanceAlias::from_wire("claude-primary").expect("alias"))
    );
}

#[test]
fn display_as_ref_and_conversions_agree_on_one_string() {
    let driver = DriverSlug::from_wire("codex-acp").expect("driver");
    assert_eq!(driver.to_string(), "codex-acp");
    assert_eq!(driver.as_ref() as &str, "codex-acp");
    assert_eq!(
        DriverSlug::try_from("codex-acp").expect("try_from &str"),
        driver
    );
    assert_eq!(String::from(driver.clone()), "codex-acp");
    assert_eq!(driver.into_inner(), "codex-acp");
}
