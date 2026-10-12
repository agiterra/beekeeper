//! Query coverage from already cursor/cwd/hash-verified native source bytes.
//! This is never a consumed watermark or permission to repeat prior effects.
pub(super) fn source_start(
    runtime: &str,
    bytes: &[u8],
    cursor: &str,
    cwd: &std::path::Path,
    capture: u64,
) -> Result<u64, String> {
    let timestamp =
        beekeeper_host_core::native_artifacts::source_timestamp(runtime, bytes, cursor, cwd)?;
    let start: u64 = chrono::DateTime::parse_from_rfc3339(&timestamp)
        .map_err(|_| "native source time malformed")?
        .timestamp()
        .try_into()
        .map_err(|_| "native source time invalid")?;
    if start > capture {
        return Err(
            "native source time is after migration capture; replay provenance conflicts".into(),
        );
    }
    Ok(start)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn replay_coverage_uses_validated_vendor_bytes_not_copy_or_shutdown_time() {
        let cwd = tempfile::tempdir().unwrap();
        let cursor = uuid::Uuid::new_v4().to_string();
        let original = serde_json::to_vec(&serde_json::json!({"type":"session_meta", "timestamp":"2026-10-11T00:00:00Z", "payload":{"id":cursor,"cwd":cwd.path()}})).unwrap();
        let timestamp = source_start("codex", &original, &cursor, cwd.path(), 1791763200).unwrap();
        assert_eq!(
            source_start("codex", &original, &cursor, cwd.path(), 1791849600).unwrap(),
            timestamp
        );
        assert!(source_start("codex", &original, &cursor, cwd.path(), timestamp - 1).is_err());
        let claude = serde_json::to_vec(&serde_json::json!({"type":"user", "timestamp":"2026-10-11T00:00:00Z", "sessionId":cursor,"cwd":cwd.path()})).unwrap();
        assert_eq!(
            source_start("claude", &claude, &cursor, cwd.path(), 1791849600).unwrap(),
            timestamp
        );
        assert!(source_start(
            "claude",
            &claude,
            &uuid::Uuid::new_v4().to_string(),
            cwd.path(),
            1791849600
        )
        .is_err());
        assert!(source_start("unknown", &claude, &cursor, cwd.path(), 1791849600).is_err());
    }
}
