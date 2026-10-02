//! Lane 233: what a plan *edited after adoption* actually does to the
//! declaration that pinned it.
//!
//! The claim under test was that editing and committing a declared plan makes
//! the declaration "stale relative to head", that the completion gate then
//! refuses on that account, and that `status` discloses head against the
//! declared commit. These tests hold the behaviour the code actually has, so
//! the difference is a failing assertion here the day someone builds the
//! head-drift check rather than a sentence in a report.
//!
//! Two paths produce the second commit — plain `git commit`, and the
//! `bee agents-repo commit` git half (`commit_drafts`, kind:44250's landing
//! step) against a throwaway bare remote. Neither is a relay call, so both
//! run offline.

use super::*;

use buzz_core::project_work_fold::{WorkDeclarationProjection, WorkDeclarationState};

use crate::commands::agents_repo_git::{
    commit_drafts, AssetBytes, CommitOutcome, CommitRequest, DraftChange, Identity,
};

const OWNER: &str = "1ead000000000000000000000000000000000000000000000000000000000000";
const AGENTS_REPO_ID: &str = "kettle-beekeeper-agents";
const GOAL_ID: &str = "a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0a0";
const PLAN_PATH: &str = "plans/kettle.md";

/// The one criterion id the *edited* plan adds. A projection that names it is
/// reading the plan at head; a projection that does not is reading the plan at
/// the commit the declaration pinned, which is the contract.
const ADDED_CRITERION: &str = "criterion-added-after-adoption";

fn project_ref() -> String {
    format!("30621:{OWNER}:kettle")
}

fn repo_coordinate() -> String {
    format!("30617:{OWNER}:{AGENTS_REPO_ID}")
}

/// The plan this session adopts, at commit A.
///
/// Written here rather than reused from the frozen fixtures because those
/// carry an `action` criterion, and an action must resolve in `actions.yml` at
/// the same commit or adoption refuses — a second moving part this test is
/// not about. Every proof here is a `review`, so the only thing that changes
/// between the two commits is the contract's own content.
const BASE_PLAN: &str = "\
---
schema: beekeeper-plan/v1
id: kettle-cli
status: in-force
title: Build and land kettle
code_repository: pivot-test
delivery_ref: refs/heads/main
criteria:
  - id: cli-behaviour
    accept: python3 -m kettle add, list and done behave as specified.
    proof: {kind: review}
  - id: usage-documentation
    accept: README has a Usage section showing add, list and done.
    proof: {kind: review}
retired_criteria: []
---
# Kettle

Build the CLI under `kettle/` with tests under `tests/`.
";

/// The same plan with one more `review` criterion — still adoptable.
fn edited_plan() -> String {
    let marker = "retired_criteria: []";
    let added = format!(
        "  - id: {ADDED_CRITERION}\n    accept: Something the lead added after adoption.\n    \
         proof: {{kind: review}}\n{marker}"
    );
    assert!(
        BASE_PLAN.contains(marker),
        "the plan no longer ends its criteria at {marker:?}"
    );
    BASE_PLAN.replacen(marker, &added, 1)
}

fn work_session() -> SessionContext {
    SessionContext {
        channel: FIXTURE_CHANNEL.to_owned(),
        session_ref: FIXTURE_SESSION.to_owned(),
        genesis_ref: "ab".repeat(32),
        project_ref: project_ref(),
        context: CodingSessionTeamFoldContext {
            channel_ref: FIXTURE_CHANNEL.to_owned(),
            session_ref: FIXTURE_SESSION.to_owned(),
            genesis_ref: "ab".repeat(32),
            founder_pubkey: "1e".repeat(32),
            active_seats: Vec::new(),
            active_grants: Vec::new(),
            verifier_required: false,
        },
        // Ledger 235: the fold only believes a **relay-signed** kind:30618,
        // so a harness with no relay key sees no branch tip and every row
        // folds to `planDrift: unknown` — which is honest, and which is
        // exactly why this file's tripwire never tripped before (refuter,
        // 2026-09-22). These tests now supply the key and sign the row with
        // it, so they assert what A10 actually promises.
        relay_self: Some(OWNER.to_owned()),
    }
}

/// The rows a relay would serve: the project's agents repository, the ref
/// state that proves a commit is fetchable, and the session's goal.
fn wire_rows(tip: &str) -> Vec<Value> {
    vec![
        json!({
            "id": "11".repeat(32), "pubkey": OWNER, "created_at": 1_789_000_000u64,
            "kind": KIND_PROJECT_PACK_SOURCE,
            "tags": [["d", project_ref()], ["repo", repo_coordinate()]],
            "content": "",
        }),
        json!({
            "id": "22".repeat(32), "pubkey": OWNER, "created_at": 1_789_000_010u64,
            "kind": KIND_GIT_REPO_STATE,
            "tags": [["d", AGENTS_REPO_ID], ["refs/heads/main", tip]],
            "content": "",
        }),
        json!({
            "id": GOAL_ID, "pubkey": "1e".repeat(32), "created_at": 1_789_000_900u64,
            "kind": KIND_CODING_SESSION_GOAL,
            "tags": [["h", FIXTURE_CHANNEL], ["d", FIXTURE_SESSION], ["csgl-v", "csgl1-1"]],
            "content": "ship the kettle",
        }),
    ]
}

fn adopt_args(dir: &Path, commit: &str, supersedes: Vec<String>) -> WorkAdoptArgs {
    WorkAdoptArgs {
        plan: Some(PLAN_PATH.to_owned()),
        commit: Some(commit.to_owned()),
        agents_repo: Some(dir.to_string_lossy().into_owned()),
        channel: Some(FIXTURE_CHANNEL.to_owned()),
        session_ref: Some(FIXTURE_SESSION.to_owned()),
        work_id: None,
        supersedes,
        decision: None,
        responsible: Some("1e".repeat(32)),
        example: None,
    }
}

// ── a git repository, with no relay anywhere ───────────────────────────────

fn git_in(cwd: &Path, args: &[&str]) -> String {
    let output = crate::commands::sessions::worktree::git_command(cwd)
        .args(args)
        .env("GIT_AUTHOR_NAME", "lane233")
        .env("GIT_AUTHOR_EMAIL", "lane233@test")
        .env("GIT_COMMITTER_NAME", "lane233")
        .env("GIT_COMMITTER_EMAIL", "lane233@test")
        .output()
        .unwrap_or_else(|error| panic!("git {args:?}: {error}"));
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A bare remote plus a clone of it, so `git branch -r --contains` can answer
/// `commit_is_published` without a relay.
struct Repo {
    root: PathBuf,
    remote: String,
    work: PathBuf,
}

impl Repo {
    /// A remote whose `main` holds `plans/kettle.md`, and a clone of it.
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!("bee-lane233-{}", uuid::Uuid::new_v4()));
        let remote_dir = root.join("remote.git");
        let seed = root.join("seed");
        std::fs::create_dir_all(&seed).expect("mkdir");
        git_in(
            &root,
            &[
                "init",
                "--bare",
                "--quiet",
                "--initial-branch=main",
                remote_dir.to_str().expect("path"),
            ],
        );
        std::fs::create_dir_all(seed.join("plans")).expect("mkdir plans");
        std::fs::write(seed.join(PLAN_PATH), BASE_PLAN).expect("write plan");
        git_in(&seed, &["init", "--quiet", "--initial-branch=main"]);
        git_in(&seed, &["add", "--all"]);
        git_in(&seed, &["commit", "--quiet", "-m", "adopt the plan"]);
        let remote = format!("file://{}", remote_dir.display());
        git_in(&seed, &["push", "--quiet", &remote, "HEAD:refs/heads/main"]);
        let work = root.join("work");
        git_in(
            &root,
            &["clone", "--quiet", &remote, work.to_str().expect("path")],
        );
        Self { root, remote, work }
    }

    /// The commit `refs/remotes/origin/main` points at in the clone.
    fn tip(&self) -> String {
        git_in(&self.work, &["fetch", "--quiet", "origin"]);
        git_in(&self.work, &["rev-parse", "refs/remotes/origin/main"])
    }

    /// Edit the plan and push it, with plain git.
    fn commit_edited_plan_with_git(&self) -> String {
        std::fs::write(self.work.join(PLAN_PATH), edited_plan()).expect("write plan");
        git_in(&self.work, &["add", "--all"]);
        git_in(&self.work, &["commit", "--quiet", "-m", "edit the plan"]);
        git_in(&self.work, &["push", "--quiet", "origin", "main"]);
        self.tip()
    }
}

impl Drop for Repo {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.root).ok();
    }
}

/// Attribute everything the stub accepted to the session's lead.
///
/// The stub relay signs each publish with a fresh generated key, so a
/// declaration it accepted is excluded as `signer_not_may_lead` — a true
/// answer about a fabricated signer, and not what this test is about. The
/// fold does not re-verify signatures (`project_work_fold.rs` § inputs), so
/// re-labelling the accepted rows is exactly the relay serving back the
/// lead's own event.
fn attribute_publishes_to_the_lead(wire: &StubWire) {
    let lead = "1e".repeat(32);
    for row in wire.rows.borrow_mut().iter_mut() {
        if row["kind"].as_u64() == Some(u64::from(KIND_PROJECT_WORK_RECORD)) {
            row["pubkey"] = json!(lead);
        }
    }
}

/// Fold this session's coverage the way `status` and the completion gate both
/// do, reading plan blobs from the clone with the production plan source.
async fn coverage(wire: &StubWire, repo: &Repo) -> WorkProjection {
    let session = work_session();
    let plans = GitPlans(Some(repo.work.to_string_lossy().into_owned()));
    coverage_with(wire, &session, &plans)
        .await
        .expect("coverage")
        .0
}

fn only(coverage: &WorkProjection, state: WorkDeclarationState) -> &WorkDeclarationProjection {
    let mut found = coverage
        .declarations
        .iter()
        .filter(|declaration| declaration.state == state);
    let first = found
        .next()
        .unwrap_or_else(|| panic!("no declaration is {state:?}: {:?}", coverage.declarations));
    assert!(
        found.next().is_none(),
        "more than one declaration is {state:?}"
    );
    first
}

// ── the claim ──────────────────────────────────────────────────────────────

/// Editing and committing a declared plan does **not** make the declaration
/// stale, does not change what the fold reads, and is not disclosed anywhere
/// in the projection. Re-adopting is what moves the contract.
#[tokio::test]
async fn a_plan_committed_after_adoption_leaves_the_declaration_head() {
    let repo = Repo::new();
    let commit_a = repo.tip();
    let wire = StubWire::new(wire_rows(&commit_a));
    let session = work_session();

    adopt_with(
        &wire,
        &session,
        &adopt_args(&repo.work, &commit_a, Vec::new()),
    )
    .await
    .expect("adopt at A");
    assert_eq!(wire.publishes(), 1, "adoption is one event");
    attribute_publishes_to_the_lead(&wire);

    let before = coverage(&wire, &repo).await;
    let head_before = only(&before, WorkDeclarationState::Head).clone();
    assert_eq!(head_before.plan_ref.commit, commit_a);
    assert!(head_before.plan_resolved, "the plan was read at A");
    assert!(
        !head_before
            .criteria
            .iter()
            .any(|criterion| criterion.criterion_id == ADDED_CRITERION),
        "the plan at A does not carry the added criterion yet"
    );

    // ── the plan changes on main, and the declaration is not re-adopted ────
    let commit_b = repo.commit_edited_plan_with_git();
    assert_ne!(commit_b, commit_a, "the edit produced a second commit");

    let after = coverage(&wire, &repo).await;
    let head_after = only(&after, WorkDeclarationState::Head).clone();

    // 1. The state is still `head`. `stale` is `goal_changed` and nothing
    //    else: `project_work_fold.rs` sets it only when the session's current
    //    goal differs from the declaration's `goalRef`.
    assert_eq!(
        head_after.state,
        WorkDeclarationState::Head,
        "a committed plan edit does not change the declaration's state"
    );
    assert_eq!(head_after.state_reason_code, None);
    assert_eq!(head_after.state_reason, None);

    // 2. The fold still reads the plan at the commit the declaration pinned,
    //    so the added criterion is invisible and coverage is unchanged.
    assert_eq!(head_after.plan_ref.commit, commit_a);
    assert!(head_after.plan_resolved);
    assert_eq!(head_after.criteria, head_before.criteria);
    assert_eq!(head_after.coverage_complete, head_before.coverage_complete);
    assert_eq!(head_after.coverage_reason, head_before.coverage_reason);
    assert_eq!(
        head_after.coverage_reason_code,
        head_before.coverage_reason_code
    );

    // 3. A local commit nobody published moves nothing: the relay's ref
    //    state still names A, so `planDrift` reads `none` and the projection
    //    names B nowhere at all. Drift is a fact about what the relay can
    //    see, never about this machine's checkout.
    let printed = serde_json::to_string(&after).expect("projection");
    assert!(printed.contains(&commit_a), "it names the declared commit");
    assert!(
        !printed.contains(&commit_b),
        "an unpublished commit reached the projection"
    );
    assert_eq!(
        head_after.plan_drift.declared_commit, commit_a,
        "drift is reported against the commit this declaration pinned"
    );
    assert_eq!(
        head_after.plan_drift.state,
        buzz_core::project_work_fold::WorkPlanDriftState::Unchanged,
        "the relay's tip is still the declared commit"
    );
    assert!(
        !printed.contains(ADDED_CRITERION),
        "the projection read the plan at head"
    );

    // 4. The completion gate reads exactly two fields of this projection —
    //    `state` and `coverage_complete` (`incomplete_head`,
    //    `operations_completion.rs:283`) — and both are unchanged, so the
    //    gate cannot tell the two folds apart. It refuses either way, because
    //    no evidence is bound, never because the plan moved.
    assert_eq!(
        (head_before.state, head_before.coverage_complete),
        (head_after.state, head_after.coverage_complete),
        "the completion gate's inputs are identical across the plan edit"
    );
    assert!(
        !head_after.coverage_complete,
        "an unevidenced declaration is refused either way"
    );

    // 5. Once the relay serves the newer tip, `planDrift` says so — and
    //    **nothing else moves**. The declaration is still `head`, its plan
    //    still resolves at A, its criteria are byte-identical and the
    //    completion gate's two inputs are unchanged. That is A10 in one
    //    assertion pair: disclosed, never enforced. (Before ledger 235 this
    //    step asserted the opposite — that the newer tip changed nothing at
    //    all — which was true only because nothing looked.)
    for row in wire.rows.borrow_mut().iter_mut() {
        if row["kind"].as_u64() == Some(u64::from(KIND_GIT_REPO_STATE)) {
            row["tags"] = json!([["d", AGENTS_REPO_ID], ["refs/heads/main", commit_b]]);
        }
    }
    let with_new_tip = coverage(&wire, &repo).await;
    let head_with_tip = only(&with_new_tip, WorkDeclarationState::Head).clone();
    assert_eq!(
        (
            head_with_tip.plan_drift.state,
            head_with_tip.plan_drift.current_commit.clone()
        ),
        (
            buzz_core::project_work_fold::WorkPlanDriftState::Drifted,
            Some(commit_b.clone())
        ),
        "the agents repository moved, and the fold says so"
    );
    assert_eq!(head_with_tip.state, WorkDeclarationState::Head);
    assert_eq!(head_with_tip.plan_ref.commit, commit_a, "it still pins A");
    assert!(head_with_tip.plan_resolved);
    assert_eq!(head_with_tip.criteria, head_after.criteria);
    assert_eq!(
        head_with_tip.coverage_complete,
        head_after.coverage_complete
    );
    // Apart from that one object, the two folds are byte-identical: the
    // newer commit reached `planDrift` and nothing that decides a verdict.
    let mut drifted_copy = with_new_tip.clone();
    for declaration in &mut drifted_copy.declarations {
        declaration.plan_drift = head_after.plan_drift.clone();
    }
    assert_eq!(
        serde_json::to_value(&drifted_copy).expect("projection"),
        serde_json::to_value(&after).expect("projection"),
        "apart from planDrift, a moved agents tip folds identically"
    );

    // ── re-adopting at B is what moves the contract ───────────────────────
    let readopt = adopt_args(
        &repo.work,
        &commit_b,
        vec![head_after.declaration_ref.clone()],
    );
    adopt_with(&wire, &session, &readopt)
        .await
        .expect("re-adopt at B");
    assert_eq!(wire.publishes(), 2, "the amendment is one more event");
    attribute_publishes_to_the_lead(&wire);

    let amended = coverage(&wire, &repo).await;
    let new_head = only(&amended, WorkDeclarationState::Head).clone();
    let superseded = only(&amended, WorkDeclarationState::Superseded).clone();
    assert_eq!(new_head.plan_ref.commit, commit_b);
    assert_eq!(superseded.declaration_ref, head_after.declaration_ref);
    assert_eq!(
        superseded.superseded_by,
        vec![new_head.declaration_ref.clone()]
    );
    assert_eq!(
        new_head.work_id, head_after.work_id,
        "the work id is stable"
    );
    assert!(
        new_head
            .criteria
            .iter()
            .any(|criterion| criterion.criterion_id == ADDED_CRITERION),
        "the new head's criteria come from the plan at B"
    );
}

/// The same thing through the `bee agents-repo commit` git half: a kind:44250
/// draft landing on `main` is an ordinary commit, and the declaration that
/// pinned the previous one is equally unmoved by it.
#[tokio::test]
async fn an_agents_repo_draft_commit_leaves_the_declaration_head() {
    let repo = Repo::new();
    let commit_a = repo.tip();
    let wire = StubWire::new(wire_rows(&commit_a));
    let session = work_session();
    adopt_with(
        &wire,
        &session,
        &adopt_args(&repo.work, &commit_a, Vec::new()),
    )
    .await
    .expect("adopt at A");
    attribute_publishes_to_the_lead(&wire);

    let base = git_in(
        &repo.work,
        &["rev-parse", &format!("{commit_a}:{PLAN_PATH}")],
    );
    let catalog = buzz_persona::template::TemplateCatalog::load(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/templates"),
        "lane233",
    )
    .expect("catalog");
    let committer = Identity {
        name: "Andy".into(),
        email: "11111111@beekeeper.local".into(),
    };
    let changes = vec![DraftChange {
        id: "33".repeat(32),
        author: "22".repeat(32),
        op: "file.put".into(),
        path: PLAN_PATH.into(),
        to: None,
        text: Some(edited_plan()),
        sha256: None,
        base: Some(base),
        message: Some("one more criterion".into()),
    }];
    let assets = AssetBytes::new();
    let outcome = commit_drafts(&CommitRequest {
        remote: &repo.remote,
        expected_tip: Some(&commit_a),
        changes: &changes,
        assets: &assets,
        message: "docs(plans): one more criterion",
        committer: &committer,
        coauthors: &[],
        catalog: &catalog,
        project: &project_ref(),
    })
    .expect("commit runs");
    let CommitOutcome::Yes {
        commit: commit_b, ..
    } = outcome
    else {
        panic!("the draft did not land: {outcome:?}");
    };
    assert_ne!(commit_b, commit_a);
    assert_eq!(repo.tip(), commit_b, "main moved to the draft commit");

    let after = coverage(&wire, &repo).await;
    let head = only(&after, WorkDeclarationState::Head).clone();
    assert_eq!(
        head.state,
        WorkDeclarationState::Head,
        "a landed draft does not make the declaration stale"
    );
    assert_eq!(head.plan_ref.commit, commit_a, "it still pins A");
    assert!(head.plan_resolved);
    // The draft landed on the agents repository's `main`, and the harness
    // serves that tip: the declaration still pins A, and the only thing that
    // notices is `planDrift`.
    for row in wire.rows.borrow_mut().iter_mut() {
        if row["kind"].as_u64() == Some(u64::from(KIND_GIT_REPO_STATE)) {
            row["tags"] = json!([["d", AGENTS_REPO_ID], ["refs/heads/main", commit_b]]);
        }
    }
    let with_landed_tip = coverage(&wire, &repo).await;
    let head_with_tip = only(&with_landed_tip, WorkDeclarationState::Head).clone();
    assert_eq!(
        head_with_tip.state,
        WorkDeclarationState::Head,
        "a landed draft still does not make the declaration stale"
    );
    assert_eq!(head_with_tip.plan_ref.commit, commit_a, "it still pins A");
    assert_eq!(
        (
            head_with_tip.plan_drift.state,
            head_with_tip.plan_drift.current_commit.clone()
        ),
        (
            buzz_core::project_work_fold::WorkPlanDriftState::Drifted,
            Some(commit_b.clone())
        ),
        "the landed commit is disclosed as drift, and only as drift"
    );
    assert_eq!(head_with_tip.criteria, head.criteria);
    assert_eq!(head_with_tip.coverage_complete, head.coverage_complete);
}
