//! The provider publishes a gate row nobody asked it for.
//!
//! Brian's 2026-09-02 ruling, end to end: a seat runs a gate, the provider
//! watches the two transcript frames it was already publishing, and a signed
//! kind 44246 row goes on the wire under the **provider's** key with
//! `source: "observed"`. The seat is not consulted, cannot suppress it, and
//! cannot sign it.
//!
//! Live-run finding 26 is what this closes: a seat's prose "`cargo test -p
//! buzz-cli` green" against a verifier reproducing red on the same patch. One
//! of the two was wrong, and nothing on the wire could say which.

use super::*;

use buzz_core::coding_session_observation::{
    CodingSessionObservationBody, CodingSessionObservationGateOutcome,
    CodingSessionObservationGateRow, CodingSessionObservationSource,
};
use buzz_sdk::coding_session_observation::parse_coding_session_observation;

/// The `buzz-agent`/L5 harness shape: the command sits on the call itself.
/// `toolKind: "execute"` is what makes a call eligible to be remembered at
/// all now (finding 69) — every real exec call carries it (`transcript.rs`),
/// so this fixture does too.
fn tool_call(tool_id: &str, command: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "tool_call",
        "tool": {
            "toolName": "Bash",
            "toolId": tool_id,
            "toolKind": "execute",
            "input": { "command": command },
        },
    })
}

fn tool_result(tool_id: &str, is_error: bool, content: &str) -> serde_json::Value {
    serde_json::json!({
        "kind": "tool_result",
        "toolId": tool_id,
        "toolName": "Bash",
        "toolKind": "execute",
        "content": content,
        "isError": is_error,
    })
}

/// One provider holding one governed session, plus that session's id.
///
/// `repo` decides whether the seat's `cwd` is a **throwaway** git repository
/// (never a worktree of this repository — the provider's HEAD resolution runs
/// real `git` in it) or a plain directory.
fn observed_fixture(dir: &tempfile::TempDir, repo: bool) -> (Provider, Uuid, String) {
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    if repo {
        init_repo(&cwd, "gate-branch");
    }
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&state_dir, None);
    let record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    let session_id = record.session_id.clone();
    provider.state.insert_session(record).expect("insert");
    (provider, channel_id, session_id)
}

/// The seat's checkout, as `observed_fixture` laid it out.
fn checkout(dir: &tempfile::TempDir) -> std::path::PathBuf {
    dir.path().join("checkout")
}

/// `git rev-parse HEAD` in `cwd`, as the test's own independent answer.
///
/// Deliberately not the provider's: a test that asked the provider what it
/// resolved would agree with itself no matter what it resolved.
fn head_of(cwd: &std::path::Path) -> String {
    let mut command = std::process::Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let output = command
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("git rev-parse");
    assert!(output.status.success(), "git rev-parse HEAD failed");
    String::from_utf8(output.stdout)
        .expect("utf-8")
        .trim()
        .to_owned()
}

/// Drain session events until the provider has applied one resolved gate row.
///
/// The row is published a moment after the gate closes, not inside
/// `handle_session_event`, because resolving `HEAD` costs two `git`
/// subprocesses that must not run on the provider loop.
async fn pump_until_gate_observed(provider: &mut Provider) {
    pump_until(provider, |event| {
        matches!(event, session::SessionEvent::GateObserved { .. })
    })
    .await;
}

/// The single gate row on the wire after a flush, decoded.
async fn published_gate_rows(provider: &mut Provider) -> Vec<CodingSessionObservationGateRow> {
    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    sink.all()
        .into_iter()
        .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_OBSERVATION)
        .flat_map(|event| {
            let payload = parse_coding_session_observation(&event).expect("a valid 44246 envelope");
            match payload.body {
                CodingSessionObservationBody::Gate(gate) => gate.rows,
                _ => Vec::new(),
            }
        })
        .collect()
}

fn feed(provider: &mut Provider, session_id: &str, items: Vec<serde_json::Value>) {
    provider
        .handle_session_event(session::SessionEvent::TranscriptItems {
            session_id: session_id.to_owned(),
            turn_id: "turn-1".to_owned(),
            items,
        })
        .expect("transcript items");
}

#[tokio::test]
async fn a_seats_failing_cargo_test_publishes_an_observed_gate_row_it_never_asked_for() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, channel_id, session_id) = observed_fixture(&dir, true);

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t1", "cargo test -p buzz-cli subcommand_"),
            tool_result(
                "t1",
                true,
                "test result: FAILED. 0 passed; 2 failed; 0 ignored",
            ),
        ],
    );
    pump_until_gate_observed(&mut provider).await;

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    let observations: Vec<Event> = sink
        .all()
        .into_iter()
        .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_OBSERVATION)
        .collect();
    assert_eq!(observations.len(), 1, "exactly one gate row for one gate");

    let event = &observations[0];
    assert_eq!(
        event.pubkey.to_hex(),
        provider.config.keys.public_key().to_hex(),
        "the mechanism that watched is the author; the seat cannot sign this"
    );
    let payload = parse_coding_session_observation(event).expect("a valid 44246 envelope");
    assert_eq!(payload.source, CodingSessionObservationSource::Observed);
    assert_eq!(payload.session_ref, "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10");
    assert_eq!(payload.genesis_ref, "ab".repeat(32));
    assert_eq!(
        payload.assignment_ref, None,
        "the provider watched a command, not an assignment"
    );
    let CodingSessionObservationBody::Gate(gate) = payload.body else {
        panic!("a gate observation carries a gate body");
    };
    assert_eq!(gate.rows.len(), 1);
    assert_eq!(gate.rows[0].gate, "cargo test");
    assert_eq!(
        gate.rows[0].outcome,
        CodingSessionObservationGateOutcome::Failed
    );
    assert_eq!(gate.rows[0].command, "cargo test -p buzz-cli subcommand_");
    assert_eq!(
        gate.rows[0].summary.as_deref(),
        Some("test result: FAILED. 0 passed; 2 failed; 0 ignored")
    );
    // The commit the gate ran against, resolved by the provider in the seat's
    // own workdir — the seat was never asked, and the seat's transcript never
    // mentioned a commit.
    assert_eq!(
        gate.rows[0].head_sha.as_deref(),
        Some(head_of(&checkout(&dir)).as_str()),
        "the row names the commit HEAD held when the gate closed"
    );
    assert_eq!(
        gate.rows[0].dirty,
        Some(false),
        "a clean checkout is published as clean, and the pair travels together"
    );
    // The envelope is filed under the channel the session publishes into.
    let tags: Vec<Vec<String>> = event
        .tags
        .iter()
        .map(|tag| tag.as_slice().to_vec())
        .collect();
    assert_eq!(tags[0], ["h".to_owned(), channel_id.to_string()]);
}

#[tokio::test]
async fn ordinary_work_publishes_no_gate_row() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, session_id) = observed_fixture(&dir, true);

    feed(
        &mut provider,
        &session_id,
        vec![
            serde_json::json!({ "kind": "assistant_text", "text": "cargo test passed, honest" }),
            tool_call("t1", "git status --porcelain"),
            tool_result("t1", false, ""),
        ],
    );

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(
        sink.all()
            .iter()
            .all(|event| u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_OBSERVATION),
        "prose claiming a gate is not a gate row, and `git status` is not a gate"
    );
}

#[tokio::test]
async fn a_session_with_no_umbrella_publishes_nothing_rather_than_inventing_a_scope() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).expect("mkdir");
    let channel_id = Uuid::new_v4();
    let mut provider = provider(&state_dir, None);
    init_repo(&cwd, "gate-branch");
    let mut record = governed_record(channel_id, &cwd, &"ab".repeat(32));
    // A solo session: no umbrella, so no `sessionRef` and no `genesisRef` for a
    // 44246 to be scoped to.
    record.session_ref = None;
    record.genesis_ref = None;
    let session_id = record.session_id.clone();
    provider.state.insert_session(record).expect("insert");

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t1", "cargo fmt --check"),
            tool_result("t1", false, ""),
        ],
    );

    let sink = CollectingSink::new();
    provider.flush(&sink).await.expect("flush");
    assert!(
        sink.all()
            .iter()
            .all(|event| u32::from(event.kind.as_u16()) != KIND_CODING_SESSION_OBSERVATION),
        "a row filed under an umbrella this session is not part of would be a lie"
    );
    // The gate still ran, and the transcript still says so.
    assert!(!sink.contents_of(KIND_CODING_SESSION_TRANSCRIPT).is_empty());
}

#[tokio::test]
async fn a_lost_mailbox_forgets_the_half_paired_call_rather_than_resolving_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, session_id) = observed_fixture(&dir, true);

    feed(
        &mut provider,
        &session_id,
        vec![tool_call("t1", "cargo clippy --all-targets -- -D warnings")],
    );
    provider
        .handle_session_event(session::SessionEvent::Exited {
            session_id: session_id.clone(),
            reason: session::ExitReason::Requested,
        })
        .expect("exit");
    assert!(
        !provider.gate_observers.contains_key(&session_id),
        "a call whose result this process will never see is not a gate anybody ran"
    );
}

/// A workdir that is not a repository yields **no** commit — absent, never an
/// empty string and never a guess.
///
/// The gate still ran and the row is still published: what the provider could
/// not observe is one field, not the whole record. A row naming no commit
/// admits no push anywhere, which is the honest consequence.
#[tokio::test]
async fn a_gate_in_a_plain_directory_publishes_a_row_that_names_no_commit() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, session_id) = observed_fixture(&dir, false);

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t1", "cargo fmt --check"),
            tool_result("t1", false, ""),
        ],
    );
    pump_until_gate_observed(&mut provider).await;

    let rows = published_gate_rows(&mut provider).await;
    assert_eq!(rows.len(), 1, "the gate ran; the row is still published");
    assert_eq!(rows[0].gate, "cargo fmt");
    assert_eq!(rows[0].head_sha, None);
    assert_eq!(rows[0].dirty, None);
}

/// A worktree with changes in it is published as dirty.
///
/// This is the field that stops a green row from admitting a commit it was
/// not measured against: the tree the gate ran over was not the tree that
/// commit names.
#[tokio::test]
async fn a_gate_run_over_a_changed_worktree_is_published_as_dirty() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, session_id) = observed_fixture(&dir, true);
    std::fs::write(checkout(&dir).join("scratch.rs"), "// uncommitted").expect("write");

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t1", "just ci"),
            tool_result("t1", false, "all gates green"),
        ],
    );
    pump_until_gate_observed(&mut provider).await;

    let rows = published_gate_rows(&mut provider).await;
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].head_sha.as_deref(),
        Some(head_of(&checkout(&dir)).as_str())
    );
    assert_eq!(
        rows[0].dirty,
        Some(true),
        "an untracked file is a change the commit does not name"
    );
}

/// Two gates run at two commits name two commits.
///
/// The point of the key, stated as a test: without it both rows would be
/// "green on this mission", and the older run's green would admit the newer
/// commit — the shape of finding 27.
#[tokio::test]
async fn two_gates_at_two_commits_name_the_commit_each_one_ran_at() {
    let dir = tempfile::tempdir().expect("tempdir");
    let (mut provider, _channel_id, session_id) = observed_fixture(&dir, true);
    let cwd = checkout(&dir);
    let first = head_of(&cwd);

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t1", "cargo fmt --check"),
            tool_result("t1", false, ""),
        ],
    );
    pump_until_gate_observed(&mut provider).await;

    // A second commit in the seat's own checkout, between the two gates.
    let git = |args: &[&str]| {
        let mut command = std::process::Command::new("git");
        for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let status = command
            .arg("-C")
            .arg(&cwd)
            .args([
                "-c",
                "user.email=probe@example.invalid",
                "-c",
                "user.name=probe",
            ])
            .args(args)
            .status()
            .expect("git");
        assert!(status.success(), "git {args:?} failed");
    };
    std::fs::write(cwd.join("second.txt"), "second").expect("write");
    git(&["add", "second.txt"]);
    git(&["commit", "-q", "--no-gpg-sign", "-m", "second"]);
    let second = head_of(&cwd);
    assert_ne!(first, second);

    feed(
        &mut provider,
        &session_id,
        vec![
            tool_call("t2", "cargo clippy --all-targets"),
            tool_result("t2", false, ""),
        ],
    );
    pump_until_gate_observed(&mut provider).await;

    let rows = published_gate_rows(&mut provider).await;
    assert_eq!(rows.len(), 2, "two gates, two rows");
    assert_eq!(rows[0].head_sha.as_deref(), Some(first.as_str()));
    assert_eq!(rows[1].head_sha.as_deref(), Some(second.as_str()));
}
