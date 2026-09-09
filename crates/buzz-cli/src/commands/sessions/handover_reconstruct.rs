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

use buzz_core::coding_session_command::CodingSessionTarget;
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
                fetch_blob(client, artifact.hash.as_deref()).await,
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

/// Read one Blossom blob back off the relay as patch text.
async fn fetch_blob(client: &BuzzClient, hash: Option<&str>) -> Result<String, CliError> {
    let hash = hash.ok_or_else(|| CliError::Other("a blob artifact named no hash".to_owned()))?;
    let bytes = client.download_media(hash).await?;
    String::from_utf8(bytes.to_vec()).map_err(|error| {
        CliError::Other(format!(
            "blob {hash} is not valid UTF-8 patch text: {error}"
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
