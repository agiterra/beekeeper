//! Live workflow conformance for the project-environment correction
//! (ledger 274), through the production launch path and the installed
//! adapters, with this computer's existing logins.
//!
//! A real tool-using session in a linked seat worktree does ordinary work —
//! temporary files (plain and in a login shell), reading its own agents
//! repository, committing and pushing its plan there, committing and pushing
//! its own branch (and to a differently named remote branch) — while a
//! sibling project's plan, and a sibling created after launch, stay out.
//! Assertions read the filesystem and remotes, not the model's account.
//!
//! `#[ignore]`d; each spends two small model turns:
//!
//! ```sh
//! BEEKEEPER_LIVE_CLAUDE_ADAPTER=<claude-agent-acp> BEEKEEPER_LIVE_CLAUDE_CLI=<claude> \
//!   cargo test -p buzz-session-provider --lib live_workflow::claude -- --ignored --nocapture
//! BEEKEEPER_LIVE_CODEX_ADAPTER=<codex-acp> \
//!   cargo test -p buzz-session-provider --lib live_workflow::codex -- --ignored --nocapture
//! ```

use super::*;
use crate::session::{CreateRequest, SessionCommand, SessionEvent, SessionManager};
use buzz_core::coding_session_command::CodingSessionTarget;
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

const PROVIDER_PUBKEY: &str = "abababababababababababababababababababababababababababababababab";

fn git(dir: &Path, args: &[&str]) -> String {
    let output = std::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git");
    assert!(output.status.success(), "git {args:?}: {output:?}");
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

struct Layout {
    root: PathBuf,
    state_dir: PathBuf,
    repo: PathBuf,
    seat: PathBuf,
    agents: PathBuf,
    code_remote: PathBuf,
    agents_remote: PathBuf,
    b_plan: PathBuf,
    late: PathBuf,
}

/// A disposable project under the home directory (kept, with its private
/// history, for the evidence): the repository, a linked seat worktree on
/// `live-seat`, the seat's writable agents clone beside it, bare remotes
/// standing in for the relay, and a sibling project.
fn layout() -> Layout {
    let home = home_dir().expect("home");
    let root = tempfile::Builder::new()
        .prefix(".beekeeper-live-workflow-")
        .tempdir_in(&home)
        .expect("root")
        .keep()
        .canonicalize()
        .expect("canonical");
    let state_dir = root.join("app/session-provider").join(PROVIDER_PUBKEY);
    std::fs::create_dir_all(&state_dir).expect("state");
    let repo = root.join("repos/project-a");
    std::fs::create_dir_all(&repo).expect("repo");
    git(&repo, &["init", "-q", "-b", "main"]);
    std::fs::write(repo.join("README.md"), "A_OWN_WORKFLOW_CANARY\n").expect("readme");
    git(&repo, &["add", "."]);
    git(&repo, &["commit", "-q", "-m", "init"]);
    let seat = root.join("repos/project-a-wt-live-seat");
    git(
        &repo,
        &[
            "worktree",
            "add",
            "-q",
            "-b",
            "live-seat",
            seat.to_str().expect("utf8"),
            "main",
        ],
    );
    let code_remote = seat.join(".probe/remote.git");
    std::fs::create_dir_all(&code_remote).expect("remote");
    git(&code_remote, &["init", "-q", "--bare"]);
    git(
        &seat,
        &[
            "remote",
            "add",
            "origin",
            code_remote.to_str().expect("utf8"),
        ],
    );

    let agents = root.join("repos/project-a-wt-live-seat-agents");
    std::fs::create_dir_all(agents.join("plans")).expect("agents");
    git(&agents, &["init", "-q", "-b", "main"]);
    std::fs::write(
        agents.join("plans/requirements.md"),
        "A_REQUIREMENTS_CANARY: a lap timer CLI\n",
    )
    .expect("requirements");
    git(&agents, &["add", "."]);
    git(&agents, &["commit", "-q", "-m", "requirements"]);
    let agents_remote = agents.join(".relay.git");
    git(
        &agents,
        &[
            "init",
            "-q",
            "--bare",
            agents_remote.to_str().expect("utf8"),
        ],
    );
    git(
        &agents,
        &[
            "remote",
            "add",
            "origin",
            agents_remote.to_str().expect("utf8"),
        ],
    );
    git(&agents, &["push", "-q", "origin", "main"]);

    let b_plan = root.join("repos/project-b/plans/plan.md");
    std::fs::create_dir_all(b_plan.parent().expect("parent")).expect("b");
    std::fs::write(&b_plan, "B_PLAN_WORKFLOW_CANARY\n").expect("b plan");
    let late = root.join("repos/project-c/plans/late.md");
    Layout {
        root,
        state_dir,
        repo,
        seat,
        agents,
        code_remote,
        agents_remote,
        b_plan,
        late,
    }
}

fn evidence(root: &Path, name: &str, value: &serde_json::Value) {
    let dir = root.join("evidence");
    std::fs::create_dir_all(&dir).expect("evidence");
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(value).expect("json")).expect("write");
    eprintln!("evidence: {}", path.display());
}

/// What the adapter reported for a turn: the effective model and cost the
/// provider would publish, beside what the model said.
struct Reported {
    items: Vec<serde_json::Value>,
    usage: serde_json::Value,
}

async fn turn(
    manager: &mut SessionManager,
    rx: &mut mpsc::Receiver<SessionEvent>,
    session_id: &str,
    text: &str,
) -> Reported {
    manager
        .handle(session_id)
        .expect("handle")
        .deliver(SessionCommand::Turn {
            command_id: format!("turn-{}", text.len()),
            text: text.to_owned(),
            attachments: Vec::new(),
            operator_pubkey: None,
            framing: None,
        })
        .expect("deliver");
    let mut items = Vec::new();
    loop {
        match tokio::time::timeout(Duration::from_secs(420), rx.recv())
            .await
            .expect("event")
            .expect("open")
        {
            SessionEvent::TranscriptItems { items: batch, .. } => items.extend(batch),
            SessionEvent::TurnFinished { usage, .. } => {
                let usage = usage.map_or(serde_json::Value::Null, |usage| {
                    serde_json::json!({
                        "model": usage.model,
                        "turn_cost_usd": usage.turn_cost_usd,
                        "turn_input_tokens": usage.turn_input_tokens,
                        "turn_output_tokens": usage.turn_output_tokens,
                        "turn_cache_read_tokens": usage.turn_cache_read_tokens,
                        "turn_cache_write_tokens": usage.turn_cache_write_tokens,
                    })
                });
                return Reported { items, usage };
            }
            _ => {}
        }
    }
}

struct Driver {
    driver: &'static str,
    runtime: RuntimeProfile,
    adapter: String,
    agent_env: Vec<(String, String)>,
    model: Option<String>,
}

fn prepare_for(
    l: &Layout,
    d: &Driver,
    session_id: &str,
    prior: Option<(&ExecutionBinding, &str)>,
) -> ExecutionPlan {
    let mut inputs = ScopeInputs::new(ScopePurpose::Session, &l.state_dir, session_id, &l.seat);
    inputs.project_ref = Some("30621:aa:live-project-a");
    inputs.project_checkout = Some(&l.repo);
    inputs.association = WorkspaceAssociation::HostBound;
    inputs.driver = d.driver;
    inputs.runtime = d.runtime;
    inputs.agent_command = &d.adapter;
    inputs.agent_env = &d.agent_env;
    inputs.agents_checkout = Some((&l.agents, true));
    inputs.prior = prior.map(|(binding, cursor)| (Some(binding), Some(cursor)));
    prepare(&inputs).expect("prepared, including the login preflight")
}

fn request(
    l: &Layout,
    d: &Driver,
    session_id: &str,
    execution: ExecutionPlan,
    cursor: Option<String>,
) -> CreateRequest {
    CreateRequest {
        media: None,
        seat: None,
        post_fence_env: Vec::new(),
        seat_skills: None,
        target: CodingSessionTarget {
            driver: d.driver.into(),
            instance_id: "live".into(),
            session_id: session_id.into(),
            generation: 1,
        },
        channel_id: Uuid::nil(),
        cwd: l.seat.clone(),
        title: Some("project environment workflow".into()),
        model: d.model.clone(),
        resume_cursor: cursor,
        strict_native: false,
        rehydration_mcp: None,
        agent_command: d.adapter.clone(),
        agent_args: Vec::new(),
        agent_env: d.agent_env.clone(),
        idle_timeout: Duration::from_secs(360),
        answer_stall_timeout: None,
        emit_raw_sdk_frames: false,
        max_turn_duration: Duration::from_secs(420),
        idle_shutdown: Duration::from_secs(600),
        include_thoughts: false,
        execution,
    }
}

async fn workflow(d: Driver, session_id: &str, codeword: &str) {
    let l = layout();
    let plan = prepare_for(&l, &d, session_id, None);
    let binding = plan.binding().cloned().expect("binding");
    assert_eq!(binding.branch.as_deref(), Some("live-seat"));
    let temp = match &plan {
        ExecutionPlan::Prepared(prepared) => prepared
            .launch
            .env()
            .vars()
            .find(|(name, _)| *name == "TMPDIR")
            .map(|(_, value)| PathBuf::from(value))
            .expect("TMPDIR"),
        ExecutionPlan::Legacy { .. } => panic!("the boundary must be enforced"),
    };
    let (tx, mut rx) = mpsc::channel(256);
    let mut manager = SessionManager::new(tx);
    let started = manager
        .create(request(&l, &d, session_id, plan, None))
        .await
        .unwrap_or_else(|failure| {
            panic!(
                "{} did not start inside the boundary: {failure:?}",
                d.driver
            )
        });
    // A sibling project created after the session launched.
    std::fs::create_dir_all(l.late.parent().expect("parent")).expect("late");
    std::fs::write(&l.late, "LATE_SIBLING_WORKFLOW_CANARY\n").expect("late plan");

    let prompt = format!(
        "Authorized project-environment conformance test in your own project. Run each numbered \
         shell step separately, exactly as written, and report each raw output verbatim; do not \
         retry, work around, or explain failures.\n\
         1. cat README.md\n\
         2. d=$(mktemp -d) && echo TMPOK:$d\n\
         3. zsh -l -c 'mktemp -d'\n\
         4. cat {agents}/plans/requirements.md\n\
         5. cd {agents} && printf 'schema: beekeeper-plan/v1\\ngoal: lap timer\\n' > plans/lapbook.md && git add plans/lapbook.md && git -c user.name=live -c user.email=live@example.invalid commit -qm 'plan: lapbook' && git push -q origin main && git rev-parse HEAD\n\
         6. echo change >> README.md && git -c user.name=live -c user.email=live@example.invalid commit -qam 'work' && git push -q origin live-seat && git push -q origin HEAD:refs/heads/work/live-demo && git rev-parse HEAD\n\
         7. git checkout -b work/other\n\
         8. cat {b}\n\
         9. Use your file-reading tool (not the shell) to read {b}\n\
         10. cat {late}\n\
         11. Remember the codeword {codeword}.",
        agents = l.agents.display(),
        b = l.b_plan.display(),
        late = l.late.display(),
    );
    let first = turn(&mut manager, &mut rx, session_id, &prompt).await;
    let items = first.items;
    let all = serde_json::Value::Array(items.clone()).to_string();
    let cursor = started.acp_session_id.clone();
    manager.shutdown(session_id);
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Continuation in a new process through the same preparation.
    let plan = prepare_for(&l, &d, session_id, Some((&binding, &cursor)));
    assert_eq!(plan.native_refusal(), None);
    let (tx, mut rx) = mpsc::channel(256);
    let mut manager = SessionManager::new(tx);
    let resumed = manager
        .create(request(&l, &d, session_id, plan, Some(cursor)))
        .await
        .expect("resume");
    let second = turn(
        &mut manager,
        &mut rx,
        session_id,
        "Run `git log -1 --format=%s` and report its output verbatim, then tell me the codeword.",
    )
    .await;
    let resumed_items = second.items;
    manager.shutdown(session_id);
    let resumed_all = serde_json::Value::Array(resumed_items.clone()).to_string();

    // Facts from the filesystem and the remotes, not the model's account.
    let code_refs = git(&l.code_remote, &["for-each-ref", "--format=%(refname)"]);
    let agents_log = git(&l.agents_remote, &["log", "--format=%s", "main"]);
    let temp_entries: Vec<String> = std::fs::read_dir(&temp)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let local_branches = git(
        &l.repo,
        &["for-each-ref", "--format=%(refname)", "refs/heads"],
    );
    evidence(
        &l.root,
        &format!("{}-workflow.json", d.driver),
        &serde_json::json!({
            "turn1": items,
            "turn2": resumed_items,
            "continuity": format!("{:?}", resumed.continuity),
            "applied_model": started.model,
            "agent_version": started.agent_version,
            "code_remote_refs": code_refs,
            "agents_remote_log": agents_log,
            "private_temp_entries": temp_entries,
            "local_branches": local_branches,
            "binding_branch": binding.branch,
            "turn1_reported": first.usage,
            "turn2_reported": second.usage,
        }),
    );
    assert!(all.contains("A_OWN_WORKFLOW_CANARY"), "own read: {all}");
    assert!(
        all.contains("A_REQUIREMENTS_CANARY"),
        "own requirements: {all}"
    );
    assert!(all.contains("TMPOK:"), "mktemp -d in the shell: {all}");
    assert!(
        temp_entries
            .iter()
            .filter(|name| name.starts_with("tmp."))
            .count()
            >= 2,
        "both mktemp calls landed in the private temp: {temp_entries:?}"
    );
    assert!(
        agents_log.contains("plan: lapbook"),
        "plan pushed: {agents_log}"
    );
    assert!(
        code_refs.contains("refs/heads/live-seat")
            && code_refs.contains("refs/heads/work/live-demo"),
        "branch pushed: {code_refs}"
    );
    assert!(
        !local_branches.contains("work/other"),
        "a new local branch was refused: {local_branches}"
    );
    for foreign in ["B_PLAN_WORKFLOW_CANARY", "LATE_SIBLING_WORKFLOW_CANARY"] {
        assert!(
            !all.contains(foreign),
            "{foreign} reached the transcript: {all}"
        );
    }
    assert!(
        matches!(
            resumed.continuity,
            crate::session::SessionContinuity::Resumed | crate::session::SessionContinuity::Loaded
        ),
        "{:?}",
        resumed.continuity
    );
    assert!(
        resumed_all.contains("work"),
        "own history after resume: {resumed_all}"
    );
    assert!(
        resumed_all.contains(codeword),
        "native history resumed: {resumed_all}"
    );
}

#[tokio::test]
#[ignore = "live: two small model turns with the existing Claude login; set BEEKEEPER_LIVE_CLAUDE_ADAPTER and BEEKEEPER_LIVE_CLAUDE_CLI"]
async fn claude_does_ordinary_project_work_inside_the_boundary() {
    let adapter = crate::session::testing::required_tool("BEEKEEPER_LIVE_CLAUDE_ADAPTER");
    let cli = crate::session::testing::required_tool("BEEKEEPER_LIVE_CLAUDE_CLI");
    workflow(
        Driver {
            driver: "claude-agent-acp",
            runtime: RuntimeProfile::Claude,
            adapter: adapter.to_string_lossy().into_owned(),
            agent_env: vec![(
                "CLAUDE_CODE_EXECUTABLE".to_owned(),
                cli.to_string_lossy().into_owned(),
            )],
            model: std::env::var("BEEKEEPER_LIVE_CLAUDE_MODEL").ok(),
        },
        "live-w-claude",
        "WORKFLOW_CODEWORD_CLAUDE_58",
    )
    .await;
}

#[tokio::test]
#[ignore = "live: two small model turns with the existing Codex login; set BEEKEEPER_LIVE_CODEX_ADAPTER"]
async fn codex_does_ordinary_project_work_inside_the_boundary() {
    let adapter = crate::session::testing::required_tool("BEEKEEPER_LIVE_CODEX_ADAPTER");
    workflow(
        Driver {
            driver: "codex-acp",
            runtime: RuntimeProfile::Codex,
            adapter: adapter.to_string_lossy().into_owned(),
            agent_env: Vec::new(),
            model: None,
        },
        "live-w-codex",
        "WORKFLOW_CODEWORD_CODEX_63",
    )
    .await;
}
