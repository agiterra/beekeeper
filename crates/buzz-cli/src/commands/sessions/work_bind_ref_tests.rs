//! Tests for `bee sessions work bind ref` (ledger 255), against the stub
//! wire: what it binds, and every refusal it makes before signing.

use super::*;

use super::super::bind_ref::bind_ref_with;

const MAIN_SHA: &str = "e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7e7";
const OBSERVATION: &str = "0b51000000000000000000000000000000000000000000000000000000000000";
const HOST_RESULT: &str = "4623000000000000000000000000000000000000000000000000000000000000";

fn happy_fixture() -> Value {
    serde_json::from_str(SEQUENCES[0].inputs).expect("inputs")
}

fn ref_state(id: &str, signer: &str, branch: &str, sha: &str, at: u64) -> Value {
    json!({"id": id, "pubkey": signer, "created_at": at, "kind": 30618, "content": "",
           "tags": [["d", "pivot-test"], [branch, sha], ["HEAD", "ref: refs/heads/main"]]})
}

fn host_result(id: &str, head_sha: &str) -> Value {
    json!({"id": id, "pubkey": "40".repeat(32), "created_at": 7u64, "kind": 46023, "tags": [],
           "content": json!({"headSha": head_sha, "exitCode": 0}).to_string()})
}

fn relay_key(fixture: &Value) -> String {
    fixture["relaySelfKey"]
        .as_str()
        .expect("a relay key")
        .to_owned()
}

fn args(declaration: &str, criteria: &[&str]) -> WorkBindRefArgs {
    WorkBindRefArgs {
        envelope: bind_envelope(declaration, criteria),
        commit: Some(MAIN_SHA.to_owned()),
        git_ref: Some("refs/heads/main".to_owned()),
        observed_by: Some(HOST_RESULT.to_owned()),
        completion: None,
    }
}

/// The ordinary path: the newest relay-signed 30618 names main at the
/// delivered commit, the host result ran there, and one `ref_observation`
/// binding is signed. The identical second binding publishes nothing.
#[tokio::test]
async fn bind_ref_binds_the_relay_observation_once() {
    let fixture = happy_fixture();
    let relay = relay_key(&fixture);
    let session = fixture_session(&fixture);
    let (declaration, rows) = bound_rows(
        &session,
        vec![
            ref_state(
                &"0a".repeat(32),
                &relay,
                "refs/heads/main",
                &"3a".repeat(20),
                40,
            ),
            ref_state(OBSERVATION, &relay, "refs/heads/main", MAIN_SHA, 50),
            // An owner's later claim is not an observation and is not newest.
            ref_state(
                &"0c".repeat(32),
                &"1e".repeat(32),
                "refs/heads/main",
                &"77".repeat(20),
                60,
            ),
            host_result(HOST_RESULT, MAIN_SHA),
        ],
    );
    let wire = StubWire::new(rows);
    let plans = FixturePlans::from(&fixture);
    let args = args(&declaration, &["delivered-main"]);
    bind_ref_with(&wire, fixture_session(&fixture), &args, &plans)
        .await
        .expect("bound");
    assert_eq!(wire.publishes(), 1);
    let published = wire.published.borrow()[0].clone();
    let row = wire
        .rows
        .borrow()
        .iter()
        .find(|row| row["id"] == published.as_str())
        .cloned()
        .expect("the stub stored it");
    let payload: Value =
        serde_json::from_str(row["content"].as_str().expect("content")).expect("json");
    assert_eq!(payload["type"], "work.evidence_bound");
    assert_eq!(payload["body"]["criterionIds"], json!(["delivered-main"]));
    assert_eq!(payload["body"]["artifactCommit"], MAIN_SHA);
    assert_eq!(
        payload["body"]["evidenceRefs"],
        json!([{"kind": "ref_observation", "eventId": OBSERVATION}])
    );

    // A second binding of the same criterion at the same commit.
    bind_ref_with(&wire, session, &args, &plans)
        .await
        .expect("reported, not republished");
    assert_eq!(wire.publishes(), 1, "a second binding was published");
}

/// Every refusal happens before anything is signed, and names itself.
#[tokio::test]
async fn bind_ref_refusals_sign_nothing() {
    let fixture = happy_fixture();
    let relay = relay_key(&fixture);
    let session = fixture_session(&fixture);
    let observed = vec![
        ref_state(OBSERVATION, &relay, "refs/heads/main", MAIN_SHA, 50),
        host_result(HOST_RESULT, MAIN_SHA),
    ];
    let (declaration, rows) = bound_rows(&session, observed);

    let mut malformed = args(&declaration, &["delivered-main"]);
    malformed.commit = Some("e7e7e7".to_owned());
    let mut wrong_ref = args(&declaration, &["delivered-main"]);
    wrong_ref.git_ref = Some("refs/heads/release".to_owned());
    let mut other_commit = args(&declaration, &["delivered-main"]);
    other_commit.commit = Some("3a".repeat(20));
    let review = args(&declaration, &["cli-behaviour"]);
    let cases = [
        (malformed, "malformed-commit"),
        (wrong_ref, "ref-not-delivery-ref"),
        (other_commit, "ref-observation-mismatch"),
        (review, "evidence-kind-mismatch"),
    ];
    for (case, code) in cases {
        let wire = StubWire::new(rows.clone());
        let error = bind_ref_with(
            &wire,
            fixture_session(&fixture),
            &case,
            &FixturePlans::from(&fixture),
        )
        .await
        .expect_err(code);
        assert!(error.to_string().contains(code), "{code}: {error}");
        assert_eq!(wire.publishes(), 0, "{code}: signed anyway");
    }

    // The host result named as corroboration ran somewhere else.
    let (declaration, rows) = bound_rows(
        &session,
        vec![
            ref_state(OBSERVATION, &relay, "refs/heads/main", MAIN_SHA, 50),
            host_result(HOST_RESULT, &"3a".repeat(20)),
        ],
    );
    let wire = StubWire::new(rows);
    let error = bind_ref_with(
        &wire,
        fixture_session(&fixture),
        &args(&declaration, &["delivered-main"]),
        &FixturePlans::from(&fixture),
    )
    .await
    .expect_err("observed-by-mismatch");
    assert!(
        error.to_string().contains("observed-by-mismatch"),
        "{error}"
    );
    assert_eq!(wire.publishes(), 0);

    // Only an owner-signed 30618 exists: nothing observed the ref.
    let (declaration, rows) = bound_rows(
        &session,
        vec![
            ref_state(
                OBSERVATION,
                &"1e".repeat(32),
                "refs/heads/main",
                MAIN_SHA,
                50,
            ),
            host_result(HOST_RESULT, MAIN_SHA),
        ],
    );
    let wire = StubWire::new(rows);
    let error = bind_ref_with(
        &wire,
        fixture_session(&fixture),
        &args(&declaration, &["delivered-main"]),
        &FixturePlans::from(&fixture),
    )
    .await
    .expect_err("no-ref-observation");
    assert!(error.to_string().contains("no-ref-observation"), "{error}");
    assert_eq!(wire.publishes(), 0);

    // The relay-signed state names another branch only.
    let (declaration, rows) = bound_rows(
        &session,
        vec![ref_state(
            OBSERVATION,
            &relay,
            "refs/heads/release",
            MAIN_SHA,
            50,
        )],
    );
    let wire = StubWire::new(rows);
    let mut unobserved = args(&declaration, &["delivered-main"]);
    unobserved.observed_by = None;
    let error = bind_ref_with(
        &wire,
        fixture_session(&fixture),
        &unobserved,
        &FixturePlans::from(&fixture),
    )
    .await
    .expect_err("no delivery ref observed");
    assert!(
        error.to_string().contains("names no refs/heads/main"),
        "{error}"
    );
    assert_eq!(wire.publishes(), 0);
}

/// With no `--commit`, the verb binds whatever the relay observed. (What
/// `status` then reads is pinned by `sequences/ref-observation-rebound/`.)
#[tokio::test]
async fn bind_ref_without_a_commit_binds_what_the_relay_observed() {
    let fixture = happy_fixture();
    let relay = relay_key(&fixture);
    let session = fixture_session(&fixture);
    let (declaration, rows) = bound_rows(
        &session,
        vec![ref_state(
            OBSERVATION,
            &relay,
            "refs/heads/main",
            MAIN_SHA,
            50,
        )],
    );
    let wire = StubWire::new(rows);
    let mut args = args(&declaration, &["delivered-main"]);
    args.commit = None;
    args.observed_by = None;
    bind_ref_with(&wire, session, &args, &FixturePlans::from(&fixture))
        .await
        .expect("bound");
    assert_eq!(wire.publishes(), 1);
}
