//! `bee preview snapshot --share` — publish the snapshot the seat just took
//! as a kind 44253 the whole session sees (WIRE-C5 § 6; LIVE_PREVIEW
//! decision 10: the seat signs).
//!
//! Order: everything that can refuse without side effects first (grant,
//! announce, page), then the upload, then the signed event. The uploaded
//! blob is the broker's PNG with every ancillary chunk the media validator
//! refuses removed (the CLI has no image codec; pixels are not re-encoded),
//! so its sha256 can differ from the local file's.

use std::path::Path;

use beekeeper_core::coding_session_command::coding_session_target_key;
use beekeeper_core::kind::KIND_SESSION_PREVIEW_ANNOUNCE;
use beekeeper_core::preview_grant::decode_preview_grant_unverified;
use beekeeper_core::session_preview::{
    redact_page_url, resolve_preview_owner, validate_session_preview_announce_envelope,
    MAX_PREVIEW_TITLE_BYTES,
};
use beekeeper_core::surface_snapshot::{
    snapshot_evidence_token, SnapshotCommit, SurfaceSnapshot, SurfaceSnapshotType,
    MAX_SURFACE_SNAPSHOT_ALT_BYTES,
};
use beekeeper_core::surface_watch::{redact_free_text, Surface};
use nostr::{Event, Keys, PublicKey};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{PreviewContext, PreviewFailure};
use crate::client::BeekeeperClient;

/// The sentence when no Browser of this session is shared.
pub const NOT_ANNOUNCED_SENTENCE: &str =
    "The Browser is not shared for this session: open it in the Beekeeper app with Share on.";

/// Why no sessionRef could be picked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PickError {
    /// No open announce in the channel.
    None,
    /// Several open, none opened by this generation.
    Ambiguous(Vec<String>),
}

/// Pick the shared preview of this session: the open one whose announce
/// names `target_key`, else the single open one. Returns the sessionRef and
/// its owner.
pub fn pick_session_ref(
    events: &[Event],
    channel: Uuid,
    target_key: Option<&str>,
) -> Result<(String, PublicKey), PickError> {
    let verified: Vec<Event> = events
        .iter()
        .filter(|event| event.verify().is_ok())
        .cloned()
        .collect();
    let mut refs: Vec<String> = verified
        .iter()
        .filter_map(|event| validate_session_preview_announce_envelope(event).ok())
        .filter(|announce| announce.channel_id == channel)
        .map(|announce| announce.session_ref)
        .collect();
    refs.sort();
    refs.dedup();
    let mut open = Vec::new();
    for session_ref in refs {
        let Some(owner) = resolve_preview_owner(&verified, channel, &session_ref) else {
            continue;
        };
        let owner_target = verified
            .iter()
            .filter(|event| event.pubkey == owner)
            .filter_map(|event| {
                validate_session_preview_announce_envelope(event)
                    .ok()
                    .map(|announce| (event, announce))
            })
            .filter(|(_, announce)| announce.session_ref == session_ref)
            .max_by_key(|(event, _)| (event.created_at, std::cmp::Reverse(event.id)))
            .and_then(|(_, announce)| announce.target_key);
        open.push((session_ref, owner, owner_target));
    }
    if let Some(target) = target_key {
        let mine: Vec<&(String, PublicKey, Option<String>)> = open
            .iter()
            .filter(|(_, _, owner_target)| owner_target.as_deref() == Some(target))
            .collect();
        if let [(session_ref, owner, _)] = mine.as_slice() {
            return Ok((session_ref.clone(), *owner));
        }
    }
    match open.as_slice() {
        [] => Err(PickError::None),
        [(session_ref, owner, _)] => Ok((session_ref.clone(), *owner)),
        many => Err(PickError::Ambiguous(
            many.iter()
                .map(|(session_ref, _, _)| session_ref.clone())
                .collect(),
        )),
    }
}

/// Keep only the PNG chunks the relay's media validator admits: critical
/// chunks and the rendering ancillaries (no text, ICC, EXIF, pHYs, …).
/// Chunks are copied whole, so their CRCs stay valid.
pub fn strip_png_metadata(bytes: &[u8]) -> Result<Vec<u8>, String> {
    const SIG: &[u8] = b"\x89PNG\r\n\x1a\n";
    const KEEP: [&[u8; 4]; 11] = [
        b"cHRM", b"gAMA", b"sBIT", b"sRGB", b"bKGD", b"hIST", b"tRNS", b"sPLT", b"acTL", b"fcTL",
        b"fdAT",
    ];
    if !bytes.starts_with(SIG) {
        return Err("the snapshot is not a PNG".into());
    }
    let mut out = SIG.to_vec();
    let mut index = SIG.len();
    loop {
        let header = bytes
            .get(index..index + 8)
            .ok_or("the PNG ends before IEND")?;
        let len = u32::from_be_bytes([header[0], header[1], header[2], header[3]]) as usize;
        let kind = &header[4..8];
        let end = index
            .checked_add(12)
            .and_then(|value| value.checked_add(len))
            .filter(|value| *value <= bytes.len())
            .ok_or("the PNG has a truncated chunk")?;
        let ancillary = kind[0] & 0x20 != 0;
        if !ancillary || KEEP.iter().any(|keep| keep.as_slice() == kind) {
            out.extend_from_slice(&bytes[index..end]);
        }
        index = end;
        if kind == b"IEND" {
            return Ok(out);
        }
    }
}

/// The PNG's pixel size from its IHDR.
pub fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    let width = u32::from_be_bytes(bytes.get(16..20)?.try_into().ok()?);
    let height = u32::from_be_bytes(bytes.get(20..24)?.try_into().ok()?);
    Some((width, height))
}

/// `SnapshotCommit` from `git rev-parse HEAD` and `git status --porcelain`.
pub fn commit_from_git(head: &str, porcelain: &str) -> Option<SnapshotCommit> {
    let sha = head.trim();
    let hex = sha.len() == 40
        && sha
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    hex.then(|| SnapshotCommit {
        sha: sha.to_owned(),
        dirty: !porcelain.trim().is_empty(),
    })
}

fn git_commit(cwd: &Path) -> Option<SnapshotCommit> {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(cwd)
            .output()
            .ok()
            .filter(|output| output.status.success())
            .map(|output| String::from_utf8_lossy(&output.stdout).into_owned())
    };
    let head = run(&["rev-parse", "HEAD"])?;
    let porcelain = run(&["status", "--porcelain"])?;
    commit_from_git(&head, &porcelain)
}

/// One line, host-local facts redacted, at most `max` bytes.
pub fn clean_text(text: &str, max: usize) -> String {
    let one_line: String = redact_free_text(text)
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let mut out = one_line.trim().to_owned();
    while out.len() > max {
        out.pop();
    }
    out
}

/// Build a relay client for `--share` from the top-level flags (the same key
/// and NIP-OA rules the rest of `bee` applies).
pub fn relay_client(
    relay: &str,
    private_key: Option<&str>,
    auth_tag: Option<&str>,
) -> Result<BeekeeperClient, PreviewFailure> {
    let no_identity = |why: String| PreviewFailure::new("preview_share_no_identity", why);
    let keys = Keys::parse(private_key.ok_or_else(|| {
        no_identity("`--share` publishes to the relay and needs BUZZ_PRIVATE_KEY".into())
    })?)
    .map_err(|error| no_identity(format!("invalid BUZZ_PRIVATE_KEY: {error}")))?;
    let (tag, tag_json) = match auth_tag.filter(|input| !input.is_empty()) {
        Some(input) => {
            let json = crate::normalize_auth_tag_input(input);
            let tag = beekeeper_sdk::nip_oa::parse_auth_tag(&json)
                .map_err(|error| no_identity(format!("BUZZ_AUTH_TAG is malformed: {error}")))?;
            beekeeper_sdk::nip_oa::verify_auth_tag(&json, &keys.public_key()).map_err(|error| {
                no_identity(format!("BUZZ_AUTH_TAG verification failed: {error}"))
            })?;
            let canonical = serde_json::to_string(tag.as_slice())
                .map_err(|error| no_identity(format!("BUZZ_AUTH_TAG: {error}")))?;
            (Some(tag), Some(canonical))
        }
        None => (None, None),
    };
    BeekeeperClient::new(
        crate::client::normalize_relay_url(relay),
        keys,
        tag,
        tag_json,
    )
    .map_err(|error| PreviewFailure::new("preview_share_no_relay", error.to_string()))
}

/// Publish the snapshot in `output` (the broker result `execute` shaped) and
/// add `"shared"` to it.
pub async fn share_snapshot(
    mut output: Map<String, Value>,
    alt: Option<&str>,
    context: &PreviewContext,
    client: Option<&BeekeeperClient>,
) -> Result<Value, PreviewFailure> {
    let client = client.ok_or_else(|| {
        PreviewFailure::new(
            "preview_share_no_relay",
            "`--share` needs a relay connection, and this invocation has none.",
        )
    })?;
    let grant = context.grant.as_deref().ok_or_else(|| {
        PreviewFailure::new(
            "preview_no_grant",
            "This process has no preview grant, so it cannot name its session.",
        )
    })?;
    let claims = decode_preview_grant_unverified(grant)
        .map_err(|error| PreviewFailure::new(error.code(), error.to_string()))?;
    let provider = PublicKey::from_hex(&claims.issuer).map_err(|_| {
        PreviewFailure::new(
            "preview_grant_invalid",
            "The grant's issuer is not a pubkey.",
        )
    })?;
    let target_key = coding_session_target_key(&claims.target);

    let page_url = output.get("url").and_then(Value::as_str).unwrap_or("");
    let page = redact_page_url(page_url).ok_or_else(|| {
        PreviewFailure::new(
            "preview_share_page_refused",
            "This page cannot be named on the relay, so the snapshot was not shared.",
        )
    })?;

    let filter = json!({
        "kinds": [KIND_SESSION_PREVIEW_ANNOUNCE],
        "#h": [claims.channel_id.to_string()],
    });
    let rows = client
        .query_paginated(filter, 1000)
        .await
        .map_err(|error| PreviewFailure::new("preview_share_no_relay", error.to_string()))?;
    let events = crate::commands::device::support::events_from_rows(rows);
    let (session_ref, _owner) = pick_session_ref(&events, claims.channel_id, Some(&target_key))
        .map_err(|error| match error {
            PickError::None => {
                PreviewFailure::new("preview_share_not_announced", NOT_ANNOUNCED_SENTENCE)
            }
            PickError::Ambiguous(refs) => PreviewFailure::new(
                "preview_share_not_announced",
                format!(
                    "Several Browsers are shared in this session and none was opened by this \
                     agent ({}).",
                    refs.join(", ")
                ),
            ),
        })?;

    let png_path = output
        .get("pngPath")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| {
            PreviewFailure::new(
                "preview_share_no_image",
                "The snapshot has no PNG to share (it was omitted or not requested).",
            )
        })?;
    let original = std::fs::read(&png_path).map_err(|error| {
        PreviewFailure::new(
            "preview_io_error",
            format!("cannot read {png_path}: {error}"),
        )
    })?;
    let bytes = strip_png_metadata(&original)
        .map_err(|why| PreviewFailure::new("preview_share_upload_failed", why))?;
    let (width, height) = png_dimensions(&bytes).ok_or_else(|| {
        PreviewFailure::new("preview_share_upload_failed", "The PNG has no IHDR.")
    })?;
    let sha256 = hex::encode(Sha256::digest(&bytes));
    let blob = client
        .upload_blob_bytes(bytes, "image/png")
        .await
        .map_err(|error| PreviewFailure::new("preview_share_upload_failed", error.to_string()))?;
    if blob.sha256 != sha256 {
        return Err(PreviewFailure::new(
            "preview_share_upload_failed",
            "The relay stored different bytes than were uploaded.",
        ));
    }

    let title = output
        .get("title")
        .and_then(Value::as_str)
        .map(|title| clean_text(title, MAX_PREVIEW_TITLE_BYTES))
        .filter(|title| !title.is_empty());
    let snapshot = SurfaceSnapshot {
        channel_id: claims.channel_id,
        snapshot_type: SurfaceSnapshotType::Snapshot,
        surface: Surface::Preview,
        key: session_ref,
        sha256: sha256.clone(),
        url: blob.url.clone(),
        mime: "image/png".into(),
        width,
        height,
        taken_at_ms: u64::try_from(context.now_ms).unwrap_or(u64::MAX),
        provider,
        requested_by: None,
        commit: git_commit(&context.cwd),
        reference: None,
        page: Some(page.clone()),
        title,
        alt: clean_text(alt.unwrap_or(""), MAX_SURFACE_SNAPSHOT_ALT_BYTES),
    };
    let builder = beekeeper_sdk::surface::build_surface_snapshot(&snapshot)
        .map_err(|error| PreviewFailure::new("preview_share_publish_failed", error.to_string()))?;
    // Exact tag layout: `sign_event` would add a NIP-OA `auth` tag the
    // 44253 layout refuses.
    let event = client
        .sign_event_unchecked(builder)
        .map_err(|error| PreviewFailure::new("preview_share_publish_failed", error.to_string()))?;
    let event_id = event.id;
    let raw = client
        .submit_event(event)
        .await
        .map_err(|error| PreviewFailure::new("preview_share_publish_failed", error.to_string()))?;
    let accepted: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
    if accepted.get("accepted").and_then(Value::as_bool) == Some(false) {
        let message = accepted
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or("");
        return Err(PreviewFailure::new(
            "preview_share_publish_failed",
            format!("The relay refused the snapshot: {message}"),
        ));
    }
    output.insert(
        "shared".into(),
        json!({
            "eventId": event_id.to_hex(),
            "token": snapshot_evidence_token(&event_id),
            "url": blob.url,
            "sha256": sha256,
            "page": page,
            "commit": snapshot.commit.as_ref().map(|commit| json!({
                "sha": commit.sha,
                "dirty": commit.dirty,
            })),
        }),
    );
    Ok(Value::Object(output))
}

#[cfg(test)]
#[path = "preview_share_tests.rs"]
mod tests;
