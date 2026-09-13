use super::*;

const OWNER: &str = "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";

fn scope(project: &str, relay: &str) -> SetupScope {
    SetupScope::new(&format!("30621:{OWNER}:{project}"), OWNER, relay).expect("scope")
}

fn seed(root: &Path) -> PathBuf {
    let seed = root.join("baseline");
    let role = seed.join("lead");
    std::fs::create_dir_all(role.join(".plugin")).expect("manifest dir");
    std::fs::create_dir_all(role.join("personas")).expect("persona dir");
    std::fs::write(
        role.join(".plugin/plugin.json"),
        r#"{
        "id":"com.example.lead","name":"Lead","version":"1.0.0",
        "description":"Project lead baseline", "personas":["personas/lead.persona.md"]
    }"#,
    )
    .expect("manifest");
    std::fs::write(role.join("personas/lead.persona.md"),
        "---\nname: lead\nrole: lead\ndisplay_name: Lead\ndescription: Project lead\n---\nMaintain this project.\n")
        .expect("persona");
    seed
}

fn repository(root: &Path) -> PathBuf {
    let project = root.join("project");
    std::fs::create_dir_all(&project).expect("project");
    let auth = crate::commands::project_git_exec::build_local_git_auth_config().expect("git");
    crate::commands::project_git_exec::run_git(&["init", "--quiet"], Some(&project), &auth)
        .expect("init");
    project
}

#[test]
fn read_is_empty_without_creating_storage() {
    let dir = tempfile::tempdir().expect("temp");
    let root = dir.path().join("storage");
    assert!(read_draft(&root, &scope("one", "wss://one.example"))
        .expect("read")
        .is_none());
    assert!(!root.exists());
}

#[test]
fn retries_preserve_edited_bytes_and_changed_input_does_not_replace_draft() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let root = dir.path().join("storage");
    let scope = scope("one", "wss://one.example");
    let first = prepare(&root, &scope, "Build a garden", &project, &seed).expect("prepare");
    let edited = Path::new(&first.roles_directory).join("lead/instructions.md");
    std::fs::write(&edited, "Project-specific changes").expect("edit");
    let second = prepare(&root, &scope, "Build a garden", &project, &seed).expect("retry");
    assert_eq!(first.setup_id, second.setup_id);
    assert_eq!(
        std::fs::read_to_string(edited).expect("read"),
        "Project-specific changes"
    );
    assert_eq!(
        prepare(&root, &scope, "Different intent", &project, &seed)
            .expect_err("refuse")
            .code,
        "existing_draft"
    );
    assert_eq!(
        read_draft(&root, &scope)
            .expect("read")
            .expect("draft")
            .intent,
        "Build a garden"
    );
    assert!(!project.join("personas").exists());
}

#[test]
fn project_community_and_identity_are_isolated() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let root = dir.path().join("storage");
    let first = prepare(
        &root,
        &scope("one", "wss://one.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("first");
    let second = prepare(
        &root,
        &scope("two", "wss://one.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("second");
    let third = prepare(
        &root,
        &scope("one", "wss://two.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("third");
    assert_ne!(first.roles_directory, second.roles_directory);
    assert_ne!(first.roles_directory, third.roles_directory);
    let another = nostr::Keys::generate().public_key().to_hex();
    let other_scope =
        SetupScope::new(&first.project_ref, &another, &first.relay_url).expect("scope");
    assert!(read_draft(&root, &other_scope).expect("read").is_none());
}

#[test]
fn rejects_non_repository_subfolder_and_oversize_intent_before_writing() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let root = dir.path().join("storage");
    let scope = scope("one", "wss://one.example");
    assert!(prepare(&root, &scope, "Intent", dir.path(), &seed).is_err());
    let project = repository(dir.path());
    let child = project.join("child");
    std::fs::create_dir(&child).expect("child");
    assert!(prepare(&root, &scope, "Intent", &child, &seed).is_err());
    assert!(prepare(
        &root,
        &scope,
        &"a".repeat(MAX_INTENT_BYTES + 1),
        &project,
        &seed
    )
    .is_err());
    assert!(!root.exists());
}

#[test]
fn validator_checks_actual_pack_and_expected_identity_without_advancing_status() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let root = dir.path().join("storage");
    let record = prepare(
        &root,
        &scope("one", "wss://one.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("prepare");
    let result = tree::validate(&record);
    assert!(result.valid, "{:?}", result.diagnostics);
    assert_eq!(result.status, SetupStatus::Draft);
    let persona = Path::new(&record.roles_directory).join("lead/personas/lead.persona.md");
    let original = std::fs::read_to_string(&persona).expect("persona");
    std::fs::write(&persona, original.replace("role: lead", "role: builder")).expect("change");
    assert!(!tree::validate(&record).valid);
    std::fs::write(&persona, original.replace("name: lead", "name: changed")).expect("change");
    assert!(!tree::validate(&record).valid);
    std::fs::write(&persona, "not a persona").expect("change");
    assert!(!tree::validate(&record).valid);
}

#[test]
fn validation_refuses_oversized_files_and_extra_role_directories() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let record = prepare(
        &dir.path().join("storage"),
        &scope("one", "wss://one.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("prepare");
    let extra = Path::new(&record.draft_directory).join("large.txt");
    std::fs::write(&extra, vec![0_u8; 256 * 1024 + 1]).expect("large");
    assert!(!tree::validate(&record).valid);
    std::fs::remove_file(extra).expect("remove");
    std::fs::create_dir(Path::new(&record.roles_directory).join("unrecognized"))
        .expect("extra role");
    assert!(!tree::validate(&record).valid);
}

#[cfg(unix)]
#[test]
fn validation_and_read_refuse_symlinks_without_touching_target() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let root = dir.path().join("storage");
    let scope = scope("one", "wss://one.example");
    let record = prepare(&root, &scope, "Intent", &project, &seed).expect("prepare");
    let outside = dir.path().join("outside");
    std::fs::write(&outside, "preserved").expect("outside");
    symlink(
        &outside,
        Path::new(&record.roles_directory).join("lead/linked"),
    )
    .expect("link");
    assert!(!tree::validate(&record).valid);
    assert_eq!(
        std::fs::read_to_string(&outside).expect("read"),
        "preserved"
    );
    let draft = Path::new(&record.draft_directory);
    let saved = draft.with_file_name("saved");
    std::fs::rename(draft, &saved).expect("rename");
    symlink(&saved, draft).expect("link draft");
    assert!(read_draft(&root, &scope).is_err());
}

#[test]
fn read_refuses_a_record_claiming_another_project() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let root = dir.path().join("storage");
    let scope = scope("one", "wss://one.example");
    let mut record = prepare(&root, &scope, "Intent", &project, &seed).expect("prepare");
    record.project_ref = format!("30621:{OWNER}:other");
    std::fs::write(
        Path::new(&record.draft_directory)
            .parent()
            .expect("parent")
            .join("record.json"),
        serde_json::to_vec(&record).expect("json"),
    )
    .expect("record");
    assert!(read_draft(&root, &scope).is_err());
}

fn add_role(roles: &Path, name: &str) {
    let role = roles.join(name);
    tree::copy_tree(&roles.join("lead"), &role).expect("copy role");
    let manifest = role.join(".plugin/plugin.json");
    let source = std::fs::read_to_string(&manifest).expect("manifest");
    std::fs::write(manifest, source.replace("lead", name)).expect("new identity");
    let persona = role.join("personas/lead.persona.md");
    let source = std::fs::read_to_string(&persona).expect("persona");
    std::fs::remove_file(persona).expect("remove old persona");
    std::fs::write(
        role.join(format!("personas/{name}.persona.md")),
        source.replace("lead", name),
    )
    .expect("new persona");
}

#[test]
fn adaptation_can_drop_optional_roles_and_add_roles_but_keeps_retained_identity() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    add_role(&seed, "builder");
    let project = repository(dir.path());
    let record = prepare(
        &dir.path().join("storage"),
        &scope("one", "wss://one.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("prepare");
    let roles = Path::new(&record.roles_directory);
    std::fs::remove_dir_all(roles.join("builder")).expect("remove optional role");
    add_role(roles, "researcher");
    let validated = tree::validate(&record);
    assert!(validated.valid, "{:?}", validated.diagnostics);
    assert_eq!(validated.roles, ["lead", "researcher"]);
    let manifest = roles.join("lead/.plugin/plugin.json");
    let source = std::fs::read_to_string(&manifest).expect("manifest");
    std::fs::write(
        manifest,
        source.replace("com.example.lead", "com.example.replacement"),
    )
    .expect("replace identity");
    assert!(!tree::validate(&record).valid);
    std::fs::remove_dir_all(roles.join("lead")).expect("remove lead");
    assert!(!tree::validate(&record).valid);
}

#[test]
fn validation_rejects_external_references_missing_skills_and_executable_configuration() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let record = prepare(
        &dir.path().join("storage"),
        &scope("one", "wss://one.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("prepare");
    let role = Path::new(&record.roles_directory).join("lead");
    let persona = role.join("personas/lead.persona.md");
    let original = std::fs::read_to_string(&persona).expect("persona");
    for reference in ["../outside", "/outside", "C:\\outside", "skills/missing"] {
        let content = original.replacen(
            "role: lead",
            &format!("role: lead\nskills:\n  - '{reference}'"),
            1,
        );
        std::fs::write(&persona, content).expect("reference");
        assert!(!tree::validate(&record).valid, "{reference}");
    }
    std::fs::write(
        &persona,
        original.replace("role: lead", "role: lead\nhooks:\n  on_start: ../outside"),
    )
    .expect("hook");
    assert!(!tree::validate(&record).valid);
    std::fs::write(&persona, &original).expect("reset");
    let manifest = role.join(".plugin/plugin.json");
    let original_manifest = std::fs::read_to_string(&manifest).expect("manifest");
    for (key, value) in [
        ("pack_instructions", "../outside"),
        ("mcp_config", "/outside"),
        ("hooks_config", "hooks.json"),
    ] {
        let mut changed: serde_json::Value =
            serde_json::from_str(&original_manifest).expect("json");
        changed[key] = value.into();
        std::fs::write(&manifest, serde_json::to_vec(&changed).expect("json")).expect("write");
        assert!(!tree::validate(&record).valid, "{key}");
    }
}

#[test]
fn validation_rejects_malformed_shared_skill_metadata() {
    let dir = tempfile::tempdir().expect("temp");
    let seed = seed(dir.path());
    let project = repository(dir.path());
    let record = prepare(
        &dir.path().join("storage"),
        &scope("one", "wss://one.example"),
        "Intent",
        &project,
        &seed,
    )
    .expect("prepare");
    let skill = Path::new(&record.roles_directory).join("lead/skills/shared");
    std::fs::create_dir_all(&skill).expect("skill");
    std::fs::write(skill.join("SKILL.md"), "No metadata").expect("write");
    assert!(!tree::validate(&record).valid);
}

#[test]
fn actual_neutral_baseline_prepares_and_validates() {
    let dir = tempfile::tempdir().expect("temp");
    let project = repository(dir.path());
    let seed = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/roles");
    let record = prepare(
        &dir.path().join("storage"),
        &scope("one", "wss://one.example"),
        "Project-specific intent",
        &project,
        &seed,
    )
    .expect("prepare actual baseline");
    assert!(record.roles.iter().any(|role| role == "project-setup"));
    let validated = tree::validate(&record);
    assert!(validated.valid, "{:?}", validated.diagnostics);
}

/// Write a bound draft record exactly where `read_draft` looks for it, without
/// a seed or repository. `relay` is canonicalized as the scope stores it.
pub(super) fn write_bound_draft(
    root: &Path,
    owner: &str,
    relay: &str,
    slug: &str,
) -> ProjectTeamSetupDraft {
    std::fs::create_dir_all(root).expect("storage root");
    let root = std::fs::canonicalize(root).expect("canonical root");
    let scope = SetupScope::new(&format!("30621:{owner}:{slug}"), owner, relay).expect("scope");
    let directory = scope.directory(&root);
    std::fs::create_dir_all(directory.join("draft/personas/roles")).expect("draft tree");
    let record = ProjectTeamSetupDraft {
        setup_id: uuid::Uuid::new_v4().to_string(),
        project_ref: scope.project.clone(),
        project_directory: root.join("project").to_string_lossy().into_owned(),
        draft_directory: directory.join("draft").to_string_lossy().into_owned(),
        roles_directory: directory
            .join("draft/personas/roles")
            .to_string_lossy()
            .into_owned(),
        status: SetupStatus::Draft,
        intent: "Build a garden".to_string(),
        owner_pubkey: scope.owner.clone(),
        relay_url: scope.relay.clone(),
        roles: Vec::new(),
        expected_roles: Vec::new(),
        created_at: "2026-09-13T00:00:00Z".to_string(),
        latest_snapshot_id: None,
    };
    std::fs::write(
        directory.join("record.json"),
        serde_json::to_vec_pretty(&record).expect("record json"),
    )
    .expect("record");
    record
}
