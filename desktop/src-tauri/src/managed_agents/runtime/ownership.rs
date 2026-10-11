/// Fail closed when this participant/community has transferred to the host.
pub(crate) fn require_desktop_runtime_owner(pubkey: &str, relay: &str) -> Result<(), String> {
    let home = beekeeper_host_core::layout::home_dir()?;
    beekeeper_host_core::ownership::require_legacy_owner(
        &home,
        crate::agent_host::instance(),
        pubkey,
        relay,
    )
}

pub(super) fn desktop_owns_marked_process(pid: u32, instance: &str) -> bool {
    !super::process::process_has_env_key(pid, beekeeper_host_core::layout::CHILD_MARKER_VAR)
        && super::process_has_beekeeper_marker(pid, instance)
}

/// Bind graceful shutdown evidence to this tracked ACP generation.
pub(super) fn stamp_desktop_ownership(
    command: &mut std::process::Command,
    app: &tauri::AppHandle,
    log_path: &std::path::Path,
) -> String {
    let nonce = uuid::Uuid::new_v4().simple().to_string();
    command
        .env("BEEKEEPER_MANAGED_AGENT", super::current_instance_id(app))
        .env("BEEKEEPER_MANAGED_AGENT_START_NONCE", &nonce)
        .env(
            "BEEKEEPER_MANAGED_AGENT_QUIESCE_RECEIPT",
            log_path.with_file_name(format!("quiesced-{nonce}.json")),
        );
    nonce
}
