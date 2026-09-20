use super::*;

fn fixture() -> (tempfile::TempDir, ProjectTeamSetupDraft, Keys, PathBuf) {
    let root = tempfile::tempdir().expect("root");
    let owner = Keys::generate();
    let draft_dir = root.path().join("draft");
    std::fs::create_dir(&draft_dir).expect("draft");
    let draft = ProjectTeamSetupDraft {
        setup_id: uuid::Uuid::new_v4().to_string(),
        project_ref: format!("30621:{}:garden", owner.public_key().to_hex()),
        project_directory: root.path().join("project").to_string_lossy().into_owned(),
        draft_directory: draft_dir.to_string_lossy().into_owned(),
        roles_directory: draft_dir
            .join("personas/roles")
            .to_string_lossy()
            .into_owned(),
        status: super::super::SetupStatus::Draft,
        intent: "Build a garden".into(),
        owner_pubkey: owner.public_key().to_hex(),
        relay_url: "wss://garden.example".into(),
        roles: Vec::new(),
        expected_roles: Vec::new(),
        created_at: crate::util::now_iso(),
        latest_snapshot_id: None,
    };
    let shipped = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/roles");
    (root, draft, owner, shipped)
}

#[test]
fn real_shipped_bytes_and_identity_survive_interrupted_store_save_and_app_update() {
    let (root, draft, owner, shipped) = fixture();
    let first = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let path = verified_pack(root.path(), &first).expect("pack");
    assert_eq!(
        pack::digest(&path).expect("digest"),
        pack::digest(&shipped.join(pack::ROLE)).expect("source digest")
    );
    let actual = pack::validate(&path).expect("actual persona");
    // The shipped pack is thin since 2026-09-18 (spec § 4.11): its text and
    // skills live in the role template, so the copy carries no skills of its
    // own and composes with them against the shipped catalog — which is how
    // the host stages the setup seat from it.
    assert!(actual.skills.is_empty());
    let catalog =
        buzz_persona_pkg::template::TemplateCatalog::load(&shipped.join("../templates"), "test")
            .expect("the shipped catalog loads");
    let composed = buzz_persona_pkg::compose::compose_role(
        &buzz_persona_pkg::compose::RoleSource::Pack {
            dir: path.clone(),
            role: pack::ROLE.to_owned(),
            persona: None,
        },
        &catalog,
        &buzz_persona_pkg::compose::ComposeOptions::local("personas/roles/project-setup"),
    )
    .expect("the copied setup pack composes");
    assert!(!composed.skills.is_empty());
    // The copied pack must still carry the *shipped* role's own paragraph.
    // Take it from the template version the copy's `@^1.0.0` include
    // resolves to, not from a sentence pinned here: this test is about the
    // bytes surviving a store save and an app update, not about which
    // template version is current.
    let setup_role = catalog
        .resolve(
            pack::ROLE,
            &buzz_persona_pkg::template::TemplateRange::parse(pack::ROLE, "^1.0.0")
                .expect("caret range parses"),
        )
        .expect("the shipped setup role resolves")
        .template;
    let paragraph = setup_role
        .body
        .trim()
        .lines()
        .next()
        .expect("the role template's own paragraph");
    assert!(
        composed.persona.prompt.contains(paragraph),
        "the copied pack lost {}@{}",
        setup_role.name,
        setup_role.version
    );
    assert_eq!(first.pack_ref.repo, "app:shipped");
    let before = std::fs::read(root.path().join("setup-actor.enc")).expect("receipt");
    // No instance save happened. A new app build may ship different packs or no old resources.
    let second = prepare_receipt(root.path(), &draft, &owner, None, "0.2.0", &[]).expect("recover");
    assert_eq!(first.pubkey, second.pubkey);
    assert_eq!(first.prepared(root.path()), second.prepared(root.path()));
    assert_eq!(second.pack_ref.sha, "0.1.0");
    assert_eq!(
        before,
        std::fs::read(root.path().join("setup-actor.enc")).expect("unchanged")
    );
    let mut agents = Vec::new();
    second.reconcile(&mut agents, &path).expect("save recovery");
    second.reconcile(&mut agents, &path).expect("retry");
    assert_eq!(agents.len(), 1);
    let record = &agents[0];
    assert!(record.team_id.is_none() && record.source_team.is_none());
    assert!(!record.start_on_app_launch && !record.auto_restart_on_config_change);
    assert_eq!(
        record.persona_id.as_deref(),
        Some(identity::definition_id(&draft).as_str())
    );
    assert!(std::fs::read_dir(&draft.draft_directory)
        .expect("draft")
        .next()
        .is_none());
}

#[test]
fn encrypted_recovery_receipt_exposes_no_nsec_and_refuses_scope_transplant() {
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let path = root.path().join("setup-actor.enc");
    let bytes = std::fs::read_to_string(&path).expect("ciphertext");
    let record = receipt.record(&verified_pack(root.path(), &receipt).expect("pack"));
    assert!(!bytes.contains(&record.private_key_nsec));
    assert!(!bytes.contains(&owner.secret_key().to_secret_hex()));
    assert!(identity::read(&path, &draft, &Keys::generate()).is_err());
    for field in ["setup", "project", "community"] {
        let mut other = draft.clone();
        match field {
            "setup" => other.setup_id = uuid::Uuid::new_v4().to_string(),
            "project" => other.project_ref.push_str("-other"),
            _ => other.relay_url.push_str("/other"),
        }
        assert!(identity::read(&path, &other, &owner).is_err(), "{field}");
    }
}

#[test]
fn missing_store_key_recovers_exact_identity_and_mismatched_identity_is_preserved() {
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let path = verified_pack(root.path(), &receipt).expect("pack");
    let original = receipt.record(&path);
    let mut agents = vec![original.clone()];
    agents[0].private_key_nsec.clear();
    receipt
        .reconcile(&mut agents, &path)
        .expect("recover exact key");
    assert_eq!(agents[0].private_key_nsec, original.private_key_nsec);
    agents[0].pubkey = Keys::generate().public_key().to_hex();
    let changed = agents[0].pubkey.clone();
    assert!(receipt.reconcile(&mut agents, &path).is_err());
    assert_eq!(agents[0].pubkey, changed);
    assert_eq!(agents.len(), 1);
}

#[test]
fn reassigned_or_autostarted_identity_is_not_silently_rewritten() {
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let path = verified_pack(root.path(), &receipt).expect("pack");
    for field in ["team", "source", "autostart", "role", "key"] {
        let mut record = receipt.record(&path);
        match field {
            "team" => record.team_id = Some("other".into()),
            "source" => record.source_team = Some("other".into()),
            "autostart" => record.start_on_app_launch = true,
            "role" => record.home_role = Some("lead".into()),
            _ => record.private_key_nsec = "changed-key".into(),
        }
        assert!(
            receipt.reconcile(&mut vec![record], &path).is_err(),
            "{field}"
        );
    }
}

#[test]
fn changed_bootstrap_bytes_are_rejected_even_if_pack_remains_valid() {
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let path = verified_pack(root.path(), &receipt).expect("pack");
    let persona = path.join("personas/project-setup.persona.md");
    let mut bytes = std::fs::read(&persona).expect("persona");
    bytes.extend_from_slice(b"\nChanged instructions.\n");
    std::fs::write(persona, bytes).expect("edit");
    assert!(!buzz_persona_pkg::validate::validate_pack(&path).has_errors());
    assert!(verified_pack(root.path(), &receipt).is_err());
    assert!(prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[]).is_err());
}

#[test]
fn lost_receipt_never_mints_a_second_identity_for_existing_assignment() {
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let record = receipt.record(&verified_pack(root.path(), &receipt).expect("pack"));
    std::fs::remove_file(root.path().join("setup-actor.enc")).expect("remove receipt");
    assert!(prepare_receipt(
        root.path(),
        &draft,
        &owner,
        Some(&shipped),
        "0.1.0",
        &[record]
    )
    .is_err());
    assert!(!root.path().join("setup-actor.enc").exists());
}

#[test]
fn staging_retries_preserve_other_custody_and_refuse_command_retargeting() {
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let pack = verified_pack(root.path(), &receipt).expect("pack");
    let record = receipt.record(&pack);
    let entry = actor_seats::build_actor_seat_entry(
        &record.pubkey,
        &record.private_key_nsec,
        record.auth_tag.as_deref(),
        &draft.relay_url,
        Some("Project setup"),
        Some((pack, pack::ROLE.into())),
        Some(receipt.pack_ref.clone()),
    )
    .expect("entry");
    let path = root.path().join("actor-seats.json");
    let first = format!("csl-{}", uuid::Uuid::new_v4());
    let other = format!("csl-{}", uuid::Uuid::new_v4());
    stage_at(&path, &first, entry.clone()).expect("first");
    stage_at(&path, &other, entry.clone()).expect("other");
    stage_at(&path, &first, entry.clone()).expect("retry");
    let mut altered = entry.clone();
    altered.pack_ref = None;
    assert!(stage_at(&path, &first, altered).is_err());
    assert!(stage_at(&path, "bad-command", entry.clone()).is_err());
    let saved = actor_seats::read_actor_seats(&path).expect("custody");
    assert_eq!(saved.pending.len(), 2);
    assert_eq!(saved.pending[&first], entry);
}

#[test]
fn authoring_brief_is_host_local_and_edits_are_preserved_with_refusal() {
    let (root, draft, _, _) = fixture();
    let workspace = root.path().join("authoring-workspace");
    std::fs::create_dir(&workspace).expect("workspace");
    ensure_brief(&workspace, &draft).expect("brief");
    let path = workspace.join("PROJECT_TEAM_SETUP.md");
    let before = std::fs::read(&path).expect("bytes");
    assert!(String::from_utf8(before.clone())
        .expect("utf8")
        .contains(&draft.roles_directory));
    ensure_brief(&workspace, &draft).expect("retry");
    assert_eq!(std::fs::read(&path).expect("bytes"), before);
    std::fs::write(&path, "changed").expect("edit");
    assert!(ensure_brief(&workspace, &draft).is_err());
    assert_eq!(std::fs::read_to_string(path).expect("preserved"), "changed");
}

#[cfg(unix)]
#[test]
fn bootstrap_and_receipt_symlinks_are_rejected() {
    use std::os::unix::fs::symlink;
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let pack = verified_pack(root.path(), &receipt).expect("pack");
    symlink(&draft.draft_directory, pack.join("linked")).expect("link");
    assert!(verified_pack(root.path(), &receipt).is_err());
    let path = root.path().join("setup-actor.enc");
    let saved = root.path().join("saved.enc");
    std::fs::rename(&path, &saved).expect("move");
    symlink(&saved, &path).expect("receipt link");
    assert!(identity::read(&path, &draft, &owner).is_err());
}

#[test]
fn bootstrap_restage_uses_original_bytes_and_keeps_existing_fence_checks() {
    use crate::managed_agents::actor_seats_restage::{restage_actor_seats_with, SeatRequest};
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let path = verified_pack(root.path(), &receipt).expect("pack");
    let record = receipt.record(&path);
    let plan = restage::preview(
        root.path(),
        &receipt,
        &record,
        pack::ROLE,
        Some(&receipt.pack_ref),
    )
    .expect("plan");
    assert_eq!(plan.pack_dir.as_deref(), path.to_str());
    assert!(restage::preview(
        root.path(),
        &receipt,
        &record,
        "lead",
        Some(&receipt.pack_ref)
    )
    .is_err());
    let mut changed = receipt.pack_ref.clone();
    changed.sha = "new-app-version".into();
    assert!(restage::preview(root.path(), &receipt, &record, pack::ROLE, Some(&changed)).is_err());
    assert!(restage::preview(root.path(), &receipt, &record, pack::ROLE, None).is_err());
    let request = SeatRequest {
        command_id: format!("csl-{}", uuid::Uuid::new_v4()),
        actor: record.pubkey.clone(),
        role: pack::ROLE.into(),
        project_ref: Some(draft.project_ref.clone()),
        session_id: "preserved-session".into(),
        generation: 1,
        pack_ref: Some(receipt.pack_ref.clone()),
        fenced: false,
    };
    let plans = std::collections::BTreeMap::from([(request.command_id.clone(), Ok(plan))]);
    let (custody, report) = restage_actor_seats_with(
        std::slice::from_ref(&request),
        &Default::default(),
        std::slice::from_ref(&record),
        &draft.relay_url,
        &plans,
    );
    assert_eq!(report.staged, 1);
    let entry = &custody.pending[&request.command_id];
    let persona = buzz_persona_pkg::resolve::resolve_persona_by_name(
        entry.pack_dir.as_deref().expect("pack directory"),
        entry.persona_id.as_deref().expect("persona"),
    )
    .expect("provider persona");
    let workdir = root.path().join("materialized");
    std::fs::create_dir(&workdir).expect("workdir");
    buzz_persona_pkg::skills::materialize_skills(&persona, &workdir)
        .expect("provider materialization");
    for skill in &persona.skills {
        assert_eq!(
            std::fs::read(workdir.join(".agents/skills").join(skill).join("SKILL.md"))
                .expect("materialized skill"),
            std::fs::read(path.join("skills").join(skill).join("SKILL.md"))
                .expect("bootstrap skill")
        );
    }
    let mut fenced = request;
    fenced.fenced = true;
    let (custody, report) = restage_actor_seats_with(
        &[fenced],
        &Default::default(),
        &[record],
        &draft.relay_url,
        &plans,
    );
    assert_eq!(report.staged, 0);
    assert_eq!(report.skipped.len(), 1);
    assert!(custody.pending.is_empty());
}

#[test]
fn setup_detection_uses_reserved_identity_assignment_not_role_label() {
    let (root, draft, owner, shipped) = fixture();
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare");
    let mut record = receipt.record(&verified_pack(root.path(), &receipt).expect("pack"));
    assert!(restage::is_setup_actor(&record));
    record.persona_id = Some("ordinary-persona".into());
    assert!(!restage::is_setup_actor(&record));
}

#[test]
fn real_draft_remains_valid_with_bootstrap_skills_and_local_authoring_brief() {
    let (root, mut draft, owner, shipped) = fixture();
    tree::copy_tree(&shipped, Path::new(&draft.roles_directory)).expect("baseline");
    draft.expected_roles = tree::seed_identities(&shipped).expect("seed identities");
    let receipt = prepare_receipt(root.path(), &draft, &owner, Some(&shipped), "0.1.0", &[])
        .expect("prepare actor");
    let prepared = receipt.prepared(root.path());
    assert_eq!(prepared.authoring_directory, draft.draft_directory);
    let cwd = Path::new(&prepared.authoring_directory);
    assert!(Path::new(&draft.roles_directory).starts_with(cwd));
    assert!(!root.path().join("setup-actor.enc").starts_with(cwd));
    assert!(!verified_pack(root.path(), &receipt)
        .expect("bootstrap")
        .starts_with(cwd));
    ensure_brief(cwd, &draft).expect("brief");
    let persona =
        pack::validate(&verified_pack(root.path(), &receipt).expect("bootstrap")).expect("persona");
    buzz_persona_pkg::skills::materialize_skills(&persona, cwd)
        .expect("provider skill materialization");
    let validation = tree::validate(&draft);
    assert!(validation.valid, "{:?}", validation.diagnostics);
}
