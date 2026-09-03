//! Red-first tests for the Land boundary.
//!
//! Every one fails on the base tree because the module did not exist. Two of
//! them are the finding-27 fixtures directly: an approving disposition over a
//! report naming a `headSha` admits, and the same fold with
//! `changes-requested` — the shape live run 3 actually had — does not.
//!
//! **No test here invokes git**, and neither does the code under test: the
//! whole boundary produces a verdict and a string to copy.

use super::*;
use buzz_sdk_pkg::coding_session_team_transaction::build_coding_session_team_transaction;
use nostr::{Keys, Timestamp};
use serde_json::json;

const SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL: &str = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const HEAD_SHA: &str = "07c470be07c470be07c470be07c470be07c470be";

// Pinned so an event id built from a fixed key is fully deterministic: a
// Nostr id hashes `created_at` along with everything else, so leaving it at
// `Timestamp::now()` made `the_typescript_decoder_fixture_is_this_adapter_s_real_output`
// fail on every run after the one that happened to write the fixture,
// regardless of `BUZZ_UPDATE_FIXTURES=1` — the write and the read-back were
// always self-consistent within one process, never across two.
const FIXED_CREATED_AT: u64 = 1_735_689_600;

fn genesis() -> String {
    "ab".repeat(32)
}

fn sign(keys: &Keys, transaction_type: &str, body: serde_json::Value) -> serde_json::Value {
    let content = json!({
        "schema": buzz_core_pkg::coding_session_team_transaction::CODING_SESSION_TEAM_TRANSACTION_SCHEMA,
        "sessionRef": SESSION,
        "genesisRef": genesis(),
        "type": transaction_type,
        "supersedes": serde_json::Value::Null,
        "deliveryCommandId": serde_json::Value::Null,
        "body": body,
    });
    let payload = serde_json::from_value(content).expect("typed fixture");
    let event = build_coding_session_team_transaction(CHANNEL, payload)
        .expect("builder")
        .custom_created_at(Timestamp::from(FIXED_CREATED_AT))
        .sign_with_keys(keys)
        .expect("sign");
    serde_json::to_value(event).expect("event as JSON")
}

fn assignment(keys: &Keys) -> serde_json::Value {
    sign(
        keys,
        "assignment",
        json!({
            "assigneeActor": "cd".repeat(32),
            "assigneeRole": "builder",
            "objective": "Land the verdict gate",
            "brief": "Implement and test the admission rule.",
            "branch": serde_json::Value::Null,
            "baseSha": serde_json::Value::Null,
            "fileOwnership": ["crates/buzz-core"],
            "acceptanceSteps": ["cargo test -p buzz-core"],
        }),
    )
}

fn report(
    keys: &Keys,
    assignment_ref: &str,
    head_sha: Option<&str>,
    branch: Option<&str>,
) -> serde_json::Value {
    sign(
        keys,
        "report",
        json!({
            "assignmentRef": assignment_ref,
            "summary": "Implemented and tested.",
            "branch": branch,
            "baseSha": serde_json::Value::Null,
            "headSha": head_sha,
            "files": ["crates/buzz-core/src/git_perms.rs"],
            "tests": [],
            "redBeforeGreen": serde_json::Value::Null,
            "deviations": [],
            "residuals": [],
            "anomalies": [],
        }),
    )
}

fn disposition(
    keys: &Keys,
    assignment_ref: &str,
    report_ref: &str,
    decision: &str,
) -> serde_json::Value {
    sign(
        keys,
        "verdict",
        json!({
            "subtype": "disposition",
            "assignmentRef": assignment_ref,
            "reportRef": report_ref,
            "refutationRef": serde_json::Value::Null,
            "decision": decision,
            "summary": "Ruling on the report.",
            "findings": [],
            "requiredAction": serde_json::Value::Null,
        }),
    )
}

fn event_id(event: &serde_json::Value) -> String {
    event["id"].as_str().expect("event id").to_owned()
}

fn protect(rules: &[&str]) -> Vec<Vec<String>> {
    let mut tag = vec!["buzz-protect".to_owned(), "refs/heads/main".to_owned()];
    tag.extend(rules.iter().map(|rule| (*rule).to_owned()));
    vec![tag]
}

struct Mission {
    founder: Keys,
    events: Vec<serde_json::Value>,
    included: Vec<String>,
}

/// The finding-27 fixture: one assignment, one report naming a commit, one
/// founder-signed disposition over it.
fn mission(decision: &str, head_sha: Option<&str>, branch: Option<&str>) -> Mission {
    let founder = Keys::generate();
    let assignment = assignment(&founder);
    let report = report(&founder, &event_id(&assignment), head_sha, branch);
    let disposition = disposition(
        &founder,
        &event_id(&assignment),
        &event_id(&report),
        decision,
    );
    let included = vec![
        event_id(&assignment),
        event_id(&report),
        event_id(&disposition),
    ];
    Mission {
        founder,
        events: vec![assignment, report, disposition],
        included,
    }
}

/// The ordinary request: the roster was read and named no extra Owner.
///
/// `Some(vec![])` rather than `None` on purpose — "read, and it adds nobody"
/// is the common case, and `None` is the disclosure a caller that *could not*
/// read the roster must carry (finding 33).
fn request(
    mission: &Mission,
    protection_tags: Option<Vec<Vec<String>>>,
) -> CodingSessionLandRequest {
    let founder_hex = mission.founder.public_key().to_hex();
    CodingSessionLandRequest {
        schema: CODING_SESSION_LAND_REQUEST_SCHEMA.to_owned(),
        ref_name: "refs/heads/main".to_owned(),
        session_ref: SESSION.to_owned(),
        genesis_ref: genesis(),
        founder_pubkey: founder_hex.clone(),
        repo_owner_pubkey: protection_tags.as_ref().map(|_| founder_hex.clone()),
        pusher_pubkey: founder_hex,
        project_owner_pubkeys: Some(Vec::new()),
        protection_tags,
        included_event_ids: mission.included.clone(),
        events: mission.events.clone(),
    }
}

#[test]
fn an_approved_commit_on_a_require_verdict_ref_is_admitted_and_names_the_commit() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");

    assert!(answer.repository_known);
    assert!(answer.rule_governs, "require-verdict must govern this ref");
    assert!(answer.admitted, "an approved commit lands");
    let evidence = answer.evidence.expect("admitted answers carry evidence");
    assert_eq!(evidence.head_sha, HEAD_SHA);
    assert_eq!(
        evidence.disposition_author_pubkey,
        mission.founder.public_key().to_hex()
    );
    // §1l: the commit, never the branch.
    assert_eq!(
        answer.command.as_deref(),
        Some(format!("git push origin {HEAD_SHA}:refs/heads/main").as_str())
    );
    assert!(answer.refusal_reason.is_none());
}

#[test]
fn changes_requested_over_the_same_commit_refuses_with_ss1j_first_string() {
    let mission = mission("changes-requested", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");

    assert!(
        !answer.admitted,
        "finding 27: a FAIL on the wire must stop it"
    );
    assert!(answer.command.is_none(), "a refused push has no command");
    let reason = answer
        .refusal_reason
        .expect("a refusal carries §1j's words");
    assert_eq!(
        reason,
        format!(
            "require-verdict is set and no mission verdict names this commit: no approved report \
             names {HEAD_SHA}. Searched 1 mission(s) — the newest 16 on this channel whose \
             founder is a founder of this repository — over one shared page of the newest 512 \
             team transactions on that channel. An older ruling can fall outside both."
        )
    );
    let newest = answer.newest_verdict.expect("the verdict it read is shown");
    assert_eq!(newest.decision, "changes-requested");
}

#[test]
fn a_report_naming_a_branch_and_no_sha_refuses_with_ss1j_second_string() {
    let mission = mission("approve", None, Some("lane/batch3-l6-relay"));
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");

    assert!(!answer.admitted);
    assert_eq!(
        answer.refusal_reason.as_deref(),
        Some(
            "require-verdict is set and no mission verdict names this commit: the approved report \
             for this work names a branch and no headSha, and a branch name is not a commit."
        )
    );
}

#[test]
fn an_approval_scoped_to_another_branch_is_not_ready_to_land_on_this_ref() {
    // REVIEW-L8 F1: an approval is scoped to the branch its report named.
    // Approving a commit *for `release`* is not approval to put it on `main`,
    // and fix round 1's copy of the predicate had no arm for it — this screen
    // would have offered `git push origin <sha>:refs/heads/main` for a push
    // the relay denies.
    let mission = mission("approve", Some(HEAD_SHA), Some("release"));
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");

    assert!(
        !answer.admitted,
        "an approval for `release` does not admit main"
    );
    assert!(answer.command.is_none(), "and offers no command");
    assert_eq!(
        answer.refusal_reason.as_deref(),
        Some(
            format!(
                "commit {HEAD_SHA} is approved for release, and this is a different ref. A \
                 verdict admits a commit to the branch its report named."
            )
            .as_str()
        )
    );
}

#[test]
fn an_approval_naming_this_ref_admits_however_the_report_spelled_it() {
    // L6's `ref_names_branch`: a report may name `main` or `refs/heads/main`,
    // and both name this ref. Matched whole — `main` never names `main-2`.
    for branch in ["main", "refs/heads/main"] {
        let mission = mission("approve", Some(HEAD_SHA), Some(branch));
        let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
            .expect("the boundary answers");
        assert!(answer.admitted, "`{branch}` names refs/heads/main");
    }
    let mission = mission("approve", Some(HEAD_SHA), Some("main-2"));
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");
    assert!(!answer.admitted, "`main-2` is not `main`");
}

#[test]
fn a_repo_with_no_require_verdict_rule_says_so_and_still_shows_the_verdict() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, Some(protect(&["no-force-push"]))))
        .expect("the boundary answers");

    assert!(answer.repository_known);
    assert!(!answer.rule_governs, "no rule governs this ref");
    assert!(!answer.admitted);
    assert!(answer.refusal_reason.is_none(), "not a refusal — no rule");
    let newest = answer.newest_verdict.expect("the verdict it read is shown");
    assert_eq!(newest.decision, "approve");
    assert_eq!(newest.head_sha.as_deref(), Some(HEAD_SHA));
}

#[test]
fn no_repository_record_is_unknown_and_never_reads_as_no_rule() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, None)).expect("the boundary answers");

    assert!(!answer.repository_known, "unknown ≠ no rule");
    assert!(!answer.rule_governs);
    assert!(!answer.admitted);
}

#[test]
fn a_ruling_signed_by_someone_other_than_the_founder_does_not_admit() {
    let mut mission = mission("approve", Some(HEAD_SHA), None);
    // Re-sign the disposition under a lead's key: canonical for the fold,
    // and not founder-signed, which is L6's shipped rule.
    let lead = Keys::generate();
    let assignment_ref = mission.included[0].clone();
    let report_ref = mission.included[1].clone();
    let lead_disposition = disposition(&lead, &assignment_ref, &report_ref, "approve");
    mission.included[2] = event_id(&lead_disposition);
    mission.events[2] = lead_disposition;

    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");
    assert!(
        !answer.admitted,
        "founder-signed only while the pusher gate is founder-only"
    );
}

#[test]
fn a_pusher_who_is_not_the_founder_gets_the_reserved_act_string() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = request(&mission, Some(protect(&["require-verdict"])));
    request.pusher_pubkey = Keys::generate().public_key().to_hex();

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(!answer.admitted);
    assert_eq!(
        answer.refusal_reason.as_deref(),
        Some(
            format!(
                "commit {HEAD_SHA} is approved, but the relay's require-verdict rule reserves a \
                 gated ref to a founder of this repository (1 founder(s): the announcement's \
                 signer, its maintainers tag, and the project roster's owners). Ask a founder \
                 to land it."
            )
            .as_str()
        )
    );
}

#[test]
fn an_event_the_fold_did_not_include_is_never_evidence() {
    let mut mission = mission("approve", Some(HEAD_SHA), None);
    // The disposition is on the wire but not canonical.
    mission.included.pop();
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");
    assert!(!answer.admitted, "only canonical records are evidence");
}

#[test]
fn a_wrong_request_schema_is_refused_before_any_event_is_read() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = request(&mission, Some(protect(&["require-verdict"])));
    request.schema = "buzz-coding-session-team-fold-request/v1".to_owned();
    let error = land_adapter(request).expect_err("a foreign schema is refused");
    assert!(
        error.contains(CODING_SESSION_LAND_REQUEST_SCHEMA),
        "{error}"
    );
}

#[test]
fn the_require_verdict_flag_is_parsed_as_a_flag_not_read_off_the_unknown_list() {
    // REVIEW-L8 F3. Fix round 1 matched the token only in
    // `parse_protection_tag_with_warnings`'s *unknown* list. That list is
    // exactly what emptied when core learned the token, and a control reading
    // it would then tell a founder the repository has no rule when it has one.
    // Both halves are asserted: core parses the token into the flag and NOT
    // into `unknown_rules`, and this boundary reads the flag.
    let (rule, unknown) = buzz_core_pkg::git_perms::parse_protection_tag_with_warnings(&[
        "refs/heads/main",
        "require-verdict",
    ])
    .expect("the tag parses");
    assert!(
        rule.require_verdict,
        "core parses `require-verdict` into the flag"
    );
    assert!(
        !unknown.iter().any(|token| token == "require-verdict"),
        "so it is no longer an unknown token — the list this must never read"
    );
    assert!(
        ref_requires_verdict(&protect(&["require-verdict"]), "refs/heads/main"),
        "and this boundary reads it as a flag"
    );
}

#[test]
fn a_tag_whose_role_is_invalid_sets_no_flag() {
    // L6's parser returns `InvalidRole` for `push:bot`, and a tag that errors
    // is not a rule — so it cannot carry the flag either.
    let tag = vec![
        "buzz-protect".to_owned(),
        "refs/heads/main".to_owned(),
        "push:bot".to_owned(),
        "require-verdict".to_owned(),
    ];
    assert!(!ref_requires_verdict(&[tag], "refs/heads/main"));
}

#[test]
fn the_require_verdict_token_is_read_through_cores_own_pattern_matcher() {
    // A rule on another ref pattern must not govern refs/heads/main.
    let mut tag = vec!["buzz-protect".to_owned(), "refs/heads/release/*".to_owned()];
    tag.push("require-verdict".to_owned());
    assert!(!ref_requires_verdict(&[tag], "refs/heads/main"));
    assert!(ref_requires_verdict(
        &protect(&["require-verdict"]),
        "refs/heads/main"
    ));
}

/// Path of the fixture the Desktop decoder test reads, relative to this crate.
const TS_DECODER_FIXTURE: &str =
    "../src/features/coding-sessions/lib/codingSessionLandAdapterResponse.fixture.json";

/// Deterministic keys, so the fixture does not churn on every run.
fn fixed_keys(byte: u8) -> Keys {
    Keys::parse(&hex::encode([byte; 32])).expect("fixed secret key")
}

/// The same fixture as `fixed_mission`, with the report naming a branch.
fn fixed_mission_on_branch(decision: &str, head_sha: Option<&str>, branch: &str) -> Mission {
    let founder = fixed_keys(0x11);
    let assignment = assignment(&founder);
    let report = report(&founder, &event_id(&assignment), head_sha, Some(branch));
    let disposition = disposition(
        &founder,
        &event_id(&assignment),
        &event_id(&report),
        decision,
    );
    let included = vec![
        event_id(&assignment),
        event_id(&report),
        event_id(&disposition),
    ];
    Mission {
        founder,
        events: vec![assignment, report, disposition],
        included,
    }
}

/// The same fixture as `mission`, with a pinned founder key.
fn fixed_mission(decision: &str, head_sha: Option<&str>) -> Mission {
    let founder = fixed_keys(0x11);
    let assignment = assignment(&founder);
    let report = report(&founder, &event_id(&assignment), head_sha, None);
    let disposition = disposition(
        &founder,
        &event_id(&assignment),
        &event_id(&report),
        decision,
    );
    let included = vec![
        event_id(&assignment),
        event_id(&report),
        event_id(&disposition),
    ];
    Mission {
        founder,
        events: vec![assignment, report, disposition],
        included,
    }
}

#[test]
fn the_typescript_decoder_fixture_is_this_adapter_s_real_output() {
    // Regenerate with
    // `BUZZ_UPDATE_FIXTURES=1 cargo test --manifest-path desktop/src-tauri/Cargo.toml coding_session_land`.
    let admitted = fixed_mission("approve", Some(HEAD_SHA));
    let refused = fixed_mission("changes-requested", Some(HEAD_SHA));
    let generated = serde_json::to_string_pretty(&json!({
        "note": "Generated by `the_typescript_decoder_fixture_is_this_adapter_s_real_output` in \
    desktop/src-tauri/src/commands/coding_session_land_tests.rs. Do not hand-edit: a hand-written \
    fixture is what let a decoder stay green while it would have thrown for every real record.",
        "admitted": land_adapter(request(&admitted, Some(protect(&["require-verdict"]))))
            .expect("admitted"),
        "refused": land_adapter(request(&refused, Some(protect(&["require-verdict"]))))
            .expect("refused"),
        "ungoverned": land_adapter(request(&admitted, Some(protect(&["no-force-push"]))))
            .expect("ungoverned"),
        "approvedForAnotherRef": land_adapter(request(
            &fixed_mission_on_branch("approve", Some(HEAD_SHA), "release"),
            Some(protect(&["require-verdict"])),
        ))
        .expect("approved for another ref"),
        "unknown": land_adapter(request(&admitted, None)).expect("unknown"),
    }))
    .expect("serialize fixture")
        + "\n";

    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(TS_DECODER_FIXTURE);
    if std::env::var("BUZZ_UPDATE_FIXTURES").is_ok() {
        std::fs::write(&path, &generated).expect("write fixture");
    }
    let stored = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    let stored_value: serde_json::Value =
        serde_json::from_str(&stored).expect("stored fixture is JSON");
    let generated_value: serde_json::Value =
        serde_json::from_str(&generated).expect("generated fixture is JSON");
    assert_eq!(
        stored_value, generated_value,
        "the Desktop land fixture is stale; regenerate it with BUZZ_UPDATE_FIXTURES=1"
    );
    assert_eq!(generated_value["admitted"]["admitted"], json!(true));
    assert_eq!(generated_value["refused"]["admitted"], json!(false));
    assert_eq!(generated_value["ungoverned"]["ruleGoverns"], json!(false));
    assert_eq!(generated_value["unknown"]["repositoryKnown"], json!(false));
    assert_eq!(
        generated_value["approvedForAnotherRef"]["admitted"],
        json!(false)
    );
}

// ── finding 33: the Land control names the founders ─────────────────────

/// A viewer who is a `maintainers` co-founder — but not the announcement's
/// signer and not the mission's founder — may land the ruling, and the answer
/// names both founders so the screen can say who they are.
#[test]
fn a_maintainer_may_land_and_the_answer_names_both_founders() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let maintainer = Keys::generate().public_key().to_hex();
    let mut tags = protect(&["require-verdict"]);
    tags.push(vec!["maintainers".to_owned(), maintainer.clone()]);
    let mut request = request(&mission, Some(tags));
    request.pusher_pubkey = maintainer.clone();

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(answer.admitted, "an equal owner lands the other's ruling");
    assert!(answer.viewer_is_founder);
    assert_eq!(
        answer.founders,
        vec![mission.founder.public_key().to_hex(), maintainer]
    );
    assert!(
        answer.founders_note.contains("(2)"),
        "the sentence names both: {}",
        answer.founders_note
    );
}

/// The roster half, which no tag carries: the caller passes the project's
/// Owners, and the same viewer becomes a founder. Passing `None` instead is
/// "not read", and the sentence says so.
#[test]
fn project_roster_owners_join_the_founder_set_and_null_is_disclosed() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let roster_owner = Keys::generate().public_key().to_hex();

    let mut read = request(&mission, Some(protect(&["require-verdict"])));
    read.pusher_pubkey = roster_owner.clone();
    read.project_owner_pubkeys = Some(vec![roster_owner.clone()]);
    let answer = land_adapter(read).expect("the boundary answers");
    assert!(answer.admitted, "a project Owner founds the repository too");
    assert!(answer.viewer_is_founder);
    assert!(
        !answer.founders_note.contains("was not read here"),
        "a read roster is not disclosed as unread: {}",
        answer.founders_note
    );

    let mut unread = request(&mission, Some(protect(&["require-verdict"])));
    unread.pusher_pubkey = roster_owner.clone();
    unread.project_owner_pubkeys = None;
    let answer = land_adapter(unread).expect("the boundary answers");
    assert!(!answer.admitted, "an unread roster grants nobody");
    assert!(!answer.viewer_is_founder);
    assert!(
        answer.founders_note.contains("was not read here"),
        "an unread roster is disclosed: {}",
        answer.founders_note
    );
}

/// With no repository record the founder line is a fact about the *read*, not
/// a claim that the repository has no founders.
#[test]
fn no_repository_record_names_no_founders_and_says_why() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, None)).expect("the boundary answers");
    assert!(!answer.repository_known);
    assert!(answer.founders.is_empty());
    assert_eq!(answer.founders_note, NO_REPOSITORY_FOUNDERS_NOTE);
    assert!(!answer.viewer_is_founder);
}
