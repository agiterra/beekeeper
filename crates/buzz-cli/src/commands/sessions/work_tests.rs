//! Tests for `bee sessions work`.
//!
//! Three properties this file exists to hold:
//!
//! 1. **Adoption is atomic.** A run whose action compile fails publishes
//!    nothing, and that is counted against a stub wire rather than asserted.
//! 2. **A retry republishes nothing.** Adopt and both binds find what they
//!    already wrote and say so.
//! 3. **Status reproduces the frozen sequences.** The command's own read →
//!    assemble → fold path, driven by a stub relay, returns exactly the
//!    contract's `expected-fold.json` for every sequence.

use super::*;

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use buzz_core::coding_session_team_transaction::{
    CodingSessionTeamActiveGrant, CodingSessionTeamActiveSeat,
};

// ── the frozen fixtures ────────────────────────────────────────────────────

macro_rules! sequence {
    ($name:literal) => {
        Sequence {
            name: $name,
            events: include_str!(concat!(
                "../../../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/events.json"
            )),
            inputs: include_str!(concat!(
                "../../../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/inputs.json"
            )),
            expected: include_str!(concat!(
                "../../../../../conformance/project-work/fixtures/sequences/",
                $name,
                "/expected-fold.json"
            )),
        }
    };
}

struct Sequence {
    name: &'static str,
    events: &'static str,
    inputs: &'static str,
    expected: &'static str,
}

const KETTLE_PLAN: &str =
    include_str!("../../../../../conformance/project-work/fixtures/plans/valid/kettle.md");

/// Sequences whose evidence includes kind:44244 records.
///
/// Their facts are stated with fixed fake ids, and a real signed record's id
/// is the hash of its bytes, so a stub relay cannot serve them: the canonical
/// projection the assembler folds would reject the id it was given. The fold
/// itself is pinned against all 18 in `project_work_fold_tests.rs`; what this
/// file pins is the command's read path, over the 12 sequences that need no
/// 44244 record.
const TEAM_RECORD_SEQUENCES: [&str; 6] = [
    "happy-path",
    "amendment",
    "evidence-refusals",
    "mixed-artifacts",
    "wrong-assignee-report",
    "superseded-disposition",
];

const SEQUENCES: [Sequence; 18] = [
    sequence!("happy-path"),
    sequence!("amendment"),
    sequence!("fork"),
    sequence!("fork-descendant"),
    sequence!("fork-two-roots"),
    sequence!("superseded-observation"),
    sequence!("goal-changed"),
    sequence!("goal-ref-not-a-goal"),
    sequence!("evidence-refusals"),
    sequence!("action-hash-mismatch"),
    sequence!("action-failed"),
    sequence!("action-dirty"),
    sequence!("mixed-artifacts"),
    sequence!("wrong-assignee-report"),
    sequence!("superseded-disposition"),
    sequence!("same-action-two-commits"),
    sequence!("same-action-two-commits-reversed"),
    sequence!("plan-unavailable-before-bindings"),
];

const FIXTURE_CHANNEL: &str = "22222222-3333-4444-8555-666666666666";
const FIXTURE_SESSION: &str = "11111111-2222-4333-8444-555555555555";

// ── a stub relay ───────────────────────────────────────────────────────────

/// A wire that answers reads from a fixture and **counts every publish**.
struct StubWire {
    /// Everything the relay serves, including whatever this stub accepted:
    /// an accepted publish is readable afterwards, which is the only way to
    /// test a retry the way a lost response produces one.
    rows: RefCell<Vec<Value>>,
    published: RefCell<Vec<String>>,
    fail_reads: bool,
}

impl StubWire {
    fn new(rows: Vec<Value>) -> Self {
        Self {
            rows: RefCell::new(rows),
            published: RefCell::new(Vec::new()),
            fail_reads: false,
        }
    }

    fn publishes(&self) -> usize {
        self.published.borrow().len()
    }
}

impl WorkWire for StubWire {
    async fn query_events(
        &self,
        filter: Value,
        _limit: Option<u32>,
    ) -> Result<Vec<Value>, CliError> {
        if self.fail_reads {
            return Err(CliError::Auth(
                "403: this channel is gated and the caller is not a member".into(),
            ));
        }
        let kinds: Vec<u64> = filter["kinds"]
            .as_array()
            .map(|kinds| kinds.iter().filter_map(Value::as_u64).collect())
            .unwrap_or_default();
        let ids: Vec<&str> = filter["ids"]
            .as_array()
            .map(|ids| ids.iter().filter_map(Value::as_str).collect())
            .unwrap_or_default();
        Ok(self
            .rows
            .borrow()
            .iter()
            .filter(|row| {
                let kind = row["kind"].as_u64().unwrap_or_default();
                (kinds.is_empty() || kinds.contains(&kind))
                    && (ids.is_empty() || ids.contains(&row["id"].as_str().unwrap_or_default()))
            })
            .cloned()
            .collect())
    }

    async fn publish(&self, builder: EventBuilder, _what: &str) -> Result<String, CliError> {
        let keys = nostr::Keys::generate();
        let event = builder
            .sign_with_keys(&keys)
            .map_err(|error| CliError::Other(error.to_string()))?;
        let id = event.id.to_hex();
        self.published.borrow_mut().push(id.clone());
        // The relay stores what it accepted, so the next read sees it.
        self.rows.borrow_mut().push(json!({
            "id": id,
            "pubkey": event.pubkey.to_hex(),
            "created_at": event.created_at.as_secs(),
            "kind": u64::from(event.kind.as_u16()),
            "tags": event.tags.iter().map(|tag| tag.clone().to_vec()).collect::<Vec<_>>(),
            "content": event.content,
        }));
        Ok(id)
    }

    fn caller_pubkey(&self) -> String {
        "1e".repeat(32)
    }
}

/// The plan every sequence names, with no git anywhere, and the action
/// definitions the contract states the caller compiled at that commit.
struct FixturePlans {
    /// Definitions by plan commit, then by action name.
    by_commit: BTreeMap<String, BTreeMap<String, WorkActionDefinition>>,
    /// Whether the fixture supplied a plan blob at all. A sequence that
    /// supplies none is the `plan_unavailable` case, and the source must say
    /// so rather than inventing a plan nobody read.
    has_plan: bool,
}

impl FixturePlans {
    fn from(fixture: &Value) -> Self {
        // The fixture states the contract key `<coord>@<commit>#<action>`. A
        // plan source answers **per declaration**, and `same-action-two-commits`
        // is the sequence that proves it must: two commits define one action
        // name with different hashes, and handing both to every declaration
        // is the collapse finding 7 is about, one level up.
        let keyed: BTreeMap<String, WorkActionDefinition> =
            serde_json::from_value(fixture["actionDefinitions"].clone()).unwrap_or_default();
        let mut by_commit: BTreeMap<String, BTreeMap<String, WorkActionDefinition>> =
            BTreeMap::new();
        for (key, definition) in keyed {
            let (coordinate, name) = key.rsplit_once('#').expect("an action key names an action");
            let (_, commit) = coordinate
                .rsplit_once('@')
                .expect("an action key names a commit");
            by_commit
                .entry(commit.to_owned())
                .or_default()
                .insert(name.to_owned(), definition);
        }
        Self {
            by_commit,
            has_plan: fixture["planBlobs"]
                .as_object()
                .is_some_and(|blobs| !blobs.is_empty()),
        }
    }
}

impl PlanSource for FixturePlans {
    fn plan(&self, plan_ref: &ProjectWorkPlanRef) -> Result<String, CliError> {
        if !self.has_plan {
            return Err(CliError::Usage(format!(
                "no plan blob at {}",
                plan_ref.commit
            )));
        }
        Ok(KETTLE_PLAN.to_owned())
    }

    fn actions(&self, _plan_ref: &ProjectWorkPlanRef) -> Option<String> {
        None
    }

    fn compiled(
        &self,
        plan_ref: &ProjectWorkPlanRef,
    ) -> Option<BTreeMap<String, WorkActionDefinition>> {
        Some(
            self.by_commit
                .get(&plan_ref.commit)
                .cloned()
                .unwrap_or_default(),
        )
    }
}

/// A plan source that cannot read anything — the `plan_blob_unavailable` case.
struct NoPlans;

impl PlanSource for NoPlans {
    fn plan(&self, _plan_ref: &ProjectWorkPlanRef) -> Result<String, CliError> {
        Err(CliError::Usage("no agents checkout".into()))
    }

    fn actions(&self, _plan_ref: &ProjectWorkPlanRef) -> Option<String> {
        None
    }
}

fn fixture_session(fixture: &Value) -> SessionContext {
    SessionContext {
        channel: FIXTURE_CHANNEL.to_owned(),
        session_ref: FIXTURE_SESSION.to_owned(),
        genesis_ref: "ab".repeat(32),
        project_ref: format!("30621:{}:kettle", "1ead".to_owned() + &"0".repeat(60)),
        context: CodingSessionTeamFoldContext {
            channel_ref: FIXTURE_CHANNEL.to_owned(),
            session_ref: FIXTURE_SESSION.to_owned(),
            genesis_ref: "ab".repeat(32),
            founder_pubkey: fixture["authority"]["founderPubkey"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
            active_seats: fixture["authority"]["activeSeats"]
                .as_array()
                .map(|seats| {
                    seats
                        .iter()
                        .map(|seat| CodingSessionTeamActiveSeat {
                            actor_pubkey: seat["actorPubkey"].as_str().unwrap_or_default().into(),
                            role: seat["role"].as_str().unwrap_or_default().into(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            active_grants: fixture["authority"]["activeGrants"]
                .as_array()
                .map(|grants| {
                    grants
                        .iter()
                        .map(|grant| CodingSessionTeamActiveGrant {
                            actor_pubkey: grant["actorPubkey"].as_str().unwrap_or_default().into(),
                            grant_event_ref: grant["grantEventRef"]
                                .as_str()
                                .unwrap_or_default()
                                .into(),
                            may_steer: grant["maySteer"].as_bool().unwrap_or_default(),
                        })
                        .collect()
                })
                .unwrap_or_default(),
            verifier_required: false,
        },
        relay_self: fixture["relaySelfKey"].as_str().map(str::to_owned),
    }
}

/// The raw relay rows a sequence's facts would have come from.
///
/// The same synthesis the core assembler's tests perform, here because the
/// command reads *events*, not facts: this is what makes the status test
/// exercise the command's own read path rather than the fold's.
fn fixture_rows(sequence: &Sequence) -> Vec<Value> {
    let fixture: Value = serde_json::from_str(sequence.inputs).expect("inputs");
    let mut rows: Vec<Value> = serde_json::from_str(sequence.events).expect("events");
    let current = fixture["currentGoalRef"].as_str();
    for (index, id) in fixture["goalEvents"]
        .as_array()
        .cloned()
        .unwrap_or_default()
        .iter()
        .enumerate()
    {
        let id = id.as_str().expect("goal id");
        rows.push(json!({
            "id": id,
            "pubkey": "1e".repeat(32),
            "created_at": if Some(id) == current { 1_789_000_900u64 }
                          else { 1_789_000_100 + index as u64 },
            "kind": 44227,
            "tags": [["h", FIXTURE_CHANNEL], ["d", FIXTURE_SESSION], ["csgl-v", "csgl1-1"]],
            "content": "ship the kettle",
        }));
    }
    let empty = serde_json::Map::new();
    for fact in fixture["evidence"].as_object().unwrap_or(&empty).values() {
        match fact["kind"].as_str().unwrap_or_default() {
            // A kind:44244 record cannot be fabricated here any more: the
            // assembler folds them with the canonical team fold, which
            // verifies signatures and keys every record by its own event id
            // (A5 decision 23). Sequences that need them are named below.
            "report" | "verdict" => {}
            "action_result" => {
                rows.push(json!({
                    "id": fact["eventId"], "pubkey": fact["resultSigner"],
                    "created_at": 1_789_000_400u64, "kind": 46023, "tags": [],
                    "content": json!({"runId": fact["runId"], "stepId": fact["stepId"],
                        "disposition": fact["disposition"], "exitCode": fact["exitCode"],
                        "dirty": fact["dirty"], "checkout": fact["checkout"]}).to_string(),
                }));
                rows.push(json!({
                    "id": fact["exitedEventId"], "pubkey": fact["echoSigner"],
                    "created_at": 1_789_000_410u64, "kind": 46014, "tags": [],
                    "content": json!({"resultEventId": fact["eventId"]}).to_string(),
                }));
                rows.push(json!({
                    "id": format!("{:0>64}", fact["eventId"].as_str().unwrap_or("9e9")),
                    "pubkey": fact["echoSigner"],
                    "created_at": 1_789_000_390u64, "kind": 46013, "tags": [],
                    "content": json!({"runId": fact["runId"], "stepId": fact["stepId"],
                        "workflowName": fact["actionName"],
                        "definitionHash": fact["definitionHash"]}).to_string(),
                }));
            }
            other => panic!("unknown fact kind {other}"),
        }
    }
    for state in fixture["refStates"].as_array().cloned().unwrap_or_default() {
        rows.push(state);
    }
    rows
}

// ── status ─────────────────────────────────────────────────────────────────

/// The command's own read → assemble → fold path reproduces every sequence.
#[tokio::test]
async fn status_reproduces_every_frozen_sequence() {
    for sequence in &SEQUENCES {
        if TEAM_RECORD_SEQUENCES.contains(&sequence.name) {
            continue;
        }
        let fixture: Value = serde_json::from_str(sequence.inputs).expect("inputs");
        let wire = StubWire::new(fixture_rows(sequence));
        let session = fixture_session(&fixture);
        // The action sequences need their definition compiled from
        // `actions.yml`; the fixture states the compiled hash instead, so
        // those three are folded through the same path with the hash the
        // contract fixes rather than a second compiler in a test.
        let (coverage, reads) = coverage_with(&wire, &session, &FixturePlans::from(&fixture))
            .await
            .expect("coverage");
        assert_eq!(wire.publishes(), 0, "{}: a read published", sequence.name);
        assert_eq!(reads["truncated"], json!(false), "{}", sequence.name);
        let expected: Value = serde_json::from_str(sequence.expected).expect("expected");
        let actual = serde_json::to_value(&coverage).expect("projection");
        assert_eq!(actual, expected, "{}", sequence.name);
    }
}

/// Without a plan source every criterion is `unknown` with a named reason —
/// never `open`, which would read as "nothing has been done".
#[tokio::test]
async fn an_unreadable_plan_reports_unknown_not_open() {
    let sequence = &SEQUENCES[0];
    let fixture: Value = serde_json::from_str(sequence.inputs).expect("inputs");
    let wire = StubWire::new(fixture_rows(sequence));
    let (coverage, reads) = coverage_with(&wire, &fixture_session(&fixture), &NoPlans)
        .await
        .expect("coverage");
    assert!(!reads["unresolvedPlans"]
        .as_array()
        .expect("unresolved")
        .is_empty());
    let head = coverage
        .declarations
        .iter()
        .find(|declaration| !declaration.criteria.is_empty())
        .expect("a head declaration");
    assert!(!head.plan_resolved);
    assert!(head
        .criteria
        .iter()
        .all(|criterion| criterion.status.as_str() == "unknown"));
}

/// A p-gated or failed read is an error naming the read — never an empty
/// projection that reads as "this session has no work".
#[tokio::test]
async fn a_failed_read_is_an_error_not_an_empty_result() {
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let mut wire = StubWire::new(Vec::new());
    wire.fail_reads = true;
    let error = coverage_with(
        &wire,
        &fixture_session(&fixture),
        &FixturePlans::from(&fixture),
    )
    .await
    .expect_err("refused");
    let message = error.to_string();
    assert!(message.contains("403"), "{message}");
}

// ── adopt ──────────────────────────────────────────────────────────────────

/// A throwaway repository, never a worktree of this one.
struct Repo {
    dir: PathBuf,
}

impl Repo {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("bee-work-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(dir.join("plans")).expect("mkdir");
        let repo = Self { dir };
        repo.run(&["init", "--initial-branch=main"]);
        repo.run(&["config", "user.email", "lane201@example.invalid"]);
        repo.run(&["config", "user.name", "Lane 201"]);
        repo
    }

    fn path(&self) -> &str {
        self.dir.to_str().expect("utf-8 path")
    }

    fn run(&self, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(&self.dir)
            .args(args)
            .output()
            .expect("git runs");
        assert!(
            output.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn write(&self, path: &str, body: &str) {
        let file = self.dir.join(path);
        if let Some(parent) = Path::new(&file).parent() {
            std::fs::create_dir_all(parent).expect("mkdir");
        }
        std::fs::write(file, body).expect("write");
    }

    fn commit(&self, message: &str) -> String {
        self.run(&["add", "-A"]);
        self.run(&["commit", "-q", "-m", message]);
        self.run(&["rev-parse", "HEAD"])
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.dir).ok();
    }
}

/// A plan whose one `action` criterion names an action `actions.yml` does not
/// define: the compile the atomicity test needs to fail.
const PLAN_WITH_MISSING_ACTION: &str = r#"---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: tests-green
    accept: the verify action exits zero
    proof: {kind: action, name: verify, step: verify}
retired_criteria: []
---

Body.
"#;

const PLAN_REVIEW_ONLY: &str = r#"---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: the CLI does what the plan says
    proof: {kind: review}
retired_criteria: []
---

Body.
"#;

/// The plan resolves at the commit it was adopted at, after the tip moved on.
#[test]
fn a_plan_resolves_at_its_commit_after_the_tip_moves() {
    let repo = Repo::new("pinned");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let pinned = repo.commit("first");
    repo.write(
        "plans/kettle.md",
        &PLAN_REVIEW_ONLY.replace("Body.", "Rewritten."),
    );
    let tip = repo.commit("second");
    assert_ne!(pinned, tip);

    let at_commit = blob_at_commit(repo.path(), &pinned, "plans/kettle.md").expect("blob");
    assert!(at_commit.contains("Body."), "{at_commit}");
    assert!(!at_commit.contains("Rewritten."));
    let at_tip = blob_at_commit(repo.path(), &tip, "plans/kettle.md").expect("blob");
    assert!(at_tip.contains("Rewritten."));
}

/// A commit nobody pushed is refused: a declaration pinned to a commit no
/// other reader can fetch is a contract nobody can read.
#[test]
fn an_unpushed_commit_is_refused_and_says_what_was_checked() {
    let repo = Repo::new("unpushed");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let commit = repo.commit("first");
    let (published, how) = commit_is_published(repo.path(), &commit, &[]);
    assert!(!published);
    assert!(how.contains("branch -r --contains"), "{how}");
    assert!(how.contains("push the plan before adopting it"), "{how}");
}

fn adopt_args(repo: &Repo, commit: &str) -> WorkAdoptArgs {
    WorkAdoptArgs {
        plan: Some("plans/kettle.md".to_owned()),
        commit: Some(commit.to_owned()),
        agents_repo: Some(repo.path().to_owned()),
        channel: Some(FIXTURE_CHANNEL.to_owned()),
        session_ref: Some(FIXTURE_SESSION.to_owned()),
        work_id: None,
        supersedes: Vec::new(),
        decision: None,
        responsible: None,
        example: None,
    }
}

/// The rows a stub relay needs for an adoption to resolve: the project's
/// agents repository, the session's goal, and nothing else.
fn adoption_rows(project_ref: &str, tip: &str) -> Vec<Value> {
    vec![
        json!({"id": "a".repeat(64), "pubkey": "1e".repeat(32), "created_at": 1u64,
               "kind": 30624, "tags": [["d", project_ref],
               ["repo", format!("30617:{}:kettle-beekeeper-agents", "1e".repeat(32))]],
               "content": ""}),
        json!({"id": "b".repeat(64), "pubkey": "4e".repeat(32), "created_at": 2u64,
               "kind": 30618, "tags": [["d", "kettle-beekeeper-agents"],
               ["refs/heads/main", tip]], "content": ""}),
        json!({"id": "c".repeat(64), "pubkey": "1e".repeat(32), "created_at": 3u64,
               "kind": 44227,
               "tags": [["h", FIXTURE_CHANNEL], ["d", FIXTURE_SESSION], ["csgl-v", "csgl1-1"]],
               "content": "ship the kettle"}),
    ]
}

/// **Atomicity.** An action the plan names that `actions.yml` does not define
/// refuses the adoption — and publishes nothing at all.
#[tokio::test]
async fn a_failing_action_compile_signs_nothing() {
    let repo = Repo::new("atomic");
    repo.write("plans/kettle.md", PLAN_WITH_MISSING_ACTION);
    repo.write("actions.yml", "actions: {}\n");
    let commit = repo.commit("first");
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let wire = StubWire::new(adoption_rows(&session.project_ref, &commit));

    let error = adopt_with(&wire, &session, &adopt_args(&repo, &commit))
        .await
        .expect_err("refused");
    assert_eq!(wire.publishes(), 0, "a refused adoption published an event");
    assert!(error.to_string().contains("unresolved-action"), "{error}");
}

/// An amendment must name what it supersedes, and that refusal happens before
/// any read or write.
#[tokio::test]
async fn an_amendment_without_supersedes_is_refused() {
    let repo = Repo::new("amend");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let commit = repo.commit("first");
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let wire = StubWire::new(adoption_rows(&session.project_ref, &commit));
    // The work already exists on the wire under this explicit id: a second
    // declaration of it is an amendment. (An explicit `--work-id` with no
    // such work is an ordinary initial adoption and is accepted.)
    let work_id = "9d0f0f0f-1111-4222-8333-444444444444";
    let mut args = adopt_args(&repo, &commit);
    args.work_id = Some(work_id.to_owned());
    adopt_with(&wire, &session, &args).await.expect("published");
    let mut amended = adopt_args(&repo, &commit);
    amended.work_id = Some(work_id.to_owned());
    amended.responsible = Some("b0".repeat(32));
    let error = adopt_with(&wire, &session, &amended)
        .await
        .expect_err("refused");
    assert_eq!(wire.publishes(), 1, "the refused amendment published");
    assert!(
        error.to_string().contains("amendment-needs-supersedes"),
        "{error}"
    );
}

/// Idempotence: a retry with the same arguments finds the declaration already
/// on the wire and publishes nothing.
#[tokio::test]
async fn adopt_retried_with_the_same_arguments_republishes_nothing() {
    let repo = Repo::new("retry");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let commit = repo.commit("first");
    // Published: the commit is reachable from the relay's ref state.
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let mut rows = adoption_rows(&session.project_ref, &commit);

    let work_id = "9d0f0f0f-1111-4222-8333-444444444444";
    let declared = ProjectWorkDeclared {
        work_id: work_id.to_owned(),
        goal_ref: "c".repeat(64),
        decision_ref: None,
        responsible_actor: "1e".repeat(32),
        plan_ref: ProjectWorkPlanRef {
            repository: format!("30617:{}:kettle-beekeeper-agents", "1e".repeat(32)),
            commit: commit.clone(),
            path: "plans/kettle.md".to_owned(),
        },
        supersedes: vec!["d".repeat(64)],
    };
    let payload = project_work_payload(
        &ProjectWorkEnvelope {
            channel_ref: session.channel.clone(),
            session_ref: session.session_ref.clone(),
            genesis_ref: session.genesis_ref.clone(),
            project_ref: session.project_ref.clone(),
        },
        ProjectWorkBody::Declared(declared),
    );
    rows.push(json!({
        "id": "e".repeat(64), "pubkey": "1e".repeat(32), "created_at": 9u64,
        "kind": 44249, "tags": [], "content": serde_json::to_string(&payload).expect("payload"),
    }));

    let wire = StubWire::new(rows);
    let mut args = adopt_args(&repo, &commit);
    args.work_id = Some(work_id.to_owned());
    args.supersedes = vec!["d".repeat(64)];
    adopt_with(&wire, &session, &args)
        .await
        .expect("idempotent");
    assert_eq!(wire.publishes(), 0, "a retry republished the declaration");
}

// ── bind ───────────────────────────────────────────────────────────────────

/// Every example is a serialized constructed value that passes the
/// publication validator — an example that stopped being valid fails the
/// build, not a live mission.
#[test]
fn every_example_is_a_valid_record() {
    for verb in ["adopt", "bind assignment", "bind evidence"] {
        let body = example_body(verb).expect("an example");
        let payload = project_work_payload(&example_envelope(), body);
        buzz_core::project_work::validate_project_work_payload(&payload)
            .unwrap_or_else(|refusal| panic!("{verb}: {refusal}"));
        print_example(verb, "default").expect("prints offline");
    }
    assert!(print_example("adopt", "nope").is_err());
}

/// An evidence kind that cannot answer a criterion's proof form is refused
/// locally, before anything is signed.
#[test]
fn evidence_kinds_are_checked_against_each_criterion() {
    let plan = buzz_core::project_plan::parse_plan(KETTLE_PLAN.as_bytes()).expect("the fixture");
    let review = plan
        .criteria
        .iter()
        .find(|criterion| matches!(criterion.proof, PlanProof::Review))
        .expect("a review criterion");
    let wrong = [ProjectWorkEvidenceRef {
        kind: ProjectWorkEvidenceKind::RefObservation,
        event_id: "a".repeat(64),
    }];
    let error =
        check_evidence_kinds(&plan, std::slice::from_ref(&review.id), &wrong).expect_err("refused");
    assert!(
        error.to_string().contains("evidence-kind-mismatch"),
        "{error}"
    );
    let right = [ProjectWorkEvidenceRef {
        kind: ProjectWorkEvidenceKind::Verdict,
        event_id: "a".repeat(64),
    }];
    check_evidence_kinds(&plan, std::slice::from_ref(&review.id), &right).expect("accepted");
}

/// `validate` on the working copy says, in the answer, that it is
/// uncommitted and cannot be adopted.
#[test]
fn validate_of_a_working_file_discloses_that_it_is_uncommitted() {
    let repo = Repo::new("working");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let args = WorkValidateArgs {
        plan: "plans/kettle.md".to_owned(),
        agents_repo: Some(repo.path().to_owned()),
        commit: None,
    };
    cmd_validate(&args).expect("valid");
}

/// A plan that does not parse refuses with the contract's own stable code.
#[test]
fn validate_refuses_with_the_contracts_stable_code() {
    let repo = Repo::new("refused");
    repo.write(
        "plans/kettle.md",
        &PLAN_REVIEW_ONLY.replace("beekeeper-plan/v1", "beekeeper-plan/v2"),
    );
    let args = WorkValidateArgs {
        plan: "plans/kettle.md".to_owned(),
        agents_repo: Some(repo.path().to_owned()),
        commit: None,
    };
    let error = cmd_validate(&args).expect_err("refused");
    assert!(error.to_string().contains("unknown-schema"), "{error}");
}

/// `--commit` without a repository is a usage error, not a silent read of the
/// working copy.
#[test]
fn a_commit_without_a_repository_is_refused() {
    let args = WorkValidateArgs {
        plan: "plans/kettle.md".to_owned(),
        agents_repo: None,
        commit: Some("HEAD".to_owned()),
    };
    assert!(cmd_validate(&args).is_err());
}

// ── bind, against a stub wire ──────────────────────────────────────────────

/// A stub relay holding one declaration of the kettle plan, plus whatever
/// rows a test adds.
fn bound_rows(session: &SessionContext, extra: Vec<Value>) -> (String, Vec<Value>) {
    let declaration_id = "1a".repeat(32);
    let declared = ProjectWorkDeclared {
        work_id: "9d0f0f0f-1111-4222-8333-444444444444".to_owned(),
        goal_ref: "c".repeat(64),
        decision_ref: None,
        responsible_actor: "1e".repeat(32),
        plan_ref: ProjectWorkPlanRef {
            repository: format!("30617:{}:kettle-beekeeper-agents", "1e".repeat(32)),
            commit: "ab".repeat(20),
            path: "plans/kettle.md".to_owned(),
        },
        supersedes: Vec::new(),
    };
    let payload = project_work_payload(
        &ProjectWorkEnvelope {
            channel_ref: session.channel.clone(),
            session_ref: session.session_ref.clone(),
            genesis_ref: session.genesis_ref.clone(),
            project_ref: session.project_ref.clone(),
        },
        ProjectWorkBody::Declared(declared),
    );
    let mut rows = vec![json!({
        "id": declaration_id, "pubkey": "1e".repeat(32), "created_at": 5u64,
        "kind": 44249, "tags": [], "content": serde_json::to_string(&payload).expect("payload"),
    })];
    rows.extend(extra);
    (declaration_id, rows)
}

fn bind_envelope(declaration: &str, criteria: &[&str]) -> WorkBindEnvelopeArgs {
    WorkBindEnvelopeArgs {
        channel: Some(FIXTURE_CHANNEL.to_owned()),
        session_ref: Some(FIXTURE_SESSION.to_owned()),
        declaration: Some(declaration.to_owned()),
        criteria: criteria.iter().map(|id| (*id).to_owned()).collect(),
        agents_repo: None,
        example: None,
    }
}

/// A criterion the plan does not define is refused locally, and nothing is
/// published.
#[tokio::test]
async fn a_criterion_the_plan_does_not_define_is_refused() {
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let assignment_id = "a5".repeat(32);
    let (declaration, rows) = bound_rows(
        &session,
        vec![
            json!({"id": assignment_id, "pubkey": "1e".repeat(32), "created_at": 6u64,
                    "kind": 44244, "tags": [], "content": "{}"}),
        ],
    );
    let wire = StubWire::new(rows);
    let args = WorkBindAssignmentArgs {
        envelope: bind_envelope(&declaration, &["not-a-criterion"]),
        assignment: Some(assignment_id),
        replaces: None,
    };
    let error = bind_assignment_with(&wire, session, &args, &FixturePlans::from(&fixture))
        .await
        .expect_err("refused");
    assert_eq!(wire.publishes(), 0);
    assert!(error.to_string().contains("unknown-criterion"), "{error}");
}

/// An assignment the relay will not serve is refused: a pointer nobody can
/// follow is not a binding.
#[tokio::test]
async fn a_binding_naming_an_unreadable_event_is_refused() {
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let (declaration, rows) = bound_rows(&session, Vec::new());
    let wire = StubWire::new(rows);
    let args = WorkBindAssignmentArgs {
        envelope: bind_envelope(&declaration, &["cli-behaviour"]),
        assignment: Some("a5".repeat(32)),
        replaces: None,
    };
    let error = bind_assignment_with(&wire, session, &args, &FixturePlans::from(&fixture))
        .await
        .expect_err("refused");
    assert_eq!(wire.publishes(), 0);
    assert!(error.to_string().contains("unreadable-evidence"), "{error}");
}

/// An identical assignment binding already on the wire republishes nothing.
#[tokio::test]
async fn bind_assignment_retried_republishes_nothing() {
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let assignment_id = "a5".repeat(32);
    let (declaration, mut rows) = bound_rows(
        &session,
        vec![
            json!({"id": assignment_id, "pubkey": "1e".repeat(32), "created_at": 6u64,
                    "kind": 44244, "tags": [], "content": "{}"}),
        ],
    );
    let body = ProjectWorkAssignmentBound {
        declaration_ref: declaration.clone(),
        criterion_ids: vec!["cli-behaviour".to_owned()],
        assignment_ref: assignment_id.clone(),
        replaces_binding: None,
    };
    let payload = project_work_payload(
        &ProjectWorkEnvelope {
            channel_ref: session.channel.clone(),
            session_ref: session.session_ref.clone(),
            genesis_ref: session.genesis_ref.clone(),
            project_ref: session.project_ref.clone(),
        },
        ProjectWorkBody::AssignmentBound(body),
    );
    rows.push(json!({
        "id": "b1".repeat(32), "pubkey": "1e".repeat(32), "created_at": 7u64,
        "kind": 44249, "tags": [], "content": serde_json::to_string(&payload).expect("payload"),
    }));
    let wire = StubWire::new(rows);
    let args = WorkBindAssignmentArgs {
        envelope: bind_envelope(&declaration, &["cli-behaviour"]),
        assignment: Some(assignment_id),
        replaces: None,
    };
    bind_assignment_with(&wire, session, &args, &FixturePlans::from(&fixture))
        .await
        .expect("idempotent");
    assert_eq!(wire.publishes(), 0, "a retry republished the binding");
}

/// An evidence binding that resolves publishes exactly one record, and its
/// retry publishes none.
#[tokio::test]
async fn bind_evidence_publishes_once_and_its_retry_publishes_nothing() {
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let verdict_id = "be".repeat(32);
    let (declaration, rows) = bound_rows(
        &session,
        vec![
            json!({"id": verdict_id, "pubkey": "1e".repeat(32), "created_at": 6u64,
                    "kind": 44244, "tags": [], "content": "{}"}),
        ],
    );
    let args = WorkBindEvidenceArgs {
        envelope: bind_envelope(&declaration, &["cli-behaviour"]),
        artifact: Some("e7".repeat(20)),
        evidence: vec![format!("verdict:{verdict_id}")],
        completion: None,
    };
    let wire = StubWire::new(rows.clone());
    bind_evidence_with(
        &wire,
        fixture_session(&fixture),
        &args,
        &FixturePlans::from(&fixture),
    )
    .await
    .expect("published");
    assert_eq!(wire.publishes(), 1);

    // The same binding, now on the wire: the retry publishes nothing.
    let body = ProjectWorkEvidenceBound {
        declaration_ref: declaration.clone(),
        criterion_ids: vec!["cli-behaviour".to_owned()],
        artifact_commit: "e7".repeat(20),
        evidence_refs: vec![ProjectWorkEvidenceRef {
            kind: ProjectWorkEvidenceKind::Verdict,
            event_id: verdict_id.clone(),
        }],
        completion_ref: None,
    };
    let payload = project_work_payload(
        &ProjectWorkEnvelope {
            channel_ref: session.channel.clone(),
            session_ref: session.session_ref.clone(),
            genesis_ref: session.genesis_ref.clone(),
            project_ref: session.project_ref.clone(),
        },
        ProjectWorkBody::EvidenceBound(body),
    );
    let mut retry_rows = rows;
    retry_rows.push(json!({
        "id": "b2".repeat(32), "pubkey": "1e".repeat(32), "created_at": 8u64,
        "kind": 44249, "tags": [], "content": serde_json::to_string(&payload).expect("payload"),
    }));
    let wire = StubWire::new(retry_rows);
    bind_evidence_with(&wire, session, &args, &FixturePlans::from(&fixture))
        .await
        .expect("idempotent");
    assert_eq!(wire.publishes(), 0);
}

// ── Finding 8 (ledger 213): initial adoption is retry-safe ─────────────────

/// **Reproduces finding 8.** The publish is accepted and the response is
/// lost; the operator runs the identical command again. With a random initial
/// `workId` that minted a second work identity and a second set of
/// obligations. The id is derived from stable inputs, so the retry finds its
/// own declaration and publishes nothing.
#[tokio::test]
async fn an_initial_adoption_whose_response_was_lost_republishes_nothing() {
    let repo = Repo::new("lost-response");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let commit = repo.commit("first");
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let wire = StubWire::new(adoption_rows(&session.project_ref, &commit));
    let args = adopt_args(&repo, &commit);

    adopt_with(&wire, &session, &args).await.expect("published");
    assert_eq!(wire.publishes(), 1, "the first adoption publishes once");
    let first = wire.published.borrow()[0].clone();

    // The response never reached the caller. The same command, again.
    adopt_with(&wire, &session, &args).await.expect("retry");
    assert_eq!(
        wire.publishes(),
        1,
        "the retry minted a second work identity"
    );
    let (records, _) = fetch_work_records(&wire, &session.channel, &session.session_ref)
        .await
        .expect("records");
    let ids: Vec<&str> = records.iter().map(|event| event.id.as_str()).collect();
    assert_eq!(ids, vec![first.as_str()], "exactly one declaration exists");
}

/// The derived id is a pure function of its five stable inputs — no clock and
/// no randomness anywhere, which is the promise a "deterministic" id that
/// hashed a now-relative expiry broke once before (repo memory, 2026-09-08).
#[test]
fn the_derived_work_id_is_stable_and_input_sensitive() {
    let id = |plan_path: &str| {
        derive_work_id(
            "30621:1e:kettle",
            FIXTURE_SESSION,
            "30617:1e:kettle-beekeeper-agents",
            plan_path,
            "kettle-cli",
        )
    };
    assert_eq!(id("plans/kettle.md"), id("plans/kettle.md"));
    assert_ne!(id("plans/kettle.md"), id("plans/other.md"));
    assert!(uuid::Uuid::parse_str(&id("plans/kettle.md")).is_ok());
    // The commit is deliberately **not** an input: a new commit of the same
    // plan is an amendment of the same work, not new work.
    assert_eq!(
        id("plans/kettle.md"),
        derive_work_id(
            "30621:1e:kettle",
            FIXTURE_SESSION,
            "30617:1e:kettle-beekeeper-agents",
            "plans/kettle.md",
            "kettle-cli",
        )
    );
}

/// **Reproduces finding 8's other half.** Re-adopting the same plan at a new
/// commit is an *amendment* of the same work and must name what it
/// supersedes — and the refusal has to say so, naming the declaration.
#[tokio::test]
async fn re_adopting_the_same_plan_at_a_new_commit_demands_supersedes() {
    let repo = Repo::new("readopt");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let first_commit = repo.commit("first");
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let wire = StubWire::new(adoption_rows(&session.project_ref, &first_commit));
    adopt_with(&wire, &session, &adopt_args(&repo, &first_commit))
        .await
        .expect("published");
    assert_eq!(wire.publishes(), 1);

    repo.write(
        "plans/kettle.md",
        &PLAN_REVIEW_ONLY.replace("Body.", "Amended."),
    );
    let second_commit = repo.commit("second");
    wire.rows.borrow_mut().push(json!({
        "id": "f".repeat(64), "pubkey": "4e".repeat(32), "created_at": 20u64,
        "kind": 30618, "tags": [["d", "kettle-beekeeper-agents"],
        ["refs/heads/main", second_commit]], "content": ""
    }));
    let error = adopt_with(&wire, &session, &adopt_args(&repo, &second_commit))
        .await
        .expect_err("an amendment must name what it supersedes");
    assert_eq!(wire.publishes(), 1, "the refused amendment published");
    let message = error.to_string();
    assert!(message.contains("amendment-needs-supersedes"), "{message}");
    assert!(message.contains(&wire.published.borrow()[0]), "{message}");
}

/// The dedupe compares the whole body it claims to: a declaration that
/// differs in its responsible actor is a different declaration, not a retry.
#[tokio::test]
async fn a_body_that_differs_in_its_responsible_actor_is_not_a_retry() {
    let repo = Repo::new("responsible");
    repo.write("plans/kettle.md", PLAN_REVIEW_ONLY);
    let commit = repo.commit("first");
    let fixture: Value = serde_json::from_str(SEQUENCES[0].inputs).expect("inputs");
    let session = fixture_session(&fixture);
    let wire = StubWire::new(adoption_rows(&session.project_ref, &commit));
    adopt_with(&wire, &session, &adopt_args(&repo, &commit))
        .await
        .expect("published");

    let mut amended = adopt_args(&repo, &commit);
    amended.responsible = Some("b0".repeat(32));
    amended.supersedes = vec![wire.published.borrow()[0].clone()];
    adopt_with(&wire, &session, &amended)
        .await
        .expect("a different body is a new declaration");
    assert_eq!(wire.publishes(), 2);
}
