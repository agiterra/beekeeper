//! The reconstruction half of `bee sessions handover continue`: turning a
//! checkpoint's artifacts into a checkout, and a checkout into a new
//! execution.
//!
//! Split out of `handover_continue.rs` so neither file passes 1,000 lines. The
//! decisions live there — standing, reachability, native or reconstruct, the
//! claim — and the doing lives here.
//!
//! # The base comes first
//!
//! A patch artifact is a diff against a **commit**. The order artifacts appear
//! in the record is the author's and nobody promised it puts the wip ref
//! first, so [`split_artifacts`] imposes the only order that means anything,
//! and [`BaseState::may_apply_overlays`] stops a diff being three-way merged
//! onto whatever the directory happened to hold when its base did not land
//! (REVIEW N9).

use std::path::{Path, PathBuf};
use std::time::Duration;

use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_handover::{
    CodingSessionHandoverArtifactKind, CodingSessionHandoverCheckpoint,
};
use buzz_core::coding_session_identity::ProviderInstanceAlias;
use buzz_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::{decode_coding_session_lifecycle_receipt, ReceiptStatus};
use buzz_core::kind::KIND_CODING_SESSION_LIFECYCLE_RECEIPT;
use buzz_sdk::builders::build_coding_session_lifecycle_command;
use nostr::Event;
use serde_json::{json, Value};
use uuid::Uuid;

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::sdk_err;

use super::crew::short_pubkey;
use super::handover::HandoverState;
use super::handover_blob::fetch_verified_blob;
use super::handover_git::{apply_patch, fetch_and_checkout, resolve_push_remote};
use super::handover_render::{render_initial_turn, VerificationNotes, RECONSTRUCTION_LIMIT};

/// How often a receipt wait re-asks the relay.
const RECEIPT_POLL: Duration = Duration::from_millis(500);

/// What a reconstruction managed to fetch and apply.
pub(super) struct Recovery {
    /// Lines a person can check, one per artifact that landed.
    pub(super) recovered: Vec<String>,
    /// Lines naming what did not, and why.
    pub(super) missing: Vec<String>,
}

/// Fetch the checkpoint's artifacts into `--cwd` and apply the patch.
///
/// Every artifact is reported either as recovered or as missing; a patch that
/// fails `git apply --check` is a `missing` line and the tree is exactly as it
/// was, because the check runs before anything is written.
pub(super) async fn recover_checkout(
    client: &BuzzClient,
    cwd: Option<&Path>,
    session8: &str,
    checkpoint: Option<&CodingSessionHandoverCheckpoint>,
    remote: Option<&str>,
    allow_no_artifact: bool,
    notes: &mut VerificationNotes,
) -> Result<Recovery, CliError> {
    let mut recovery = Recovery {
        recovered: Vec::new(),
        missing: Vec::new(),
    };
    let Some(checkpoint) = checkpoint else {
        recovery.missing.push(
            "no authorized checkpoint existed, so no artifact was fetched and this checkout is \
             whatever it already was"
                .to_owned(),
        );
        return Ok(recovery);
    };
    let Some(cwd) = cwd else {
        recovery.missing.push(
            "no --cwd was given, so no checkout was prepared: the artifacts below exist on the \
             relay and nothing was fetched"
                .to_owned(),
        );
        notes.not_verified(
            "no --cwd was given, so the checkpoint's artifacts were not fetched or applied"
                .to_owned(),
        );
        for artifact in &checkpoint.artifacts {
            recovery.missing.push(format!(
                "not fetched: {}",
                super::handover_render::artifact_line(artifact)
            ));
        }
        return Ok(recovery);
    };
    let cwd: PathBuf = cwd.to_path_buf();

    // ── the base, first, whatever order the list is in ───────────────────
    //
    // A patch is a diff **against a commit**. Applying one before its base is
    // checked out — or after that checkout failed — either fails noisily or,
    // worse, three-way merges into whatever the directory happened to hold and
    // calls it recovered. Artifact order in the record is the author's, not a
    // sequence anybody promised, so the base is established here regardless of
    // it, and the patches below run only if it landed.
    let (bases, overlays) = split_artifacts(&checkpoint.artifacts);
    let mut base: BaseState = BaseState::NoWipRef;
    for artifact in bases {
        base = recover_wip_ref(
            client,
            &cwd,
            session8,
            artifact,
            remote,
            allow_no_artifact,
            notes,
            &mut recovery,
        )
        .await?;
        if base == BaseState::CheckedOut {
            break;
        }
    }

    // ── then the bytes that sit on it ────────────────────────────────────
    for artifact in overlays {
        let (label, patch) = match artifact.kind {
            CodingSessionHandoverArtifactKind::WipRef => continue,
            CodingSessionHandoverArtifactKind::Patch => (
                format!(
                    "patch {}",
                    artifact.event_id.as_deref().unwrap_or("<unknown>")
                ),
                fetch_patch_event(client, artifact.event_id.as_deref()).await,
            ),
            CodingSessionHandoverArtifactKind::Blob => (
                format!("blob {}", artifact.hash.as_deref().unwrap_or("<unknown>")),
                // Verified before the caller is handed anything it could
                // apply: a blob is addressed by a hash and a byte count the
                // author stated, and nothing in the fetch proves the served
                // body matches either.
                fetch_verified_blob(client, artifact.hash.as_deref(), artifact.bytes).await,
            ),
        };
        if !base.may_apply_overlays() {
            recovery.missing.push(format!(
                "{label}: not applied — its base commit {} was not checked out (see the wip-ref \
                 line above), and a diff three-way merged onto some other commit is not this \
                 work",
                artifact.base_sha.as_deref().unwrap_or("<unknown>")
            ));
            continue;
        }
        match patch {
            Ok(patch) => match apply_patch(&cwd, &patch) {
                Ok(()) => recovery.recovered.push(format!(
                    "{label} applied against {}",
                    artifact.base_sha.as_deref().unwrap_or("<unknown>")
                )),
                Err(error) => recovery.missing.push(format!("{label}: {error}")),
            },
            Err(error) => recovery.missing.push(format!("{label}: {error}")),
        }
    }
    // The checkpoint's own missing list travels with the continuation: what
    // the author could not preserve is still not preserved, and a reader of
    // the continuation alone must not have to go and find the checkpoint to
    // learn it.
    for line in &checkpoint.missing {
        recovery
            .missing
            .push(format!("still missing, per the checkpoint: {line}"));
    }
    Ok(recovery)
}

/// Split a checkpoint's artifacts into the base to check out and the bytes
/// that sit on it, preserving each group's own order.
///
/// The record's order is the author's and nobody promised it puts the wip ref
/// first, so the reconstruction imposes the only order that means anything: a
/// diff is a diff **against a commit** (REVIEW N9).
pub(super) fn split_artifacts(
    artifacts: &[buzz_core::coding_session_handover::CodingSessionHandoverArtifact],
) -> (
    Vec<&buzz_core::coding_session_handover::CodingSessionHandoverArtifact>,
    Vec<&buzz_core::coding_session_handover::CodingSessionHandoverArtifact>,
) {
    artifacts
        .iter()
        .partition(|artifact| artifact.kind == CodingSessionHandoverArtifactKind::WipRef)
}

/// What happened to the commit a patch expects to sit on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum BaseState {
    /// The checkpoint named no wip ref, so the caller's own checkout is the
    /// base and the patch is applied against it.
    NoWipRef,
    /// The wip ref was fetched and checked out at the checkpoint's sha.
    CheckedOut,
    /// A wip ref was named and could not be placed. Nothing is applied on top.
    CheckoutFailed,
}

impl BaseState {
    /// Whether a patch or blob may be applied on top of this base.
    ///
    /// A named base that did not land is the one answer that is `false`: a
    /// three-way merge onto whatever the directory happened to hold would
    /// report "recovered" over a tree that is not this work.
    pub(super) const fn may_apply_overlays(self) -> bool {
        matches!(self, Self::NoWipRef | Self::CheckedOut)
    }
}

/// Fetch one wip-ref artifact and check it out, or say why it could not be.
#[allow(clippy::too_many_arguments)]
pub(super) async fn recover_wip_ref(
    client: &BuzzClient,
    cwd: &Path,
    session8: &str,
    artifact: &buzz_core::coding_session_handover::CodingSessionHandoverArtifact,
    remote: Option<&str>,
    allow_no_artifact: bool,
    notes: &mut VerificationNotes,
    recovery: &mut Recovery,
) -> Result<BaseState, CliError> {
    let (Some(ref_name), Some(sha)) = (artifact.r#ref.as_deref(), artifact.sha.as_deref()) else {
        recovery
            .missing
            .push("a wip-ref artifact named no ref or sha".to_owned());
        return Ok(BaseState::CheckoutFailed);
    };
    match relay_shows_sha(client, &artifact.repo_ref, sha).await {
        Some(true) => notes.verified(format!(
            "the relay's kind-30618 ref state names {sha} for repository {}",
            artifact.repo_ref
        )),
        Some(false) if !allow_no_artifact => {
            return Err(CliError::NotFound(format!(
                "the relay's kind-30618 ref state for repository {} does not name {sha}, so the \
                 checkpoint's commit is not on the relay and this reconstruction would be built \
                 on a commit nobody can fetch. Pass --allow-no-artifact to proceed and have that \
                 recorded.",
                artifact.repo_ref
            )))
        }
        Some(false) => notes.not_verified(format!(
            "the relay's kind-30618 ref state does not name {sha}; --allow-no-artifact was \
             given, so the fetch was attempted anyway"
        )),
        None => notes.not_verified(format!(
            "the relay's kind-30618 ref state for repository {} could not be read, so whether \
             {sha} is on the relay is unknown",
            artifact.repo_ref
        )),
    }
    let remote = match remote {
        Some(remote) => remote.to_owned(),
        None => match resolve_push_remote(cwd, None) {
            Some(remote) => remote,
            None => {
                recovery.missing.push(format!(
                    "{ref_name} at {sha}: git's configuration names no remote to fetch from; \
                     pass --remote"
                ));
                return Ok(BaseState::CheckoutFailed);
            }
        },
    };
    // `handover/<session8>`, the contract's name (§4). Naming it after the head
    // sha instead gave every checkpoint its own branch, so the containment
    // guard in `fetch_and_checkout` never had an existing branch to protect
    // and two reconstructions of one session left two unrelated branches
    // (composition run 5, finding 3).
    let branch = format!("handover/{session8}");
    match fetch_and_checkout(cwd, &remote, ref_name, sha, &branch) {
        Ok(()) => {
            recovery
                .recovered
                .push(format!("wip-ref {ref_name} at {sha} on branch {branch}"));
            Ok(BaseState::CheckedOut)
        }
        Err(error) => {
            recovery
                .missing
                .push(format!("{ref_name} at {sha}: {error}"));
            Ok(BaseState::CheckoutFailed)
        }
    }
}

/// Whether the relay's kind-30618 ref state names `sha` for this repository.
///
/// `None` means the read failed, which is neither yes nor no and is reported
/// as unknown rather than folded into "not present".
async fn relay_shows_sha(client: &BuzzClient, repo_ref: &str, sha: &str) -> Option<bool> {
    let oids = super::worktree::relay_ref_oids(client, repo_ref)
        .await
        .ok()?;
    Some(oids.contains(&sha.to_ascii_lowercase()))
}

/// Read one NIP-34 patch event's content back off the relay.
async fn fetch_patch_event(
    client: &BuzzClient,
    event_id: Option<&str>,
) -> Result<String, CliError> {
    let event_id =
        event_id.ok_or_else(|| CliError::Other("a patch artifact named no event id".to_owned()))?;
    let events = client
        .query_all(json!({ "kinds": [1617], "ids": [event_id] }))
        .await?;
    events
        .first()
        .and_then(|value| value.get("content").and_then(Value::as_str))
        .map(str::to_owned)
        .ok_or_else(|| {
            CliError::NotFound(format!(
                "patch event {event_id} is not on the relay, so its bytes could not be recovered"
            ))
        })
}

/// Publish the `session.create` that joins this umbrella, and wait for it.
#[allow(clippy::too_many_arguments)]
pub(super) async fn create_execution(
    client: &BuzzClient,
    state: &HandoverState,
    provider_authority: &str,
    provider_instance: Option<&str>,
    projects_file: Option<&Path>,
    recovered_cwd: Option<&Path>,
    checkpoint: Option<&CodingSessionHandoverCheckpoint>,
    checkpoint_ref: Option<&str>,
    author: &str,
    wait_secs: u64,
    notes: &mut VerificationNotes,
) -> Result<CodingSessionTarget, CliError> {
    let provider_instance = provider_instance.ok_or_else(|| {
        CliError::Usage(
            "a reconstruction runs on a provider instance and this command will not choose one: \
             pass --provider-instance <alias>, the same value `bee sessions create \
             --provider-instance` takes"
                .into(),
        )
    })?;
    let instance = ProviderInstanceAlias::from_wire(provider_instance)
        .map_err(|error| CliError::Usage(format!("--provider-instance: {error}")))?;
    let initial_turn = checkpoint
        .map(|body| render_initial_turn(body, checkpoint_ref, author, &state.session_ref));
    let command_id = Uuid::new_v4().to_string();
    let payload = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: checkpoint.and_then(|body| body.revision.repo_ref.clone()),
            session_ref: Some(state.session_ref.clone()),
            genesis_ref: Some(state.genesis_ref.clone()),
            provider_instance_ref: instance,
            provider_authority_pubkey: provider_authority.to_owned(),
            model: None,
            title: None,
            initial_turn,
            actor: None,
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    // The binding, written **before** the create is published. The provider
    // resolves a create's working directory in the order
    // `pending[commandId]` → this hint file → project → channel
    // (`crates/buzz-session-provider/src/commands.rs`,
    // `ProjectsFile::resolve`), re-reading on every lifecycle command, so the
    // hint has to be on disk by the time the command arrives — and a failure
    // to write it has to stop the run before anything is claimed.
    if let (Some(path), Some(cwd)) = (projects_file, recovered_cwd) {
        // A reconstruction's create names no project (`project_ref: None`
        // above), so its hint binds none.
        let binding = bind_pending_directory(path, &command_id, cwd, None)?;
        notes.verified(format!(
            "wrote the pending hint {} binding this create to {}, which is what the provider \
             resolves its working directory from",
            binding.hint_path.display(),
            binding.directory.display()
        ));
        notes.not_verified(BINDING_IS_HOST_LOCAL.to_owned());
    } else if recovered_cwd.is_some() {
        notes.not_verified(
            "no pending hint was written, so where the new execution runs is whatever its \
             provider already had mapped — not necessarily the recovered checkout"
                .to_owned(),
        );
    }

    let channel_uuid = Uuid::parse_str(&state.channel)
        .map_err(|error| CliError::Usage(format!("--channel is not a UUID: {error}")))?;
    let builder =
        build_coding_session_lifecycle_command(channel_uuid, &payload).map_err(sdk_err)?;
    let event = client.sign_event_unchecked(builder)?;
    let since = chrono::Utc::now().timestamp() - 1;
    let raw = client.submit_event(event).await?;
    crate::commands::parse_write_response(&raw, "lifecycle command already accepted")?;
    notes.verified(format!(
        "the relay accepted a session.create joining {} on provider {} as command {command_id}",
        state.session_ref,
        short_pubkey(provider_authority)
    ));
    notes.not_verified(RECONSTRUCTION_LIMIT.to_owned());

    match await_create(client, &state.channel, &command_id, since, wait_secs).await {
        Some((ReceiptStatus::Created, Some(target))) => {
            notes.verified(format!(
                "the provider answered created for command {command_id}"
            ));
            Ok(target)
        }
        Some((status, Some(target))) => {
            notes.not_verified(format!(
                "the provider answered {status:?} rather than created for command {command_id}"
            ));
            Ok(target)
        }
        Some((status, None)) => Err(CliError::Refused(format!(
            "the provider refused the reconstruction's session.create with {status:?}; the claim \
             stands and no continuation was published. Re-run once the provider can accept a \
             create."
        ))),
        None => Err(CliError::Unconfirmed(format!(
            "the relay accepted the reconstruction's session.create as command {command_id}, but \
             no provider receipt arrived within {wait_secs}s. The claim stands and no \
             continuation was published; do not re-run blindly — check \
             `bee sessions status --channel {}` for the execution first.",
            state.channel
        ))),
    }
}

/// Wait, bounded, for the create receipt answering `command_id`.
async fn await_create(
    client: &BuzzClient,
    channel: &str,
    command_id: &str,
    since: i64,
    wait_secs: u64,
) -> Option<(ReceiptStatus, Option<CodingSessionTarget>)> {
    let deadline = std::time::Instant::now() + Duration::from_secs(wait_secs);
    let filter = json!({
        "kinds": [KIND_CODING_SESSION_LIFECYCLE_RECEIPT],
        "#h": [channel],
        "#csl-command": [command_id],
        "since": since,
    });
    loop {
        if let Ok(events) = client.query_all(filter.clone()).await {
            if let Some(answer) = create_answer(&events, command_id) {
                return Some(answer);
            }
        }
        if std::time::Instant::now() + RECEIPT_POLL >= deadline {
            return None;
        }
        tokio::time::sleep(RECEIPT_POLL).await;
    }
}

/// The create outcome a receipt set reports for `command_id`.
pub(super) fn create_answer(
    events: &[Value],
    command_id: &str,
) -> Option<(ReceiptStatus, Option<CodingSessionTarget>)> {
    for value in events {
        let Ok(event) = serde_json::from_value::<Event>(value.clone()) else {
            continue;
        };
        let Ok(receipt) = decode_coding_session_lifecycle_receipt(&event.content) else {
            continue;
        };
        if receipt.command_id != command_id {
            continue;
        }
        match receipt.status {
            ReceiptStatus::Created
            | ReceiptStatus::CreatedWithFailedInitialTurn
            | ReceiptStatus::Failed => return Some((receipt.status, receipt.session)),
            _ => {}
        }
    }
    None
}

// ── binding the create to the recovered checkout ─────────────────────────

/// Where a reconstruction tells the provider its create should run.
///
/// **Not `projects.json`.** That file is *generated*: the desktop
/// rematerializes it from its own canonical store whenever any unrelated hint
/// is saved, so a pending entry written into it is erased at an arbitrary
/// moment — quite possibly between this command publishing the create and the
/// provider admitting it. The entry would be gone exactly when it was needed,
/// and nothing would say so. So the binding goes in a **one-shot hint file**
/// beside it, which nothing regenerates:
///
/// ```text
/// <dir of the projects file>/pending-hints/<createCommandId>.json
/// {"commandId":"…","path":"/absolute/recovered/cwd","writtenAt":1700000000}
/// ```
///
/// The provider resolves `pending[commandId]` → this hint file → project →
/// channel, and consumes the file when it admits or refuses the create.
///
/// # Why this seam exists at all
///
/// A `session.create` names a `sessionRef`, a `repoRef` and a provider — it
/// has no field for a directory, and there is nowhere on the wire to put one.
/// The provider resolves the working directory from its own host-local
/// configuration (`crates/buzz-session-provider/src/commands.rs`,
/// `ProjectsFile::resolve`). So a reconstruction that fetched a wip ref into
/// `--cwd` and then published a create would have the model open the
/// *project's* old folder while the continuation said "recovered": the
/// artifacts in one directory, the agent in another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ProjectsBinding {
    /// The hint file that was written.
    pub(super) hint_path: PathBuf,
    /// The absolute directory bound to the create.
    pub(super) directory: PathBuf,
}

/// The environment variable the provider is launched with, and the one this
/// command defaults `--projects-file` to.
pub(super) const PROJECTS_FILE_ENV: &str = "BUZZ_CSP_PROJECTS_FILE";

/// The directory of one-shot hints, beside the projects file.
pub(super) const PENDING_HINTS_DIR: &str = "pending-hints";

/// Mode for the hints directory: the owner's, and nobody else's.
#[cfg(unix)]
const HINT_DIR_MODE: u32 = 0o700;

/// Mode for a hint file. It names a path on this machine, so it is not
/// world-readable.
#[cfg(unix)]
const HINT_FILE_MODE: u32 = 0o600;

/// The `pending-hints` directory for a given projects file.
pub(super) fn pending_hints_dir(projects_file: &Path) -> PathBuf {
    projects_file
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(PENDING_HINTS_DIR)
}

/// The remedy sentence a caller with no projects file is given.
pub(super) fn projects_file_remedy(cwd: &Path) -> String {
    format!(
        "the provider resolves its working directory from its projects file, not from this \
         command; pass --projects-file <the provider's {PROJECTS_FILE_ENV}> (desktop: \
         <state-dir>/projects.json) or configure the project's directory to {}. The flag is used \
         to locate the directory: the binding is written as a one-shot hint at \
         <dir of the projects file>/{PENDING_HINTS_DIR}/<createCommandId>.json, and the projects \
         file itself is never modified — it is generated, and anything written into it is erased \
         the next time the desktop rematerializes it",
        cwd.display()
    )
}

/// Resolve the projects file to bind beside, or refuse with the remedy.
///
/// Refused **before** anything is claimed: a claim moves the fence for
/// everybody, and taking a session over only to discover the new execution
/// cannot be pointed at the recovered work is a worse place to stop than not
/// starting.
pub(super) fn resolve_projects_file(
    explicit: Option<&Path>,
    cwd: &Path,
) -> Result<PathBuf, CliError> {
    if let Some(path) = explicit {
        return Ok(path.to_path_buf());
    }
    match std::env::var(PROJECTS_FILE_ENV) {
        Ok(value) if !value.trim().is_empty() => Ok(PathBuf::from(value)),
        _ => Err(CliError::Usage(projects_file_remedy(cwd))),
    }
}

/// Check a directory a caller names for a create (`bee sessions create
/// --cwd`) and resolve the projects file its hint goes beside, refusing with
/// the remedy before anything is written or published.
///
/// The provider ignores a hint whose path is not an existing absolute
/// directory and falls through to the project or channel default, so a
/// directory that would be ignored is refused here instead of silently
/// running the session somewhere else.
pub(super) fn resolve_create_directory(
    cwd: &Path,
    explicit_projects_file: Option<&Path>,
) -> Result<PathBuf, CliError> {
    if !cwd.is_absolute() {
        return Err(CliError::Usage(format!(
            "--cwd {} is not an absolute path, and the provider ignores a relative working \
             directory; pass the absolute path",
            cwd.display()
        )));
    }
    if !cwd.is_dir() {
        return Err(CliError::Usage(format!(
            "--cwd {} is not an existing directory, and the provider ignores a hint naming one; \
             create it first",
            cwd.display()
        )));
    }
    resolve_projects_file(explicit_projects_file, cwd)
}

/// Write the one-shot hint binding `command_id` to `directory`.
///
/// Atomic at the rename, because the provider may read this directory at any
/// moment and a half-written hint is one it would refuse to parse. The
/// temporary file is created in the same directory so the rename cannot cross
/// a filesystem, and it carries its mode before the rename so the file is
/// never briefly world-readable under its real name.
///
/// The projects file itself is **neither read nor written**.
///
/// `project_ref` is the NIP-MP coordinate the create names, when it names one.
/// It is written as the hint's `projectRef`, which is the only thing that lets
/// the provider treat a directory outside the project's recorded checkout as
/// that project's workspace (`ProjectsFile::hint_binds_project` in
/// `crates/buzz-session-provider/src/commands.rs`). A projected create whose
/// hint omits it is refused `EXECUTION_SCOPE_INVALID`. `None` writes no key,
/// so a projectless hint stays byte-for-byte the three-key body.
///
/// # Errors
/// A directory that cannot be created, a `commandId` that is not safe as a
/// file name, or a working directory that does not resolve to an absolute
/// path — each before the create is published, so a failure here never leaves
/// a claimed session with an unbound execution.
pub(super) fn bind_pending_directory(
    projects_file: &Path,
    command_id: &str,
    directory: &Path,
    project_ref: Option<&str>,
) -> Result<ProjectsBinding, CliError> {
    // The command id becomes a path component. It is a UUID this process
    // minted, so this can only fail if that ever changes — which is exactly
    // when a path traversal would otherwise become possible.
    if command_id.is_empty()
        || !command_id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-')
    {
        return Err(CliError::Other(format!(
            "refusing to write a pending hint for command id {command_id:?}: it is used as a file \
             name and must be alphanumeric"
        )));
    }
    let directory = directory.canonicalize().map_err(|error| {
        CliError::Usage(format!(
            "the working directory {} cannot be resolved to an absolute path ({error}), and the \
             provider ignores a relative working directory",
            directory.display()
        ))
    })?;

    let hints = pending_hints_dir(projects_file);
    let existed = hints.is_dir();
    std::fs::create_dir_all(&hints).map_err(|error| {
        CliError::Usage(format!(
            "cannot create the pending-hints directory {} ({error}), so the working directory \
             could not be bound to this create and nothing was claimed or published",
            hints.display()
        ))
    })?;
    #[cfg(unix)]
    if !existed {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&hints, std::fs::Permissions::from_mode(HINT_DIR_MODE)).map_err(
            |error| {
                CliError::Usage(format!(
                    "cannot set the mode of {}: {error}",
                    hints.display()
                ))
            },
        )?;
    }
    #[cfg(not(unix))]
    let _ = existed;

    let mut hint = json!({
        "commandId": command_id,
        "path": directory.to_string_lossy(),
        "writtenAt": chrono::Utc::now().timestamp(),
    });
    if let (Some(object), Some(project_ref)) = (hint.as_object_mut(), project_ref) {
        object.insert("projectRef".into(), json!(project_ref));
    }
    let body = serde_json::to_string(&hint)
        .map_err(|error| CliError::Other(format!("pending hint serialization failed: {error}")))?;

    let hint_path = hints.join(format!("{command_id}.json"));
    let temporary = hints.join(format!(".{command_id}.{}.tmp", uuid::Uuid::new_v4()));
    std::fs::write(&temporary, &body).map_err(|error| {
        CliError::Usage(format!("cannot write {}: {error}", temporary.display()))
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Err(error) =
            std::fs::set_permissions(&temporary, std::fs::Permissions::from_mode(HINT_FILE_MODE))
        {
            let _ = std::fs::remove_file(&temporary);
            return Err(CliError::Usage(format!(
                "cannot set the mode of {}: {error}",
                temporary.display()
            )));
        }
    }
    // Rename replaces any hint already written for this command id: a rerun
    // that recovered into a different checkout must bind the new one, and two
    // hints for one create would be a question nobody can answer.
    std::fs::rename(&temporary, &hint_path).map_err(|error| {
        let _ = std::fs::remove_file(&temporary);
        CliError::Usage(format!(
            "cannot place the pending hint at {}: {error}",
            hint_path.display()
        ))
    })?;

    Ok(ProjectsBinding {
        hint_path,
        directory,
    })
}

/// The sentence every reconstruction prints about where the binding lives.
pub(super) const BINDING_IS_HOST_LOCAL: &str =
    "the working directory is bound by a one-shot hint file on this machine, never on the wire: a \
     create names a session and a provider, and has no field for a directory. The provider \
     consumes the hint when it admits or refuses the create.";

/// Check that the new execution is actually running in the recovered checkout.
///
/// The create's receipt says an execution exists; it does not say **where**.
/// The provider's first 44223 metadata carries the worktree it probed —
/// `branch` and `observedCommit` — and those are the only evidence this
/// command can offer that the binding took. The patch is applied uncommitted,
/// so `HEAD` is still the checkpoint's `headSha`.
///
/// Returns the `missing` line to record, or `None` when the execution is where
/// it should be.
pub(super) fn workdir_mismatch_line(
    branch: Option<&str>,
    observed_commit: Option<&str>,
    expected_branch: &str,
    expected_head: Option<&str>,
) -> Option<String> {
    let branch_matches = branch == Some(expected_branch);
    let head_matches = match (observed_commit, expected_head) {
        // No head to compare against is not a mismatch; it is one fewer fact.
        (_, None) => true,
        (Some(observed), Some(expected)) => observed.eq_ignore_ascii_case(expected),
        (None, Some(_)) => false,
    };
    if branch_matches && head_matches {
        return None;
    }
    Some(format!(
        "the execution reports branch {} at {}, not the recovered checkout — it is running \
         somewhere else",
        branch.unwrap_or("<none>"),
        observed_commit.unwrap_or("<none>")
    ))
}

/// Wait, bounded, for the new execution's first metadata and check where it is.
pub(super) async fn verify_execution_workdir(
    client: &BuzzClient,
    channel: &str,
    target: &CodingSessionTarget,
    expected_branch: &str,
    expected_head: Option<&str>,
    since: i64,
    wait_secs: u64,
) -> Option<String> {
    let target_key = coding_session_target_key(target);
    let deadline = std::time::Instant::now() + Duration::from_secs(wait_secs);
    let filter = json!({
        "kinds": [buzz_core::kind::KIND_CODING_SESSION_METADATA],
        "#h": [channel],
        "since": since,
    });
    loop {
        if let Ok(events) = client.query_all(filter.clone()).await {
            let (records, _) = super::decode_metadata(&events);
            if let Some(record) = records
                .iter()
                .filter(|record| record.target_key == target_key)
                .min_by_key(|record| record.created_at)
            {
                return workdir_mismatch_line(
                    record.metadata.branch.as_deref(),
                    record.metadata.observed_commit.as_deref(),
                    expected_branch,
                    expected_head,
                );
            }
        }
        if std::time::Instant::now() + RECEIPT_POLL >= deadline {
            return Some(format!(
                "the new execution published no metadata within {wait_secs}s, so this run cannot \
                 say whether it is running in the recovered checkout"
            ));
        }
        tokio::time::sleep(RECEIPT_POLL).await;
    }
}
