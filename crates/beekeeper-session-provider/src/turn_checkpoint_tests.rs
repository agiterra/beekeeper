//! Mapping a capture onto the wire (NIP-CSCK § Rules), and the actor's
//! bounded baseline wait. No git runs here; the end-to-end turns are in
//! `tests/turn_checkpoint_tests.rs`.

use super::*;
use crate::turn_checkpoint_git::{FileChange, OmitReason};
use beekeeper_core::coding_session_checkpoint::{
    decode_coding_session_checkpoint, encode_coding_session_checkpoint,
};

fn oid(byte: char) -> String {
    byte.to_string().repeat(40)
}

fn target() -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "instance-1".into(),
        session_id: "sess-1".into(),
        generation: 2,
    }
}

fn tree(omitted: Vec<OmittedPath>) -> CapturedTree {
    CapturedTree {
        tree: oid('a'),
        commit: oid('c'),
        head: Some(oid('d')),
        branch: Some("main".into()),
        complete: omitted.is_empty(),
        omitted,
        omitted_not_listed: 0,
        boundary_enforced: true,
    }
}

fn changed(path: &str) -> ChangedFile {
    ChangedFile {
        path: path.into(),
        status: FileChange::Modified,
        from: None,
        additions: Some(1),
        deletions: None,
    }
}

fn capture(base: Option<&str>, measured: Measured) -> TurnCheckpointCapture {
    TurnCheckpointCapture {
        channel_id: Uuid::nil(),
        target: target(),
        turn_id: "turn-1".into(),
        from_seq: 41,
        through_seq: 58,
        base_tree: base.map(str::to_owned),
        previous: None,
        measured,
        overlapped: Default::default(),
    }
}

fn captured(base: Option<&str>, files: Vec<ChangedFile>, not_listed: u64) -> TurnCheckpointCapture {
    capture(
        base,
        Measured::Captured {
            tree: tree(Vec::new()),
            files: base.map(|_| DiffResult { files, not_listed }),
        },
    )
}

/// Every payload the mapper builds is one the wire accepts.
fn encodes(payload: &CodingSessionCheckpointPayload) {
    let content = encode_coding_session_checkpoint(payload).expect("valid checkpoint");
    decode_coding_session_checkpoint(&content).expect("round trip");
}

#[test]
fn turn_checkpoint_maps_coverage_files_and_git() {
    let base = oid('b');
    let payload = checkpoint_payload(
        &captured(Some(&base), vec![changed("src/main.rs")], 0),
        None,
    );
    encodes(&payload);
    assert_eq!(payload.turn_id.as_deref(), Some("turn-1"));
    assert_eq!(payload.reason, CodingSessionCheckpointReason::Turn);
    assert_eq!(
        (payload.coverage.from_seq, payload.coverage.through_seq),
        (41, 58)
    );
    assert!(
        payload.restorable,
        "git and a baseTree from a build that rewinds"
    );
    assert_eq!(payload.summary, None);
    assert_eq!(payload.unavailable, None);
    let git = payload.git.expect("git");
    assert_eq!(git.base_tree.as_deref(), Some(base.as_str()));
    assert_eq!(git.tree, oid('a'));
    assert_eq!(git.head, Some(oid('d')));
    assert_eq!(git.branch.as_deref(), Some("main"));
    assert!(git.complete);
    assert_eq!(payload.files.len(), 1);
    assert_eq!(payload.files[0].path, "src/main.rs");
    assert_eq!(
        payload.files[0].deletions, None,
        "a missing count stays null, not 0"
    );
}

#[test]
fn turn_checkpoint_caps_files_at_256_and_counts_the_rest() {
    let base = oid('b');
    let mut files: Vec<_> = (0..300).map(|n| changed(&format!("f{n:03}.txt"))).collect();
    files.push(changed("refs/heads/sneaky"));
    files.push(changed("../outside"));
    let payload = checkpoint_payload(&captured(Some(&base), files, 5), None);
    encodes(&payload);
    assert_eq!(payload.files.len(), MAX_CHECKPOINT_FILES);
    assert_eq!(payload.files_not_listed, 300 - 256 + 2 + 5);
    assert!(payload
        .files
        .iter()
        .all(|file| !file.path.starts_with("refs/")));
}

#[test]
fn turn_checkpoint_caps_omitted_at_32_and_is_never_complete_with_omissions() {
    let omitted: Vec<_> = (0..40)
        .map(|n| OmittedPath {
            path: format!("big{n:02}.bin"),
            reason: OmitReason::TooLarge,
        })
        .collect();
    let mut tree = tree(omitted);
    tree.omitted_not_listed = 3;
    let base = oid('b');
    let payload = checkpoint_payload(
        &capture(
            Some(&base),
            Measured::Captured {
                tree,
                files: Some(DiffResult::default()),
            },
        ),
        None,
    );
    encodes(&payload);
    let git = payload.git.expect("git");
    assert_eq!(git.omitted.len(), MAX_CHECKPOINT_OMITTED);
    assert_eq!(git.omitted_not_listed, 40 - 32 + 3);
    assert!(!git.complete);
}

#[test]
fn turn_checkpoint_outside_turn_compares_the_previous_tree_with_this_base() {
    let base = oid('b');
    let same = checkpoint_payload(&captured(Some(&base), Vec::new(), 0), Some(&base));
    let moved = checkpoint_payload(&captured(Some(&base), Vec::new(), 0), Some(&oid('e')));
    let first = checkpoint_payload(&captured(Some(&base), Vec::new(), 0), None);
    let unbased = checkpoint_payload(&captured(None, Vec::new(), 0), Some(&base));
    let outside =
        |payload: &CodingSessionCheckpointPayload| payload.git.as_ref().expect("git").outside_turn;
    assert_eq!(outside(&same), Some(false));
    assert_eq!(outside(&moved), Some(true));
    assert_eq!(outside(&first), None, "no previous checkpoint");
    assert_eq!(outside(&unbased), None, "no baseTree to compare");
}

#[test]
fn turn_checkpoint_without_a_baseline_lists_no_files() {
    let mut capture = captured(None, Vec::new(), 0);
    // Even a diff, were one present, is not a range without a baseline.
    capture.measured = Measured::Captured {
        tree: tree(Vec::new()),
        files: Some(DiffResult {
            files: vec![changed("a.txt")],
            not_listed: 1,
        }),
    };
    let payload = checkpoint_payload(&capture, None);
    encodes(&payload);
    assert_eq!(payload.git.as_ref().expect("git").base_tree, None);
    assert!(payload.files.is_empty());
    assert_eq!(payload.files_not_listed, 0);
}

#[test]
fn turn_checkpoint_failure_is_unavailable_with_no_git() {
    let payload = checkpoint_payload(
        &capture(
            None,
            Measured::Failed(CaptureFailure {
                code: UnavailableCode::NotARepository,
                sentence: "Not a repository.".into(),
                already_pinned: None,
            }),
        ),
        None,
    );
    encodes(&payload);
    assert_eq!(payload.git, None);
    let unavailable = payload.unavailable.expect("unavailable");
    assert_eq!(
        unavailable.code,
        CodingSessionCheckpointUnavailableCode::NotARepository
    );
    assert!(payload.files.is_empty());
}

#[test]
fn turn_checkpoint_fits_32_kib_by_counting_files_it_cannot_name() {
    let base = oid('b');
    let long = "d/".repeat(400);
    let files: Vec<_> = (0..256)
        .map(|n| changed(&format!("{long}{n:03}.txt")))
        .collect();
    let payload = checkpoint_payload(&captured(Some(&base), files, 0), None);
    assert!(serde_json::to_string(&payload).expect("json").len() > 32 * 1024);
    let (builder, tree) = fit_checkpoint(Uuid::nil(), payload).expect("fits");
    let event = builder
        .sign_with_keys(&nostr::Keys::generate())
        .expect("sign");
    let fitted = decode_coding_session_checkpoint(&event.content).expect("decodes");
    assert!(event.content.len() <= MAX_CODING_SESSION_CHECKPOINT_CONTENT_BYTES);
    assert_eq!(
        fitted.files.len() as u64 + fitted.files_not_listed,
        256,
        "every file is either named or counted"
    );
    assert_eq!(tree, Some(oid('a')));
}

#[test]
fn turn_checkpoint_coverage_opens_on_the_unsteered_prompt() {
    let prompt = serde_json::json!({ "kind": "user_prompt", "steered": false });
    let steered = serde_json::json!({ "kind": "user_prompt", "steered": true });
    let text = serde_json::json!({ "kind": "assistant_text", "text": "hi" });
    assert_eq!(CoverageItem::of(&prompt), CoverageItem::Prompt);
    assert_eq!(CoverageItem::of(&steered), CoverageItem::Other);
    assert_eq!(CoverageItem::of(&text), CoverageItem::Other);
}

/// Drive one baseline request: `answer` plays the provider.
async fn baseline_with<F>(
    mailbox: &mpsc::Receiver<()>,
    answer: F,
) -> (Option<String>, tokio::time::Duration)
where
    F: FnOnce(watch::Sender<BaselineAnswer>) + Send + 'static,
{
    let (events, mut inbox) = mpsc::channel(8);
    let (_stop, shutdown) = watch::channel(false);
    let started = tokio::time::Instant::now();
    let provider = tokio::spawn(async move {
        let Some(SessionEvent::TurnBaselineRequested { reply, .. }) = inbox.recv().await else {
            panic!("a baseline request");
        };
        answer(reply);
        inbox.recv().await
    });
    let repo = tempfile::tempdir().expect("tempdir");
    std::fs::create_dir(repo.path().join(".git")).expect("a .git entry");
    let turn = BaselineRequest {
        cwd: repo.path(),
        session_id: "s",
        turn_id: "t",
    };
    await_baseline(&events, &shutdown, mailbox, turn).await;
    let elapsed = started.elapsed();
    drop(events);
    let forwarded = match provider.await.expect("joined") {
        Some(SessionEvent::BaselineSettled { base_tree, .. }) => base_tree,
        _ => None,
    };
    (forwarded, elapsed)
}

/// The actor's wait: answered, refused, never taken up, taken up and never
/// answered, and cut short by a command in the mailbox.
#[tokio::test(start_paused = true)]
async fn turn_checkpoint_baseline_wait_is_bounded() {
    let (_mail, empty) = mpsc::channel::<()>(1);

    let (tree, _) = baseline_with(&empty, |reply| {
        let _ = reply.send(BaselineAnswer::Capturing);
        let _ = reply.send(BaselineAnswer::Captured(oid('b')));
        std::mem::forget(reply);
    })
    .await;
    assert_eq!(tree, Some(oid('b')), "a captured baseline is forwarded");

    let (tree, elapsed) = baseline_with(&empty, drop).await;
    assert_eq!(tree, None);
    assert!(elapsed < BASELINE_PICKUP_WAIT, "a refusal returns at once");

    let (tree, elapsed) = baseline_with(&empty, std::mem::forget).await;
    assert_eq!(tree, None);
    assert!(
        elapsed >= BASELINE_PICKUP_WAIT && elapsed < BASELINE_WAIT,
        "a request nobody takes up holds the prompt only briefly: {elapsed:?}"
    );

    let (tree, elapsed) = baseline_with(&empty, |reply| {
        let _ = reply.send(BaselineAnswer::Capturing);
        std::mem::forget(reply);
    })
    .await;
    assert_eq!(tree, None);
    assert!(
        elapsed >= BASELINE_WAIT,
        "a capture that never ends is capped"
    );
    assert!(elapsed < BASELINE_WAIT + Duration::from_millis(100));

    let (mail, pending) = mpsc::channel::<()>(2);
    mail.try_send(()).expect("a turn already queued");
    let (tree, elapsed) = baseline_with(&pending, |reply| {
        let _ = reply.send(BaselineAnswer::Capturing);
        let _ = reply.send(BaselineAnswer::Captured(oid('b')));
        std::mem::forget(reply);
    })
    .await;
    assert_eq!(
        tree,
        Some(oid('b')),
        "a turn already queued does not skip it"
    );
    assert!(elapsed < BASELINE_PICKUP_WAIT);

    let arriving = mail.clone();
    let (tree, elapsed) = baseline_with(&pending, move |reply| {
        let _ = reply.send(BaselineAnswer::Capturing);
        std::mem::forget(reply);
        arriving.try_send(()).expect("an interrupt arrives");
    })
    .await;
    assert_eq!(tree, None);
    assert!(
        elapsed < BASELINE_PICKUP_WAIT,
        "a command arriving ends the wait"
    );
    assert_eq!(pending.len(), 2, "and is left for the prompt loop");
}

/// Outside a repository nothing is asked and the prompt is not held.
#[tokio::test]
async fn turn_checkpoint_outside_a_repository_asks_for_no_baseline() {
    let (events, mut inbox) = mpsc::channel(8);
    let (_stop, shutdown) = watch::channel(false);
    let (_mail, mailbox) = mpsc::channel::<()>(1);
    let plain = tempfile::tempdir().expect("tempdir");
    assert!(
        !inside_work_tree(plain.path()),
        "the temp dir is outside any repository"
    );
    let turn = BaselineRequest {
        cwd: plain.path(),
        session_id: "s",
        turn_id: "t",
    };
    await_baseline(&events, &shutdown, &mailbox, turn).await;
    assert!(inbox.try_recv().is_err(), "no baseline request");
}

/// SV-29: `restorable` needs the trees a rewind restores to. No baseline, or
/// no git facts at all, and the checkpoint says it cannot be rewound to.
#[test]
fn turn_checkpoint_is_restorable_only_with_git_and_a_base_tree() {
    let without_base = checkpoint_payload(&captured(None, Vec::new(), 0), None);
    encodes(&without_base);
    assert!(!without_base.restorable);
    let failed = checkpoint_payload(
        &capture(Some(&oid('b')), Measured::Failed(not_measured())),
        None,
    );
    encodes(&failed);
    assert!(!failed.restorable);
}
