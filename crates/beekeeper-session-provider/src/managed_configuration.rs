//! Executor-private actor configuration and strict one-way native import.
//! Only protected local snapshots may alter commands/environment; public30177 cannot.
#[path = "private_persona.rs"]
pub(crate) mod private_persona;
use crate::{config::Config, session::CreateFailure};
use beekeeper_core::coding_session_runtime::RuntimeDescriptor;
use serde_json::Value;
use std::path::{Path, PathBuf};

fn refused(message: &str) -> CreateFailure {
    crate::execution_scope::refuse("AGENT_PRIVATE_CONFIG_UNAVAILABLE", message)
}
pub(crate) fn protected(path: &Path) -> Result<Option<Value>, CreateFailure> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(refused("private actor configuration unavailable")),
    };
    if !metadata.file_type().is_file() || metadata.len() > 1024 * 1024 {
        return Err(refused(
            "private actor configuration is not a bounded regular file",
        ));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(refused(
                "private actor configuration custody permissions unsafe",
            ));
        }
    }
    let bytes =
        std::fs::read(path).map_err(|_| refused("private actor configuration unreadable"))?;
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| refused("private actor configuration malformed"))
}
/// Preserved effective settings owned by this executor's custody adapter.
pub(crate) struct Settings {
    pub record: Value,
}
impl Settings {
    pub(crate) fn load(
        config: &Config,
        actor: Option<&str>,
    ) -> Result<Option<Self>, CreateFailure> {
        let Some(actor) = actor else { return Ok(None) };
        let path = config
            .state_dir
            .join("actors")
            .join(actor)
            .join("configuration.json");
        let Some(value) = protected(&path)? else {
            return Ok(None);
        };
        if value.get("schema").and_then(Value::as_str) != Some("beekeeper-private-agent-config/v1")
            || value.get("actorPubkey").and_then(Value::as_str) != Some(actor)
            || value
                .get("relayUrl")
                .and_then(Value::as_str)
                .map(beekeeper_host_core::record::canonical_relay_key)
                != Some(beekeeper_host_core::record::canonical_relay_key(
                    &config.relay_url,
                ))
        {
            return Err(refused("private configuration scope mismatch"));
        }
        let definition: nostr::Event = serde_json::from_value(
            value
                .get("definition")
                .cloned()
                .ok_or_else(|| refused("private configuration revision missing"))?,
        )
        .map_err(|_| refused("private configuration revision malformed"))?;
        let auth = std::fs::read_to_string(
            config
                .state_dir
                .join("actors")
                .join(actor)
                .join("auth-tag.json"),
        )
        .map_err(|_| refused("actor custody attestation unavailable"))?;
        let actor = nostr::PublicKey::from_hex(actor).map_err(|_| refused("actor invalid"))?;
        let owner = beekeeper_sdk::nip_oa::verify_auth_tag(&auth, &actor)
            .map_err(|_| refused("actor attestation invalid"))?;
        if definition.pubkey != owner
            || definition.kind.as_u16() != 30177
            || definition.verify().is_err()
            || !definition
                .tags
                .iter()
                .any(|t| t.as_slice() == ["d", &actor.to_hex()])
        {
            return Err(refused(
                "private configuration definition authority invalid",
            ));
        }
        Ok(Some(Self {
            record: value
                .get("record")
                .cloned()
                .ok_or_else(|| refused("private configuration record missing"))?,
        }))
    }
    pub(crate) fn apply_runtime(
        &self,
        runtime: &mut RuntimeDescriptor,
    ) -> Result<(), CreateFailure> {
        if runtime.instance_ref == "managed-actor" {
            if self
                .record
                .get("effective_command")
                .and_then(Value::as_str)
                .is_none_or(|command| command.trim().is_empty())
            {
                return Err(refused("managed-actor runtime requires protected configured command; no placeholder fallback"));
            }
            runtime.runtime = self
                .record
                .get("runtime")
                .and_then(Value::as_str)
                .filter(|name| !name.trim().is_empty())
                .ok_or_else(|| refused("managed-actor runtime label missing"))?
                .into();
        }
        if let Some(command) = self.record.get("effective_command").and_then(Value::as_str) {
            if command.trim().is_empty() {
                return Err(refused("effective runtime command empty"));
            }
            runtime.agent_command = command.into();
        }
        if let Some(args) = self.record.get("effective_args") {
            runtime.agent_args = serde_json::from_value(args.clone())
                .map_err(|_| refused("effective runtime arguments malformed"))?;
        }
        Ok(())
    }
    pub(crate) fn environment(&self) -> Result<Vec<(String, String)>, CreateFailure> {
        let Some(value) = self
            .record
            .get("effective_env")
            .or_else(|| self.record.get("env_vars"))
        else {
            return Ok(vec![]);
        };
        let env: std::collections::BTreeMap<String, String> = serde_json::from_value(value.clone())
            .map_err(|_| refused("private runtime environment malformed"))?;
        for key in env.keys() {
            if matches!(
                key.as_str(),
                "BEEKEEPER_PRIVATE_KEY"
                    | "NOSTR_PRIVATE_KEY"
                    | "BEEKEEPER_AUTH_TAG"
                    | "BEEKEEPER_CSP_PRIVATE_KEY"
            ) {
                return Err(refused(
                    "private runtime settings cannot replace identity custody",
                ));
            }
        }
        Ok(env.into_iter().collect())
    }
    /// Resolve only the custody adapter's effective catalog selection.
    /// Deprecated record.mcp_command is not an execution authority.
    pub(crate) fn mcp_command(&self) -> Result<Option<PathBuf>, CreateFailure> {
        let command = match self.record.get("effective_mcp_command") {
            None | Some(Value::Null) => return Ok(None),
            Some(Value::String(command)) if command.trim().is_empty() => return Ok(None),
            Some(Value::String(command)) => command,
            _ => return Err(refused("effective MCP executable malformed")),
        };
        let candidate = Path::new(command);
        let resolved = if candidate.is_absolute() {
            Some(candidate.to_path_buf())
        } else if candidate.components().count() == 1 {
            std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|dir| dir.join(command)))
                .filter(|path| path.is_file())
                .or_else(|| {
                    std::env::var_os("PATH").and_then(|paths| {
                        std::env::split_paths(&paths)
                            .map(|dir| dir.join(command))
                            .find(|path| path.is_file())
                    })
                })
        } else {
            None
        }
        .ok_or_else(|| {
            refused("configured MCP executable unavailable; repair installed runtime tools")
        })?;
        let metadata = std::fs::metadata(&resolved)
            .map_err(|_| refused("configured MCP executable unavailable"))?;
        if !metadata.is_file() {
            return Err(refused("configured MCP executable is not a file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return Err(refused("configured MCP executable is not executable"));
            }
        }
        resolved
            .canonicalize()
            .map(Some)
            .map_err(|_| refused("configured MCP executable unavailable"))
    }
    pub(crate) fn parallelism(&self) -> Result<Option<usize>, CreateFailure> {
        match self
            .record
            .get("effective_parallelism")
            .or_else(|| self.record.get("parallelism"))
        {
            None | Some(Value::Null) => Ok(None),
            Some(value) => value
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .filter(|value| *value > 0)
                .map(Some)
                .ok_or_else(|| refused("private parallelism must be positive")),
        }
    }
    pub(crate) fn skills(
        &self,
        state: &Path,
        session: &str,
    ) -> Result<Option<crate::session::SeatSkills>, CreateFailure> {
        let directory = self
            .record
            .get("persona_team_dir")
            .or_else(|| self.record.get("persona_pack_path"))
            .and_then(Value::as_str);
        let name = self
            .record
            .get("persona_name_in_team")
            .or_else(|| self.record.get("persona_name_in_pack"))
            .and_then(Value::as_str);
        match (directory, name) {
            (None, None) => Ok(None),
            (Some(directory), Some(name)) if !name.trim().is_empty() => {
                let path = PathBuf::from(directory);
                if !path.is_absolute() {
                    return Err(refused("private persona pack path must be absolute"));
                }
                let path = path
                    .canonicalize()
                    .map_err(|_| refused("private persona pack source unavailable"))?;
                beekeeper_persona::resolve::resolve_persona_by_name(&path, name)
                    .map_err(|_| refused("private persona pack source or role unsupported"))?;
                Ok(Some(crate::session::SeatSkills {
                    pack_dir: path,
                    persona_id: name.into(),
                    bundle_dir: crate::session::seat_bundle_dir(state, session),
                    pack_ref: None,
                    compose_ref: None,
                    agents_checkout: None,
                }))
            }
            _ => Err(refused(
                "private persona pack source and name must both be supplied",
            )),
        }
    }
    pub(crate) fn duration(
        &self,
        key: &str,
        default: std::time::Duration,
    ) -> Result<std::time::Duration, CreateFailure> {
        match self.record.get(key) {
            None | Some(Value::Null) => Ok(default),
            Some(value) => value
                .as_u64()
                .filter(|seconds| *seconds > 0)
                .map(std::time::Duration::from_secs)
                .ok_or_else(|| refused("private timeout must be positive seconds")),
        }
    }
}
pub(crate) type NativeImport = (
    String,
    Vec<(String, String)>,
    Vec<beekeeper_host_core::native_artifacts::NativeArtifact>,
);
/// Import one exact legacy context or refuse without fresh conversation fallback.
pub(crate) fn native(
    config: &Config,
    actor: Option<&str>,
    operation: &str,
    channel: uuid::Uuid,
    cwd: &Path,
    runtime: &RuntimeDescriptor,
) -> Result<Option<NativeImport>, CreateFailure> {
    let Some(value) = protected(
        &config
            .state_dir
            .join(format!("native-import-{operation}.json")),
    )?
    else {
        return Ok(None);
    };
    if value.get("schema").and_then(Value::as_str) != Some("beekeeper-legacy-native-context/v1")
        || value.get("actorPubkey").and_then(Value::as_str) != actor
        || value
            .get("relayUrl")
            .and_then(Value::as_str)
            .map(beekeeper_host_core::record::canonical_relay_key)
            != Some(beekeeper_host_core::record::canonical_relay_key(
                &config.relay_url,
            ))
    {
        return Err(refused("native import scope mismatch"));
    }
    let sessions = value
        .get("sessions")
        .and_then(Value::as_array)
        .ok_or_else(|| refused("native import sessions missing"))?;
    let selected: Vec<_> = sessions
        .iter()
        .filter(|session| {
            session.get("channelId").and_then(Value::as_str) == Some(&channel.to_string())
        })
        .collect();
    if selected.len() != 1 {
        return Err(refused(
            "native context missing or ambiguous; no fresh fallback",
        ));
    }
    let session = selected[0];
    let command = session
        .get("runtimeCommand")
        .and_then(Value::as_str)
        .ok_or_else(|| refused("native runtime unavailable; repair required"))?;
    let home = session
        .get("sourceHome")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| refused("native account home unavailable; repair required"))?;
    let prior_cwd = session
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .ok_or_else(|| refused("native workspace unavailable"))?;
    if command != runtime.agent_command
        || !home.is_dir()
        || prior_cwd
            .canonicalize()
            .map_err(|_| refused("native workspace unavailable"))?
            != cwd
                .canonicalize()
                .map_err(|_| refused("native workspace unavailable"))?
    {
        return Err(refused(
            "native runtime/account/workspace incompatible; no fresh fallback",
        ));
    }
    let cursor = session
        .get("resumeCursor")
        .and_then(Value::as_str)
        .filter(|v| !v.trim().is_empty())
        .ok_or_else(|| refused("native cursor unavailable; repair required"))?;
    let mut environment = std::collections::BTreeMap::from([(
        "HOME".to_owned(),
        home.to_string_lossy().into_owned(),
    )]);
    if let Some(value) = session.get("runtimeHomeEnvironment") {
        let scoped: std::collections::BTreeMap<String, String> =
            serde_json::from_value(value.clone())
                .map_err(|_| refused("native runtime home environment malformed"))?;
        for (key, path) in scoped {
            if !matches!(key.as_str(), "HOME" | "CODEX_HOME" | "CLAUDE_CONFIG_DIR")
                || !Path::new(&path).is_absolute()
                || !Path::new(&path).is_dir()
            {
                return Err(refused(
                    "native runtime home environment incompatible; no fresh fallback",
                ));
            }
            environment.insert(key, path);
        }
    }
    if environment.get("HOME").map(PathBuf::from).as_ref() != Some(&home) {
        return Err(refused(
            "native actual HOME facts conflict; no fresh fallback",
        ));
    }
    if let Some(config_home) = session
        .get("nativeConfigHome")
        .or_else(|| session.get("runtimeConfigHome"))
        .and_then(Value::as_str)
    {
        let key = match runtime.runtime.as_str() {
            "codex" => "CODEX_HOME",
            "claude" => "CLAUDE_CONFIG_DIR",
            _ => return Err(refused("native vendor config home unsupported")),
        };
        if environment.get(key).map(String::as_str) != Some(config_home) {
            return Err(refused("native vendor config home facts conflict"));
        }
    }
    let vendor_key = match runtime.runtime.as_str() {
        "codex" => "CODEX_HOME",
        "claude" => "CLAUDE_CONFIG_DIR",
        _ => return Err(refused("native runtime unsupported; no fresh fallback")),
    };
    if !environment.contains_key(vendor_key)
        || session.get("runtimeName").and_then(Value::as_str) != Some(runtime.runtime.as_str())
    {
        return Err(refused(
            "native runtime/vendor configuration provenance unavailable",
        ));
    }
    let artifacts: Vec<beekeeper_host_core::native_artifacts::NativeArtifact> =
        serde_json::from_value(
            session
                .get("nativeArtifacts")
                .cloned()
                .ok_or_else(|| refused("verified native artifacts missing"))?,
        )
        .map_err(|_| refused("native artifact descriptors malformed"))?;
    if artifacts.is_empty() || !matches!(runtime.runtime.as_str(), "codex" | "claude") {
        return Err(refused(
            "native artifact context unavailable or unsupported; no fresh fallback",
        ));
    }
    Ok(Some((
        cursor.into(),
        environment.into_iter().collect(),
        artifacts,
    )))
}

/// Install selected history into the existing execution machinery's private home.
pub(crate) fn materialize(
    import: &NativeImport,
    execution: &crate::execution_scope::ExecutionPlan,
) -> Result<(), CreateFailure> {
    let home = match execution {
        crate::execution_scope::ExecutionPlan::Prepared(prepared) => native_destination(
            prepared.runtime,
            prepared.codex_home.as_deref(),
            prepared.launch.env(),
        )?,
        _ => {
            return Err(refused(
                "native import requires a verified prepared runtime home",
            ))
        }
    };
    for artifact in &import.2 {
        materialize_artifact(artifact, home)?;
    }
    Ok(())
}
fn native_destination<'a>(
    runtime: crate::execution_scope::RuntimeProfile,
    codex_home: Option<&'a Path>,
    env: &'a beekeeper_acp::exec_env::ResolvedEnv,
) -> Result<&'a Path, CreateFailure> {
    match runtime {
        crate::execution_scope::RuntimeProfile::Codex => codex_home,
        crate::execution_scope::RuntimeProfile::Claude => {
            env.get("CLAUDE_CONFIG_DIR").map(Path::new)
        }
        _ => None,
    }
    .ok_or_else(|| refused("prepared native runtime home unavailable"))
}

fn materialize_artifact(
    artifact: &beekeeper_host_core::native_artifacts::NativeArtifact,
    home: &Path,
) -> Result<(), CreateFailure> {
    use sha2::{Digest, Sha256};
    if artifact.relative_path.is_absolute()
        || artifact
            .relative_path
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
        || artifact.bytes > 64 * 1024 * 1024
        || !artifact.source_path.is_absolute()
    {
        return Err(refused("native artifact path or size invalid"));
    }
    let metadata = std::fs::symlink_metadata(&artifact.source_path)
        .map_err(|_| refused("native artifact source unavailable"))?;
    if !metadata.file_type().is_file() || metadata.len() != artifact.bytes {
        return Err(refused("native artifact source changed"));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err(refused("native artifact source permissions unsafe"));
        }
    }
    let bytes = std::fs::read(&artifact.source_path)
        .map_err(|_| refused("native artifact source unreadable"))?;
    if bytes.len() as u64 != artifact.bytes
        || hex::encode(Sha256::digest(&bytes)) != artifact.sha256
    {
        return Err(refused("native artifact hash changed after verified drain"));
    }
    let mut directory = home.to_path_buf();
    let root =
        std::fs::symlink_metadata(home).map_err(|_| refused("native runtime home unavailable"))?;
    if !root.file_type().is_dir() {
        return Err(refused("native runtime home must not be a symlink"));
    }
    if let Some(parent) = artifact.relative_path.parent() {
        for component in parent.components() {
            directory.push(component);
            match std::fs::symlink_metadata(&directory) {
                Ok(metadata) if metadata.file_type().is_dir() => {}
                Ok(_) => return Err(refused("native artifact parent must not be a symlink")),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(_) => return Err(refused("native artifact parent unavailable")),
            }
        }
    }
    let target = home.join(&artifact.relative_path);
    if target.exists() {
        let metadata = std::fs::symlink_metadata(&target)
            .map_err(|_| refused("native actor context unavailable"))?;
        if !metadata.file_type().is_file() {
            return Err(refused("native actor context is not a regular file"));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o077 != 0 {
                return Err(refused("native actor context permissions unsafe"));
            }
        }
        use std::io::{BufRead, Read};
        let source_line = bytes
            .split(|byte| *byte == b'\n')
            .next()
            .ok_or_else(|| refused("native source provenance missing"))?;
        let source: Value = serde_json::from_slice(source_line)
            .map_err(|_| refused("native source provenance malformed"))?;
        let file = std::fs::File::open(&target)
            .map_err(|_| refused("native owned history unavailable"))?;
        let mut line = String::new();
        std::io::BufReader::new(file.take(64 * 1024))
            .read_line(&mut line)
            .map_err(|_| refused("native owned provenance unreadable"))?;
        let held: Value = serde_json::from_str(&line)
            .map_err(|_| refused("native owned provenance malformed"))?;
        if source.get("type").and_then(Value::as_str) != Some("session_meta")
            || held.get("type") != source.get("type")
            || source["payload"]["id"].as_str().is_none()
            || held["payload"]["id"] != source["payload"]["id"]
            || held["payload"]["cwd"] != source["payload"]["cwd"]
        {
            return Err(refused(
                "owned native history belongs to another cursor or workspace",
            ));
        }
        // This actor's runtime may already have appended new history. Never
        // replace its current native conversation with the pre-migration copy.
        return Ok(());
    }
    let parent = target
        .parent()
        .ok_or_else(|| refused("native artifact destination invalid"))?;
    beekeeper_host_core::atomic_write::create_dir_all_restricted(parent)
        .map_err(|_| refused("native artifact directory unavailable"))?;
    beekeeper_host_core::atomic_write::atomic_write_json_restricted(&target, &bytes)
        .map_err(|_| refused("native artifact materialization failed"))
}

#[cfg(test)]
#[path = "managed_configuration_tests.rs"]
mod tests;
