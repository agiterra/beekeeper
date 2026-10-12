//! Existing ACP servers bound to the execution's prepared custody/environment.
use super::CreateRequest;
use beekeeper_acp::acp::{AcpError, EnvVar, McpServer};
use beekeeper_core::coding_session_command::coding_session_target_key;
use beekeeper_core::coding_session_context::validate_coding_session_first_turn_brief_json;

pub(super) fn servers(request: &CreateRequest) -> Result<Vec<McpServer>, AcpError> {
    let mut servers = rehydration_mcp_servers(request)?;
    let (command, environment) = match &request.execution {
        crate::execution_scope::ExecutionPlan::Prepared(prepared) => (
            prepared.configured_mcp_command.as_deref(),
            prepared
                .launch
                .env()
                .vars()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect::<Vec<_>>(),
        ),
        crate::execution_scope::ExecutionPlan::Legacy {
            configured_mcp_command,
            ..
        } => {
            // Preserve the existing no-backend spawn contract, not a filesystem
            // sandbox claim. Runtime overrides pass the same credential fence;
            // only the exact actor custody adapter may override NIP identity.
            let mut env: std::collections::BTreeMap<String, String> = request
                .agent_env
                .iter()
                .filter(|(name, _)| !crate::agent_fence::FENCE.covers(name))
                .cloned()
                .collect();
            env.extend(request.post_fence_env.iter().cloned());
            (configured_mcp_command.as_deref(), env.into_iter().collect())
        }
    };
    if let Some(command) = command {
        let command = command.to_str().ok_or_else(|| {
            AcpError::Protocol("configured MCP executable path must be UTF-8".into())
        })?;
        let name = std::path::Path::new(command)
            .file_stem()
            .and_then(|v| v.to_str())
            .ok_or_else(|| {
                AcpError::Protocol("configured MCP executable name unavailable".into())
            })?;
        if servers.iter().any(|server| server.name == name) {
            return Err(AcpError::Protocol(
                "configured MCP conflicts with the execution context server".into(),
            ));
        }
        let env = environment
            .into_iter()
            .filter(|(key, _)| {
                matches!(
                    key.as_str(),
                    "BEEKEEPER_RELAY_URL"
                        | "BEEKEEPER_PRIVATE_KEY"
                        | "BEEKEEPER_AUTH_TAG"
                        | "BEEKEEPER_ACP_DISPLAY_NAME"
                        | "HOME"
                        | "TMPDIR"
                        | "XDG_CONFIG_HOME"
                        | "PATH"
                )
            })
            .map(|(name, value)| EnvVar { name, value })
            .collect();
        servers.push(McpServer {
            name: name.into(),
            command: command.into(),
            args: vec![],
            env: EnvVar::with_legacy_mirrors(env),
        });
    }
    Ok(servers)
}

/// Build the sole private context MCP descriptor for an ACP session open.
///
/// The MCP receives only the package path and the generation directory holding
/// it. Provider credentials, the opaque native-session cursor, the package id
/// and the agent's runtime environment are deliberately absent. Invalid paths
/// fail before any session open and are described without echoing host-private
/// values.
pub(super) fn rehydration_mcp_servers(request: &CreateRequest) -> Result<Vec<McpServer>, AcpError> {
    let Some(descriptor) = request.rehydration_mcp.as_ref() else {
        return Ok(Vec::new());
    };
    if !descriptor.command.is_absolute()
        || !descriptor.package_path.is_absolute()
        || !descriptor.package_dir.is_absolute()
    {
        return Err(AcpError::Protocol(
            "session context MCP command and package paths must be absolute".into(),
        ));
    }
    validate_coding_session_first_turn_brief_json(&descriptor.first_turn_brief).map_err(
        |error| {
            AcpError::Protocol(format!(
                "session context first-turn brief is invalid: {error}"
            ))
        },
    )?;
    let command = descriptor.command.to_str().ok_or_else(|| {
        AcpError::Protocol("session context MCP command path must be valid UTF-8".into())
    })?;
    let package_path = descriptor.package_path.to_str().ok_or_else(|| {
        AcpError::Protocol("session context MCP package path must be valid UTF-8".into())
    })?;
    let package_dir = descriptor.package_dir.to_str().ok_or_else(|| {
        AcpError::Protocol("session context MCP package directory must be valid UTF-8".into())
    })?;
    Ok(vec![McpServer {
        name: "beekeeper-session-context".into(),
        command: command.to_owned(),
        args: Vec::new(),
        // Each name in both spellings too, for a context sidecar built before
        // the BUZZ_ → BEEKEEPER_ rename.
        env: EnvVar::with_legacy_mirrors(vec![
            EnvVar {
                name: "BEEKEEPER_SESSION_CONTEXT_PACKAGE".into(),
                value: package_path.to_owned(),
            },
            EnvVar {
                name: "BEEKEEPER_SESSION_CONTEXT_PACKAGE_DIR".into(),
                value: package_dir.to_owned(),
            },
            // Which execution the sidecar is *serving*, so `session_inbox` can
            // page the commands addressed to this seat instead of every
            // sibling's. A public wire identity, not a credential: it is the
            // same `cs-target` key every one of this execution's events
            // already carries.
            EnvVar {
                name: "BEEKEEPER_SESSION_CONTEXT_SELF_TARGET".into(),
                value: coding_session_target_key(&request.target),
            },
        ]),
    }])
}

#[cfg(test)]
mod tests {
    use crate::session::testing::{fake_agent, legacy_request, GOOD_AGENT};
    use crate::session::{ActorQuiescence, SessionManager, ACTOR_RETIRE_GRACE};
    use nostr::Keys;

    #[tokio::test]
    async fn selected_catalog_mcp_survives_forced_legacy_with_exact_actor_fence_and_no_sandbox_claim(
    ) {
        let dir = tempfile::tempdir().expect("fixture");
        let wire = dir.path().join("private-acp-wire.jsonl");
        let script = GOOD_AGENT.replace(
            "while IFS= read -r line; do",
            &format!(
                "umask 077\nwhile IFS= read -r line; do\n  printf '%s\\n' \"$line\" >> '{}'",
                wire.display()
            ),
        );
        let agent = fake_agent(dir.path(), "mcp-capable-client", &script);
        let command = std::path::PathBuf::from(&agent)
            .canonicalize()
            .expect("installed tool fixture");
        let actor = Keys::generate();
        let owner = Keys::generate();
        let provider = Keys::generate();
        let auth = beekeeper_sdk::nip_oa::compute_auth_tag(&owner, &actor.public_key(), "")
            .expect("attestation");
        let mut request = legacy_request(&agent, dir.path(), "legacy-mcp");
        request.execution = crate::execution_scope::ExecutionPlan::Legacy {
            reason: "forced-no-backend",
            configured_mcp_command: Some(command.clone()),
        };
        assert_eq!(
            request.execution.state(),
            crate::execution_scope::BoundaryState::NotEnforced {
                reason: "forced-no-backend"
            }
        );
        request.agent_env = vec![
            (
                "BEEKEEPER_PRIVATE_KEY".into(),
                provider.secret_key().to_secret_hex(),
            ),
            (
                "BEEKEEPER_CSP_PRIVATE_KEY".into(),
                provider.secret_key().to_secret_hex(),
            ),
            (
                "NOSTR_PRIVATE_KEY".into(),
                provider.secret_key().to_secret_hex(),
            ),
        ];
        request.post_fence_env = vec![
            (
                "BEEKEEPER_PRIVATE_KEY".into(),
                actor.secret_key().to_secret_hex(),
            ),
            ("BEEKEEPER_AUTH_TAG".into(), auth),
            ("BEEKEEPER_RELAY_URL".into(), "ws://127.0.0.1:1".into()),
        ];
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let mut manager = SessionManager::new(tx);
        manager
            .create(request)
            .await
            .expect("actual legacy ACP open");
        let open: serde_json::Value = std::fs::read_to_string(&wire)
            .expect("private fixture wire")
            .lines()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .find(|value| value["method"] == "session/new")
            .expect("actual session/new");
        let servers = open["params"]["mcpServers"].as_array().expect("servers");
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0]["command"].as_str(), command.to_str());
        assert_eq!(servers[0]["args"], serde_json::json!([]));
        let env: std::collections::BTreeMap<_, _> = servers[0]["env"]
            .as_array()
            .expect("environment")
            .iter()
            .map(|entry| {
                (
                    entry["name"].as_str().expect("name"),
                    entry["value"].as_str().expect("value"),
                )
            })
            .collect();
        let actual = Keys::parse(env.get("BEEKEEPER_PRIVATE_KEY").expect("actor key"))
            .expect("valid scoped key");
        assert!(
            actual.public_key() == actor.public_key(),
            "MCP must receive the exact actor, not provider/owner custody"
        );
        assert!(env.get("BEEKEEPER_AUTH_TAG").is_some_and(|tag| {
            beekeeper_sdk::nip_oa::verify_auth_tag(tag, &actor.public_key())
                .is_ok_and(|key| key == owner.public_key())
        }));
        assert!(!env.contains_key("BEEKEEPER_CSP_PRIVATE_KEY"));
        assert!(!env.contains_key("NOSTR_PRIVATE_KEY"));
        assert_ne!(
            manager
                .shutdown_and_join("legacy-mcp", ACTOR_RETIRE_GRACE)
                .await,
            ActorQuiescence::Unproven
        );
        std::fs::remove_file(wire).expect("remove private wire fixture");
    }
}
