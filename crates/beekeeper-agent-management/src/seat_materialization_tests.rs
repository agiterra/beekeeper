//! The existing Desktop composition oracle, exercised without Tauri.
use super::*;
use beekeeper_core::project_pack_source::DEFAULT_PACK_PATH;

fn scratch_root() -> PathBuf {
    tempfile::tempdir().expect("scratch").keep()
}

fn write(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("parent");
    }
    std::fs::write(path, contents).expect("write");
}

/// Write one role pack — `.plugin/plugin.json` plus a persona declaring
/// the role — under `<root>/personas/roles/<role>`.
fn write_role_pack(root: &Path, role: &str, body: &str) {
    let pack = root.join(DEFAULT_PACK_PATH).join(role);
    write(
        &pack.join(".plugin/plugin.json"),
        &format!(
            r#"{{"id":"com.test.{role}","name":"{role}","version":"0.1.0","personas":["personas/{role}.persona.md"]}}"#
        ),
    );
    write(
        &pack.join(format!("personas/{role}.persona.md")),
        &format!(
            "---\nname: {role}\ndisplay_name: {role}\ndescription: The {role}.\nrole: {role}\n---\n{body}\n"
        ),
    );
}

#[test]
fn a_flat_role_file_is_located_after_a_pack_directory() {
    let root = scratch_root();
    let checkout = root.join("checkout");
    write(
        &checkout.join("agents").join("roles/builder.md"),
        "You build, flat.\n",
    );
    assert_eq!(
        locate_role_source(&checkout, "agents", "builder"),
        Some(RoleSource::Flat {
            root: checkout.join("agents"),
            role: "builder".to_owned(),
        })
    );
    assert!(locate_role_source(&checkout, "agents", "runner").is_none());
    assert!(
        locate_role_source(&checkout, "agents", "../builder").is_none(),
        "a traversing role is not a role slug"
    );
    // A pack directory beside the flat file wins, byte for byte the old rule.
    let pack = checkout.join("agents").join("builder");
    write(
        &pack.join(".plugin/plugin.json"),
        r#"{"id":"com.test.builder","name":"builder","version":"0.1.0","personas":["personas/builder.persona.md"]}"#,
    );
    write(
        &pack.join("personas/builder.persona.md"),
        "---\nname: builder\ndisplay_name: builder\ndescription: The builder.\nrole: builder\n---\nPacked.\n",
    );
    assert!(matches!(
        locate_role_source(&checkout, "agents", "builder"),
        Some(RoleSource::Pack { .. })
    ));
    assert_eq!(
        pack_ref_path(
            &RoleSource::Flat {
                root: checkout.clone(),
                role: "builder".into()
            },
            "agents"
        ),
        "agents/roles/builder"
    );
    assert_eq!(
        pack_ref_path(
            &RoleSource::Pack {
                dir: pack,
                role: "builder".into(),
                persona: None
            },
            DEFAULT_PACK_PATH
        ),
        "personas/roles/builder"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// Staging is keyed by the composition's digest: the same inputs land in
/// the same immutable directory and write nothing the second time; a
/// change lands beside it. The staged directory is an ordinary pack the
/// resolver reads, never the source itself.
#[test]
fn staging_a_composed_pack_is_digest_keyed_and_idempotent() {
    let root = scratch_root();
    let packs_root = root.join("packs");
    let checkout = root.join("checkout");
    write_role_pack(&checkout, "builder", "You build.");
    let source = locate_role_source(&checkout, DEFAULT_PACK_PATH, "builder").expect("located");
    let catalog = TemplateCatalog::empty("0.5.16");
    let key = local_source_key(&checkout.join(DEFAULT_PACK_PATH));
    let provenance = SourceProvenance::local("personas/roles/builder");

    let first = stage_composed_pack(&packs_root, &key, &source, &catalog, provenance.clone())
        .expect("staged");
    assert!(first
        .dir
        .starts_with(staged_packs_root(&packs_root).join(&key)));
    assert_eq!(first.persona, "builder");
    assert!(first.digest.starts_with("sha256:"));
    assert!(first.warnings.is_empty());
    assert_ne!(first.dir, checkout.join(DEFAULT_PACK_PATH).join("builder"));
    let resolved = beekeeper_persona::resolve::resolve_persona_by_name(&first.dir, "builder")
        .expect("the staged pack is a pack");
    assert_eq!(resolved.system_prompt, "You build.\n");
    assert_eq!(resolved.role.as_deref(), Some("builder"));
    let stamp = std::fs::metadata(first.dir.join("compose.json"))
        .and_then(|m| m.modified())
        .expect("mtime");

    let again = stage_composed_pack(&packs_root, &key, &source, &catalog, provenance.clone())
        .expect("staged again");
    assert_eq!(again.dir, first.dir);
    assert_eq!(again.digest, first.digest);
    assert_eq!(
        std::fs::metadata(first.dir.join("compose.json"))
            .and_then(|m| m.modified())
            .expect("mtime"),
        stamp,
        "an identical composition writes nothing"
    );

    write_role_pack(&checkout, "builder", "You build, revised.");
    let changed = stage_composed_pack(&packs_root, &key, &source, &catalog, provenance)
        .expect("staged changed");
    assert_ne!(
        changed.dir, first.dir,
        "a changed source lands beside the old one"
    );
    assert!(
        first.dir.join("personas/builder.persona.md").is_file(),
        "the old staged copy is untouched"
    );
    std::fs::remove_dir_all(&root).ok();
}

/// A pack that cannot be composed — here, an include of a template this
/// build does not ship — refuses with the composer's reason, and stages
/// nothing.
#[test]
fn an_uncomposable_pack_refuses_and_stages_nothing() {
    let root = scratch_root();
    let packs_root = root.join("packs");
    let checkout = root.join("checkout");
    write_role_pack(
        &checkout,
        "builder",
        "![[beekeeper/memory@^1.0.0]]\nYou build.",
    );
    let source = locate_role_source(&checkout, DEFAULT_PACK_PATH, "builder").expect("located");
    let error = stage_composed_pack(
        &packs_root,
        "k",
        &source,
        &TemplateCatalog::empty("0.5.16"),
        SourceProvenance::local("personas/roles/builder"),
    )
    .expect_err("refused");
    assert!(error.contains("no template catalog"), "{error}");
    assert!(!staged_packs_root(&packs_root).join("k").exists());
    std::fs::remove_dir_all(&root).ok();
}
