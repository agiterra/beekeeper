//! Tests for the Blossom leg: what is uploaded when a patch is too big for an
//! event, and what must be true of it before it is applied.
//!
//! The relay here is a local axum server on an ephemeral port serving the two
//! routes this path uses — `PUT /upload` and `GET /media/<hash>` — plus the
//! NIP-11 document whose `max_message_length` decides event-or-blob. Nothing
//! reaches a real relay, and no Postgres or Redis is involved.

use std::sync::Arc;

use axum::body::Bytes;
use axum::extract::{Path as AxumPath, State};
use axum::response::{IntoResponse, Json};
use axum::routing::{get, put};
use axum::Router;
use sha2::{Digest, Sha256};

use beekeeper_core::coding_session_handover::{
    CodingSessionHandoverArtifactKind, CodingSessionHandoverPreserved,
};

use super::*;
use crate::commands::sessions::handover_checkpoint::{carry_patch, decide_preserved};
use crate::commands::sessions::handover_git::{CapturedTree, IgnoredPaths, OmittedPath};

/// How the fake relay should answer an upload.
#[derive(Clone, Copy, PartialEq, Eq)]
enum UploadBehaviour {
    /// Store the body and answer with its descriptor.
    Accept,
    /// Answer 503, the way an overloaded store would.
    Fail,
}

/// What the fake relay should serve back for a blob.
#[derive(Clone)]
enum DownloadBehaviour {
    /// The exact bytes that were uploaded.
    Exact,
    /// Different bytes of the same length — a hash mismatch.
    Corrupted,
    /// Extra bytes appended — a length mismatch, and a longer body.
    Extended,
    /// 404, the way a pruned or never-stored blob answers.
    Absent,
}

struct FakeRelay {
    stored: std::sync::Mutex<Option<Vec<u8>>>,
    upload: UploadBehaviour,
    download: DownloadBehaviour,
}

/// Start the fake relay and return a client pointed at it.
async fn fake_relay(
    upload: UploadBehaviour,
    download: DownloadBehaviour,
    max_message_length: u64,
) -> (BuzzClient, Arc<FakeRelay>, tokio::task::JoinHandle<()>) {
    let state = Arc::new(FakeRelay {
        stored: std::sync::Mutex::new(None),
        upload,
        download,
    });
    let app = Router::new()
        .route(
            "/",
            get({
                let limit = max_message_length;
                move || async move {
                    Json(serde_json::json!({
                        "self": "ab".repeat(32),
                        "limitation": { "max_message_length": limit },
                    }))
                }
            }),
        )
        .route(
            "/upload",
            put(
                |State(state): State<Arc<FakeRelay>>, body: Bytes| async move {
                    if state.upload == UploadBehaviour::Fail {
                        return (
                            axum::http::StatusCode::SERVICE_UNAVAILABLE,
                            "blob store unavailable",
                        )
                            .into_response();
                    }
                    let sha256 = hex::encode(Sha256::digest(&body));
                    let size = body.len();
                    *state.stored.lock().expect("stored") = Some(body.to_vec());
                    Json(serde_json::json!({
                        "url": format!("http://example.invalid/media/{sha256}"),
                        "sha256": sha256,
                        "size": size,
                        "type": PATCH_BLOB_MIME,
                        "uploaded": 1_700_000_000_i64,
                    }))
                    .into_response()
                },
            ),
        )
        .route(
            "/media/{hash}",
            get(
                |State(state): State<Arc<FakeRelay>>, AxumPath(_hash): AxumPath<String>| async move {
                    let stored = state.stored.lock().expect("stored").clone();
                    let Some(bytes) = stored else {
                        return (axum::http::StatusCode::NOT_FOUND, "no such blob").into_response();
                    };
                    match state.download {
                        DownloadBehaviour::Absent => {
                            (axum::http::StatusCode::NOT_FOUND, "no such blob").into_response()
                        }
                        DownloadBehaviour::Exact => bytes.into_response(),
                        DownloadBehaviour::Corrupted => {
                            // Same length, different bytes: only the hash can
                            // catch this one.
                            let mut corrupted = bytes.clone();
                            if let Some(first) = corrupted.first_mut() {
                                *first = first.wrapping_add(1);
                            }
                            corrupted.into_response()
                        }
                        DownloadBehaviour::Extended => {
                            let mut longer = bytes.clone();
                            longer.extend_from_slice(b"\nextra\n");
                            longer.into_response()
                        }
                    }
                },
            ),
        )
        .with_state(state.clone());

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("listener");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let server = tokio::spawn(async move {
        axum::serve(listener, app).await.expect("fake relay");
    });
    let client = BuzzClient::new(url, nostr::Keys::generate(), None, None).expect("client");
    (client, state, server)
}

/// A capture holding `patch`, with nothing omitted and nothing ignored.
fn captured(patch: String) -> CapturedTree {
    CapturedTree {
        patch,
        omitted: Vec::new(),
        changed_paths: vec!["big.txt".to_owned()],
        ignored: IgnoredPaths::default(),
    }
}

/// A patch body large enough to exceed a small advertised event limit.
fn large_patch() -> String {
    let mut patch = String::from("diff --git a/big.txt b/big.txt\n");
    for index in 0..2000 {
        patch.push_str(&format!("+line {index}\n"));
    }
    patch
}

// ── (a) upload above the limit ───────────────────────────────────────────

#[tokio::test]
async fn a_patch_above_the_event_limit_becomes_a_blob_artifact_with_its_hash_and_bytes() {
    let patch = large_patch();
    let expected_hash = hex::encode(Sha256::digest(patch.as_bytes()));
    // An advertised ceiling far below the patch, so the blob branch is the
    // only one that can be taken.
    let (client, _state, server) =
        fake_relay(UploadBehaviour::Accept, DownloadBehaviour::Exact, 9_216).await;

    let capture = captured(patch.clone());
    let (artifact, note) = carry_patch(&client, &capture, "beekeeper", None, &"a".repeat(40))
        .await
        .expect("a patch over the event limit must still travel");

    assert_eq!(artifact.kind, CodingSessionHandoverArtifactKind::Blob);
    assert_eq!(artifact.hash.as_deref(), Some(expected_hash.as_str()));
    assert_eq!(artifact.bytes, Some(patch.len() as u64));
    assert_eq!(artifact.base_sha.as_deref(), Some("a".repeat(40).as_str()));
    assert!(artifact.event_id.is_none(), "a blob names no event");
    assert!(
        note.contains("exceeds the relay's") && note.contains("uploaded as Blossom blob"),
        "the run says why it took the blob route: {note}"
    );
    assert_eq!(
        decide_preserved(true, &capture, true),
        CodingSessionHandoverPreserved::All,
        "nothing was omitted and the patch travelled, so every uncommitted byte is preserved"
    );

    server.abort();
}

// ── (b) download, verify, apply ──────────────────────────────────────────

#[tokio::test]
async fn a_blob_round_trips_and_applies() {
    let repo_dir = tempfile::tempdir().expect("tempdir");
    let repo = repo_dir.path().join("repo");
    std::fs::create_dir_all(&repo).expect("mkdir");
    for args in [
        vec!["init", "--quiet", "--initial-branch=main"],
        vec!["config", "user.email", "c@example.invalid"],
        vec!["config", "user.name", "lane c"],
    ] {
        std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .output()
            .expect("git");
    }
    std::fs::write(repo.join("tracked.txt"), "one\n").expect("write");
    for args in [vec!["add", "-A"], vec!["commit", "--quiet", "-m", "init"]] {
        std::process::Command::new("git")
            .args(&args)
            .current_dir(&repo)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_AUTHOR_NAME", "lane c")
            .env("GIT_AUTHOR_EMAIL", "c@example.invalid")
            .env("GIT_COMMITTER_NAME", "lane c")
            .env("GIT_COMMITTER_EMAIL", "c@example.invalid")
            .output()
            .expect("git");
    }

    // A real patch, produced by git so `git apply` will take it.
    let patch = "diff --git a/tracked.txt b/tracked.txt\n\
                 index 5626abf..814f4a4 100644\n\
                 --- a/tracked.txt\n\
                 +++ b/tracked.txt\n\
                 @@ -1 +1,2 @@\n \
                 one\n\
                 +recovered from a blob\n"
        .to_owned();
    let hash = hex::encode(Sha256::digest(patch.as_bytes()));
    let bytes = patch.len() as u64;

    let (client, state, server) =
        fake_relay(UploadBehaviour::Accept, DownloadBehaviour::Exact, 9_216).await;
    *state.stored.lock().expect("stored") = Some(patch.clone().into_bytes());

    let recovered = fetch_verified_blob(&client, Some(&hash), Some(bytes))
        .await
        .expect("a blob that matches its record must be usable");
    assert_eq!(recovered, patch);
    super::super::handover_git::apply_patch(&repo, &recovered).expect("apply");
    assert_eq!(
        std::fs::read_to_string(repo.join("tracked.txt")).expect("read"),
        "one\nrecovered from a blob\n"
    );

    server.abort();
}

// ── (c) hash mismatch, (d) length mismatch ───────────────────────────────

#[tokio::test]
async fn a_blob_whose_bytes_hash_differently_is_never_applied() {
    let patch = "diff --git a/x b/x\n".to_owned();
    let hash = hex::encode(Sha256::digest(patch.as_bytes()));
    let bytes = patch.len() as u64;
    let (client, state, server) =
        fake_relay(UploadBehaviour::Accept, DownloadBehaviour::Corrupted, 9_216).await;
    *state.stored.lock().expect("stored") = Some(patch.into_bytes());

    let error = fetch_verified_blob(&client, Some(&hash), Some(bytes))
        .await
        .expect_err("must refuse");
    let message = error.to_string();
    assert!(
        message.contains("hashes to") && message.contains(&hash),
        "the reason names both hashes: {message}"
    );
    assert!(
        message.contains("nothing was applied"),
        "and says the tree was not touched: {message}"
    );

    server.abort();
}

#[tokio::test]
async fn a_blob_longer_than_its_record_is_refused_unread() {
    let patch = "diff --git a/x b/x\n".to_owned();
    let hash = hex::encode(Sha256::digest(patch.as_bytes()));
    let bytes = patch.len() as u64;
    let (client, state, server) =
        fake_relay(UploadBehaviour::Accept, DownloadBehaviour::Extended, 9_216).await;
    *state.stored.lock().expect("stored") = Some(patch.into_bytes());

    let error = fetch_verified_blob(&client, Some(&hash), Some(bytes))
        .await
        .expect_err("must refuse");
    let message = error.to_string();
    assert!(
        message.contains("longer body than the record is refused unread"),
        "a body longer than the record is refused for being longer: {message}"
    );
    assert!(message.contains(&bytes.to_string()), "{message}");

    server.abort();
}

#[test]
fn the_three_ways_a_blob_can_be_wrong_are_named_apart() {
    let body = b"a patch\n";
    let hash = hex::encode(Sha256::digest(body));

    assert_eq!(
        verify_blob(body, &hash, Some(body.len() as u64)).expect("good"),
        "a patch\n"
    );

    assert!(matches!(
        verify_blob(body, &hash, Some(4)),
        Err(BlobRejection::TooLong {
            got: 8,
            expected: 4
        })
    ));
    assert!(matches!(
        verify_blob(body, &hash, Some(99)),
        Err(BlobRejection::TooShort {
            got: 8,
            expected: 99
        })
    ));
    assert!(matches!(
        verify_blob(body, &"0".repeat(64), Some(body.len() as u64)),
        Err(BlobRejection::HashMismatch { .. })
    ));
    // Length is checked before the hash, so a wrong-length body is reported
    // as a wrong length rather than as a hash failure — the caller learns
    // which of the two facts to distrust.
    assert!(matches!(
        verify_blob(body, &"0".repeat(64), Some(4)),
        Err(BlobRejection::TooLong { .. })
    ));

    let not_text = vec![0xff_u8, 0xfe, 0xfd];
    let binary_hash = hex::encode(Sha256::digest(&not_text));
    assert!(matches!(
        verify_blob(&not_text, &binary_hash, Some(3)),
        Err(BlobRejection::NotUtf8(_))
    ));
}

// ── (e) failed upload ────────────────────────────────────────────────────

#[tokio::test]
async fn a_blob_upload_that_fails_yields_no_artifact_and_never_preserved_all() {
    let patch = large_patch();
    let (client, _state, server) =
        fake_relay(UploadBehaviour::Fail, DownloadBehaviour::Exact, 9_216).await;

    let capture = captured(patch);
    let error = carry_patch(&client, &capture, "beekeeper", None, &"a".repeat(40))
        .await
        .expect_err("a 5xx from the blob store is not a carried patch");
    assert!(
        error.contains("blob upload failed"),
        "the reason a checkpoint lists says the upload failed: {error}"
    );

    // No artifact was produced, so the checkpoint computes `preserved` with
    // `carried_patch = false` — and a dirty tree whose patch did not travel
    // preserved nothing. It can never read "all".
    assert_eq!(
        decide_preserved(true, &capture, false),
        CodingSessionHandoverPreserved::None,
        "the uncommitted bytes did not travel, and the record must say so"
    );

    server.abort();
}

// ── (f) failed download ──────────────────────────────────────────────────

#[tokio::test]
async fn a_blob_that_cannot_be_fetched_is_listed_rather_than_fatal() {
    let patch = "diff --git a/x b/x\n".to_owned();
    let hash = hex::encode(Sha256::digest(patch.as_bytes()));
    let (client, _state, server) =
        fake_relay(UploadBehaviour::Accept, DownloadBehaviour::Absent, 9_216).await;

    let error = fetch_verified_blob(&client, Some(&hash), Some(patch.len() as u64))
        .await
        .expect_err("a 404 is not a recovered blob");
    let message = error.to_string();
    assert!(
        message.contains("could not be fetched from the relay"),
        "{message}"
    );
    assert!(
        message.contains("were not recovered"),
        "the sentence a continuation lists under `missing` says what was lost: {message}"
    );

    server.abort();
}

#[test]
fn a_blob_artifact_naming_no_hash_is_refused_before_any_fetch() {
    let error = futures_lite_block_on(fetch_verified_blob_no_hash());
    assert!(error.to_string().contains("named no hash"), "got {error}");
}

/// Drive the no-hash branch, which never awaits anything.
async fn fetch_verified_blob_no_hash() -> CliError {
    let client = BuzzClient::new(
        "http://127.0.0.1:1".to_owned(),
        nostr::Keys::generate(),
        None,
        None,
    )
    .expect("client");
    fetch_verified_blob(&client, None, None)
        .await
        .expect_err("no hash is not a blob")
}

/// Run one future to completion on a current-thread runtime.
///
/// A plain `#[test]` rather than `#[tokio::test]` because the branch under
/// test returns before it awaits, and spinning a multi-thread runtime to prove
/// that would be testing tokio.
fn futures_lite_block_on<F: std::future::Future>(future: F) -> F::Output {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime")
        .block_on(future)
}

/// The one omission that must never be silent: a capture that dropped paths
/// still reports `partial`, blob or no blob.
#[test]
fn an_omitted_path_keeps_preserved_at_partial_whatever_carried_the_patch() {
    let capture = CapturedTree {
        patch: "diff --git a/x b/x\n".to_owned(),
        omitted: vec![OmittedPath {
            path: "huge.bin".to_owned(),
            reason: "over the per-file capture bound".to_owned(),
        }],
        changed_paths: vec!["x".to_owned(), "huge.bin".to_owned()],
        ignored: IgnoredPaths::default(),
    };
    assert_eq!(
        decide_preserved(true, &capture, true),
        CodingSessionHandoverPreserved::Partial
    );
}
