//! Project setup uses the editable draft as its seated working directory.
//! These fixtures exercise the real provider transport and fence computation;
//! the scripted child is not evidence of model judgment or SDK enforcement.

use super::*;
use crate::agent_fence::{write_fence_rules, WriteFenceLayout};

fn edit_rule(path: &Path, directory: bool) -> String {
    format!(
        "Edit(//{}{})",
        path.to_string_lossy().trim_start_matches('/'),
        if directory { "/**" } else { "" }
    )
}

#[test]
fn setup_draft_is_writable_while_existing_private_siblings_are_denied() {
    let temp = tempfile::tempdir().expect("fixture");
    let home = temp.path().join("home");
    let app = home.join("Library/Application Support/io.agiterra.beekeeper.app");
    let scope = app.join("project-team-setup/scope");
    let draft = scope.join("draft");
    let roles = draft.join("personas/roles");
    let bootstrap = scope.join("bootstrap");
    let old_workspace = scope.join("authoring-workspace");
    for directory in [&roles, &bootstrap, &old_workspace] {
        std::fs::create_dir_all(directory).expect("fixture directory");
    }
    let journal = scope.join("launch.json");
    let custody = scope.join("setup-actor.enc");
    std::fs::write(&journal, "private launch state").expect("journal");
    std::fs::write(&custody, "encrypted identity receipt").expect("custody");
    let state = app.join("session-provider/provider");
    let actor = "a".repeat(64);
    let rules = write_fence_rules(&WriteFenceLayout::new(&draft, &home, Some(&state), &actor))
        .expect("draft fence");

    // No concrete ancestor or descendant of the editable draft is denied.
    for ancestor in roles.ancestors() {
        assert!(!rules.contains(&edit_rule(ancestor, true)), "{rules:?}");
    }
    assert!(rules.contains(&edit_rule(&journal, false)));
    assert!(rules.contains(&edit_rule(&custody, false)));
    assert!(rules.contains(&edit_rule(&bootstrap, true)));
    // Every rule is a write rule: since 2026-09-18 the code repository holds
    // no team definitions, so the fence writes nothing inside the seat's
    // own tree (spec § 4.10, struck).
    assert!(rules.iter().all(|rule| rule.starts_with("Edit(")));

    // The previous sibling CWD really did fence out the entire editable draft.
    let previous = write_fence_rules(&WriteFenceLayout::new(
        &old_workspace,
        &home,
        Some(&state),
        &actor,
    ))
    .expect("previous fence");
    assert!(previous.contains(&edit_rule(&draft, true)));
}

const SETUP_AGENT: &str = r#"
set -eu
while IFS= read -r line; do
  printf '%s\n' "$line" >> "$SETUP_TEST_LOG/requests.jsonl"
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{"jsonrpc":"2.0","id":%s,"result":{"protocolVersion":1,"agentInfo":{"name":"%s"}}}\n' "$id" "$SETUP_TEST_AGENT_NAME" ;;
    *'"method":"session/new"'*)
      setup_cwd=$(printf '%s' "$line" | sed -n 's/.*"cwd":"\([^"]*\)".*/\1/p')
      cd "$setup_cwd"
      pwd -P > "$SETUP_TEST_LOG/cwd"
      cat "$SETUP_TEST_BUNDLE/skills/setup-project/SKILL.md" > "$SETUP_TEST_LOG/skill"
      cat .claude/settings.local.json > "$SETUP_TEST_LOG/fence.json"
      printf '{"jsonrpc":"2.0","id":%s,"result":{"sessionId":"setup-acp-session"}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      cat PROJECT_TEAM_SETUP.md > "$SETUP_TEST_LOG/brief"
      repository=$(sed -n 's/^Repository: //p' PROJECT_TEAM_SETUP.md)
      cat "$repository/inspection.txt" > "$SETUP_TEST_LOG/inspection"
      printf 'fixture-authored procedure\n' > personas/roles/lead/procedure.md
      printf '{"jsonrpc":"2.0","id":%s,"result":{"stopReason":"end_turn"}}\n' "$id" ;;
  esac
done
"#;

#[tokio::test]
async fn setup_first_turn_reaches_real_child_with_draft_cwd_brief_and_shipped_role() {
    let temp = tempfile::tempdir().expect("fixture");
    let scope = temp.path().join("setup");
    let draft = scope.join("draft");
    let repository = temp.path().join("project-repository");
    let log = temp.path().join("log");
    for directory in [
        draft.join("personas/roles/lead"),
        repository.clone(),
        log.clone(),
    ] {
        std::fs::create_dir_all(directory).expect("fixture directory");
    }
    std::fs::write(repository.join("inspection.txt"), "observed project fact")
        .expect("repository evidence");
    let brief = format!(
        "Repository: {}\nAdapt the draft role procedures; do not publish.\n",
        repository.display()
    );
    std::fs::write(draft.join("PROJECT_TEAM_SETUP.md"), &brief).expect("host brief");
    std::fs::write(scope.join("setup-actor.enc"), "private receipt").expect("receipt");
    // The shipped pack is thin since 2026-09-18 — two include lines whose
    // text and skills live in the shipped template catalog — and a host
    // never hands the provider a pack it has not composed. Compose it the way
    // the host stages it, into a directory the seat keeps for its life.
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("repository root");
    let pack = temp.path().join("staged/project-setup");
    let catalog =
        buzz_persona::template::TemplateCatalog::load(&shipped.join("personas/templates"), "test")
            .expect("the shipped catalog loads");
    let composed = buzz_persona::compose::compose_role(
        &buzz_persona::compose::RoleSource::Pack {
            dir: shipped.join("personas/roles/project-setup"),
            role: "project-setup".into(),
            persona: None,
        },
        &catalog,
        &buzz_persona::compose::ComposeOptions::local("personas/roles/project-setup"),
    )
    .expect("the shipped setup role composes");
    buzz_persona::compose::write_staged_pack(&composed, &pack).expect("staged pack");
    // The seat's skills live in its own bundle, outside the draft it writes
    // into — the child reads them by absolute path, as its briefing names them.
    let bundle_dir = crate::session::seat_bundle_dir(
        &temp.path().join("app/session-provider/abc"),
        "setup-session",
    );
    let agent = testing::fake_agent(temp.path(), "setup-agent", SETUP_AGENT);
    let (tx, mut rx) = mpsc::channel(64);
    let mut manager = SessionManager::new(tx);
    let startup = manager
        .create(CreateRequest {
            target: CodingSessionTarget {
                driver: crate::agent_fence::CLAUDE_DRIVER.into(),
                instance_id: "local-instance".into(),
                session_id: "setup-session".into(),
                generation: 1,
            },
            channel_id: Uuid::nil(),
            cwd: draft.clone(),
            title: Some("Project team setup".into()),
            model: None,
            resume_cursor: None,
            strict_native: false,
            rehydration_mcp: None,
            agent_command: "bash".into(),
            agent_args: vec![agent],
            agent_env: vec![
                ("SETUP_TEST_LOG".into(), log.to_string_lossy().into_owned()),
                (
                    "SETUP_TEST_AGENT_NAME".into(),
                    buzz_acp::acp::CLAUDE_AGENT_ACP_NAME.into(),
                ),
                (
                    "SETUP_TEST_BUNDLE".into(),
                    bundle_dir.to_string_lossy().into_owned(),
                ),
            ],
            seat: Some(SeatIdentity {
                actor_pubkey: "a".repeat(64),
                role: "project-setup".into(),
                relay_url: "wss://relay.test".into(),
            }),
            post_fence_env: Vec::new(),
            seat_skills: Some(SeatSkills {
                pack_dir: pack.clone(),
                persona_id: "project-setup".into(),
                bundle_dir: bundle_dir.clone(),
                pack_ref: None,
                compose_ref: None,
                agents_checkout: None,
            }),
            media: None,
            idle_timeout: Duration::from_secs(5),
            answer_stall_timeout: None,
            emit_raw_sdk_frames: false,
            max_turn_duration: Duration::from_secs(10),
            idle_shutdown: Duration::from_secs(30),
            include_thoughts: true,
        })
        .await
        .expect("real seated provider create");
    assert_eq!(startup.acp_session_id, "setup-acp-session");
    let initial_turn = "Read PROJECT_TEAM_SETUP.md in this execution’s working directory, then build the project’s draft role packs as instructed. Report evidence and limitations; do not publish or change project access.";
    manager
        .handle("setup-session")
        .expect("session")
        .deliver(SessionCommand::Turn {
            command_id: "setup-initial-turn".into(),
            text: initial_turn.into(),
            attachments: Vec::new(),
            operator_pubkey: None,
            framing: None,
        })
        .expect("first turn");
    let outcome = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await.expect("provider event") {
                SessionEvent::TurnFinished { outcome, .. } => break outcome,
                SessionEvent::Exited { reason, .. } => panic!("child exited early: {reason:?}"),
                _ => {}
            }
        }
    })
    .await
    .expect("bounded first turn");
    manager.shutdown("setup-session");
    assert_eq!(
        outcome,
        TurnOutcome::Completed {
            stop_reason: StopReason::EndTurn
        }
    );

    let requests: Vec<serde_json::Value> = std::fs::read_to_string(log.join("requests.jsonl"))
        .expect("real ACP requests")
        .lines()
        .map(|line| serde_json::from_str(line).expect("request JSON"))
        .collect();
    let opened = requests
        .iter()
        .find(|r| r["method"] == "session/new")
        .expect("session/new");
    assert_eq!(
        Path::new(opened["params"]["cwd"].as_str().expect("ACP cwd"))
            .canonicalize()
            .expect("cwd"),
        draft.canonicalize().expect("draft")
    );
    let prompt = opened["params"]["_meta"]["systemPrompt"]["append"]
        .as_str()
        .expect("Claude system prompt");
    assert!(prompt.contains("seated with the role \"project-setup\""));
    assert!(prompt.contains("Turn the project's intent into useful, versioned working procedures."));
    assert!(prompt.contains(&format!(
        "setup-project — {}",
        bundle_dir
            .join("skills/setup-project/SKILL.md")
            .canonicalize()
            .expect("the skill the seat was briefed on")
            .display()
    )));
    assert!(
        !draft.join(".agents").exists(),
        "the setup draft must hold no materialized pack"
    );
    let turn = requests
        .iter()
        .find(|r| r["method"] == "session/prompt")
        .expect("session/prompt");
    assert_eq!(turn["params"]["prompt"][0]["text"], initial_turn);
    assert_eq!(
        std::fs::read_to_string(log.join("cwd"))
            .expect("child cwd")
            .trim(),
        draft.canonicalize().expect("draft").to_string_lossy()
    );
    assert_eq!(
        std::fs::read_to_string(log.join("brief")).expect("child brief"),
        brief
    );
    assert_eq!(
        std::fs::read(log.join("skill")).expect("child skill"),
        std::fs::read(pack.join("skills/setup-project/SKILL.md")).expect("shipped skill")
    );
    assert_eq!(
        std::fs::read_to_string(log.join("inspection")).expect("project read"),
        "observed project fact"
    );
    assert_eq!(
        std::fs::read_to_string(draft.join("personas/roles/lead/procedure.md"))
            .expect("draft write"),
        "fixture-authored procedure\n"
    );
    let fence: serde_json::Value =
        serde_json::from_slice(&std::fs::read(log.join("fence.json")).expect("child fence"))
            .expect("settings JSON");
    assert!(!fence["permissions"]["deny"]
        .as_array()
        .expect("deny rules")
        .is_empty());
    assert_eq!(
        std::fs::read_to_string(scope.join("setup-actor.enc")).expect("private receipt"),
        "private receipt"
    );
}
