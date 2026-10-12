//! Select a commissioned descriptor; the worker owns protected adapter validation/launch.
use beekeeper_agent_management::PrivateConfiguration;
use beekeeper_core_pkg::coding_session_runtime::RuntimeDescriptor;

pub(super) fn select<'a>(
    runtimes: &'a [RuntimeDescriptor],
    private: &PrivateConfiguration,
) -> Result<&'a RuntimeDescriptor, String> {
    let command = private
        .record
        .get("effective_command")
        .and_then(serde_json::Value::as_str)
        .filter(|command| !command.trim().is_empty())
        .ok_or("protected runtime command unavailable")?;
    let generic = beekeeper_host_core::managed_runtime::INSTANCE_REF;
    if let Some(exact) = runtimes
        .iter()
        .find(|runtime| runtime.instance_ref != generic && runtime.agent_command == command)
    {
        return Ok(exact);
    }
    if let Some(known) = crate::managed_agents::known_acp_runtime(command).and_then(|known| {
        runtimes
            .iter()
            .find(|runtime| runtime.instance_ref != generic && runtime.runtime == known.id)
    }) {
        return Ok(known);
    }
    // This is an explicitly configured private adapter, not a vendor fallback
    // or executable from public30177. Shared Create validates/stages the whole
    // protected snapshot; worker Settings must replace the sentinel before launch.
    if command == beekeeper_host_core::managed_runtime::descriptor().agent_command
        || private
            .record
            .get("runtime")
            .and_then(serde_json::Value::as_str)
            .is_none_or(|runtime| runtime.trim().is_empty())
    {
        return Err(
            "managed adapter selection requires protected command and runtime label".into(),
        );
    }
    runtimes
        .iter()
        .find(|runtime| runtime.instance_ref == generic)
        .ok_or_else(|| "selected runtime is not commissioned".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn private(command: &str) -> PrivateConfiguration {
        PrivateConfiguration {
            record: json!({"effective_command":command,"runtime":"custom",
            "effective_args":["--acp","--custom-option"],"effective_env":{"ADAPTER_OPTION":"retained"},
            "model":"configured-model","access_mode":"owner-only","parallelism":2}),
        }
    }
    #[test]
    fn protected_custom_adapter_selects_existing_generic_slot_without_changing_configuration() {
        let runtimes = vec![beekeeper_host_core::managed_runtime::descriptor()];
        let private = private("/private/configured/custom-acp");
        let before = serde_json::to_vec(&private).expect("protected source");
        let descriptor = select(&runtimes, &private).expect("commissioned generic slot");
        assert_eq!(
            descriptor.instance_ref,
            beekeeper_host_core::managed_runtime::INSTANCE_REF
        );
        assert_eq!(descriptor.driver, "managed-acp");
        assert_eq!(
            descriptor, &runtimes[0],
            "selection does not rewrite the commissioned descriptor"
        );
        assert_eq!(
            serde_json::to_vec(&private).expect("private config"),
            before
        );
        assert_eq!(
            private.record["effective_command"],
            "/private/configured/custom-acp"
        );
        assert_eq!(private.record["effective_args"][1], "--custom-option");
        assert_eq!(
            private.record["effective_env"]["ADAPTER_OPTION"],
            "retained"
        );
    }
    #[test]
    fn exact_and_known_commissioned_adapters_are_preferred_over_generic_slot() {
        let generic = beekeeper_host_core::managed_runtime::descriptor();
        let mut exact = generic.clone();
        exact.instance_ref = "custom-primary".into();
        exact.agent_command = "/private/configured/custom-acp".into();
        let mut known = generic.clone();
        known.instance_ref = "codex-primary".into();
        known.runtime = "codex".into();
        known.agent_command = "/private/installed/codex-acp".into();
        let runtimes = vec![generic, known, exact];
        assert_eq!(
            select(&runtimes, &private("/private/configured/custom-acp"))
                .expect("exact")
                .instance_ref,
            "custom-primary"
        );
        assert_eq!(
            select(&runtimes, &private("codex-acp"))
                .expect("known")
                .instance_ref,
            "codex-primary"
        );
    }
    #[test]
    fn missing_slot_or_missing_private_command_runtime_never_selects_placeholder() {
        let runtimes = vec![beekeeper_host_core::managed_runtime::descriptor()];
        assert!(select(&[], &private("custom-acp")).is_err());
        for record in [
            json!({}),
            json!({"effective_command":"","runtime":"custom"}),
            json!({"effective_command":"custom-acp"}),
            json!({"effective_command":"custom-acp","runtime":" "}),
            json!({"effective_command":runtimes[0].agent_command,"runtime":"custom"}),
        ] {
            assert!(select(&runtimes, &PrivateConfiguration { record }).is_err());
        }
    }
}
