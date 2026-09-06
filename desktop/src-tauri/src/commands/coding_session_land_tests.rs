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

fn assignment(keys: &Keys, assignee: &str) -> serde_json::Value {
    sign(
        keys,
        "assignment",
        json!({
            "assigneeActor": assignee,
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

fn refutation(
    keys: &Keys,
    assignment_ref: &str,
    report_ref: &str,
    decision: &str,
) -> serde_json::Value {
    sign(
        keys,
        "verdict",
        json!({
            "subtype": "refutation",
            "assignmentRef": assignment_ref,
            "reportRef": report_ref,
            "decision": decision,
            "summary": "Could not break it.",
            "findings": [],
            "requiredAction": serde_json::Value::Null,
        }),
    )
}

fn event_id(event: &serde_json::Value) -> String {
    event["id"].as_str().expect("event id").to_owned()
}

/// The repository name these fixtures announce, as its `d` tag.
///
/// Finding 91 made the announcement's `d` load-bearing: the coordinate
/// `30617:<signer>:<d>` is what a mission must be bound to, so a fixture with
/// no `d` would exercise the binding check against an empty string.
const REPO_D: &str = "beekeeper";

/// A stand-in kind 44245 record id, so the answer's policy evidence has one
/// to name (finding 89).
const POLICY_EVENT_ID: &str = "9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a9a";

/// The `30617` coordinate `protect()`'s announcement addresses, for `founder`.
fn repository_of(mission: &Mission) -> String {
    format!("30617:{}:{REPO_D}", mission.founder.public_key().to_hex())
}

fn protect(rules: &[&str]) -> Vec<Vec<String>> {
    let mut tag = vec!["buzz-protect".to_owned(), "refs/heads/main".to_owned()];
    tag.extend(rules.iter().map(|rule| (*rule).to_owned()));
    vec![vec!["d".to_owned(), REPO_D.to_owned()], tag]
}

struct Mission {
    founder: Keys,
    builder: Keys,
    verifier: Keys,
    events: Vec<serde_json::Value>,
    included: Vec<String>,
}

impl Mission {
    /// The seats this mission's authority fold would hand the Land command.
    fn seats(&self) -> Vec<CodingSessionLandSeat> {
        vec![
            CodingSessionLandSeat {
                actor_pubkey: self.builder.public_key().to_hex(),
                role: "builder".to_owned(),
            },
            CodingSessionLandSeat {
                actor_pubkey: self.verifier.public_key().to_hex(),
                role: "verifier".to_owned(),
            },
        ]
    }
}

/// The finding-27 fixture, in the shape the 2026-09-03 ruling judges: the
/// builder reports, the founder settles, and a verifier independently fails
/// to refute it. `cleared` false drops that last record.
fn mission_with(
    decision: &str,
    head_sha: Option<&str>,
    branch: Option<&str>,
    cleared: bool,
    keys: (Keys, Keys, Keys),
) -> Mission {
    let (founder, builder, verifier) = keys;
    let assignment = assignment(&founder, &builder.public_key().to_hex());
    let report = report(&builder, &event_id(&assignment), head_sha, branch);
    let disposition = disposition(
        &founder,
        &event_id(&assignment),
        &event_id(&report),
        decision,
    );
    let mut included = vec![
        event_id(&assignment),
        event_id(&report),
        event_id(&disposition),
    ];
    let mut events = vec![assignment.clone(), report.clone(), disposition];
    if cleared {
        let cleared = refutation(
            &verifier,
            &event_id(&assignment),
            &event_id(&report),
            "not-refuted",
        );
        included.push(event_id(&cleared));
        events.push(cleared);
    }
    Mission {
        founder,
        builder,
        verifier,
        events,
        included,
    }
}

fn mission(decision: &str, head_sha: Option<&str>, branch: Option<&str>) -> Mission {
    mission_with(
        decision,
        head_sha,
        branch,
        true,
        (Keys::generate(), Keys::generate(), Keys::generate()),
    )
}

/// Every default gate, observed green on the pushed commit by a key that is
/// neither the pusher nor any seat — the shape both arm (B) and, since the
/// 2026-09-03 follow-up ruling, arm (C) require.
fn green_rows() -> Vec<crate::commands::coding_session_land::CodingSessionLandObservedGate> {
    buzz_core_pkg::coding_session_verdict_admission::DEFAULT_REQUIRED_GATES
        .iter()
        .map(
            |gate| crate::commands::coding_session_land::CodingSessionLandObservedGate {
                author_pubkey: "aa".repeat(32),
                source: "observed".to_owned(),
                gate: (*gate).to_owned(),
                outcome: "passed".to_owned(),
                head_sha: Some(HEAD_SHA.to_owned()),
                dirty: Some(false),
            },
        )
        .collect()
}

/// The ordinary request: the roster was read and named no extra Owner.
///
/// `Some(vec![])` rather than `None` on purpose — "read, and it adds nobody"
/// is the common case, and `None` is the disclosure a caller that *could not*
/// read the roster must carry (finding 33).
///
/// It carries the gate rows **and** `verifierRequired: true` since L27: arm
/// (C) now wants both halves, and the flag keeps arm (B) silent so a case
/// about the verdict is answered by the verdict. `request_without_gate_rows`
/// is the same fixture short of the new half.
fn request(
    mission: &Mission,
    protection_tags: Option<Vec<Vec<String>>>,
) -> CodingSessionLandRequest {
    CodingSessionLandRequest {
        observed_gates: green_rows(),
        gate_policy: Some(
            crate::commands::coding_session_land::CodingSessionLandGatePolicy {
                verifier_required: Some(true),
                required_gates: None,
                event_id: Some(POLICY_EVENT_ID.to_owned()),
                resolution: None,
                unreadable_reason: None,
            },
        ),
        ..request_without_gate_rows(mission, protection_tags)
    }
}

/// The pre-L27 request: a verifier's clearance and no gate row at all.
fn request_without_gate_rows(
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
        // Read-optional: no view has read a rule record here, which is the
        // "signed before the kind existed" shape every one of these cases is
        // written against.
        rule_records: None,
        // A seat, not the founder: arm (A) admits a founder outright, so a
        // founder-pushed fixture would answer "ready" over every mission and
        // exercise none of the rule. The founder's own push has its own case.
        pusher_pubkey: mission.builder.public_key().to_hex(),
        project_owner_pubkeys: Some(Vec::new()),
        protection_tags,
        active_seats: mission.seats(),
        observed_gates: Vec::new(),
        gate_policy: None,
        // Read: this view knows the mission works on the repository it is
        // being asked about. `None` is the caller that did not read it, and
        // `boundRepositoriesRead` says which one answered.
        bound_repositories: Some(vec![format!(
            "30617:{}:{REPO_D}",
            mission.founder.public_key().to_hex()
        )]),
        included_event_ids: mission.included.clone(),
        events: mission.events.clone(),
    }
}

/// The same request, with every default gate observed green on the pushed
/// commit by a key that is not the pusher — arm (B).
///
/// The mission it is built from holds a **`changes-requested`** verdict, so
/// nothing here can be admitted by arm (C): what admits it is the gate rows
/// and only the gate rows.
fn observed_gates_request(
    mission: &Mission,
    protection_tags: Option<Vec<Vec<String>>>,
) -> CodingSessionLandRequest {
    CodingSessionLandRequest {
        observed_gates: green_rows(),
        gate_policy: Some(
            crate::commands::coding_session_land::CodingSessionLandGatePolicy {
                verifier_required: Some(false),
                required_gates: None,
                event_id: Some(POLICY_EVENT_ID.to_owned()),
                resolution: None,
                unreadable_reason: None,
            },
        ),
        ..request_without_gate_rows(mission, protection_tags)
    }
}

/// The same request, pushed by a founder — arm (A).
fn founder_request(
    mission: &Mission,
    protection_tags: Option<Vec<Vec<String>>>,
) -> CodingSessionLandRequest {
    CodingSessionLandRequest {
        pusher_pubkey: mission.founder.public_key().to_hex(),
        ..request(mission, protection_tags)
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

/// L27: arm (C) is the clearance **and** the rows, and the evidence names
/// both — a confirm step saying only "a verifier cleared it" over an arm that
/// also checked three gates would be telling half the truth.
#[test]
fn an_arm_c_admission_names_the_gates_it_also_stood_on() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");
    let evidence = answer.evidence.expect("admitted answers carry evidence");
    assert_eq!(evidence.arm, "verifier-verdict");
    assert_eq!(
        evidence.observed_gates,
        buzz_core_pkg::coding_session_verdict_admission::DEFAULT_REQUIRED_GATES.to_vec()
    );
}

/// The same cleared report with no gate row naming the commit is refused, and
/// the refusal names the half that *is* satisfied first.
#[test]
fn a_clearance_with_no_gate_rows_is_refused_and_names_both_halves() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request_without_gate_rows(
        &mission,
        Some(protect(&["require-verdict"])),
    ))
    .expect("the boundary answers");
    assert!(!answer.admitted, "a clearance alone no longer lands");
    let reason = answer.refusal_reason.expect("a refusal carries its reason");
    assert!(
        reason.contains(&mission.verifier.public_key().to_hex()),
        "{reason}"
    );
    for gate in buzz_core_pkg::coding_session_verdict_admission::DEFAULT_REQUIRED_GATES {
        assert!(reason.contains(gate), "{reason}");
    }
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
             names {HEAD_SHA}. Searched only mission {SESSION}, the one this screen is showing. \
             An older ruling can fall outside both. No observed gate row names {HEAD_SHA} \
             either, so the gate-row route is not open for it: that route wants every required \
             gate published green on this exact commit, by the mission's own provider, over a \
             clean worktree. Run each gate as its own command so the host can record it."
        )
    );
    // The newest *disposition* is the one shown; the verifier's refutation is
    // a ruling about the report, not about the assignment's fate.
    let newest = answer.newest_verdict.expect("the verdict it read is shown");
    assert!(
        matches!(
            newest.decision.as_str(),
            "changes-requested" | "not-refuted"
        ),
        "the newest verdict is one of the mission's two, got {}",
        newest.decision
    );
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
    assert!(
        matches!(newest.decision.as_str(), "approve" | "not-refuted"),
        "the newest verdict is one of the mission's two, got {}",
        newest.decision
    );
}

/// The **only** surface that can still show `RepositoryUnbound` (L21).
///
/// The relay reaches its own unbound branch for nobody now: arm (A) admits
/// every founder before it, and an unbound repository grants a non-founder no
/// git role at all, so `policy.rs` denies with the `no_channel_binding`
/// remediation token first
/// (`verdict_admission_tests::live::an_unbound_repository_refuses_a_seat_at_the_binding_gate_first`).
/// This adapter has no role gate in front of it, so §1j's fourth string is
/// pinned here or nowhere.
#[test]
fn a_governed_repository_bound_to_no_channel_says_so_to_a_seat() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = request(&mission, Some(protect(&["require-verdict"])));
    // Tags but no owner: a governed repository whose binding never reached
    // this view.
    request.repo_owner_pubkey = None;
    let answer = land_adapter(request).expect("the boundary answers");

    assert!(answer.repository_known, "a rule reached this view");
    assert!(answer.rule_governs);
    assert!(!answer.admitted);
    assert_eq!(
        answer.refusal_reason.as_deref(),
        Some(
            "require-verdict is set and there is nowhere to look for a mission verdict: this \
             key holds no seat in the newest 512 authority transitions this relay could read, \
             this repository names no project, and it is bound to no channel. Remove the rule, \
             put the repository in a project, or bind it to the mission's channel."
        )
    );
}

#[test]
fn no_repository_record_is_unknown_and_never_reads_as_no_rule() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let answer = land_adapter(request(&mission, None)).expect("the boundary answers");

    assert!(!answer.repository_known, "unknown ≠ no rule");
    assert!(!answer.rule_governs);
    assert!(!answer.admitted);
}

/// An approval nobody checked does not admit a seat's push. Since the
/// 2026-09-03 ruling the missing fact is the **verifier's clearance**, not the
/// approval's signer: a lead may settle, and a lead's settlement is not proof.
#[test]
fn an_approval_no_verifier_cleared_does_not_admit_a_seat_push() {
    let mission = mission_with(
        "approve",
        Some(HEAD_SHA),
        None,
        false,
        (Keys::generate(), Keys::generate(), Keys::generate()),
    );
    let answer = land_adapter(request(&mission, Some(protect(&["require-verdict"]))))
        .expect("the boundary answers");
    assert!(!answer.admitted, "settling is not checking");
    assert!(
        answer
            .refusal_reason
            .as_deref()
            .is_some_and(|reason| reason
                .contains("no active verifier seat has cleared the report it approves")),
        "{:?}",
        answer.refusal_reason
    );
}

/// Arm (A): a founder's push is admitted with no verdict at all — here over a
/// mission whose only ruling is `changes-requested`.
#[test]
fn a_founder_push_is_admitted_with_no_verdict_and_says_which_arm() {
    let mission = mission("changes-requested", Some(HEAD_SHA), None);
    let answer = land_adapter(founder_request(
        &mission,
        Some(protect(&["require-verdict"])),
    ))
    .expect("the boundary answers");
    assert!(answer.admitted, "a founder's push needs no verdict");
    let evidence = answer.evidence.expect("an admission names its arm");
    assert_eq!(evidence.arm, "founder");
    assert_eq!(evidence.head_sha, HEAD_SHA);
    assert!(
        evidence.session_ref.is_empty() && evidence.disposition_event_id.is_empty(),
        "arm (A) reads no mission, so it names no ruling"
    );
}

/// **Finding 91, where the rule is the only guard.** The relay narrows its
/// search to the repository's own channels; this surface does not — it asks
/// about the one mission on screen. So the binding check inside the rule is
/// what stops a mission that works on another repository from admitting a
/// push here, and the refusal names both the mission and the coordinate.
#[test]
fn a_mission_bound_to_another_repository_does_not_admit_a_push_here() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    request.bound_repositories = Some(vec![format!(
        "30617:{}:tankloop",
        mission.founder.public_key().to_hex()
    )]);

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(
        !answer.admitted,
        "a shared founder is not a binding (finding 91)"
    );
    let refusal = answer.refusal_reason.unwrap_or_default();
    assert!(refusal.contains(SESSION), "{refusal}");
    assert!(refusal.contains(&repository_of(&mission)), "{refusal}");
    assert!(
        answer.bound_repositories_read,
        "this caller sent the binding, and the answer must say so"
    );

    // The control: the same request bound to the repository on screen does
    // admit, so the refusal above is about the binding and nothing else.
    let mut bound = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    bound.bound_repositories = Some(vec![repository_of(&mission)]);
    assert!(
        land_adapter(bound).expect("the boundary answers").admitted,
        "the same mission bound to this repository admits it"
    );
}

/// A caller that read no binding is answered as it always was — and the answer
/// discloses that this surface assumed one, rather than implying it checked.
#[test]
fn an_unread_binding_is_assumed_and_the_answer_says_so() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    request.bound_repositories = None;

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(
        answer.admitted,
        "the pre-finding-91 caller still gets an answer"
    );
    assert!(
        !answer.bound_repositories_read,
        "and it says the binding was assumed, not read"
    );
}

/// **Finding 89 on this surface.** A caller that read a policy record it could
/// not decode says `unreadable`, and the rule refuses rather than falling back
/// to the defaults — the one direction the old `Option` could not express.
#[test]
fn an_unreadable_policy_refuses_here_too() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    request.gate_policy = Some(
        crate::commands::coding_session_land::CodingSessionLandGatePolicy {
            verifier_required: Some(false),
            required_gates: None,
            event_id: Some(POLICY_EVENT_ID.to_owned()),
            resolution: Some("unreadable".to_owned()),
            unreadable_reason: Some("gates.verifierRequired is not a boolean".to_owned()),
        },
    );

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(
        !answer.admitted,
        "nothing admits under a policy nobody read"
    );
    let refusal = answer.refusal_reason.unwrap_or_default();
    assert!(refusal.contains(POLICY_EVENT_ID), "{refusal}");

    // A resolution word this build does not know is unreadable too — never
    // silently `present`.
    let mut unknown = observed_gates_request(&mission, Some(protect(&["require-verdict"])));
    unknown.gate_policy = Some(
        crate::commands::coding_session_land::CodingSessionLandGatePolicy {
            verifier_required: Some(false),
            required_gates: None,
            event_id: Some(POLICY_EVENT_ID.to_owned()),
            resolution: Some("provisional".to_owned()),
            unreadable_reason: None,
        },
    );
    assert!(
        !land_adapter(unknown)
            .expect("the boundary answers")
            .admitted,
        "a word nobody recognises must not become a policy"
    );
}

/// An admission says which policy it stood on, and a founder's says that none
/// was consulted — the audit's Q1 receipt, on the screen a person reads.
#[test]
fn an_admission_names_the_policy_it_stood_on() {
    let mission = mission("changes-requested", Some(HEAD_SHA), None);
    let observed = land_adapter(observed_gates_request(
        &mission,
        Some(protect(&["require-verdict"])),
    ))
    .expect("the boundary answers");
    let evidence = observed.evidence.expect("arm (B) names its evidence");
    assert_eq!(evidence.arm, "observed-gates");
    assert_eq!(evidence.policy_resolution, "present");
    assert_eq!(evidence.policy_event_id.as_deref(), Some(POLICY_EVENT_ID));
    assert_eq!(evidence.policy_not_evaluated, None);

    let founder = land_adapter(founder_request(
        &mission,
        Some(protect(&["require-verdict"])),
    ))
    .expect("the boundary answers");
    let evidence = founder.evidence.expect("arm (A) names its evidence");
    assert_eq!(evidence.arm, "founder");
    assert_eq!(
        evidence.policy_not_evaluated.as_deref(),
        Some("founder_exception"),
        "a founder's landing must never read as verifier-approved"
    );
    assert!(evidence.policy_resolution.is_empty());
    assert_eq!(evidence.policy_event_id, None);
}

#[test]
fn a_pusher_who_is_neither_founder_nor_seat_gets_the_unseated_string() {
    let mission = mission("approve", Some(HEAD_SHA), None);
    let mut request = request(&mission, Some(protect(&["require-verdict"])));
    request.pusher_pubkey = Keys::generate().public_key().to_hex();

    let answer = land_adapter(request).expect("the boundary answers");
    assert!(!answer.admitted);
    assert_eq!(
        answer.refusal_reason.as_deref(),
        Some(
            format!(
                "commit {HEAD_SHA} is cleared on mission {SESSION}, and this key is not an \
                 active seat of it (2 seat(s)). Both routes require the pusher to hold an active \
                 seat of that mission: resume the session so the host re-stages the seat, then \
                 push again."
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
        ref_requires_verdict(
            &protect(&["require-verdict"]),
            None,
            &no_founders(),
            "refs/heads/main"
        ),
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
    assert!(!ref_requires_verdict(
        &[tag],
        None,
        &no_founders(),
        "refs/heads/main"
    ));
}

#[test]
fn the_require_verdict_token_is_read_through_cores_own_pattern_matcher() {
    // A rule on another ref pattern must not govern refs/heads/main.
    let mut tag = vec!["buzz-protect".to_owned(), "refs/heads/release/*".to_owned()];
    tag.push("require-verdict".to_owned());
    assert!(!ref_requires_verdict(
        &[tag],
        None,
        &no_founders(),
        "refs/heads/main"
    ));
    assert!(ref_requires_verdict(
        &protect(&["require-verdict"]),
        None,
        &no_founders(),
        "refs/heads/main"
    ));
}

/// The founder set for a case that reads no rule record: with no records to
/// filter, the set is never consulted, and passing an empty one makes that
/// explicit rather than borrowing a mission's.
fn no_founders() -> buzz_core_pkg::repository_founders::RepositoryFounders {
    buzz_core_pkg::repository_founders::RepositoryFounders::from_parts("", &[])
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
    mission_with(
        decision,
        head_sha,
        Some(branch),
        true,
        (fixed_keys(0x11), fixed_keys(0x22), fixed_keys(0x33)),
    )
}

/// The same fixture as `mission`, with a pinned founder key.
fn fixed_mission(decision: &str, head_sha: Option<&str>) -> Mission {
    mission_with(
        decision,
        head_sha,
        None,
        true,
        (fixed_keys(0x11), fixed_keys(0x22), fixed_keys(0x33)),
    )
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
        "founderPush": land_adapter(founder_request(
            &fixed_mission("changes-requested", Some(HEAD_SHA)),
            Some(protect(&["require-verdict"])),
        ))
        .expect("founder push"),
        "refused": land_adapter(request(&refused, Some(protect(&["require-verdict"]))))
            .expect("refused"),
        "ungoverned": land_adapter(request(&admitted, Some(protect(&["no-force-push"]))))
            .expect("ungoverned"),
        "approvedForAnotherRef": land_adapter(request(
            &fixed_mission_on_branch("approve", Some(HEAD_SHA), "release"),
            Some(protect(&["require-verdict"])),
        ))
        .expect("approved for another ref"),
        "observedGates": land_adapter(observed_gates_request(
            &refused,
            Some(protect(&["require-verdict"])),
        ))
        .expect("observed gates"),
        // L27: the same cleared report, with no gate row naming the commit.
        // The screen must say why a verifier's clearance stopped being enough.
        "verifiedButGatesNotGreen": land_adapter(request_without_gate_rows(
            &admitted,
            Some(protect(&["require-verdict"])),
        ))
        .expect("verified but gates not green"),
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
    // The same mission, refused by arm (C) and admitted by arm (B): what
    // changed is the gate rows, and the arm the evidence names says so.
    assert_eq!(generated_value["observedGates"]["admitted"], json!(true));
    assert_eq!(
        generated_value["observedGates"]["evidence"]["arm"],
        json!("observed-gates")
    );
    // L27: a clearance with no rows is refused, and the sentence names both
    // the verifier who cleared it and the gate list that is owed.
    assert_eq!(
        generated_value["verifiedButGatesNotGreen"]["admitted"],
        json!(false)
    );
    let refusal = generated_value["verifiedButGatesNotGreen"]["refusalReason"]
        .as_str()
        .unwrap_or_default();
    assert!(refusal.starts_with("verifier "), "{refusal}");
    for gate in buzz_core_pkg::coding_session_verdict_admission::DEFAULT_REQUIRED_GATES {
        assert!(refusal.contains(gate), "{refusal}");
    }
    assert_eq!(generated_value["ungoverned"]["ruleGoverns"], json!(false));
    assert_eq!(generated_value["unknown"]["repositoryKnown"], json!(false));
    assert_eq!(
        generated_value["approvedForAnotherRef"]["admitted"],
        json!(false)
    );
}

/// The governance half of these tests — rule records, maintainers, roster —
/// lives in a sibling file so neither passes the 1,000-line ceiling.
#[path = "coding_session_land_founders_tests.rs"]
mod founders_tests;
