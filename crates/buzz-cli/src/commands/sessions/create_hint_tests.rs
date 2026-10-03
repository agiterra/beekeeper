//! The one-shot `--cwd` hint `bee sessions create` writes, read back by the
//! provider's own code.
//!
//! A projected create (`--project`) whose directory is not the project's
//! recorded checkout is admitted only when the hint binds that directory to
//! the project. The hint used to omit `projectRef`, so the provider refused
//! every such create live with `EXECUTION_SCOPE_INVALID` ("no host record
//! binds it to that project"). These tests call the provider's
//! `ProjectsFile::hint_binds_project` directly, so the writer and the reader
//! cannot drift apart without one of them failing here.

use buzz_session_provider::commands::ProjectsFile;

use super::handover_reconstruct::bind_pending_directory;

const PROJECT: &str = "30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:demo";
const COMMAND: &str = "0b4f0d3e-6c1a-4c55-9a39-2b7f6f1f0c11";

/// A projects file mapping `PROJECT` to one folder, and a different folder the
/// create is run in.
fn fixture() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let recorded = dir.path().join("recorded-checkout");
    let elsewhere = dir.path().join("lab-worktree");
    std::fs::create_dir_all(&recorded).expect("mkdir recorded");
    std::fs::create_dir_all(&elsewhere).expect("mkdir elsewhere");
    let projects = dir.path().join("projects.json");
    std::fs::write(
        &projects,
        serde_json::json!({
            "version": 1,
            "pending": {},
            "projects": { PROJECT: recorded.to_string_lossy() },
            "channels": {},
        })
        .to_string(),
    )
    .expect("seed projects file");
    (dir, projects, elsewhere)
}

#[test]
fn a_projected_create_hint_carries_project_ref() {
    let (_dir, projects, elsewhere) = fixture();
    let binding =
        bind_pending_directory(&projects, COMMAND, &elsewhere, Some(PROJECT)).expect("bind");
    let body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&binding.hint_path).expect("read hint"))
            .expect("hint is JSON");
    assert_eq!(body["projectRef"], serde_json::json!(PROJECT), "{body}");
    assert_eq!(body["commandId"], serde_json::json!(COMMAND), "{body}");
    assert_eq!(
        body.as_object().expect("object").len(),
        4,
        "commandId, path, writtenAt and projectRef — nothing else: {body}"
    );
}

#[test]
fn the_provider_binds_the_directory_the_cli_hinted_to_the_project() {
    let (_dir, projects, elsewhere) = fixture();
    let binding =
        bind_pending_directory(&projects, COMMAND, &elsewhere, Some(PROJECT)).expect("bind");

    let file = ProjectsFile::load(Some(&projects));
    assert_eq!(
        file.resolve_hinted(COMMAND),
        Some(binding.directory.clone()),
        "the provider resolves the hinted directory for this command"
    );
    assert!(
        file.hint_binds_project(COMMAND, PROJECT),
        "the provider reads the CLI's hint as binding the directory to the project"
    );
    assert!(
        !file.hint_binds_project(COMMAND, "30621:bb:other"),
        "and to that project only"
    );
}

#[test]
fn a_projectless_hint_binds_no_project_and_keeps_its_three_keys() {
    let (_dir, projects, elsewhere) = fixture();
    let binding = bind_pending_directory(&projects, COMMAND, &elsewhere, None).expect("bind");
    let body: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&binding.hint_path).expect("read hint"))
            .expect("hint is JSON");
    assert!(body.get("projectRef").is_none(), "{body}");
    assert_eq!(body.as_object().expect("object").len(), 3, "{body}");

    let file = ProjectsFile::load(Some(&projects));
    assert!(
        !file.hint_binds_project(COMMAND, PROJECT),
        "a hint naming no project binds nothing — which is what refused the create before"
    );
}
