//! Owner-authorized bootstrap; the host retains only its scoped identity.
use std::path::PathBuf;

use beekeeper_core::coding_session_runtime::RuntimeDescriptor;
use beekeeper_host_core::{atomic_write, config, layout, provision, record};
use clap::Args;
use nostr::{Keys, Tag};

use crate::{client::BeekeeperClient, error::CliError};

#[derive(Args)]
pub struct CommissionArgs {
    /// Explicit service home; defaults to BEEKEEPER_HOST_HOME, then OS home.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Adapter runtime slug. No account fallback is attempted.
    #[arg(long, value_parser = ["claude", "codex", "goose"])]
    pub runtime: String,
    /// Installed ACP adapter executable (resolved once into an absolute path).
    #[arg(long)]
    pub adapter: PathBuf,
    /// Adapter model identifier. Availability is confirmed by the provider.
    #[arg(long, default_value = "default")]
    pub model: String,
    /// Installed session-provider binary.
    #[arg(long)]
    pub provider_command: Option<PathBuf>,
    /// Restricted runtime credential file, resolved again at each session start/resume.
    #[arg(long)]
    pub credential_file: Option<PathBuf>,
    /// Explicit read-only runtime installation directory, including bundled dependencies.
    #[arg(long)]
    pub runtime_install_root: Vec<PathBuf>,
}

pub async fn commission(
    args: &CommissionArgs,
    relay: &str,
    owner_key: Option<&str>,
) -> Result<(), CliError> {
    let owner = Keys::parse(owner_key.ok_or_else(|| {
        CliError::Auth("BEEKEEPER_PRIVATE_KEY is required to attest this host".into())
    })?)
    .map_err(|_| CliError::Auth("invalid owner private key".into()))?;
    let home = args
        .home
        .clone()
        .map(Ok)
        .unwrap_or_else(layout::home_dir)
        .map_err(CliError::Usage)?;
    if !home.is_absolute() {
        return Err(CliError::Usage("--home must be absolute".into()));
    }
    let adapter = executable(&args.adapter)?;
    let relay = crate::client::to_ws_url(relay);
    let base = layout::host_dir(
        &home,
        layout::Instance::from_env().map_err(CliError::Usage)?,
    )
    .join("service/session-provider");
    atomic_write::create_dir_all_restricted(&base).map_err(CliError::Other)?;
    let store_path = base.join(record::STORE_FILE_NAME);
    // Persist mint before contacting the relay: lost replies/retry retain identity.
    let mut store = record::load_provider_store_from(&store_path).map_err(CliError::Other)?;
    let mut identity = match store.get(&relay) {
        Some(existing) => existing.clone(),
        None => provision::mint_provider_record(&owner, &relay).map_err(CliError::Other)?,
    };
    let attestation = identity
        .auth_tag
        .clone()
        .ok_or_else(|| CliError::Auth("provider has no owner attestation".into()))?;
    let auth: Vec<String> = serde_json::from_str(&attestation)
        .map_err(|_| CliError::Auth("invalid provider attestation".into()))?;
    if auth.get(1) != Some(&owner.public_key().to_hex()) {
        return Err(CliError::Auth(
            "existing provider belongs to a different owner; identity preserved".into(),
        ));
    }
    if identity.private_key_nsec.is_empty() {
        identity.private_key_nsec = std::fs::read_to_string(layout::host_key_file_path(
            &home,
            layout::Instance::from_env().map_err(CliError::Usage)?,
        ))
        .map_err(|_| {
            CliError::Auth(
                "existing provider key is unavailable; refusing to mint a replacement".into(),
            )
        })?;
    }
    let keys = Keys::parse(&identity.private_key_nsec)
        .map_err(|_| CliError::Auth("existing provider key is invalid".into()))?;
    if keys.public_key().to_hex() != identity.provider_pubkey {
        return Err(CliError::Auth(
            "existing provider key mismatches record".into(),
        ));
    }
    store.upsert(&relay, identity.clone());
    atomic_write::atomic_write_json_restricted(
        &store_path,
        &serde_json::to_vec_pretty(&store).map_err(|e| CliError::Other(e.to_string()))?,
    )
    .map_err(CliError::Other)?;
    let instance = layout::Instance::from_env().map_err(CliError::Usage)?;
    let state =
        record::provider_state_dir_in(&base, &identity.provider_pubkey).map_err(CliError::Other)?;
    atomic_write::create_dir_all_restricted(&state).map_err(CliError::Other)?;
    if !args.runtime_install_root.is_empty() {
        let mut roots = Vec::new();
        let os_home = dirs::home_dir()
            .ok_or_else(|| CliError::Usage("cannot resolve native runtime home".into()))?
            .canonicalize()
            .map_err(|_| CliError::Usage("native runtime home is unavailable".into()))?;
        let service_home = home
            .canonicalize()
            .map_err(|_| CliError::Usage("service home is unavailable".into()))?;
        let custody = layout::host_dir(&service_home, instance)
            .canonicalize()
            .map_err(|_| CliError::Usage("host custody directory is unavailable".into()))?;
        for root in &args.runtime_install_root {
            let root = std::fs::canonicalize(root)
                .map_err(|_| CliError::Usage("runtime installation root is unavailable".into()))?;
            if !root.is_dir()
                || os_home.starts_with(&root)
                || service_home.starts_with(&root)
                || root.starts_with(&custody)
                || custody.starts_with(&root)
                || (service_home != os_home && root.starts_with(&service_home))
            {
                return Err(CliError::Usage(
                    "runtime installation grant must name a specific installed runtime directory"
                        .into(),
                ));
            }
            roots.push(root);
        }
        atomic_write::atomic_write_json_restricted(
            &state.join("runtime-installations.json"),
            &serde_json::to_vec(
                &serde_json::json!({"runtimes":{format!("{}-primary",args.runtime):roots}}),
            )
            .map_err(|e| CliError::Other(e.to_string()))?,
        )
        .map_err(CliError::Other)?;
    }
    if let Some(path) = &args.credential_file {
        let path = std::fs::canonicalize(path)
            .map_err(|_| CliError::Usage("runtime credential file is unavailable".into()))?;
        if !path.is_file() {
            return Err(CliError::Usage(
                "runtime credential source must be a file".into(),
            ));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if std::fs::metadata(&path)
                .map_err(|e| CliError::Other(e.to_string()))?
                .permissions()
                .mode()
                & 0o077
                != 0
            {
                return Err(CliError::Usage(
                    "runtime credential file must be owner-only (0600)".into(),
                ));
            }
        }
        atomic_write::atomic_write_json_restricted(
            &state.join("runtime-credentials-source.json"),
            &serde_json::to_vec(&serde_json::json!({"path":path}))
                .map_err(|e| CliError::Other(e.to_string()))?,
        )
        .map_err(CliError::Other)?;
    }
    let driver = match args.runtime.as_str() {
        "claude" => "claude-agent-acp",
        "codex" => "codex-acp",
        _ => "goose-acp",
    };
    let descriptor = RuntimeDescriptor {
        instance_ref: format!("{}-primary", args.runtime),
        driver: driver.into(),
        runtime: args.runtime.clone(),
        agent_command: adapter.to_string_lossy().into_owned(),
        agent_args: if args.runtime == "goose" {
            vec!["acp".into()]
        } else {
            vec![]
        },
        cli_env: None,
        default_model: args.model.clone(),
        allowed_models: vec![args.model.clone()],
        discover_models: args.runtime != "goose",
        capabilities: None,
        steer_idle_guard: None,
        title_model: Some(None),
    };
    let config = config::HostConfig {
        version: config::HOST_CONFIG_VERSION,
        instance,
        relay_url: relay.clone(),
        provider_pubkey: identity.provider_pubkey.clone(),
        session_provider_base_dir: base,
        provider_state_dir: state,
        runtimes: vec![descriptor],
        max_sessions: store.max_sessions,
        turn_idle_timeout_secs: store.turn_idle_timeout_secs,
        turn_budget: store.turn_budget,
        provider_command: args
            .provider_command
            .as_ref()
            .map(|p| executable(p))
            .transpose()?,
        written_at: chrono::Utc::now().to_rfc3339(),
    };
    persist_commissioned_custody(&home, &config, &store_path, &mut store, &identity)?;
    let tag =
        Tag::parse(auth).map_err(|_| CliError::Auth("invalid provider attestation tag".into()))?;
    let client = BeekeeperClient::new(
        crate::client::normalize_relay_url(&relay),
        keys,
        Some(tag),
        Some(attestation),
    )?;
    // Authenticate as the scoped identity through the real relay. This proves
    // admission, not model/account readiness or that a worker is running.
    client
        .query(&serde_json::json!({"kinds":[44220],"authors":[identity.provider_pubkey],"limit":1}))
        .await?;
    println!(
        "{}",
        serde_json::json!({"provider_pubkey":identity.provider_pubkey,"instance_id":identity.instance_id,
        "commissioned":true,"relay_admitted":true,"runtime_prerequisite":"adapter_executable",
        "runtime_account":"unconfirmed","worker":"not_started","home":home,"state_dir":config.provider_state_dir})
    );
    Ok(())
}

// A headless caller has not verified a Keychain write. Preserve only the scoped
// provider key in restricted custody; a different client's secret-store adapter
// may remove its inline copy only after establishing its own durable custody.
fn persist_commissioned_custody(
    home: &std::path::Path,
    config: &config::HostConfig,
    store_path: &std::path::Path,
    store: &mut record::CodingSessionProviderStore,
    identity: &record::CodingSessionProviderRecord,
) -> Result<(), CliError> {
    // Serialize explicit full commissioning with scoped limit patches and
    // desktop preservation; no lock is held while contacting the relay.
    let _lock = beekeeper_host_core::ownership::management_lock(home, config.instance)
        .map_err(CliError::Other)?;
    provision::write_commissioning(home, config, identity).map_err(CliError::Other)?;
    store.upsert(&config.relay_url, identity.clone());
    atomic_write::atomic_write_json_restricted(
        store_path,
        &serde_json::to_vec_pretty(store).map_err(|error| CliError::Other(error.to_string()))?,
    )
    .map_err(CliError::Other)
}

fn executable(path: &std::path::Path) -> Result<PathBuf, CliError> {
    let found = if path.components().count() > 1 || path.is_absolute() {
        std::fs::canonicalize(path).ok()
    } else {
        std::env::var_os("PATH")
            .and_then(|v| {
                std::env::split_paths(&v)
                    .map(|d| d.join(path))
                    .find(|p| p.is_file())
            })
            .and_then(|p| std::fs::canonicalize(p).ok())
    }
    .ok_or_else(|| {
        CliError::Usage(format!(
            "executable prerequisite missing: {}",
            path.display()
        ))
    })?;
    if !found.is_file() {
        return Err(CliError::Usage(format!(
            "not an executable file: {}",
            found.display()
        )));
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(&found)
            .map_err(|e| CliError::Other(e.to_string()))?
            .permissions()
            .mode()
            & 0o111
            == 0
        {
            return Err(CliError::Usage(format!(
                "executable prerequisite lacks execute permission: {}",
                found.display()
            )));
        }
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::ToBech32;
    #[test]
    fn repeated_headless_commission_retains_scoped_fallback_without_owner_custody() {
        let home = tempfile::tempdir().expect("isolated home");
        let owner = Keys::generate();
        let identity =
            provision::mint_provider_record(&owner, "ws://127.0.0.1:3037").expect("mint");
        let base = home.path().join("service/session-provider");
        atomic_write::create_dir_all_restricted(&base).expect("private base");
        let store_path = base.join(record::STORE_FILE_NAME);
        let config = config::HostConfig {
            version: config::HOST_CONFIG_VERSION,
            instance: layout::Instance::Production,
            relay_url: identity.relay_url.clone(),
            provider_pubkey: identity.provider_pubkey.clone(),
            session_provider_base_dir: base.clone(),
            provider_state_dir: base.join(&identity.provider_pubkey),
            runtimes: Vec::new(),
            max_sessions: None,
            turn_idle_timeout_secs: None,
            turn_budget: None,
            provider_command: None,
            written_at: identity.created_at.clone(),
        };
        let mut store = record::load_provider_store_from(&store_path).expect("new store");
        persist_commissioned_custody(home.path(), &config, &store_path, &mut store, &identity)
            .expect("commission");
        let mut restored =
            record::load_provider_store_from(&store_path).expect("reload no Keychain");
        let scoped = restored
            .get(&identity.relay_url)
            .expect("retained identity")
            .clone();
        assert!(!scoped.private_key_nsec.is_empty());
        assert!(scoped.provider_pubkey == identity.provider_pubkey);
        assert!(
            Keys::parse(&scoped.private_key_nsec)
                .expect("scoped key")
                .public_key()
                .to_hex()
                == identity.provider_pubkey
        );
        persist_commissioned_custody(home.path(), &config, &store_path, &mut restored, &scoped)
            .expect("supported retry");
        let reloaded = record::load_provider_store_from(&store_path).expect("retry reload");
        assert!(
            reloaded
                .get(&identity.relay_url)
                .expect("same identity")
                .private_key_nsec
                == scoped.private_key_nsec
        );
        let bytes = std::fs::read_to_string(&store_path).expect("private store");
        assert!(!bytes.contains(&owner.secret_key().to_secret_hex()));
        assert!(!bytes.contains(&owner.secret_key().to_bech32().expect("owner encoding")));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&store_path)
                    .expect("metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}
