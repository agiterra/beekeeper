//! `gates.verifierRequired`, as the CLI reads it off the wire.
//!
//! The relay reads (`fetch_context_with_verifier_gate`, `fetch_policy_records`)
//! need a live relay and are exercised by `crates/buzz-cli/TESTING.md`'s
//! runbook. The rule those reads feed — which published record wins, and what
//! its absence means — is pure, and is what these tests pin.

use super::*;

use buzz_core::coding_session_authority_transition::CodingSessionAuthorityTransitionType;
use buzz_core::coding_session_policy::CODING_SESSION_POLICY_SCHEMA;
use nostr::{EventBuilder, Keys, Kind, Tag};
use serde_json::Value;

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";

fn genesis() -> String {
    "ab".repeat(32)
}

/// One signed kind-44245 record carrying exactly the `gates` object given.
fn policy(author: &Keys, gates: Option<Value>) -> Event {
    let mut content = serde_json::json!({
        "schema": CODING_SESSION_POLICY_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
    });
    if let Some(gates) = gates {
        content["gates"] = gates;
    }
    EventBuilder::new(
        Kind::Custom(KIND_CODING_SESSION_POLICY as u16),
        content.to_string(),
    )
    .tags([
        Tag::parse(["h", CHANNEL]).expect("h tag"),
        Tag::parse(["d", SESSION]).expect("d tag"),
        Tag::parse(["csp-v", CODING_SESSION_POLICY_SCHEMA]).expect("version tag"),
        Tag::parse(["csp-genesis", genesis().as_str()]).expect("genesis tag"),
    ])
    .sign_with_keys(author)
    .expect("a test policy signs")
}

fn requires(records: &[Event], founder: &Keys, grants: &[CodingSessionPolicyGrant]) -> bool {
    policy_requires_a_verifier(
        records,
        SESSION,
        &genesis(),
        &founder.public_key().to_hex(),
        grants,
    )
}

#[test]
fn no_policy_at_all_requires_no_verifier() {
    let founder = Keys::generate();
    // Run 2's shape: the completion `f85635ad` folded with no 44245 record in
    // the session at all. The fold must behave exactly as it did that day.
    assert!(!requires(&[], &founder, &[]));
}

#[test]
fn a_policy_that_sets_no_gates_requires_no_verifier() {
    let founder = Keys::generate();
    assert!(!requires(&[policy(&founder, None)], &founder, &[]));
    assert!(!requires(
        &[policy(
            &founder,
            Some(serde_json::json!({"redFirst": true}))
        )],
        &founder,
        &[]
    ));
    // Run 3's shape: a policy with gates that does not name the flag.
    assert!(!requires(
        &[policy(
            &founder,
            Some(serde_json::json!({"verifierRequired": false}))
        )],
        &founder,
        &[]
    ));
}

#[test]
fn the_founders_policy_setting_the_flag_requires_a_verifier() {
    let founder = Keys::generate();
    assert!(requires(
        &[policy(
            &founder,
            Some(serde_json::json!({"verifierRequired": true}))
        )],
        &founder,
        &[]
    ));
}

#[test]
fn a_stranger_cannot_impose_a_verifier_requirement() {
    // The same standing rule `bee sessions policy get` and the session
    // provider apply, because it is the same fold (REVIEW-B2 F1). A record
    // nobody with standing signed is excluded, and the answer stays `false`.
    let founder = Keys::generate();
    let stranger = Keys::generate();
    assert!(!requires(
        &[policy(
            &stranger,
            Some(serde_json::json!({"verifierRequired": true}))
        )],
        &founder,
        &[]
    ));

    // With an operator grant accepted before it was published, the same
    // record wins.
    let grants = [CodingSessionPolicyGrant {
        grantee: stranger.public_key().to_hex(),
        accepted_at: 0,
        transition_type: CodingSessionAuthorityTransitionType::GrantOperator,
    }];
    assert!(requires(
        &[policy(
            &stranger,
            Some(serde_json::json!({"verifierRequired": true}))
        )],
        &founder,
        &grants
    ));
}
