//! Pulse's display name goes through the shared resolver (SV-31): a stranger's
//! 44229 is no longer the session's name, and a provider's 44252 shows as a
//! generated title, labelled as one.

use serde_json::{json, Value};

use super::super::{fold_pulse_digest, PulseDigestSession};
use super::session_name_record_from_json;
use crate::coding_session_title::{
    resolve_session_display_name, SessionDisplayNameOrigin, SessionDisplayNameScope,
};

const PROJECT: &str =
    "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo";
const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const TARGET: &str = "coding-session/v1|3:acp6:inst-16:sess-11:1";
const CREATOR: &str = "a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1a1";
const FOUNDER: &str = "f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0f0";
const PROVIDER: &str = "d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4d4";
const STRANGER: &str = "b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2b2";
const GENESIS_ID: &str = "00000000000000000000000000000000000000000000000000000000000000a0";
const CREATE_ID: &str = "0000000000000000000000000000000000000000000000000000000000000101";
const NOW: i64 = 1_785_600_000;

fn id(n: u32) -> String {
    format!("{n:064x}")
}

/// One accepted generation-1 create, its receipt and metadata — the
/// `idle-hours-old-with-live-authorized-lease` pulse vector — with the create
/// naming a genesis by event id when `genesis` is set.
fn umbrella(genesis: bool) -> Vec<Value> {
    let genesis_ref = if genesis {
        format!(r#","genesisRef":"{GENESIS_ID}""#)
    } else {
        String::new()
    };
    let mut events = vec![
        json!({
            "id": CREATE_ID, "pubkey": CREATOR, "created_at": 1_785_589_900, "kind": 44221,
            "tags": [["h", CHANNEL], ["csl-v", "csl1-1"], ["csl-command", "create-live-1"]],
            "content": format!(
                r#"{{"schema":"buzz-coding-session-lifecycle-command/v1","commandId":"create-live-1","action":{{"type":"session.create","projectRef":"{PROJECT}","repoRef":null,"sessionRef":"{SESSION_REF}"{genesis_ref},"providerInstanceRef":"provider-1","providerAuthorityPubkey":"{PROVIDER}","model":null,"title":null,"initialTurn":null}}}}"#
            ),
        }),
        json!({
            "id": id(0x102), "pubkey": PROVIDER, "created_at": 1_785_589_901, "kind": 44224,
            "tags": [["h", CHANNEL], ["cslr-v", "cslr1-1"], ["csl-command", "create-live-1"],
                     ["csl-key", "coding-session-lifecycle-receipt/v1|13:create-live-1"]],
            "content": r#"{"schema":"buzz-coding-session-lifecycle-receipt/v1","commandId":"create-live-1","status":"created","session":{"driver":"acp","instanceId":"inst-1","sessionId":"sess-1","generation":1},"error":null}"#,
        }),
        json!({
            "id": id(0x103), "pubkey": PROVIDER, "created_at": 1_785_590_000, "kind": 44223,
            "tags": [["h", CHANNEL], ["cs-target", TARGET]],
            "content": format!(
                r#"{{"schema":"buzz-coding-session-metadata/v1","session":{{"driver":"acp","instanceId":"inst-1","sessionId":"sess-1","generation":1}},"projectRef":"{PROJECT}","repoRef":null,"title":null,"agentRef":null,"provider":"provider-1","runtime":"claude","model":null,"status":"idle","branch":null,"capabilities":{{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true}},"sessionRef":"{SESSION_REF}","observedCommit":null,"dirty":null,"relayReachable":null,"verifiedAt":null}}"#
            ),
        }),
    ];
    if genesis {
        events.push(json!({
            "id": GENESIS_ID, "pubkey": FOUNDER, "created_at": 1_785_589_800, "kind": 44226,
            "tags": [["h", CHANNEL], ["csg-v", "csg1-1"], ["csg-session", SESSION_REF]],
            "content": format!(r#"{{"sessionRef":"{SESSION_REF}","v":1}}"#),
        }));
    }
    events
}

fn name(event_id: u32, signer: &str, created_at: i64, content: &str) -> Value {
    json!({
        "id": id(event_id), "pubkey": signer, "created_at": created_at, "kind": 44229,
        "tags": [["h", CHANNEL], ["d", SESSION_REF], ["csnm-v", "csnm1-1"]],
        "content": content,
    })
}

fn title(event_id: u32, signer: &str, created_at: i64, text: &str) -> Value {
    json!({
        "id": id(event_id), "pubkey": signer, "created_at": created_at, "kind": 44252,
        "tags": [["h", CHANNEL], ["d", SESSION_REF], ["cstl-v", "cstl1-1"], ["cs-target", TARGET]],
        "content": json!({
            "schema": "buzz-coding-session-title/v1",
            "title": text,
            "model": "haiku",
            "basis": "first-message",
            "sourceCommand": null,
            "createEventId": CREATE_ID,
        }).to_string(),
    })
}

fn fold(events: Vec<Value>) -> PulseDigestSession {
    let digest = fold_pulse_digest(PROJECT, NOW, Vec::new(), &events);
    assert_eq!(digest.sessions.len(), 1, "one umbrella: {digest:?}");
    digest
        .sessions
        .into_iter()
        .next()
        .unwrap_or_else(|| unreachable!())
}

#[test]
fn a_foreign_44229_no_longer_wins_in_pulse_fold() {
    let mut events = umbrella(true);
    events.push(name(0x201, FOUNDER, 1_785_590_100, "Auth rework"));
    // Newer, and from a key that is not the founder: before SV-31 this won.
    events.push(name(0x202, STRANGER, 1_785_590_200, "Hijacked"));
    let session = fold(events);
    assert_eq!(session.name.as_deref(), Some("Auth rework"));
    assert_eq!(session.name_origin.as_deref(), Some("person"));
    assert_eq!(session.name_model, None);
    assert_eq!(session.name_signer, None);
    assert!(session.source_event_ids.contains(&id(0x201)));
    assert!(!session.source_event_ids.contains(&id(0x202)));
}

#[test]
fn a_foreign_44229_alone_names_nothing() {
    let mut events = umbrella(true);
    events.push(name(0x202, STRANGER, 1_785_590_200, "Hijacked"));
    let session = fold(events);
    assert_eq!(session.name, None);
    assert_eq!(session.name_origin, None);
    let serialized = serde_json::to_value(&session).unwrap_or_default();
    assert!(serialized.get("nameOrigin").is_none(), "{serialized}");
}

/// The create's own signer is not the founder: only the genesis it names is.
#[test]
fn without_a_genesis_no_44229_is_a_persons_name() {
    let mut events = umbrella(false);
    events.push(name(0x201, CREATOR, 1_785_590_100, "Auth rework"));
    assert_eq!(fold(events).name, None);
}

#[test]
fn a_genesis_for_another_umbrella_proves_no_founder() {
    let mut events = umbrella(true);
    if let Some(genesis) = events.iter_mut().find(|event| event["kind"] == 44226) {
        genesis["content"] =
            json!(r#"{"sessionRef":"6b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10","v":1}"#);
    }
    events.push(name(0x201, FOUNDER, 1_785_590_100, "Auth rework"));
    assert_eq!(fold(events).name, None);
}

#[test]
fn a_provider_title_shows_as_generated_with_model_and_signer() {
    let mut events = umbrella(true);
    events.push(title(0x301, PROVIDER, 1_785_590_100, "Fix login redirect"));
    let session = fold(events);
    assert_eq!(session.name.as_deref(), Some("Fix login redirect"));
    assert_eq!(session.name_origin.as_deref(), Some("generated"));
    assert_eq!(session.name_model.as_deref(), Some("haiku"));
    assert_eq!(session.name_signer.as_deref(), Some(PROVIDER));
    assert!(session.source_event_ids.contains(&id(0x301)));
    // The session object's bytes are pinned by the shared pulse vectors, so
    // the origin rides beside it, never inside.
    let serialized = serde_json::to_value(&session).unwrap_or_default();
    assert_eq!(serialized["name"], "Fix login redirect");
    for key in ["nameOrigin", "nameModel", "nameSigner"] {
        assert!(serialized.get(key).is_none(), "{key} in {serialized}");
    }
}

#[test]
fn an_older_person_name_beats_a_newer_generated_title() {
    let mut events = umbrella(true);
    events.push(name(0x201, FOUNDER, 1_785_590_050, "Auth rework"));
    events.push(title(0x301, PROVIDER, 1_785_590_100, "Fix login redirect"));
    let session = fold(events);
    assert_eq!(session.name.as_deref(), Some("Auth rework"));
    assert_eq!(session.name_origin.as_deref(), Some("person"));
    assert!(!session.source_event_ids.contains(&id(0x301)));
}

#[test]
fn the_earliest_generated_title_wins() {
    let mut events = umbrella(true);
    events.push(title(0x302, PROVIDER, 1_785_590_200, "Later title"));
    events.push(title(0x301, PROVIDER, 1_785_590_100, "First title"));
    let session = fold(events);
    assert_eq!(session.name.as_deref(), Some("First title"));
    assert!(session.source_event_ids.contains(&id(0x301)));
    assert!(!session.source_event_ids.contains(&id(0x302)));
}

#[test]
fn a_title_from_a_signer_without_an_execution_is_ignored() {
    let mut events = umbrella(true);
    events.push(title(0x301, STRANGER, 1_785_590_100, "Fix login redirect"));
    let session = fold(events);
    assert_eq!(session.name, None);
    assert_eq!(session.name_origin, None);
}

/// The JSON-to-record path Pulse and `bee` share resolves every shared
/// conformance vector exactly as the resolver does, forwards and reversed.
#[test]
fn shared_vectors_pass_through_the_json_record_path() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../conformance/session-display-name/fixtures/vectors.json"
    ))
    .unwrap_or_default();
    let vectors = corpus["vectors"].as_array().cloned().unwrap_or_default();
    assert!(!vectors.is_empty());
    for vector in vectors {
        let scope: SessionDisplayNameScope = serde_json::from_value(vector["scope"].clone())
            .unwrap_or_else(|error| {
                panic!("scope of {} decodes: {error}", vector["name"]);
            });
        let mut records: Vec<_> = vector["events"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|event| {
                session_name_record_from_json(event)
                    .unwrap_or_else(|| panic!("event of {} converts", vector["name"]))
            })
            .collect();
        for _ in 0..2 {
            let resolved = resolve_session_display_name(&scope, &records);
            assert_eq!(
                serde_json::to_value(&resolved).unwrap_or_default(),
                vector["expected"],
                "vector {}",
                vector["name"]
            );
            let winner = super::winning_name_event_id(&scope, &records, &resolved);
            assert_eq!(
                winner.is_some(),
                resolved.origin != SessionDisplayNameOrigin::Fallback,
                "vector {}",
                vector["name"]
            );
            records.reverse();
        }
    }
}
