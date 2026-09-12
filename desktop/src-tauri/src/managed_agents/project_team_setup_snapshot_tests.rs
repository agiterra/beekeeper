use super::*;

const OWNER: &str = "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";

struct Fixture {
    _temp: tempfile::TempDir,
    root: PathBuf,
    scope: SetupScope,
    record: ProjectTeamSetupDraft,
}

impl Fixture {
    fn new() -> Self {
        let temp = tempfile::tempdir().expect("temp");
        let seed = temp.path().join("seed");
        let lead = seed.join("lead");
        std::fs::create_dir_all(lead.join(".plugin")).expect("manifest dir");
        std::fs::create_dir_all(lead.join("personas")).expect("persona dir");
        std::fs::create_dir_all(lead.join("skills/plan")).expect("skill dir");
        std::fs::write(lead.join(".plugin/plugin.json"),
            r#"{"id":"example.lead","name":"Lead","version":"1","personas":["personas/lead.persona.md"]}"#)
            .expect("manifest");
        std::fs::write(lead.join("personas/lead.persona.md"),
            "---\nname: lead\nrole: lead\ndisplay_name: Lead\ndescription: Lead the project\nskills: [\"./skills/plan/\"]\n---\nLead.\n")
            .expect("persona");
        std::fs::write(
            lead.join("skills/plan/SKILL.md"),
            "---\nname: plan\ndescription: Plan project work\n---\nUse project evidence.\n",
        )
        .expect("skill");
        let project = temp.path().join("project");
        std::fs::create_dir(&project).expect("project dir");
        let auth = crate::commands::project_git_exec::build_local_git_auth_config().expect("auth");
        crate::commands::project_git_exec::run_git(&["init", "--quiet"], Some(&project), &auth)
            .expect("git init");
        let root = temp.path().join("storage");
        let scope = SetupScope::new(
            &format!("30621:{OWNER}:garden"),
            OWNER,
            "wss://example.test",
        )
        .expect("scope");
        let record = super::super::prepare(&root, &scope, "Grow a garden", &project, &seed)
            .expect("prepare");
        Self {
            _temp: temp,
            root,
            scope,
            record,
        }
    }

    fn freeze(&self) -> Result<ProjectTeamSetupSnapshot, SetupError> {
        run(&self.root, &self.scope, &self.record.setup_id, None)
    }

    fn verify(&self, id: &str) -> Result<ProjectTeamSetupSnapshot, SetupError> {
        run(&self.root, &self.scope, &self.record.setup_id, Some(id))
    }

    fn skill(&self) -> PathBuf {
        Path::new(&self.record.roles_directory).join("lead/skills/plan/SKILL.md")
    }
}

#[test]
fn stable_content_reuses_a_verified_candidate_without_timestamp_hash_drift() {
    let fixture = Fixture::new();
    let first = fixture.freeze().expect("snapshot");
    let mut record = fixture.record.clone();
    record.created_at = "a later date".into();
    let second = create(&record).expect("same bytes");
    assert_eq!(first.snapshot_id, second.snapshot_id);
    assert_eq!(first.roles_directory, second.roles_directory);
    assert_eq!(first.snapshot_id.len(), 64);
    assert!(!Path::new(&first.manifest_path).starts_with(&fixture.record.draft_directory));
    let bytes = std::fs::read(&first.manifest_path).expect("manifest");
    assert_eq!(hash(&bytes), first.snapshot_id);
    let manifest: Manifest = serde_json::from_slice(&bytes).expect("decode");
    assert_eq!(manifest.files.len(), 3);
    assert!(manifest
        .files
        .iter()
        .all(|entry| entry.path.starts_with("lead/")));
}

#[test]
fn source_edits_create_another_candidate_and_cannot_change_the_previous_bytes() {
    let fixture = Fixture::new();
    let first = fixture.freeze().expect("first");
    let original = std::fs::read(fixture.skill()).expect("source");
    let mut updated = original.clone();
    updated.extend_from_slice(b"\nA new project procedure.\n");
    std::fs::write(fixture.skill(), updated).expect("edit");
    let second = fixture.freeze().expect("second");
    assert_ne!(first.snapshot_id, second.snapshot_id);
    assert_eq!(
        std::fs::read(Path::new(&first.roles_directory).join("lead/skills/plan/SKILL.md"))
            .expect("old bytes"),
        original
    );
    fixture
        .verify(&first.snapshot_id)
        .expect("old candidate still valid");
}

#[test]
fn failed_pack_validation_or_size_limit_preserves_previous_snapshot() {
    let fixture = Fixture::new();
    let first = fixture.freeze().expect("first");
    std::fs::write(fixture.skill(), "missing skill metadata").expect("invalid edit");
    assert!(fixture.freeze().is_err());
    std::fs::write(fixture.skill(), vec![b'x'; MAX_FILE_BYTES as usize + 1]).expect("large edit");
    assert!(fixture.freeze().is_err());
    fixture
        .verify(&first.snapshot_id)
        .expect("previous survives");
    assert_eq!(
        std::fs::read_dir(storage(&fixture.record).expect("storage"))
            .expect("read")
            .count(),
        1
    );
}

#[test]
fn tampered_manifest_or_candidate_bytes_fail_verification_and_are_not_overwritten() {
    let fixture = Fixture::new();
    let candidate = fixture.freeze().expect("snapshot");
    let manifest = std::fs::read(&candidate.manifest_path).expect("read");
    let mut altered = manifest.clone();
    altered.push(b' ');
    std::fs::write(&candidate.manifest_path, &altered).expect("alter");
    assert!(fixture.verify(&candidate.snapshot_id).is_err());
    assert!(
        fixture.freeze().is_err(),
        "retry cannot replace an altered candidate"
    );
    assert_eq!(
        std::fs::read(&candidate.manifest_path).expect("retained"),
        altered
    );
    std::fs::write(&candidate.manifest_path, manifest).expect("restore");
    std::fs::write(
        Path::new(&candidate.roles_directory).join("lead/skills/plan/SKILL.md"),
        "tampered",
    )
    .expect("alter bytes");
    assert!(fixture.verify(&candidate.snapshot_id).is_err());
}

#[test]
fn unmanifested_files_and_directories_are_rejected() {
    let fixture = Fixture::new();
    let candidate = fixture.freeze().expect("snapshot");
    let roles = Path::new(&candidate.roles_directory);
    let extra = roles.join("lead/extra.md");
    std::fs::write(&extra, "not in manifest").expect("extra");
    assert!(fixture.verify(&candidate.snapshot_id).is_err());
    std::fs::remove_file(extra).expect("remove");
    let empty = roles.join("lead/unmanifested");
    std::fs::create_dir(&empty).expect("empty dir");
    assert!(fixture.verify(&candidate.snapshot_id).is_err());
    std::fs::remove_dir(empty).expect("remove");
    std::fs::write(
        roles.parent().expect("parent").join("unmanifested"),
        "extra",
    )
    .expect("container extra");
    assert!(fixture.verify(&candidate.snapshot_id).is_err());
}

#[test]
fn identity_community_project_and_setup_id_cannot_select_another_snapshot() {
    let fixture = Fixture::new();
    let candidate = fixture.freeze().expect("snapshot");
    for scope in [
        SetupScope::new(
            &fixture.scope.project,
            &nostr::Keys::generate().public_key().to_hex(),
            &fixture.scope.relay,
        )
        .expect("owner"),
        SetupScope::new(&fixture.scope.project, OWNER, "wss://other.test").expect("community"),
        SetupScope::new(&format!("30621:{OWNER}:other"), OWNER, &fixture.scope.relay)
            .expect("project"),
    ] {
        assert!(run(
            &fixture.root,
            &scope,
            &fixture.record.setup_id,
            Some(&candidate.snapshot_id)
        )
        .is_err());
    }
    assert!(run(
        &fixture.root,
        &fixture.scope,
        "another-setup",
        Some(&candidate.snapshot_id)
    )
    .is_err());
    assert!(fixture.verify("../../draft").is_err());
    assert!(fixture.verify(&"a".repeat(63)).is_err());
}

#[test]
fn snapshot_contains_only_role_files_and_refuses_git_metadata_within_them() {
    let fixture = Fixture::new();
    std::fs::write(
        Path::new(&fixture.record.draft_directory).join("authoring-notes.md"),
        "not a role file",
    )
    .expect("notes");
    let candidate = fixture.freeze().expect("snapshot");
    assert!(!Path::new(&candidate.roles_directory)
        .join("authoring-notes.md")
        .exists());
    let git = Path::new(&fixture.record.roles_directory).join("lead/.git");
    std::fs::create_dir(&git).expect("git dir");
    std::fs::write(git.join("config"), "[filter]").expect("git config");
    assert!(fixture.freeze().is_err());
}

#[cfg(unix)]
#[test]
fn source_and_snapshot_symlinks_are_refused_without_touching_target() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new();
    let candidate = fixture.freeze().expect("snapshot");
    let outside = fixture._temp.path().join("outside");
    std::fs::write(&outside, "preserved").expect("outside");
    symlink(
        &outside,
        Path::new(&fixture.record.roles_directory).join("lead/link"),
    )
    .expect("source link");
    assert!(fixture.freeze().is_err());
    let manifest = Path::new(&candidate.manifest_path);
    std::fs::remove_file(manifest).expect("remove manifest");
    symlink(&outside, manifest).expect("snapshot link");
    assert!(fixture.verify(&candidate.snapshot_id).is_err());
    assert_eq!(std::fs::read_to_string(outside).expect("read"), "preserved");
}

#[test]
fn portable_component_rules_are_independent_of_native_path_separators() {
    for good in [
        "lead",
        ".plugin",
        "plugin.json",
        "SKILL.md",
        "design notes.md",
        "garden-🌿.md",
    ] {
        assert!(portable_component(good), "{good}");
    }
    for bad in [
        "", ".", "..", ".git", ".GIT", "a/b", "a\\b", "C:", "a?b", "a*b", "a<b", "a>b", "a|b",
        "a\"b", "a\nb", "end.", "end ", "CON", "con.md", "NUL", "COM1.txt", "LPT9", "COM¹",
        "CONOUT$",
    ] {
        assert!(!portable_component(bad), "{bad}");
    }
    assert_eq!(
        portable_path(&Path::new("lead").join("skills").join("SKILL.md"))
            .expect("native nested path"),
        "lead/skills/SKILL.md"
    );
    let mut seen = HashMap::new();
    check_collision("lead/skills/Plan", &mut seen).expect("first directory");
    assert!(check_collision("lead/skills/plan", &mut seen).is_err());
}

#[test]
fn saved_id_is_recoverable_and_failure_preserves_the_previous_pointer() {
    let fixture = Fixture::new();
    assert!(read_draft(&fixture.root, &fixture.scope)
        .expect("read")
        .expect("draft")
        .latest_snapshot_id
        .is_none());
    let first = fixture.freeze().expect("first");
    let pointer = scoped_file(&fixture.record, "latest-snapshot.json").expect("pointer");
    let original_pointer = std::fs::read(&pointer).expect("read pointer");
    let reopened = read_draft(&fixture.root, &fixture.scope)
        .expect("read")
        .expect("draft");
    assert_eq!(
        reopened.latest_snapshot_id.as_deref(),
        Some(first.snapshot_id.as_str())
    );
    let old_skill = std::fs::read(fixture.skill()).expect("skill");
    std::fs::write(fixture.skill(), "invalid skill").expect("break");
    assert!(fixture.freeze().is_err());
    assert_eq!(
        std::fs::read(&pointer).expect("preserved pointer"),
        original_pointer
    );
    let mut changed = old_skill;
    changed.extend_from_slice(b"\nNew procedure.\n");
    std::fs::write(fixture.skill(), changed).expect("edit");
    let second = fixture.freeze().expect("second");
    assert_ne!(first.snapshot_id, second.snapshot_id);
    assert_eq!(
        latest(&fixture.record).expect("latest"),
        Some(second.snapshot_id)
    );
    fixture
        .verify(&first.snapshot_id)
        .expect("old candidate still accessible");
}

#[test]
fn reverify_and_reopen_never_change_the_latest_pointer_or_create_files() {
    let fixture = Fixture::new();
    let first = fixture.freeze().expect("first");
    let mut bytes = std::fs::read(fixture.skill()).expect("skill");
    bytes.extend_from_slice(b"\nNew evidence.\n");
    std::fs::write(fixture.skill(), bytes).expect("edit");
    let second = fixture.freeze().expect("second");
    let scoped = Path::new(&fixture.record.draft_directory)
        .parent()
        .expect("parent");
    let before = capture(scoped, false).expect("before").manifest;
    fixture
        .verify(&first.snapshot_id)
        .expect("verify earlier candidate");
    let read = read_draft(&fixture.root, &fixture.scope)
        .expect("get")
        .expect("draft");
    assert_eq!(read.latest_snapshot_id, Some(second.snapshot_id));
    assert_eq!(capture(scoped, false).expect("after").manifest, before);
}

#[test]
fn foreign_or_malformed_latest_pointer_is_refused_without_guessing() {
    let fixture = Fixture::new();
    let candidate = fixture.freeze().expect("snapshot");
    let path = scoped_file(&fixture.record, "latest-snapshot.json").expect("pointer");
    for pointer in [
        LatestSnapshot {
            setup_id: "foreign".into(),
            snapshot_id: candidate.snapshot_id.clone(),
        },
        LatestSnapshot {
            setup_id: fixture.record.setup_id.clone(),
            snapshot_id: "../latest".into(),
        },
    ] {
        std::fs::write(&path, serde_json::to_vec(&pointer).expect("json")).expect("tamper");
        assert!(read_draft(&fixture.root, &fixture.scope).is_err());
    }
}

#[test]
fn size_entry_and_depth_limits_apply_before_a_candidate_is_saved() {
    let fixture = Fixture::new();
    let roles = Path::new(&fixture.record.roles_directory);
    let large = roles.join("lead/large");
    std::fs::create_dir(&large).expect("large dir");
    for index in 0..33 {
        std::fs::write(
            large.join(format!("{index}.txt")),
            vec![b'x'; MAX_FILE_BYTES as usize],
        )
        .expect("file");
    }
    assert!(fixture.freeze().is_err());
    std::fs::remove_dir_all(&large).expect("remove");
    let entries = roles.join("lead/entries");
    std::fs::create_dir(&entries).expect("entries");
    for index in 0..MAX_ENTRIES {
        std::fs::create_dir(entries.join(index.to_string())).expect("entry");
    }
    assert!(fixture.freeze().is_err());
    std::fs::remove_dir_all(entries).expect("remove");
    let deep = roles.join("lead/deep");
    std::fs::create_dir_all((0..MAX_DEPTH).fold(deep, |path, _| path.join("next"))).expect("deep");
    assert!(fixture.freeze().is_err());
    assert!(!storage(&fixture.record).expect("storage").exists());
}

#[test]
fn snapshot_lock_is_an_operating_system_file_lock() {
    let fixture = Fixture::new();
    let first = process_lock(&fixture.record).expect("first lock");
    let path = scoped_file(&fixture.record, "snapshot.lock").expect("path");
    let second = std::fs::OpenOptions::new()
        .write(true)
        .open(path)
        .expect("second handle");
    assert!(second.try_lock().is_err());
    drop(first);
    second.try_lock().expect("released lock");
}
