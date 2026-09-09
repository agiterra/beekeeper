//! `bee sessions handover checkpoint` — say what the work is, durably.
//!
//! Four preservation steps, in order, each of which can fail on its own
//! without stopping the others, and every failure ends up under the
//! checkpoint's `missing` rather than in an exception:
//!
//! 1. **Read the revision.** Head, branch, merge base, dirty.
//! 2. **Push `HEAD`** to `refs/heads/wip/<role-or-owner>/<session8>` with the
//!    caller's own key, never forced. A failed push is a `missing` line and no
//!    `wip-ref` artifact — the checkpoint never claims a ref it did not write.
//! 3. **Capture the working tree**, complete or enumerated
//!    ([`super::handover_git::capture_working_tree`]).
//! 4. **Carry the patch**: a NIP-34 patch event when it fits under the relay's
//!    own advertised message limit, a Blossom blob when it does not.
//!
//! # `preserved` is stated, never inferred
//!
//! `dirty: true` plus a patch does **not** mean everything survived. This
//! command sets `revision.preserved` from what actually happened — `all` only
//! when nothing was omitted and the patch was carried, `partial` when some
//! bytes travelled and some did not, `none` when none did — and kind 44247
//! refuses a record that says anything but `all` with an empty `missing`
//! list, so the two can never disagree.
//!
//! One cap sits on top of that arithmetic: a worktree holding **ignored**
//! files never reports `all`. `git add -A` honours `.gitignore` and
//! `git status --porcelain` does not list ignored files, so a checkout whose
//! only uncommitted change is a modified `.env` reads as clean, captures
//! nothing, and would otherwise sign "every byte travelled" over the one file
//! the next participant needs. Those paths are named under `missing` and
//! `preserved` is capped at `partial` (REVIEW S3).

use std::path::{Path, PathBuf};

use buzz_core::coding_session_handover::{
    CodingSessionHandoverArtifact, CodingSessionHandoverArtifactKind, CodingSessionHandoverBody,
    CodingSessionHandoverCheckpoint, CodingSessionHandoverDecision, CodingSessionHandoverPreserved,
    CodingSessionHandoverRevision, CodingSessionHandoverTest, CodingSessionHandoverTestOutcome,
    MAX_HANDOVER_MISSING,
};
use buzz_core::seat_git_hooks::wip_ref_name;
use buzz_sdk::{GitPatchMeta, GitRepoCoord};
use serde_json::{json, Value};

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{sdk_err, validate_lower_hex64};
use crate::HandoverCheckpointArgs;

use super::handover::{
    build_handover_event, classify_own_write, load_handover_state, OwnWriteOutcome,
};
use super::handover_blob::PATCH_BLOB_MIME;
use super::handover_claim::refuse_retired;
use super::handover_git::{
    capture_working_tree, push_wip_ref, read_revision, resolve_push_remote, CapturedTree,
    OmittedPath, MAX_CAPTURE_FILE_BYTES, MAX_CAPTURE_PATCH_BYTES,
};
use super::handover_render::{artifact_line, VerificationNotes, WHOLE_SESSION_DISCLOSURE};

/// The wip-ref segment used when the caller holds no seat role.
///
/// A literal word rather than a pubkey because a checkpoint's ref is scoped by
/// the session already: `wip/owner/<session8>` is unique to one umbrella, and
/// only somebody with standing on that umbrella writes one.
const OWNER_SEGMENT: &str = "owner";

/// Bytes reserved for the event envelope when deciding patch event vs blob.
///
/// The relay advertises a whole-message ceiling; the patch is only part of the
/// message. Reserving a margin means the decision is wrong in the safe
/// direction — a blob when an event would just have fitted — rather than
/// publishing an event the relay then refuses.
const EVENT_ENVELOPE_MARGIN: usize = 8 * 1024;

/// Fallback message ceiling when the relay's NIP-11 does not advertise one.
const DEFAULT_RELAY_MESSAGE_LIMIT: usize = 262_144;

/// `bee sessions handover checkpoint`.
pub(super) async fn cmd_checkpoint(
    client: &BuzzClient,
    args: HandoverCheckpointArgs,
) -> Result<(), CliError> {
    let state = load_handover_state(
        client,
        &args.channel,
        &args.session_ref,
        args.genesis.as_deref(),
    )
    .await?;
    refuse_retired(&state)?;

    let cwd = resolve_cwd(args.cwd.as_deref())?;
    let mut notes = VerificationNotes::default();
    if let Some(reason) = &state.genesis_unavailable {
        // Disclosed rather than refused: a checkpoint is a record about the
        // author's own work, it binds nothing, and refusing to write one
        // because a relay read came back empty would lose the very statement
        // somebody else needs. What it must not do is imply the coordinates
        // were checked.
        notes.not_verified(format!(
            "{reason}, so this checkpoint's sessionRef/genesisRef pair was not confirmed against \
             a genesis on the relay"
        ));
    }
    let revision = read_revision(&cwd)?;
    notes.verified(format!(
        "the worktree at {} is at {} on branch {} and is {}",
        cwd.display(),
        revision.head_sha,
        revision.branch.as_deref().unwrap_or("(detached HEAD)"),
        if revision.dirty { "dirty" } else { "clean" }
    ));
    if revision.base_sha.is_none() {
        notes.not_verified(
            "no merge base resolved against main/master, so this checkpoint names no baseSha"
                .to_owned(),
        );
    }

    let repo_ref = args
        .repo
        .clone()
        .or_else(|| super::worktree::repo_id_of(&cwd));
    let mut missing: Vec<String> = Vec::new();
    let mut artifacts: Vec<CodingSessionHandoverArtifact> = Vec::new();

    // ── 2. the wip ref ───────────────────────────────────────────────────
    let session8: String = state.session_ref.chars().take(8).collect();
    let role = args
        .role
        .clone()
        .unwrap_or_else(|| OWNER_SEGMENT.to_owned());
    let ref_name = wip_ref_name(&role, &session8).map_err(CliError::Usage)?;
    if args.no_push {
        notes.not_verified(format!(
            "--no-push was given, so {ref_name} was not written and this checkpoint carries no \
             wip-ref artifact: the commits at {} exist only on this machine",
            revision.head_sha
        ));
        missing.push(format!(
            "commits at {}: --no-push was given, so nothing was pushed to {ref_name}",
            revision.head_sha
        ));
    } else {
        match push_head(
            &cwd,
            args.remote.as_deref(),
            &ref_name,
            revision.branch.as_deref(),
        ) {
            Ok(remote) => {
                notes.verified(format!(
                    "pushed {} to {ref_name} on remote '{remote}' without --force",
                    revision.head_sha
                ));
                match &repo_ref {
                    Some(repo_ref) => artifacts.push(CodingSessionHandoverArtifact {
                        kind: CodingSessionHandoverArtifactKind::WipRef,
                        repo_ref: repo_ref.clone(),
                        r#ref: Some(ref_name.clone()),
                        sha: Some(revision.head_sha.clone()),
                        event_id: None,
                        hash: None,
                        base_sha: None,
                        bytes: None,
                    }),
                    None => missing.push(format!(
                        "{ref_name} at {}: pushed, but no repository coordinate resolved, so it \
                         could not be recorded as an artifact (pass --repo)",
                        revision.head_sha
                    )),
                }
            }
            Err(error) => {
                notes.not_verified(format!("the wip ref was not written: {error}"));
                missing.push(format!(
                    "commits at {}: the push to {ref_name} failed ({error}), so they exist only \
                     on this machine",
                    revision.head_sha
                ));
            }
        }
    }

    // ── 3. the working tree ──────────────────────────────────────────────
    let captured = capture_working_tree(
        &cwd,
        &revision.head_sha,
        MAX_CAPTURE_FILE_BYTES,
        MAX_CAPTURE_PATCH_BYTES,
    )?;
    notes.verified(format!(
        "captured {} changed path(s) against {} with a temporary index, so staged, unstaged, \
         untracked and binary content are all in one patch",
        captured.changed_paths.len(),
        revision.head_sha
    ));
    missing.extend(captured.omitted.iter().map(OmittedPath::line));
    if captured.ignored.any() {
        // Always disclosed, never inferred from `dirty`. `git status` does not
        // list ignored files, so without this line a worktree holding a
        // modified `.env` reads as clean and the checkpoint would sign
        // "preserved: all" over bytes that never left the machine.
        notes.not_verified(format!(
            "{} path(s) are ignored by .gitignore and were not captured: nothing here carries \
             them, and this checkpoint says so rather than claiming every byte travelled",
            captured.ignored.total
        ));
        missing.push(captured.ignored.line());
    }

    // ── 4. carry the patch ───────────────────────────────────────────────
    let mut carried_patch = false;
    if captured.has_patch() {
        match &repo_ref {
            None => missing.push(
                "the captured working-tree patch: no repository coordinate resolved, so it could \
                 not be published as an artifact (pass --repo)"
                    .to_owned(),
            ),
            Some(repo_ref) => {
                match carry_patch(
                    client,
                    &captured,
                    repo_ref,
                    args.repo_owner.as_deref(),
                    &revision.head_sha,
                )
                .await
                {
                    Ok((artifact, note)) => {
                        notes.verified(note);
                        artifacts.push(artifact);
                        carried_patch = true;
                    }
                    Err(error) => {
                        notes.not_verified(format!(
                            "the working-tree patch was not carried: {error}"
                        ));
                        missing.push(format!(
                            "the captured working-tree patch ({} bytes): {error}",
                            captured.patch.len()
                        ));
                    }
                }
            }
        }
    }

    let preserved = decide_preserved(revision.dirty, &captured, carried_patch);
    if preserved != CodingSessionHandoverPreserved::All && missing.is_empty() {
        // Cannot happen through the branches above, and if a future edit makes
        // it possible the record would be a lie rather than an error. Say what
        // is unknown instead.
        missing.push(
            "some uncommitted bytes were not preserved and this run could not enumerate which"
                .to_owned(),
        );
    }
    let bounded_missing = bound_missing(missing);

    // The author's own previous checkpoint of this umbrella, which this one
    // replaces. Within one author, because that is what the field means: a
    // link naming somebody else's record would retire a statement this author
    // has no standing to retire.
    let caller = client.keys().public_key().to_hex();
    let prev_checkpoint_ref = state
        .latest_checkpoint_by(&caller)
        .map(|entry| entry.event_id.clone());
    match &prev_checkpoint_ref {
        Some(previous) => notes.verified(format!(
            "this checkpoint replaces {previous}, this author's previous one for this umbrella, \
             so a reader inside one second does not have to order two records by their hashes"
        )),
        None => notes.verified(
            "this is this author's first checkpoint of this umbrella, so it replaces nothing"
                .to_owned(),
        ),
    }

    let body = CodingSessionHandoverCheckpoint {
        prev_checkpoint_ref,
        task: args
            .task
            .clone()
            .unwrap_or_else(|| "not stated by this checkpoint's author".to_owned()),
        assignment_refs: validated_ids("--assignment", &args.assignment)?,
        decisions: parse_decisions(&args.decision)?,
        revision: CodingSessionHandoverRevision {
            repo_ref: repo_ref.clone(),
            base_sha: revision.base_sha.clone(),
            head_sha: Some(revision.head_sha.clone()),
            branch: revision.branch.clone(),
            dirty: revision.dirty,
            preserved,
        },
        artifacts,
        tests: parse_tests(&args.test)?,
        unresolved: args.unresolved.clone(),
        next_action: args
            .next
            .clone()
            .unwrap_or_else(|| "not stated by this checkpoint's author".to_owned()),
        missing: bounded_missing,
    };

    let event = build_handover_event(
        client,
        &state.channel,
        &state.session_ref,
        &state.genesis_ref,
        CodingSessionHandoverBody::Checkpoint(body.clone()),
    )?;
    let event_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    match classify_own_write(&raw)? {
        OwnWriteOutcome::Published => notes.verified(format!("published checkpoint {event_id}")),
        // Two identical checkpoints in one second are one checkpoint. Saying
        // "already on the relay" is the truth; exiting non-zero over it would
        // report a durable record as a failed write.
        OwnWriteOutcome::AlreadyPresent => notes.verified(format!(
            "checkpoint {event_id} was already on the relay under this exact id — the same bytes \
             were published before, so this run added nothing and lost nothing"
        )),
    }
    notes.not_verified(
        "no provider was contacted: publishing a checkpoint fences nothing and steers nothing. \
         Run `bee sessions handover claim` to take the session over."
            .to_owned(),
    );

    report(&event_id, &body, &notes, args.json);
    Ok(())
}

/// Resolve `--cwd`, defaulting to the process's own directory.
fn resolve_cwd(cwd: Option<&Path>) -> Result<PathBuf, CliError> {
    match cwd {
        Some(path) => Ok(path.to_path_buf()),
        None => std::env::current_dir().map_err(|error| {
            CliError::Usage(format!(
                "no --cwd was given and the current directory could not be read: {error}"
            ))
        }),
    }
}

/// Push `HEAD` to `ref_name`, resolving the remote from git's own config.
fn push_head(
    cwd: &Path,
    remote: Option<&str>,
    ref_name: &str,
    branch: Option<&str>,
) -> Result<String, String> {
    let remote = match remote {
        Some(remote) => remote.to_owned(),
        None => resolve_push_remote(cwd, branch).ok_or_else(|| {
            "git's push configuration names no remote for this branch, and there is more than \
             one (or no) remote to fall back to; pass --remote"
                .to_owned()
        })?,
    };
    push_wip_ref(cwd, &remote, ref_name)?;
    Ok(remote)
}

/// Publish the captured patch as a NIP-34 patch event, or as a Blossom blob.
pub(super) async fn carry_patch(
    client: &BuzzClient,
    captured: &CapturedTree,
    repo_ref: &str,
    repo_owner: Option<&str>,
    base_sha: &str,
) -> Result<(CodingSessionHandoverArtifact, String), String> {
    let bytes = captured.patch.len();
    let limit = relay_patch_limit(client).await;
    if bytes <= limit {
        let owner = match repo_owner {
            Some(owner) => owner.to_ascii_lowercase(),
            None => client.keys().public_key().to_hex(),
        };
        let event_id = publish_patch_event(client, captured, repo_ref, &owner, base_sha)
            .await
            .map_err(|error| error.to_string())?;
        return Ok((
            CodingSessionHandoverArtifact {
                kind: CodingSessionHandoverArtifactKind::Patch,
                repo_ref: repo_ref.to_owned(),
                r#ref: None,
                sha: None,
                event_id: Some(event_id.clone()),
                hash: None,
                base_sha: Some(base_sha.to_owned()),
                bytes: Some(bytes as u64),
            },
            format!(
                "published the {bytes}-byte working-tree patch as NIP-34 patch event {event_id} \
                 against {base_sha}, under repository coordinate 30617:{owner}:{repo_ref}"
            ),
        ));
    }
    let descriptor = client
        .upload_blob_bytes(captured.patch.clone().into_bytes(), PATCH_BLOB_MIME)
        .await
        .map_err(|error| {
            format!("it exceeds the relay's {limit}-byte event budget and the blob upload failed: {error}")
        })?;
    Ok((
        CodingSessionHandoverArtifact {
            kind: CodingSessionHandoverArtifactKind::Blob,
            repo_ref: repo_ref.to_owned(),
            r#ref: None,
            sha: None,
            event_id: None,
            hash: Some(descriptor.sha256.clone()),
            base_sha: Some(base_sha.to_owned()),
            bytes: Some(bytes as u64),
        },
        format!(
            "the {bytes}-byte patch exceeds the relay's {limit}-byte event budget, so it was \
             uploaded as Blossom blob {} against {base_sha}",
            descriptor.sha256
        ),
    ))
}

/// Build and publish one NIP-34 patch event carrying the capture.
async fn publish_patch_event(
    client: &BuzzClient,
    captured: &CapturedTree,
    repo_ref: &str,
    owner: &str,
    base_sha: &str,
) -> Result<String, CliError> {
    validate_lower_hex64("--repo-owner", owner)?;
    let repo = GitRepoCoord {
        owner: owner.to_owned(),
        id: repo_ref.to_owned(),
    };
    let meta = GitPatchMeta {
        euc: None,
        recipients: Vec::new(),
        reply_to: None,
        // A capture is a standalone patch, not a revision of an earlier one.
        root: true,
        root_revision: false,
        // The capture is *uncommitted* work, so it produces no commit to name;
        // what it does name is the commit it applies to.
        commit: None,
        parent_commit: Some(base_sha.to_owned()),
        commit_pgp_sig: None,
        committer: None,
    };
    let builder = crate::commands::with_git_provenance(
        buzz_sdk::build_git_patch(&repo, &captured.patch, &meta).map_err(sdk_err)?,
    )?;
    let event = client.sign_event(builder)?;
    let event_id = event.id.to_hex();
    let raw = client.submit_event(event).await?;
    // A repeat checkpoint of an unchanged tree in the same second signs the
    // same bytes and therefore the same id. The relay says "duplicate", which
    // is the relay confirming the artifact is there — so the checkpoint keeps
    // it rather than reporting the patch as lost.
    classify_own_write(&raw)?;
    Ok(event_id)
}

/// The largest patch this relay will take inside one event.
///
/// Read from the relay's own NIP-11 `limitation.max_message_length` rather
/// than assumed, minus an envelope margin. An unreadable document falls back
/// to the documented default, which is a stated guess and not a silent one:
/// being wrong here costs a blob upload, never a lost patch.
async fn relay_patch_limit(client: &BuzzClient) -> usize {
    let advertised = client
        .get_public("/")
        .await
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
        .and_then(|info| {
            info.get("limitation")
                .and_then(|limits| limits.get("max_message_length"))
                .and_then(Value::as_u64)
        })
        .and_then(|limit| usize::try_from(limit).ok())
        .unwrap_or(DEFAULT_RELAY_MESSAGE_LIMIT);
    advertised.saturating_sub(EVENT_ENVELOPE_MARGIN)
}

/// Decide `revision.preserved` from what the run actually achieved.
///
/// Kept as a pure function so the honesty rule is testable without a relay:
/// a clean tree is `all`, a dirty tree whose whole capture travelled is `all`,
/// anything omitted or uncarried is `partial`, and a dirty tree that carried
/// nothing at all is `none`.
pub(super) fn decide_preserved(
    dirty: bool,
    captured: &CapturedTree,
    carried_patch: bool,
) -> CodingSessionHandoverPreserved {
    let computed = if !dirty {
        CodingSessionHandoverPreserved::All
    } else if !carried_patch {
        CodingSessionHandoverPreserved::None
    } else if captured.omitted.is_empty() {
        CodingSessionHandoverPreserved::All
    } else {
        CodingSessionHandoverPreserved::Partial
    };
    // The cap. `preserved` describes uncommitted bytes, and git does not count
    // an ignored file as uncommitted work — so the computed answer above is
    // right on its own terms and is left alone in every other respect. What it
    // must not do is read as **"all"** while a modified `.env` sits in the
    // worktree that no artifact carries: "all" is the one value a reader acts
    // on without checking `missing`, so a capture that saw ignored paths never
    // claims it (REVIEW S3).
    if captured.ignored.any() && computed == CodingSessionHandoverPreserved::All {
        return CodingSessionHandoverPreserved::Partial;
    }
    computed
}

/// Keep `missing` inside the record's own bound without dropping the fact
/// that there was more.
///
/// The last line says how many were not listed, so a truncated enumeration is
/// still an enumeration rather than a shorter list that reads complete.
pub(super) fn bound_missing(missing: Vec<String>) -> Vec<String> {
    if missing.len() <= MAX_HANDOVER_MISSING {
        return missing;
    }
    let mut bounded: Vec<String> = missing
        .iter()
        .take(MAX_HANDOVER_MISSING - 1)
        .cloned()
        .collect();
    bounded.push(format!(
        "and {} more path(s) not preserved, beyond this record's {MAX_HANDOVER_MISSING}-line bound",
        missing.len() - (MAX_HANDOVER_MISSING - 1)
    ));
    bounded
}

/// Validate every repeated 64-hex argument.
fn validated_ids(label: &str, values: &[String]) -> Result<Vec<String>, CliError> {
    values
        .iter()
        .map(|value| {
            let value = value.to_ascii_lowercase();
            validate_lower_hex64(label, &value)?;
            Ok(value)
        })
        .collect()
}

/// Parse `--decision <eventId>[:<summary>]`.
///
/// A bare id is accepted and its summary says, in words, that the author did
/// not state one. Inventing a summary from the id would put a sentence in the
/// author's mouth, and this record is the author's statement.
pub(super) fn parse_decisions(
    values: &[String],
) -> Result<Vec<CodingSessionHandoverDecision>, CliError> {
    values
        .iter()
        .map(|value| {
            let (event_id, summary) = match value.split_once(':') {
                Some((id, summary)) if !summary.trim().is_empty() => {
                    (id.to_ascii_lowercase(), summary.trim().to_owned())
                }
                _ => (
                    value.trim_end_matches(':').to_ascii_lowercase(),
                    "no summary was stated by this checkpoint's author".to_owned(),
                ),
            };
            validate_lower_hex64("--decision", &event_id)?;
            Ok(CodingSessionHandoverDecision { event_id, summary })
        })
        .collect()
}

/// Parse `--test 'name:outcome:command'`, split on the first two colons so a
/// command may contain colons of its own — the shape `--gate` already uses.
pub(super) fn parse_tests(values: &[String]) -> Result<Vec<CodingSessionHandoverTest>, CliError> {
    values
        .iter()
        .map(|value| {
            let (name, rest) = value.split_once(':').ok_or_else(|| {
                CliError::Usage(format!(
                    "--test must be 'name:outcome:command'; got {value:?}"
                ))
            })?;
            let (outcome, command) = rest.split_once(':').ok_or_else(|| {
                CliError::Usage(format!(
                    "--test must be 'name:outcome:command'; got {value:?}"
                ))
            })?;
            let outcome = match outcome {
                "passed" => CodingSessionHandoverTestOutcome::Passed,
                "failed" => CodingSessionHandoverTestOutcome::Failed,
                "not-run" => CodingSessionHandoverTestOutcome::NotRun,
                other => {
                    return Err(CliError::Usage(format!(
                        "--test outcome must be passed, failed or not-run; got {other:?}"
                    )))
                }
            };
            if name.trim().is_empty() || command.trim().is_empty() {
                return Err(CliError::Usage(format!(
                    "--test needs a name and a command: {value:?}"
                )));
            }
            Ok(CodingSessionHandoverTest {
                name: name.to_owned(),
                command: command.to_owned(),
                outcome,
            })
        })
        .collect()
}

/// Print the event id, every artifact and every missing line.
fn report(
    event_id: &str,
    body: &CodingSessionHandoverCheckpoint,
    notes: &VerificationNotes,
    as_json: bool,
) {
    if as_json {
        println!(
            "{}",
            json!({
                "eventId": event_id,
                "accepted": true,
                "type": "checkpoint",
                "checkpoint": body,
                "scope": WHOLE_SESSION_DISCLOSURE,
                "notes": notes.to_json(),
            })
        );
        return;
    }
    println!("checkpoint {event_id}");
    println!("scope: {WHOLE_SESSION_DISCLOSURE}");
    println!(
        "revision: head {} branch {} base {} dirty {} preserved {}",
        body.revision.head_sha.as_deref().unwrap_or("null"),
        body.revision.branch.as_deref().unwrap_or("null"),
        body.revision.base_sha.as_deref().unwrap_or("null"),
        body.revision.dirty,
        super::handover_render::preserved_word(body.revision.preserved),
    );
    for artifact in &body.artifacts {
        println!("artifact: {}", artifact_line(artifact));
    }
    if body.artifacts.is_empty() {
        println!("artifact: none");
    }
    for line in &body.missing {
        println!("missing: {line}");
    }
    if body.missing.is_empty() {
        println!("missing: nothing");
    }
    print!("{}", notes.render());
}
