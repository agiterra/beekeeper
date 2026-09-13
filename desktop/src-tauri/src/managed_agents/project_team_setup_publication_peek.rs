//! Strictly read-only view of the last recorded publication. Unlike
//! `project_team_setup_get_publication` it never reconciles against the relay,
//! saves, locks or creates a file, so a page visit cannot race the locked
//! install and lead writers or rewrite an adopted journal.

use super::*;

/// The recorded publication projection for this scope, exactly as saved.
fn peek(
    root: &Path,
    scope: &SetupScope,
    setup_id: &str,
) -> Result<Option<ProjectTeamSetupPublication>, SetupError> {
    let draft = bound_draft(root, scope, setup_id)?;
    Ok(load_journal(&draft)?.as_ref().map(project))
}

/// Read the last recorded publication status without observing the relay or
/// writing anything. The status may be stale; `get_publication` reconciles.
#[tauri::command]
pub async fn project_team_setup_peek_publication(
    app: AppHandle,
    state: State<'_, AppState>,
    project_ref: String,
    expected_relay_url: String,
    setup_id: String,
) -> Result<Option<ProjectTeamSetupPublication>, SetupError> {
    let (root, scope) = context(&app, &state, &project_ref, &expected_relay_url)?;
    let publication = peek(&root, &scope, &setup_id)?;
    verify_context(&state, &scope)?;
    Ok(publication)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn peek_returns_recorded_adopted_status_without_reconciling_or_writing() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("project-team-setup");
        let owner = nostr::Keys::generate();
        let owner_hex = owner.public_key().to_hex();
        let draft = super::super::super::tests::write_bound_draft(
            &root,
            &owner_hex,
            "wss://garden.example",
            "garden",
        );
        let root = std::fs::canonicalize(&root).expect("canonical root");
        let scope =
            SetupScope::new(&draft.project_ref, &owner_hex, &draft.relay_url).expect("scope");
        let mut journal = super::super::tests::installed_journal(
            &draft,
            &"c".repeat(40),
            &[("lead", &"d".repeat(64))],
            None,
        );
        journal.source_event = Some(source_event(&journal, &owner).expect("source event"));
        save_journal(&draft, &journal).expect("adopted journal");

        // A reconciling read with no observable source would downgrade it.
        let mut reconciled = journal.clone();
        reconcile_source_observation(&mut reconciled, None).expect("reconcile");
        assert_eq!(reconciled.status, PublicationStatus::SourceUnknown);

        let path = journal_path(&draft).expect("journal path");
        let storage = path.parent().expect("storage").to_path_buf();
        let before = (
            std::fs::read(&path).expect("bytes"),
            std::fs::metadata(&path)
                .expect("meta")
                .modified()
                .expect("mtime"),
            std::fs::metadata(&storage)
                .expect("dir")
                .modified()
                .expect("dir mtime"),
            std::fs::read_dir(&storage).expect("list").count(),
        );
        let peeked = peek(&root, &scope, &draft.setup_id)
            .expect("peek")
            .expect("recorded publication");
        let after = (
            std::fs::read(&path).expect("bytes"),
            std::fs::metadata(&path)
                .expect("meta")
                .modified()
                .expect("mtime"),
            std::fs::metadata(&storage)
                .expect("dir")
                .modified()
                .expect("dir mtime"),
            std::fs::read_dir(&storage).expect("list").count(),
        );
        assert_eq!(before, after, "peek must not write, rename or create files");
        assert_eq!(peeked.status, PublicationStatus::Adopted);
        assert_eq!(peeked.publication_id, journal.publication_id);
        assert_eq!(
            peeked.source_event_id,
            journal.source_event.as_ref().map(|event| event.id.to_hex())
        );

        let other = SetupScope::new(&draft.project_ref, &owner_hex, "wss://orchard.example")
            .expect("scope");
        assert!(peek(&root, &other, &draft.setup_id).is_err());
        assert!(peek(&root, &scope, &uuid::Uuid::new_v4().to_string()).is_err());
    }

    #[test]
    fn peek_without_a_journal_is_none_and_creates_nothing() {
        let temp = tempfile::tempdir().expect("temp");
        let root = temp.path().join("project-team-setup");
        let owner = nostr::Keys::generate().public_key().to_hex();
        let draft = super::super::super::tests::write_bound_draft(
            &root,
            &owner,
            "wss://garden.example",
            "garden",
        );
        let root = std::fs::canonicalize(&root).expect("canonical root");
        let scope = SetupScope::new(&draft.project_ref, &owner, &draft.relay_url).expect("scope");
        assert!(peek(&root, &scope, &draft.setup_id)
            .expect("peek")
            .is_none());
        assert!(!journal_path(&draft).expect("path").exists());
    }
}
