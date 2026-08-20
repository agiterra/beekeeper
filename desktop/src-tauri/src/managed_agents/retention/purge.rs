//! Relay-identity purge for a retention scope.
//!
//! Split out of `retention.rs` to keep that file under the repository-wide
//! 1000-line gate.

use std::path::Path;

use super::open_retention_db;

/// Drop every retained event in the scope's database.
///
/// Called when the relay at a scope's URL comes back advertising a different
/// NIP-11 `self` key: the URL is the same, but the community behind it is a
/// new instance, so nothing filed under this scope — least of all rows still
/// flagged `pending_sync`, which would otherwise be published *to the new
/// relay* — describes reality any more.
///
/// Deletes rows rather than unlinking the file: an unlink would leave any
/// already-open connection (e.g. the flush loop's) writing to an orphaned
/// inode, and would also discard the legacy-migration marker, letting the
/// pre-scoping global database be re-imported on the next boot.
///
/// A scope whose database was never created is `Ok(0)`, not an error.
pub fn purge_retention_scope(db_path: &Path) -> Result<usize, String> {
    if !db_path.exists() {
        return Ok(0);
    }
    let conn = open_retention_db(db_path)?;
    conn.execute("DELETE FROM persona_events", [])
        .map_err(|error| format!("failed to purge retention scope: {error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::managed_agents::retention::{
        get_pending_sync, has_retained_personas, retain_event, RetainedEvent,
    };

    fn sample_event() -> RetainedEvent {
        RetainedEvent {
            kind: 30175,
            pubkey: "abc123".to_string(),
            d_tag: "test-persona".to_string(),
            content: r#"{"display_name":"Test"}"#.to_string(),
            created_at: 1000,
            raw_event: r#"{"id":"..."}"#.to_string(),
            pending_sync: true,
        }
    }

    #[test]
    fn purge_retention_scope_clears_rows_and_ignores_missing_db() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("never-created.db");
        // A scope that never wrote anything is not an error.
        assert_eq!(purge_retention_scope(&missing).unwrap(), 0);
        assert!(!missing.exists());

        let path = dir.path().join("scope.db");
        {
            let conn = open_retention_db(&path).unwrap();
            retain_event(&conn, &sample_event()).unwrap();
            let mut other = sample_event();
            other.d_tag = "second".to_string();
            retain_event(&conn, &other).unwrap();
            assert_eq!(get_pending_sync(&conn).unwrap().len(), 2);
        }

        assert_eq!(purge_retention_scope(&path).unwrap(), 2);

        // The database survives (marker tables and WAL intact); only the
        // events are gone, so nothing pending republishes to the new relay.
        let conn = open_retention_db(&path).unwrap();
        assert!(get_pending_sync(&conn).unwrap().is_empty());
        assert!(!has_retained_personas(&conn, "abc123").unwrap());
    }
}
