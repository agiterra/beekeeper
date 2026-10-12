use super::*;
use sha2::{Digest, Sha256};
fn artifact(root: &Path) -> beekeeper_host_core::native_artifacts::NativeArtifact {
    let path = root.join("selected.jsonl");
    let bytes = b"{\"type\":\"session_meta\",\"payload\":{\"id\":\"fixture\"}}\n";
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(&path, bytes)
        .expect("private fixture");
    beekeeper_host_core::native_artifacts::NativeArtifact {
        source_path: path,
        relative_path: PathBuf::from("sessions/2026/10/11/selected.jsonl"),
        bytes: bytes.len() as u64,
        sha256: hex::encode(Sha256::digest(bytes)),
    }
}
#[test]
fn selected_history_is_hash_verified_and_appended_owned_history_is_not_replaced() {
    let root = tempfile::tempdir().expect("root");
    let home = root.path().join("runtime-home");
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&home).expect("private home");
    let selected = artifact(root.path());
    let mut bad = selected.clone();
    bad.sha256 = "00".repeat(32);
    assert!(materialize_artifact(&bad, &home).is_err());
    assert!(!home.join(&selected.relative_path).exists());
    materialize_artifact(&selected, &home).expect("selected import");
    let target = home.join(&selected.relative_path);
    assert_eq!(
        std::fs::read(&target).expect("history"),
        std::fs::read(&selected.source_path).expect("source")
    );
    let mut current = std::fs::read(&target).expect("existing owned history");
    current.extend_from_slice(b"{\"type\":\"fixture-new-native-message\"}\n");
    std::fs::write(&target, &current).expect("current native history");
    materialize_artifact(&selected, &home).expect("resume preserves history");
    assert_eq!(std::fs::read(target).expect("current"), current);
}
#[cfg(unix)]
#[test]
fn selected_history_refuses_parent_traversal_and_symlinked_runtime_destination() {
    let root = tempfile::tempdir().expect("root");
    let home = root.path().join("runtime-home");
    let outside = root.path().join("outside");
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&home).expect("home");
    std::fs::create_dir(&outside).expect("outside");
    let mut selected = artifact(root.path());
    selected.relative_path = PathBuf::from("../outside/leak.jsonl");
    assert!(materialize_artifact(&selected, &home).is_err());
    assert!(!outside.join("leak.jsonl").exists());
    selected.relative_path = PathBuf::from("sessions/leak.jsonl");
    std::os::unix::fs::symlink(&outside, home.join("sessions")).expect("hostile link");
    assert!(materialize_artifact(&selected, &home).is_err());
    assert!(!outside.join("leak.jsonl").exists());
}
#[test]
fn private_skill_source_pair_is_required_even_for_attributed_solo() {
    let root = tempfile::tempdir().expect("root");
    let settings = Settings {
        record: serde_json::json!({"persona_team_dir":root.path()}),
    };
    assert!(settings.skills(root.path(), "session").is_err());
    assert!(Settings {
        record: serde_json::json!({})
    }
    .skills(root.path(), "session")
    .expect("no pack")
    .is_none());
}
#[test]
fn managed_slot_preserves_actual_custom_command_label_and_conservative_capabilities() {
    let mut descriptor = beekeeper_host_core::managed_runtime::descriptor();
    let capabilities = descriptor.capabilities;
    let settings = Settings {
        record: serde_json::json!({"effective_command":"/private/custom-adapter","effective_args":["--acp"],"runtime":"custom-runtime","effective_env":{"CUSTOM_OPTION":"preserved"}}),
    };
    settings
        .apply_runtime(&mut descriptor)
        .expect("configured adapter");
    assert_eq!(descriptor.agent_command, "/private/custom-adapter");
    assert_eq!(descriptor.agent_args, ["--acp"]);
    assert_eq!(descriptor.runtime, "custom-runtime");
    assert_eq!(descriptor.capabilities, capabilities);
    assert_eq!(
        settings.environment().expect("env"),
        [("CUSTOM_OPTION".into(), "preserved".into())]
    );
    assert!(Settings {
        record: serde_json::json!({"runtime":"custom-runtime"})
    }
    .apply_runtime(&mut descriptor)
    .is_err());
}
#[test]
fn selected_solo_persona_skills_materialize_through_existing_bundle_machinery() {
    let root = tempfile::tempdir().expect("fixture");
    let pack = root.path().join("pack");
    std::fs::create_dir_all(pack.join(".plugin")).expect("manifest dir");
    std::fs::create_dir_all(pack.join("personas")).expect("personas");
    std::fs::create_dir_all(pack.join("skills/ordinary")).expect("skill");
    std::fs::write(pack.join(".plugin/plugin.json"),r#"{"id":"com.test.solo","name":"Solo","version":"1.0.0","personas":["personas/helper.persona.md"]}"#).expect("manifest");
    std::fs::write(pack.join("personas/helper.persona.md"),"---\nname: helper\ndisplay_name: Helper\ndescription: Helps ordinary work.\nskills:\n  - skills/ordinary\n---\nOrdinary conversation instructions.\n").expect("persona");
    std::fs::write(
        pack.join("skills/ordinary/SKILL.md"),
        "# Ordinary skill\nPreserved selected helper instructions.\n",
    )
    .expect("skill bytes");
    let settings = Settings {
        record: serde_json::json!({"persona_team_dir":pack,"persona_name_in_team":"helper"}),
    };
    let skills = settings
        .skills(root.path(), "session")
        .expect("private selected source")
        .expect("skills");
    let persona =
        beekeeper_persona::resolve::resolve_persona_by_name(&skills.pack_dir, &skills.persona_id)
            .expect("existing pack resolver");
    assert!(persona.role.is_none());
    let destination = skills.bundle_dir.join("skills");
    beekeeper_persona::skills::materialize_skill_bundle(&persona, &destination)
        .expect("existing runtime skill machinery");
    assert_eq!(
        std::fs::read_to_string(destination.join("ordinary/SKILL.md")).expect("materialized"),
        "# Ordinary skill\nPreserved selected helper instructions.\n"
    );
}

#[test]
fn effective_catalog_mcp_selection_refuses_malformed_or_missing_tools_and_ignores_deprecated_field()
{
    assert!(Settings {
        record: serde_json::json!({"mcp_command":"/untrusted/deprecated"})
    }
    .mcp_command()
    .expect("deprecated field ignored")
    .is_none());
    assert!(Settings {
        record: serde_json::json!({"effective_mcp_command":false})
    }
    .mcp_command()
    .is_err());
    let dir = tempfile::tempdir().expect("fixture");
    assert!(Settings {
        record: serde_json::json!({"effective_mcp_command":dir.path().join("missing")})
    }
    .mcp_command()
    .is_err());
}

#[test]
fn claude_native_destination_uses_existing_scoped_config_without_codex_home() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("claude-config");
    std::fs::create_dir(&config).unwrap();
    let mut env = beekeeper_acp::exec_env::ResolvedEnv::default();
    env.scope("CLAUDE_CONFIG_DIR", config.to_str().unwrap())
        .unwrap();
    assert_eq!(
        native_destination(crate::execution_scope::RuntimeProfile::Claude, None, &env).unwrap(),
        config
    );
    assert!(native_destination(crate::execution_scope::RuntimeProfile::Codex, None, &env).is_err());
    env.remove("CLAUDE_CONFIG_DIR");
    assert!(
        native_destination(crate::execution_scope::RuntimeProfile::Claude, None, &env).is_err()
    );
}
#[test]
fn claude_native_import_preserves_scoped_home_cursor_and_workspace_without_codex() {
    let root = tempfile::tempdir().unwrap();
    let host = crate::tests::provider(root.path(), None);
    let channel = uuid::Uuid::new_v4();
    let actor = "ab".repeat(32);
    let cursor = "11111111-1111-4111-8111-111111111111";
    let cwd = root.path().join("workspace");
    let home = root.path().join("home");
    let config_home = root.path().join("claude");
    for path in [&cwd, &home, &config_home] {
        std::fs::create_dir(path).unwrap();
    }
    let mut runtime = host.config.runtimes[0].clone();
    runtime.runtime = "claude".into();
    runtime.agent_command = "claude-agent-acp".into();
    let selected = artifact(root.path());
    let mut value = serde_json::json!({"schema":"beekeeper-legacy-native-context/v1","actorPubkey":actor,"relayUrl":host.config.relay_url,"sessions":[{"channelId":channel,"runtimeName":"claude","runtimeCommand":"claude-agent-acp","resumeCursor":cursor,"cwd":cwd,"sourceHome":home,"runtimeHomeEnvironment":{"HOME":home,"CLAUDE_CONFIG_DIR":config_home},"nativeConfigHome":config_home,"nativeArtifacts":[selected]}]});
    let path = host.config.state_dir.join("native-import-test.json");
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    let imported = native(&host.config, Some(&actor), "test", channel, &cwd, &runtime)
        .unwrap()
        .unwrap();
    assert_eq!(imported.0, cursor);
    assert!(imported.1.iter().any(|(key, _)| key == "CLAUDE_CONFIG_DIR"));
    assert!(!imported.1.iter().any(|(key, _)| key == "CODEX_HOME"));
    value["sessions"][0]["nativeConfigHome"] = serde_json::json!(home);
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(
        &path,
        &serde_json::to_vec(&value).unwrap(),
    )
    .unwrap();
    assert!(native(&host.config, Some(&actor), "test", channel, &cwd, &runtime).is_err());
}
