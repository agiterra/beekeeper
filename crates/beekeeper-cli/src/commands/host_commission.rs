//! Owner-authorized bootstrap; the host retains only its scoped identity.
use std::path::{Path, PathBuf};

use beekeeper_core::coding_session_runtime::{
    credential_env_name_rejection, RuntimeDescriptor, MAX_CREDENTIAL_ENV,
};
use beekeeper_host_core::{atomic_write, config, layout, provision, record};
use clap::Args;
use nostr::{Keys, Tag};

use crate::{client::BeekeeperClient, error::CliError};

/// Set up this machine as an agent host from a terminal. No desktop needed.
///
/// Runs once per machine as the owner (BEEKEEPER_PRIVATE_KEY): mints the
/// host's own scoped identity, writes `host.json` naming the runtime and the
/// ACP adapter that drives it, and proves the relay admits that identity.
/// It does not start anything. Next: `bee host install` (or `beekeeper-host
/// run` in the foreground), then `bee host status`, then `bee agents create
/// --provider-instance <runtime>-primary ...`.
#[derive(Args)]
#[command(after_help = "Examples:\n  \
bee host commission --runtime claude --adapter /opt/beekeeper/bin/claude-agent-acp\n  \
bee host commission --runtime acme --driver /opt/acme/bin/acme-acp \\\n      \
--credential-env ACME_TOKEN --credential-file /etc/beekeeper/acme.json \\\n      \
--credential-source acme-vault\n\n\
Re-running with the same --home keeps the same host identity. Running again \
after a change to --runtime, --driver or --model rewrites host.json; then \
`bee host bind` restarts the running host onto it.")]
pub struct CommissionArgs {
    /// Directory the host keeps its state in. Defaults to BEEKEEPER_HOST_HOME,
    /// then the OS home. Must be absolute; pass the same value to every
    /// later `bee host ...` and `bee agents ...` call that takes --home.
    #[arg(long)]
    pub home: Option<PathBuf>,
    /// Which runtime this host runs. `claude`, `codex` and `goose` have
    /// built-in drivers; any other lowercase word (letters, digits, `-`,
    /// `_`) is a runtime you plug in, and needs --driver. The word also
    /// names the instance that `bee agents create --provider-instance
    /// <word>-primary` refers to.
    #[arg(long)]
    pub runtime: String,
    /// The older spelling of --driver, kept so existing scripts keep working
    /// for the built-in runtimes (claude, codex, goose) only. Any other
    /// runtime must pass --driver; when both are given, --driver wins.
    #[arg(long)]
    pub adapter: Option<PathBuf>,
    /// The ACP adapter command that drives this runtime: an absolute path, or
    /// a name on PATH, resolved once to an absolute path. Required for any
    /// runtime other than claude, codex or goose. Its file name becomes the
    /// driver slug written into every session (`/opt/acme/bin/acme-acp` ->
    /// `acme-acp`), so name the file accordingly.
    #[arg(long, required_unless_present = "adapter")]
    pub driver: Option<PathBuf>,
    /// An argument passed to the driver after the command, in order
    /// (repeatable). For claude, codex or goose this replaces the built-in
    /// arguments rather than adding to them.
    #[arg(long = "driver-arg")]
    pub driver_args: Vec<String>,
    /// Name of an environment variable the driver reads its credential from
    /// (repeatable; names only, never values). The provider sets it from
    /// --credential-file at each session start. Required with
    /// --credential-file for a plugged runtime; for claude, codex or goose it
    /// narrows the built-in list.
    #[arg(long = "credential-env")]
    pub credential_env: Vec<String>,
    /// A label for where this runtime's credential comes from (e.g.
    /// `acme-vault`), echoed into session metadata and auth-failure receipts
    /// so a reader can tell which credential failed. A label, never a secret
    /// or a path.
    #[arg(long)]
    pub credential_source: Option<String>,
    /// Model identifier to ask the adapter for. Not checked here: the provider
    /// confirms the model is available when a session starts.
    #[arg(long, default_value = "default")]
    pub model: String,
    /// The `beekeeper-session-provider` binary the host should run. Defaults
    /// to the one beside `beekeeper-host`, then PATH.
    #[arg(long)]
    pub provider_command: Option<PathBuf>,
    /// Owner-only (chmod 600) JSON file holding the runtime's credentials:
    /// `{"runtimes": {"<runtime>-primary": {"<VAR>": "<value>"}}}`, where each
    /// <VAR> is a name given with --credential-env. Read again at every
    /// session start, never copied into host state.
    #[arg(long)]
    pub credential_file: Option<PathBuf>,
    /// A directory the runtime is installed in (repeatable), granted to
    /// sessions read-only along with its bundled dependencies. Must be a
    /// specific install directory: not your home, not --home, and not inside
    /// the host's own state.
    #[arg(long)]
    pub runtime_install_root: Vec<PathBuf>,
    /// A capability tag this host declares (repeatable): `gpu`,
    /// `toolchain:rust`, `os:macos`. Lowercase letters, digits, `.`, `_`,
    /// `-`, `:`; at most 64 bytes each. Written to host.json and carried on
    /// every host-step claim this host signs, so a project step that says
    /// `requires: [gpu]` is won only by a host that declared it. Nothing
    /// checks the claim: it is your word as the commissioning operator.
    #[arg(long = "capability")]
    pub capabilities: Vec<String>,
}

pub async fn commission(
    args: &CommissionArgs,
    relay: &str,
    owner_key: Option<&str>,
) -> Result<(), CliError> {
    let owner = Keys::parse(owner_key.ok_or_else(|| {
        CliError::Auth(
            "BEEKEEPER_PRIVATE_KEY is required to attest this host: export the owner's key (hex \
             or nsec) or pass --private-key"
                .into(),
        )
    })?)
    .map_err(|_| {
        CliError::Auth(
            "BEEKEEPER_PRIVATE_KEY is not a valid key (expected 64 hex chars or an nsec)".into(),
        )
    })?;
    let home = args
        .home
        .clone()
        .map(Ok)
        .unwrap_or_else(layout::home_dir)
        .map_err(CliError::Usage)?;
    if !home.is_absolute() {
        return Err(CliError::Usage(format!(
            "--home must be an absolute path, got {}",
            home.display()
        )));
    }
    let adapter = executable(
        args.driver
            .as_deref()
            .or(args.adapter.as_deref())
            .ok_or_else(|| {
                CliError::Usage(
                    "pass --driver <command> naming the ACP adapter that drives this runtime"
                        .into(),
                )
            })?,
    )?;
    let descriptor = runtime_descriptor(args, &adapter)?;
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
        return Err(CliError::Auth(format!(
            "this host was commissioned by a different owner; its identity is preserved. Re-run \
             with that owner's BEEKEEPER_PRIVATE_KEY, or commission a fresh --home ({} holds \
             the existing one)",
            store_path.display()
        )));
    }
    if identity.private_key_nsec.is_empty() {
        let key_file = layout::host_key_file_path(
            &home,
            layout::Instance::from_env().map_err(CliError::Usage)?,
        );
        identity.private_key_nsec = std::fs::read_to_string(&key_file).map_err(|_| {
            CliError::Auth(format!(
                "this host already has an identity but its key file {} cannot be read; refusing \
                 to mint a replacement. Restore the file, or commission a fresh --home",
                key_file.display()
            ))
        })?;
    }
    let keys = Keys::parse(&identity.private_key_nsec).map_err(|_| {
        CliError::Auth(
            "the existing host key is not a valid key; restore it from backup or commission a \
             fresh --home"
                .into(),
        )
    })?;
    if keys.public_key().to_hex() != identity.provider_pubkey {
        return Err(CliError::Auth(format!(
            "the existing host key does not match the identity recorded in {}; restore both \
             from the same backup, or commission a fresh --home",
            store_path.display()
        )));
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
            let given = root;
            let root = std::fs::canonicalize(root).map_err(|_| {
                CliError::Usage(format!(
                    "--runtime-install-root {}: no such directory",
                    given.display()
                ))
            })?;
            if !root.is_dir()
                || os_home.starts_with(&root)
                || service_home.starts_with(&root)
                || root.starts_with(&custody)
                || custody.starts_with(&root)
                || (service_home != os_home && root.starts_with(&service_home))
            {
                return Err(CliError::Usage(format!(
                    "--runtime-install-root {} must be a specific installed runtime directory: \
                     not a parent of your home or of --home, and not inside the host's state \
                     under {}",
                    given.display(),
                    custody.display()
                )));
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
        let given = path;
        let path = std::fs::canonicalize(path).map_err(|_| {
            CliError::Usage(format!(
                "--credential-file {}: cannot be read. Create it as owner-only JSON: \
                 {{\"runtimes\": {{\"{}-primary\": {{\"<VAR>\": \"<value>\"}}}}}}",
                given.display(),
                args.runtime
            ))
        })?;
        if !path.is_file() {
            return Err(CliError::Usage(format!(
                "--credential-file {} is not a regular file",
                given.display()
            )));
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
                return Err(CliError::Usage(format!(
                    "--credential-file {} is readable by others; run `chmod 600 {}`",
                    given.display(),
                    path.display()
                )));
            }
        }
        atomic_write::atomic_write_json_restricted(
            &state.join("runtime-credentials-source.json"),
            &serde_json::to_vec(&serde_json::json!({"path":path}))
                .map_err(|e| CliError::Other(e.to_string()))?,
        )
        .map_err(CliError::Other)?;
    }
    beekeeper_core::host_step::validate_host_capabilities("--capability", &args.capabilities)
        .map_err(CliError::Usage)?;
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
        checkpoint_on_stop: store.checkpoint_on_stop,
        capabilities: args.capabilities.clone(),
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

/// The driver slug and default adapter arguments a known runtime word mints
/// without being told; any other word is a plugged runtime that must name
/// its driver.
fn known_runtime(runtime: &str) -> Option<(&'static str, &'static [&'static str])> {
    match runtime {
        "claude" => Some(("claude-agent-acp", &[])),
        "codex" => Some(("codex-acp", &[])),
        "goose" => Some(("goose-acp", &["acp"])),
        _ => None,
    }
}

/// The known runtime word whose built-in driver is `slug`, if any. The host's
/// own `managed-acp` driver counts: it is minted by the host for the managed
/// profile and is never a plugged adapter's name.
fn known_runtime_for_driver(slug: &str) -> Option<&'static str> {
    match slug {
        "claude-agent-acp" => Some("claude"),
        "codex-acp" => Some("codex"),
        "goose-acp" => Some("goose"),
        "managed-acp" => Some("managed"),
        _ => None,
    }
}

/// A runtime word or driver slug: lowercase ASCII letters, digits, `-` and
/// `_`, starting with a letter or digit, at most 32 bytes. It is minted into
/// instance refs (`<word>-primary`) and session targets, so it has to be a
/// token every reader of those accepts.
fn is_slug(word: &str) -> bool {
    !word.is_empty()
        && word.len() <= 32
        && word.as_bytes()[0].is_ascii_alphanumeric()
        && word.bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
        })
}

/// The descriptor this commissioning writes: a known word keeps today's
/// driver, arguments and model discovery; any other word takes its driver
/// slug from the resolved adapter's file name, runs with the arguments
/// given, and discovers nothing it was not told.
fn runtime_descriptor(
    args: &CommissionArgs,
    adapter: &Path,
) -> Result<RuntimeDescriptor, CliError> {
    if !is_slug(&args.runtime) {
        return Err(CliError::Usage(format!(
            "--runtime {:?} is not a runtime word (lowercase letters, digits, '-' or '_', at most 32 bytes)",
            args.runtime
        )));
    }
    let (driver, default_args) = match known_runtime(&args.runtime) {
        Some((driver, default_args)) => (driver.to_owned(), default_args),
        None => {
            if args.driver.is_none() {
                return Err(CliError::Usage(format!(
                    "runtime {:?} has no built-in driver; pass --driver <command> naming the ACP adapter that drives it",
                    args.runtime
                )));
            }
            let slug = adapter
                .file_name()
                .and_then(|name| name.to_str())
                .filter(|name| is_slug(name))
                .ok_or_else(|| {
                    CliError::Usage(format!(
                        "the driver's file name {:?} is not a driver slug (lowercase letters, digits, '-' or '_'); rename or symlink the adapter",
                        adapter.file_name().unwrap_or_default()
                    ))
                })?;
            // The driver slug selects the provider's adapter profile, so a
            // plugged word whose adapter happens to be named like a known
            // runtime's driver would run on that runtime's profile under a
            // foreign name. Say which word to use instead.
            if let Some(word) = known_runtime_for_driver(slug) {
                return Err(CliError::Usage(format!(
                    "the driver's file name {slug:?} is the {word} runtime's own driver; commission it as --runtime {word} (no --driver), or give the plugged adapter a different file name"
                )));
            }
            (slug.to_owned(), &[][..])
        }
    };
    if args.credential_env.len() > MAX_CREDENTIAL_ENV {
        return Err(CliError::Usage(format!(
            "--credential-env may be given at most {MAX_CREDENTIAL_ENV} times; keep only the variables \
             the driver actually reads"
        )));
    }
    for name in &args.credential_env {
        if let Some(reason) = credential_env_name_rejection(name) {
            return Err(CliError::Usage(format!("--credential-env: {reason}")));
        }
    }
    let credential_source = match args.credential_source.as_deref().map(str::trim) {
        None => None,
        Some(label) if label.is_empty() || label.len() > 128 => {
            return Err(CliError::Usage(
                "--credential-source must be a non-empty label of at most 128 bytes".into(),
            ))
        }
        Some(label) => Some(label.to_owned()),
    };
    Ok(RuntimeDescriptor {
        instance_ref: format!("{}-primary", args.runtime),
        driver,
        runtime: args.runtime.clone(),
        agent_command: adapter.to_string_lossy().into_owned(),
        agent_args: if args.driver_args.is_empty() {
            default_args.iter().map(|arg| (*arg).to_owned()).collect()
        } else {
            args.driver_args.clone()
        },
        cli_env: None,
        default_model: args.model.clone(),
        allowed_models: vec![args.model.clone()],
        discover_models: matches!(args.runtime.as_str(), "claude" | "codex"),
        capabilities: None,
        steer_idle_guard: None,
        title_model: Some(None),
        credential_env: args.credential_env.clone(),
        credential_source,
    })
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
            "cannot find {}: give an absolute path, or put it on PATH (install the ACP adapter \
             for this runtime first)",
            path.display()
        ))
    })?;
    if !found.is_file() {
        return Err(CliError::Usage(format!(
            "{} is not a file; name the adapter executable itself, not its directory",
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
                "{} is not executable; run `chmod +x {}`",
                found.display(),
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

    fn args(runtime: &str) -> CommissionArgs {
        CommissionArgs {
            home: None,
            runtime: runtime.to_owned(),
            adapter: None,
            driver: None,
            driver_args: Vec::new(),
            credential_env: Vec::new(),
            credential_source: None,
            model: "default".into(),
            provider_command: None,
            credential_file: None,
            runtime_install_root: Vec::new(),
            capabilities: Vec::new(),
        }
    }

    fn acme_args() -> CommissionArgs {
        let mut acme = args("acme");
        acme.driver = Some("/opt/acme/bin/acme-acp".into());
        acme.driver_args = vec!["--acp".into()];
        acme.credential_env = vec!["ACME_TOKEN".into()];
        acme.credential_source = Some(" acme-vault ".into());
        acme
    }

    /// A plugged adapter whose file name is a known runtime's driver would
    /// land on that runtime's profile; the refusal names the word to use.
    #[test]
    fn a_plugged_driver_named_like_a_known_runtime_is_refused_by_name() {
        for (file, word) in [
            ("claude-agent-acp", "claude"),
            ("codex-acp", "codex"),
            ("goose-acp", "goose"),
            ("managed-acp", "managed"),
        ] {
            let mut plugged = args("acme");
            plugged.driver = Some(format!("/opt/acme/bin/{file}").into());
            let error = runtime_descriptor(&plugged, Path::new(&format!("/opt/acme/bin/{file}")))
                .expect_err(file);
            let text = error.to_string();
            assert!(text.contains(file), "{text}");
            assert!(text.contains(&format!("--runtime {word}")), "{text}");
        }
        // A known word with its own driver name is not a plugged runtime.
        let claude = runtime_descriptor(&args("claude"), Path::new("/opt/bin/claude-agent-acp"))
            .expect("known");
        assert_eq!(claude.driver, "claude-agent-acp");
    }

    /// Reserved names are refused at commission time with the parser's reason.
    #[test]
    fn reserved_credential_env_names_are_refused_at_commission() {
        for reserved in [
            "HOME",
            "PATH",
            "BEEKEEPER_RELAY_URL",
            "BUZZ_PRIVATE_KEY",
            "LD_PRELOAD",
            "DYLD_LIBRARY_PATH",
        ] {
            let mut plugged = acme_args();
            plugged.credential_env = vec![reserved.into()];
            let error = runtime_descriptor(&plugged, Path::new("/opt/acme/bin/acme-acp"))
                .expect_err(reserved);
            assert!(error.to_string().contains(reserved), "{error}");
        }
    }

    /// Known words keep exactly the descriptor they minted before the plug.
    #[test]
    fn known_runtime_words_keep_their_drivers_arguments_and_discovery() {
        let claude = runtime_descriptor(&args("claude"), Path::new("/opt/bin/claude-agent-acp"))
            .expect("claude");
        assert_eq!(claude.instance_ref, "claude-primary");
        assert_eq!(claude.driver, "claude-agent-acp");
        assert!(claude.agent_args.is_empty());
        assert!(claude.discover_models);
        assert!(claude.credential_env.is_empty());
        assert_eq!(claude.credential_source, None);
        assert_eq!(claude.title_model, Some(None));

        let codex =
            runtime_descriptor(&args("codex"), Path::new("/opt/bin/codex-acp")).expect("codex");
        assert_eq!(codex.driver, "codex-acp");
        assert!(codex.discover_models);

        let goose = runtime_descriptor(&args("goose"), Path::new("/opt/bin/goose")).expect("goose");
        assert_eq!(goose.driver, "goose-acp");
        assert_eq!(goose.agent_args, vec!["acp".to_owned()]);
        assert!(!goose.discover_models);

        // Explicit driver arguments replace a known word's defaults.
        let mut custom = args("goose");
        custom.driver_args = vec!["acp".into(), "--quiet".into()];
        assert_eq!(
            runtime_descriptor(&custom, Path::new("/opt/bin/goose"))
                .expect("goose")
                .agent_args,
            vec!["acp".to_owned(), "--quiet".to_owned()]
        );
    }

    /// The plug: any word, driven by the command named, with the variables
    /// and label it declares — and refused, by name of the fix, without one.
    #[test]
    fn any_runtime_word_is_accepted_with_a_driver_and_refused_without_one() {
        let descriptor =
            runtime_descriptor(&acme_args(), Path::new("/opt/acme/bin/acme-acp")).expect("acme");
        assert_eq!(descriptor.instance_ref, "acme-primary");
        assert_eq!(descriptor.driver, "acme-acp");
        assert_eq!(descriptor.runtime, "acme");
        assert_eq!(descriptor.agent_command, "/opt/acme/bin/acme-acp");
        assert_eq!(descriptor.agent_args, vec!["--acp".to_owned()]);
        assert!(!descriptor.discover_models);
        assert_eq!(descriptor.credential_env, vec!["ACME_TOKEN".to_owned()]);
        assert_eq!(descriptor.credential_source.as_deref(), Some("acme-vault"));
        assert!(
            descriptor.capabilities.is_none(),
            "the baseline vector is the provider's to decide"
        );

        let refused = runtime_descriptor(&args("acme"), Path::new("/opt/acme/bin/acme-acp"))
            .expect_err("no driver");
        assert!(refused.to_string().contains("--driver"), "{refused}");

        let mut spaced = acme_args();
        spaced.driver = Some("/opt/acme/bin/acme acp".into());
        assert!(runtime_descriptor(&spaced, Path::new("/opt/acme/bin/acme acp")).is_err());

        for bad in ["Acme", "acme!", "", "-"] {
            let mut entry = acme_args();
            entry.runtime = bad.into();
            assert!(
                runtime_descriptor(&entry, Path::new("/opt/acme/bin/acme-acp")).is_err(),
                "{bad:?}"
            );
        }
        let mut bad_env = acme_args();
        bad_env.credential_env = vec!["ACME-TOKEN".into()];
        assert!(runtime_descriptor(&bad_env, Path::new("/opt/acme/bin/acme-acp")).is_err());
        let mut blank_label = acme_args();
        blank_label.credential_source = Some("  ".into());
        assert!(runtime_descriptor(&blank_label, Path::new("/opt/acme/bin/acme-acp")).is_err());
    }
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
            checkpoint_on_stop: false,
            // Round 2 Lane F: what `--capability` writes reaches host.json.
            capabilities: vec!["gpu".into(), "toolchain:rust".into()],
            provider_command: None,
            written_at: identity.created_at.clone(),
        };
        let mut store = record::load_provider_store_from(&store_path).expect("new store");
        persist_commissioned_custody(home.path(), &config, &store_path, &mut store, &identity)
            .expect("commission");
        let written = config::HostConfig::load(&layout::host_config_path(
            home.path(),
            layout::Instance::Production,
        ))
        .expect("host.json")
        .expect("commissioned");
        assert_eq!(written.capabilities, config.capabilities);
        assert_eq!(
            written.capabilities_env_value().as_deref(),
            Some("gpu,toolchain:rust")
        );
        assert!(beekeeper_core::host_step::validate_host_capabilities(
            "--capability",
            &["GPU".into()]
        )
        .expect_err("uppercase is refused at commission, by flag name")
        .contains("--capability"));
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
