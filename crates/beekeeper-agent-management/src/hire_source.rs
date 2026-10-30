//! Owner-authorized project source resolution for role installation and hire.
use crate::management::local::{fail, owner};
use crate::seat_materialization::*;
use crate::{Context, Error, RelayTransport};
use beekeeper_core::coding_session_payload::PackRef;
use beekeeper_core::project_pack_source::{decode_project_pack_source, PackPin};
use beekeeper_host_core::{config::HostConfig, layout, project::ProjectsFile, record};
use nostr::Event;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::path::{Path, PathBuf};

/// Immutable role material retained in the canonical private configuration.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleMaterial {
    /// Authorized project coordinate.
    pub project: String,
    /// Exact signed project source.
    pub source: Event,
    /// Repository, commit, role and path disclosed by execution metadata.
    pub pack_ref: PackRef,
    /// Content-addressed composed pack, outside the working tree.
    pub pack_dir: PathBuf,
    /// Full composition digest, not a directory-name assertion.
    pub digest: String,
    /// Hash of every staged file byte, independently of the composition marker.
    pub content_digest: String,
    /// Immutable source clone used when cutting the seat's sibling clone.
    pub source_checkout: PathBuf,
    /// Agents-repository access from the resolved role manifest.
    pub agents_access: String,
}

pub(crate) fn git(context: &Context, root: &Path, args: &[&str]) -> Result<String, Error> {
    // status can execute clean filters; checkout can execute smudge filters.
    // Reuse the host's existing project boundary for both, with no owner auth.
    let mut command = if matches!(
        args.first(),
        Some(&"status") | Some(&"checkout") | Some(&"diff")
    ) {
        let plan = git_boundary(context, root, None)?;
        plan.git_command(root, args)
    } else {
        let mut command = std::process::Command::new("git");
        for name in [
            "GIT_DIR",
            "GIT_WORK_TREE",
            "GIT_INDEX_FILE",
            "GIT_COMMON_DIR",
            "GIT_NAMESPACE",
            "GIT_OBJECT_DIRECTORY",
            "GIT_ALTERNATE_OBJECT_DIRECTORIES",
            "GIT_CONFIG_COUNT",
            "GIT_CONFIG_PARAMETERS",
        ] {
            command.env_remove(name);
        }
        command
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1");
        command
            .args([
                "-c",
                "core.hooksPath=/dev/null",
                "-c",
                "core.fsmonitor=false",
            ])
            .args(args);
        command
    };
    let output = command
        .stdin(std::process::Stdio::null())
        .current_dir(root)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env_remove("BEEKEEPER_PRIVATE_KEY")
        .env_remove("NOSTR_PRIVATE_KEY")
        .env_remove("BEEKEEPER_AUTH_TAG")
        .output()
        .map_err(fail)?;
    if !output.status.success() {
        return Err(Error::Unconfirmed(format!(
            "local git materialization outcome unavailable: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn host(context: &Context, client: &impl RelayTransport) -> Result<HostConfig, Error> {
    let signer = owner(client)?;
    let config = HostConfig::load(&layout::host_config_path(&context.home, context.instance))
        .map_err(fail)?
        .ok_or_else(|| Error::Refused("host is not commissioned".into()))?;
    if record::canonical_relay_key(&crate::to_ws_url(client.relay_url()))
        != record::canonical_relay_key(&config.relay_url)
    {
        return Err(Error::Auth(
            "commissioned host relay differs from management relay".into(),
        ));
    }
    let store = record::load_provider_store_from(&config.record_store_path()).map_err(fail)?;
    let provider = store
        .get(&config.relay_url)
        .ok_or_else(|| fail("provider record missing"))?;
    if provider.provider_pubkey != config.provider_pubkey
        || beekeeper_sdk::nip_oa::verify_auth_tag(
            provider
                .auth_tag
                .as_deref()
                .ok_or_else(|| fail("provider owner attestation missing"))?,
            &nostr::PublicKey::from_hex(&config.provider_pubkey).map_err(fail)?,
        )
        .map_err(fail)?
        .to_hex()
            != signer
    {
        return Err(Error::Auth(
            "management signer does not own commissioned provider".into(),
        ));
    }
    Ok(config)
}

pub(crate) fn projects(config: &HostConfig) -> Result<ProjectsFile, Error> {
    serde_json::from_slice(
        &std::fs::read(
            config
                .provider_state_dir
                .join(beekeeper_host_core::env::PROJECTS_FILE_NAME),
        )
        .map_err(fail)?,
    )
    .map_err(fail)
}

pub(crate) async fn source(client: &impl RelayTransport, project: &str) -> Result<Event, Error> {
    let signer = owner(client)?;
    crate::authorize_project(client, project, &signer).await?;
    let creator = project
        .split(':')
        .nth(1)
        .ok_or_else(|| Error::Usage("invalid project".into()))?;
    let mut rows: Vec<Event> = client
        .query_all(json!({"kinds":[30624],"authors":[creator],"#d":[project]}))
        .await?
        .into_iter()
        .filter_map(|v| serde_json::from_value::<Event>(v).ok())
        .filter(|e| {
            e.pubkey.to_hex() == creator
                && e.verify().is_ok()
                && decode_project_pack_source(e).is_ok_and(|s| s.project() == project)
        })
        .collect();
    rows.sort_by(|a, b| b.created_at.cmp(&a.created_at).then(a.id.cmp(&b.id)));
    rows.into_iter().next().ok_or_else(|| {
        Error::Refused("HIRE_PACK_SOURCE_UNAVAILABLE: no current creator-signed source".into())
    })
}

/// Resolve the project's trusted, signed source and compose the canonical role.
/// The source must be registered by `host trust-project`; moving refs are
/// fetched using the existing owner-scoped git service, never ambient credentials.
pub async fn materialize(
    context: &Context,
    client: &impl RelayTransport,
    project: &str,
    role: &str,
    templates: &Path,
) -> Result<RoleMaterial, Error> {
    let config = host(context, client)?;
    let state = projects(&config)?;
    let trusted = state.agents_repos.get(project).ok_or_else(|| {
        Error::Refused("HIRE_CHECKOUT_NOT_RECORDED: run host trust-project".into())
    })?;
    let event = source(client, project).await?;
    let decoded = decode_project_pack_source(&event).map_err(fail)?;
    if trusted.ref_name != decoded.pin().value() {
        return Err(Error::Refused(
            "HIRE_PACK_SOURCE_STALE: refresh trusted project source".into(),
        ));
    }
    let sha = current_tip(context, client, trusted, &event).await?;
    let root = layout::host_dir(&context.home, context.instance).join("management/role-sources");
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&root).map_err(fail)?;
    // Source selection is immutable per commit; a moving ref cannot mutate
    // another operation's already installed source checkout.
    let cache_key = format!("{}-{sha}", event.id.to_hex());
    let lock_path = root.join(format!("{cache_key}.lock"));
    if std::fs::symlink_metadata(&lock_path).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(fail("role source lock must not be a symlink"));
    }
    let mut options = std::fs::OpenOptions::new();
    options.create(true).read(true).write(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options.open(lock_path).map_err(fail)?;
    lock.lock().map_err(fail)?;
    let checkout = root.join(cache_key);
    if !checkout.exists() {
        git(
            context,
            &root,
            &[
                "clone",
                "--no-hardlinks",
                "--no-checkout",
                "--",
                trusted
                    .path
                    .to_str()
                    .ok_or_else(|| fail("source path is not UTF-8"))?,
                checkout
                    .to_str()
                    .ok_or_else(|| fail("cache path is not UTF-8"))?,
            ],
        )?;
    }
    git(context, &checkout, &["checkout", "--detach", &sha])?;
    if !git(context, &checkout, &["status", "--porcelain"])?.is_empty() {
        return Err(Error::Refused(
            "immutable role source clone is dirty".into(),
        ));
    }
    let role_source = locate_role_source(&checkout, decoded.path(), role).ok_or_else(|| {
        Error::Refused("HIRE_PACK_UNAVAILABLE: signed repository does not define role".into())
    })?;
    let path = pack_ref_path(&role_source, decoded.path());
    let catalog = TemplateCatalog::load(templates, env!("CARGO_PKG_VERSION")).map_err(fail)?;
    let composed = stage_composed_pack(
        &root.join("packs"),
        &event.id.to_hex(),
        &role_source,
        &catalog,
        SourceProvenance {
            kind: "repository".into(),
            repo: Some(decoded.repo().into()),
            sha: Some(sha.clone()),
            path: path.clone(),
        },
    )
    .map_err(fail)?;
    let agents_access = match composed.agents_repo {
        AgentsRepoAccess::None => "none",
        AgentsRepoAccess::Read => "read",
        AgentsRepoAccess::Write => "write",
    }
    .to_string();
    Ok(RoleMaterial {
        project: project.into(),
        pack_ref: PackRef {
            repo: decoded.repo().into(),
            sha: sha.clone(),
            role: role.into(),
            path,
        },
        source: event,
        content_digest: crate::hire_pack_integrity::digest(&composed.dir)?,
        pack_dir: composed.dir,
        digest: composed.digest,
        source_checkout: checkout,
        agents_access,
    })
}

/// Resolve the authorized source with the existing owner-scoped git reader.
pub(crate) async fn current_tip(
    context: &Context,
    client: &impl RelayTransport,
    trusted: &beekeeper_host_core::project::AgentsRepoRecord,
    event: &Event,
) -> Result<String, Error> {
    let source = decode_project_pack_source(event).map_err(fail)?;
    let parts = source.repo().splitn(3, ':').collect::<Vec<_>>();
    let expected = format!(
        "{}/git/{}/{}",
        client.relay_url().trim_end_matches('/'),
        parts[1],
        parts[2]
    );
    if trusted.url.as_deref() != Some(&expected) || trusted.ref_name != source.pin().value() {
        return Err(Error::Refused(
            "trusted source no longer matches signed repository/ref".into(),
        ));
    }
    if let PackPin::Sha(sha) = source.pin() {
        if git(
            context,
            &trusted.path,
            &["rev-parse", &format!("{sha}^{{commit}}")],
        )? != *sha
        {
            return Err(Error::Refused("signed source commit unavailable".into()));
        }
        return Ok(sha.clone());
    }
    let helper = std::env::current_exe()
        .map_err(fail)?
        .parent()
        .ok_or_else(|| fail("management executable directory missing"))?
        .join(if cfg!(windows) {
            "git-credential-nostr.exe"
        } else {
            "git-credential-nostr"
        });
    let auth = beekeeper_session_provider::agents_checkout::ScopedGitAuth::new(
        client.keys().clone(),
        None,
        helper,
    )
    .map_err(fail)?;
    let tip = beekeeper_session_provider::agents_checkout::pinned_tip_scoped(trusted, Some(&auth))
        .await
        .map_err(fail)?;
    if tip.stale {
        return Err(Error::Refused(
            "HIRE_PACK_SOURCE_STALE: current authorized source could not be fetched".into(),
        ));
    }
    Ok(tip.sha)
}

/// Prepare the existing host boundary around repository-executed filters or seed recipes.
pub(crate) fn git_boundary(
    context: &Context,
    tree: &Path,
    checkout: Option<&Path>,
) -> Result<beekeeper_session_provider::execution_scope_host::HostLaunchPlan, Error> {
    use beekeeper_session_provider::execution_scope_host::{
        prepare_host_command, HostCommandScope,
    };
    let state = layout::host_dir(&context.home, context.instance).join("management/git-boundaries");
    beekeeper_host_core::atomic_write::create_dir_all_restricted(&state).map_err(fail)?;
    let mut scope = HostCommandScope::git(&state, None, checkout, tree, "hire-materialization");
    // The selected branch remains protected by the durable hire plan and the
    // exact worktree checks; this does not authorize arbitrary branch movement.
    scope.host_branch = None;
    prepare_host_command(&scope).map_err(|e| Error::Refused(format!("{}: {}", e.code, e.message)))
}
