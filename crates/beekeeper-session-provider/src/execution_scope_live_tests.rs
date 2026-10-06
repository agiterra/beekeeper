//! Live conformance through the production launch path, against the
//! installed Claude runtime and this computer's existing login.
//!
//! `#[ignore]`d: opt in by naming the installed runtime. The startup
//! diagnosis spends no model turn; the workflow spends two small ones. Both
//! use only a disposable fixture under the home directory (kept, with the
//! evidence, after the run); the runtime's configuration and history live in
//! the fixture's private directory, never in the operator's `~/.claude`.
//!
//! ```sh
//! BEEKEEPER_LIVE_CLAUDE_ADAPTER=<claude-agent-acp> \
//! BEEKEEPER_LIVE_CLAUDE_CLI=<claude> \
//! [BEEKEEPER_LIVE_CLAUDE_MODEL=<an offered value, default haiku>] \
//! [BEEKEEPER_LIVE_EVIDENCE_DIR=<dir>] \
//!   cargo test -p beekeeper-session-provider --lib <test name> -- --ignored --exact --nocapture
//! ```

use super::*;
use crate::session::{CreateRequest, SessionCommand, SessionEvent, SessionManager};
use beekeeper_core::coding_session_command::CodingSessionTarget;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::mpsc;
use uuid::Uuid;

const PROVIDER_PUBKEY: &str = "abababababababababababababababababababababababababababababababab";

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        if let Ok(mut inner) = self.0.lock() {
            inner.extend_from_slice(buf);
        }
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

/// The installed runtime an opted-in run names, and the model it asks for
/// (a value the adapter offers; the startup diagnosis lists them).
struct Runtime {
    adapter: PathBuf,
    cli: PathBuf,
    model: String,
}

fn runtime() -> Runtime {
    Runtime {
        adapter: crate::session::testing::required_tool("BEEKEEPER_LIVE_CLAUDE_ADAPTER"),
        cli: crate::session::testing::required_tool("BEEKEEPER_LIVE_CLAUDE_CLI"),
        model: std::env::var("BEEKEEPER_LIVE_CLAUDE_MODEL").unwrap_or_else(|_| "haiku".to_owned()),
    }
}

/// A disposable fixture root under the home directory, so a foreign sibling
/// sits in the operator's own data region like a real neighbour project.
/// Kept after the run: it holds the private native history the evidence
/// cites.
fn fixture_root(home: &Path) -> PathBuf {
    tempfile::Builder::new()
        .prefix(".beekeeper-live-fixture-")
        .tempdir_in(home)
        .expect("fixture root")
        .keep()
}

/// Where evidence goes: `BEEKEEPER_LIVE_EVIDENCE_DIR`, else the fixture's own
/// `evidence/`.
fn evidence(root: &Path, name: &str, value: &serde_json::Value) {
    let dir = std::env::var_os("BEEKEEPER_LIVE_EVIDENCE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("evidence"));
    std::fs::create_dir_all(&dir).expect("evidence dir");
    let path = dir.join(name);
    std::fs::write(&path, serde_json::to_string_pretty(value).expect("json")).expect("evidence");
    eprintln!("evidence: {}", path.display());
}

fn log_of(captured: &Captured) -> String {
    String::from_utf8_lossy(&captured.0.lock().expect("log")).into_owned()
}

fn capture(filter: &str) -> (Captured, tracing::subscriber::DefaultGuard) {
    let captured = Captured::default();
    let writer = captured.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_writer(move || writer.clone())
        .with_env_filter(filter)
        .with_ansi(false)
        .finish();
    (captured, tracing::subscriber::set_default(subscriber))
}

fn request(
    cwd: &Path,
    runtime: &Runtime,
    execution: ExecutionPlan,
    cursor: Option<String>,
) -> CreateRequest {
    CreateRequest {
        media: None,
        seat: None,
        post_fence_env: Vec::new(),
        seat_skills: None,
        target: CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "live".into(),
            session_id: "live-s1".into(),
            generation: 1,
        },
        channel_id: Uuid::nil(),
        cwd: cwd.to_path_buf(),
        title: Some("boundary conformance".into()),
        model: Some(runtime.model.clone()),
        resume_cursor: cursor,
        strict_native: false,
        rehydration_mcp: None,
        agent_command: runtime.adapter.to_string_lossy().into_owned(),
        agent_args: Vec::new(),
        agent_env: vec![(
            "CLAUDE_CODE_EXECUTABLE".into(),
            runtime.cli.to_string_lossy().into_owned(),
        )],
        idle_timeout: Duration::from_secs(240),
        answer_stall_timeout: None,
        emit_raw_sdk_frames: true,
        max_turn_duration: Duration::from_secs(300),
        idle_shutdown: Duration::from_secs(600),
        include_thoughts: false,
        transcript_paragraph_flush: false,
        execution,
    }
}

async fn turn(
    manager: &mut SessionManager,
    rx: &mut mpsc::Receiver<SessionEvent>,
    text: &str,
) -> Vec<serde_json::Value> {
    turn_in(manager, rx, "live-s1", text).await
}

async fn turn_in(
    manager: &mut SessionManager,
    rx: &mut mpsc::Receiver<SessionEvent>,
    session_id: &str,
    text: &str,
) -> Vec<serde_json::Value> {
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
        match tokio::time::timeout(Duration::from_secs(300), rx.recv())
            .await
            .expect("event")
            .expect("open")
        {
            SessionEvent::TranscriptItems { items: batch, .. } => items.extend(batch),
            SessionEvent::TurnFinished { .. } => return items,
            _ => {}
        }
    }
}

/// The prepared Session scope for project A in `root`, with `identity` as
/// the host-owned identity it must keep.
fn prepare_live(
    root: &Path,
    runtime: &Runtime,
    agent_env: &[(String, String)],
    identity: &[(String, String)],
    prior: Option<(&ExecutionBinding, &str)>,
) -> ExecutionPlan {
    let state_dir = root.join("app/session-provider").join(PROVIDER_PUBKEY);
    let a = root.join("repos/project-a");
    let adapter_command = runtime.adapter.to_string_lossy().into_owned();
    let mut inputs = ScopeInputs::new(ScopePurpose::Session, &state_dir, "live-s1", &a);
    inputs.project_ref = Some("30621:aa:live-project-a");
    inputs.association = WorkspaceAssociation::HostBound;
    inputs.driver = "claude-agent-acp";
    inputs.runtime = RuntimeProfile::Claude;
    inputs.agent_command = &adapter_command;
    inputs.agent_env = agent_env;
    inputs.identity_env = identity;
    inputs.prior = prior.map(|(binding, cursor)| (Some(binding), Some(cursor)));
    prepare(&inputs).expect("prepared, including the login preflight")
}

/// Startup only, no model turn: which model values the installed adapter
/// offers, and whether the requested one was applied.
#[tokio::test]
#[ignore = "live: starts the installed Claude adapter (no model turn); set BEEKEEPER_LIVE_CLAUDE_ADAPTER and BEEKEEPER_LIVE_CLAUDE_CLI"]
async fn the_installed_claude_adapter_reports_the_model_it_applies() {
    let runtime = runtime();
    let (captured, _guard) = capture("csp::session=warn,csp::scope=info");
    let root = fixture_root(&home_dir().expect("home"));
    std::fs::create_dir_all(root.join("repos/project-a")).expect("fixture");
    std::fs::create_dir_all(root.join("app/session-provider").join(PROVIDER_PUBKEY))
        .expect("fixture");
    let agent_env = vec![(
        "CLAUDE_CODE_EXECUTABLE".to_owned(),
        runtime.cli.to_string_lossy().into_owned(),
    )];
    let plan = prepare_live(&root, &runtime, &agent_env, &[], None);
    let (tx, _rx) = mpsc::channel(256);
    let mut manager = SessionManager::new(tx);
    let started = manager
        .create(request(&root.join("repos/project-a"), &runtime, plan, None))
        .await
        .expect("the adapter starts inside the boundary");
    manager.shutdown("live-s1");
    let log = log_of(&captured);
    let not_offered = log
        .lines()
        .find(|line| line.contains("does not offer model"))
        .map(str::to_owned);
    evidence(
        &root,
        "claude-startup-model.json",
        &serde_json::json!({
            "requested": runtime.model,
            "applied": started.model,
            "not_offered_line": not_offered,
            "agent_version": started.agent_version,
        }),
    );
    assert_eq!(
        started.model.as_deref(),
        Some(runtime.model.as_str()),
        "{not_offered:?}"
    );
}

/// The measured workflow: own work, denied neighbours, the project's own
/// settings and MCP server loaded natively without displacing the host's
/// pinned identity or private configuration, native resume, and the model
/// that actually answered read from the private native transcript.
#[tokio::test]
#[ignore = "live: spends two small model turns with the existing Claude login; set BEEKEEPER_LIVE_CLAUDE_ADAPTER and BEEKEEPER_LIVE_CLAUDE_CLI"]
async fn the_installed_claude_runtime_works_inside_the_boundary_and_reaches_nothing_else() {
    let runtime = runtime();
    let (captured, _guard) =
        capture("acp::sdk_frame=info,csp::scope=info,csp::session=warn,acp::stderr=info");
    let home = home_dir().expect("home");
    let root = fixture_root(&home);
    let state_dir = root.join("app/session-provider").join(PROVIDER_PUBKEY);
    let a = root.join("repos/project-a");
    let b_plan = root.join("repos/project-b/plans/plan.md");
    let cache_plan = root.join("app/packs/other-agents/plans/kettle.md");
    for dir in [
        &state_dir,
        &a.join(".claude"),
        &a.join("tools"),
        &b_plan.parent().expect("p").to_path_buf(),
        &cache_plan.parent().expect("p").to_path_buf(),
    ] {
        std::fs::create_dir_all(dir).expect("fixture");
    }
    std::fs::write(a.join("README.md"), "A_OWN_LIVE_CANARY\n").expect("own");
    std::fs::write(&b_plan, "B_PLAN_LIVE_CANARY\n").expect("b");
    std::fs::write(&cache_plan, "CACHED_FOREIGN_LIVE_CANARY\n").expect("cache");
    // The project's own settings try to replace the host's identity and the
    // private configuration directory, and set a variable of their own.
    std::fs::write(
        a.join(".claude/settings.json"),
        serde_json::json!({ "env": {
            "BUZZ_RELAY_URL": "wss://project-override.invalid",
            "CLAUDE_CONFIG_DIR": root.join("project-override-config"),
            "PROJECT_SETTING_CANARY": "from_project_settings",
        }})
        .to_string(),
    )
    .expect("project settings");
    // The project's own stdio MCP server.
    let server = a.join("tools/mcp_canary.py");
    std::fs::write(
        &server,
        r#"import json, sys
for line in sys.stdin:
    msg = json.loads(line)
    mid = msg.get("id")
    method = msg.get("method")
    if mid is None:
        continue
    if method == "initialize":
        result = {"protocolVersion": msg["params"].get("protocolVersion", "2025-06-18"), "capabilities": {"tools": {}}, "serverInfo": {"name": "proj", "version": "1"}}
    elif method == "tools/list":
        result = {"tools": [{"name": "canary", "description": "Returns the project canary.", "inputSchema": {"type": "object", "properties": {}}}]}
    elif method == "tools/call":
        result = {"content": [{"type": "text", "text": "PROJECT_MCP_CANARY_OK"}]}
    else:
        result = {}
    sys.stdout.write(json.dumps({"jsonrpc": "2.0", "id": mid, "result": result}) + "\n")
    sys.stdout.flush()
"#,
    )
    .expect("server");
    std::fs::write(
        a.join(".mcp.json"),
        serde_json::json!({ "mcpServers": { "proj_stdio": { "command": "/usr/bin/python3", "args": [server] } } })
            .to_string(),
    )
    .expect(".mcp.json");

    let agent_env = vec![(
        "CLAUDE_CODE_EXECUTABLE".to_owned(),
        runtime.cli.to_string_lossy().into_owned(),
    )];
    let identity = vec![(
        "BUZZ_RELAY_URL".to_owned(),
        "wss://relay.fixture.invalid".to_owned(),
    )];
    let plan = prepare_live(&root, &runtime, &agent_env, &identity, None);
    let binding = plan.binding().cloned().expect("binding");
    let (tx, mut rx) = mpsc::channel(256);
    let mut manager = SessionManager::new(tx);
    let started = match manager.create(request(&a, &runtime, plan, None)).await {
        Ok(started) => started,
        Err(failure) => {
            let log = log_of(&captured);
            evidence(
                &root,
                "claude-start-failure.json",
                &serde_json::json!({ "failure": format!("{failure:?}"), "log": log }),
            );
            panic!("the installed adapter did not start inside the boundary: {failure:?}");
        }
    };
    let items = turn(
        &mut manager,
        &mut rx,
        &format!(
            "Authorized sandbox conformance test. Run each step separately and report each raw \
             output verbatim; do not retry or work around failures.\n\
             1. Bash: cat README.md\n\
             2. Bash: cat {b}\n\
             3. Read tool on {b}\n\
             4. Bash: python3 -c \"print(open('{b}').read())\"\n\
             5. Bash: cat {cache}\n\
             6. Bash: ls ~/.claude ~/.claude/projects\n\
             7. Bash: echo \"relay=$BUZZ_RELAY_URL config=$CLAUDE_CONFIG_DIR project=$PROJECT_SETTING_CANARY\"\n\
             8. Call the MCP tool mcp__proj_stdio__canary and report its output.\n\
             9. Remember the codeword LIVE_CODEWORD_31.",
            b = b_plan.display(),
            cache = cache_plan.display()
        ),
    )
    .await;
    let all = serde_json::Value::Array(items.clone()).to_string();
    let frames = log_of(&captured);
    let init = frames
        .lines()
        .find(|line| line.contains("\"subtype\":\"init\""))
        .unwrap_or_default()
        .to_owned();
    evidence(
        &root,
        "claude-turn1.json",
        &serde_json::json!({ "items": items, "init_frame": init }),
    );
    assert!(all.contains("A_OWN_LIVE_CANARY"), "own read failed: {all}");
    for foreign in ["B_PLAN_LIVE_CANARY", "CACHED_FOREIGN_LIVE_CANARY"] {
        assert!(
            !all.contains(foreign),
            "{foreign} reached the transcript: {all}"
        );
    }
    assert!(
        all.contains("Operation not permitted") || all.contains("EPERM"),
        "{all}"
    );
    // The project's settings loaded; the host's pins held against them.
    assert!(
        all.contains("relay=wss://relay.fixture.invalid"),
        "the pinned identity lost: {all}"
    );
    assert!(
        all.contains("project=from_project_settings"),
        "project settings did not load: {all}"
    );
    assert!(
        !all.contains("project-override"),
        "a project setting displaced a host pin: {all}"
    );
    assert!(
        all.contains("PROJECT_MCP_CANARY_OK"),
        "the project's own MCP server was not callable: {all}"
    );
    // What reached the model: the project's own server only, no user MCP,
    // plugins, skills or agents.
    assert!(!init.is_empty(), "no system/init frame captured");
    let frame: serde_json::Value = init
        .find("{\"agents\"")
        .and_then(|at| {
            serde_json::Deserializer::from_str(&init[at..])
                .into_iter::<serde_json::Value>()
                .next()
                .and_then(Result::ok)
        })
        .expect("init frame JSON");
    let servers: Vec<&str> = frame["mcp_servers"]
        .as_array()
        .expect("mcp_servers")
        .iter()
        .filter_map(|server| server["name"].as_str())
        .collect();
    assert_eq!(servers, vec!["proj_stdio"], "{frame}");
    assert!(
        frame["plugins"]
            .as_array()
            .expect("plugins")
            .iter()
            .all(|plugin| plugin["path"] == "builtin"),
        "only the CLI's built-in plugins may load: {frame}"
    );
    let user_skills: Vec<String> = std::fs::read_dir(home.join(".claude/skills"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    let loaded: Vec<&str> = frame["skills"]
        .as_array()
        .expect("skills")
        .iter()
        .filter_map(serde_json::Value::as_str)
        .collect();
    for skill in &user_skills {
        assert!(
            !loaded.contains(&skill.as_str()),
            "user skill {skill} reached the project"
        );
    }
    let cursor = started.acp_session_id.clone();
    manager.shutdown("live-s1");
    tokio::time::sleep(Duration::from_secs(1)).await;

    // Native resume in a new process, bound to the same scope.
    let plan = prepare_live(
        &root,
        &runtime,
        &agent_env,
        &identity,
        Some((&binding, &cursor)),
    );
    assert_eq!(plan.native_refusal(), None);
    let (tx, mut rx) = mpsc::channel(256);
    let mut manager = SessionManager::new(tx);
    let resumed = manager
        .create(request(&a, &runtime, plan, Some(cursor)))
        .await
        .expect("resume");
    assert!(
        matches!(
            resumed.continuity,
            crate::session::SessionContinuity::Resumed | crate::session::SessionContinuity::Loaded
        ),
        "{:?}",
        resumed.continuity
    );
    let items = turn(
        &mut manager,
        &mut rx,
        "What was the codeword? Answer with the codeword only.",
    )
    .await;
    let all = serde_json::Value::Array(items.clone()).to_string();
    manager.shutdown("live-s1");

    // The native history is in the fixture's private configuration, and the
    // model that answered is what its transcript says, not what was asked.
    let private = std::fs::read_dir(state_dir.join(EXECUTIONS_DIR))
        .expect("executions")
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().starts_with("live-s1-"))
        .expect("owner dir")
        .path()
        .join("state/claude-config/projects");
    assert!(private.is_dir(), "{}", private.display());
    let answered = recorded_models(&private, |entry| {
        (entry["type"] == "assistant")
            .then(|| entry["message"]["model"].as_str())
            .flatten()
    });
    evidence(
        &root,
        "claude-turn2.json",
        &serde_json::json!({
            "items": items,
            "continuity": format!("{:?}", resumed.continuity),
            "requested_model": runtime.model,
            "applied_model": started.model,
            "transcript_models": answered,
            "private_history": private,
        }),
    );
    assert!(all.contains("LIVE_CODEWORD_31"), "{all}");
    assert!(
        !answered.is_empty(),
        "the private transcript records no answering model"
    );
    assert!(
        answered
            .iter()
            .all(|model| model.contains(runtime.model.as_str())),
        "asked for {}, answered by {answered:?}",
        runtime.model
    );
}

/// Every `"model"` a native history's JSONL records under `dir`, at the
/// JSON path `pick` selects from each line.
fn recorded_models(dir: &Path, pick: fn(&serde_json::Value) -> Option<&str>) -> Vec<String> {
    let mut models = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "jsonl") {
                for line in std::fs::read_to_string(&path).unwrap_or_default().lines() {
                    let entry: serde_json::Value = serde_json::from_str(line).unwrap_or_default();
                    models.extend(pick(&entry).map(str::to_owned));
                }
            }
        }
    }
    models.sort();
    models.dedup();
    models
}

/// Codex through the production preparation: own work, a denied neighbour,
/// the host's identity, the private Codex home holding the native history,
/// the model that answered read from that history, and a native resume in a
/// new process with the bounded mode restored after the load.
#[tokio::test]
#[ignore = "live: spends two small model turns with the existing Codex login; set BEEKEEPER_LIVE_CODEX_ADAPTER"]
async fn the_installed_codex_runtime_works_inside_the_boundary_and_reaches_nothing_else() {
    let adapter = crate::session::testing::required_tool("BEEKEEPER_LIVE_CODEX_ADAPTER");
    let (captured, _guard) = capture("csp::scope=info,csp::session=warn,acp::stderr=info");
    let home = home_dir().expect("home");
    let root = fixture_root(&home);
    let state_dir = root.join("app/session-provider").join(PROVIDER_PUBKEY);
    let a = root.join("repos/project-a");
    let b_plan = root.join("repos/project-b/plans/plan.md");
    for dir in [&state_dir, &a, &b_plan.parent().expect("p").to_path_buf()] {
        std::fs::create_dir_all(dir).expect("fixture");
    }
    std::fs::write(a.join("README.md"), "A_OWN_CODEX_CANARY\n").expect("own");
    std::fs::write(&b_plan, "B_PLAN_CODEX_CANARY\n").expect("b");

    let adapter_command = adapter.to_string_lossy().into_owned();
    let identity = vec![(
        "BUZZ_RELAY_URL".to_owned(),
        "wss://relay.fixture.invalid".to_owned(),
    )];
    let mut inputs = ScopeInputs::new(ScopePurpose::Session, &state_dir, "live-c1", &a);
    inputs.project_ref = Some("30621:aa:live-project-a");
    inputs.association = WorkspaceAssociation::HostBound;
    inputs.driver = "codex-acp";
    inputs.runtime = RuntimeProfile::Codex;
    inputs.agent_command = &adapter_command;
    inputs.identity_env = &identity;
    let plan = prepare(&inputs).expect("prepared, including the Codex login route");
    let binding = plan.binding().cloned().expect("binding");
    let (tx, mut rx) = mpsc::channel(256);
    let mut manager = SessionManager::new(tx);
    let codex_request = |execution: ExecutionPlan, resume_cursor: Option<String>| CreateRequest {
        media: None,
        seat: None,
        post_fence_env: Vec::new(),
        seat_skills: None,
        target: CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "live".into(),
            session_id: "live-c1".into(),
            generation: 1,
        },
        channel_id: Uuid::nil(),
        cwd: a.clone(),
        title: Some("boundary conformance".into()),
        model: None,
        resume_cursor,
        strict_native: false,
        rehydration_mcp: None,
        agent_command: adapter_command.clone(),
        agent_args: Vec::new(),
        agent_env: Vec::new(),
        idle_timeout: Duration::from_secs(240),
        answer_stall_timeout: None,
        emit_raw_sdk_frames: false,
        max_turn_duration: Duration::from_secs(300),
        idle_shutdown: Duration::from_secs(600),
        include_thoughts: false,
        transcript_paragraph_flush: false,
        execution,
    };
    let started = match manager.create(codex_request(plan, None)).await {
        Ok(started) => started,
        Err(failure) => {
            evidence(
                &root,
                "codex-start-failure.json",
                &serde_json::json!({ "failure": format!("{failure:?}"), "log": log_of(&captured) }),
            );
            panic!("the installed Codex adapter did not start inside the boundary: {failure:?}");
        }
    };
    let items = turn_in(
        &mut manager,
        &mut rx,
        "live-c1",
        &format!(
            "Authorized sandbox conformance test. Run each shell command separately and report \
             each raw output verbatim; do not retry or work around failures.\n\
             1. cat README.md\n\
             2. cat {b}\n\
             3. echo \"relay=$BUZZ_RELAY_URL home=$CODEX_HOME\"\n\
             4. Remember the codeword CODEX_CODEWORD_47.",
            b = b_plan.display()
        ),
    )
    .await;
    let cursor = started.acp_session_id.clone();
    manager.shutdown("live-c1");
    tokio::time::sleep(Duration::from_secs(1)).await;
    let all = serde_json::Value::Array(items.clone()).to_string();

    // Native resume through the same preparation, in a new process. The
    // bounded mode is asserted again on the load; a shell command running
    // shows Codex's own sandbox is not back in its way.
    let mut resumed_inputs = inputs.clone();
    resumed_inputs.prior = Some((Some(&binding), Some(&cursor)));
    let plan = prepare(&resumed_inputs).expect("prepared for resume");
    assert_eq!(plan.native_refusal(), None);
    let (tx, mut rx) = mpsc::channel(256);
    let mut manager = SessionManager::new(tx);
    let resumed = manager
        .create(codex_request(plan, Some(cursor)))
        .await
        .expect("the Codex session resumes inside the boundary");
    let resumed_items = turn_in(
        &mut manager,
        &mut rx,
        "live-c1",
        "Run `cat README.md` and report its output verbatim, then tell me the codeword.",
    )
    .await;
    manager.shutdown("live-c1");
    let resumed_all = serde_json::Value::Array(resumed_items.clone()).to_string();
    let codex_home = std::fs::read_dir(state_dir.join(EXECUTIONS_DIR))
        .expect("executions")
        .flatten()
        .find(|entry| entry.file_name().to_string_lossy().starts_with("live-c1-"))
        .expect("owner dir")
        .path()
        .join("state/codex-home");
    let answered = recorded_models(&codex_home.join("sessions"), |entry| {
        (entry["type"] == "turn_context")
            .then(|| entry["payload"]["model"].as_str())
            .flatten()
    });
    evidence(
        &root,
        "codex-turn.json",
        &serde_json::json!({
            "items": items,
            "resumed_items": resumed_items,
            "continuity": format!("{:?}", resumed.continuity),
            "applied_model": started.model,
            "agent_version": started.agent_version,
            "transcript_models": answered,
            "private_history": codex_home.join("sessions"),
        }),
    );
    assert!(all.contains("A_OWN_CODEX_CANARY"), "own read failed: {all}");
    assert!(
        !all.contains("B_PLAN_CODEX_CANARY"),
        "the neighbour reached the transcript: {all}"
    );
    assert!(
        all.contains("relay=wss://relay.fixture.invalid"),
        "the pinned identity lost: {all}"
    );
    assert!(
        all.contains(&format!("home={}", codex_home.display())),
        "not the private Codex home: {all}"
    );
    assert!(
        !answered.is_empty(),
        "the private Codex history records no answering model"
    );
    assert!(
        matches!(
            resumed.continuity,
            crate::session::SessionContinuity::Resumed | crate::session::SessionContinuity::Loaded
        ),
        "{:?}",
        resumed.continuity
    );
    assert!(
        resumed_all.contains("A_OWN_CODEX_CANARY"),
        "a shell command failed after the load: {resumed_all}"
    );
    assert!(
        resumed_all.contains("CODEX_CODEWORD_47"),
        "the native history was not resumed: {resumed_all}"
    );
}
