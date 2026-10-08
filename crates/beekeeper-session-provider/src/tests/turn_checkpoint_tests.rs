//! SV-28 through the real provider: a scripted agent's turn in a throwaway
//! repository, captured before the prompt and after the terminal, published
//! as one kind 44231.
//!
//! Every repository here is created by the test in its own temporary
//! directory and deleted with it — never this checkout.

use super::*;
use beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT;
use std::time::SystemTime;

/// `git` in `cwd`, hermetic: no inherited repository selection, no global or
/// system configuration, a fixed identity.
pub(super) fn git(cwd: &Path, args: &[&str]) -> String {
    let mut command = std::process::Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let output = command
        .arg("-C")
        .arg(cwd)
        .args(args)
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
        .output()
        .expect("git runs");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_owned()
}

/// A repository on `main` at `dir` with `a.txt`, `b.txt` and one commit.
pub(super) fn init_repo(dir: &Path) {
    std::fs::create_dir_all(dir).expect("mkdir");
    git(dir, &["init", "-q", "-b", "main", "."]);
    std::fs::write(dir.join("a.txt"), "one\n").expect("write");
    std::fs::write(dir.join("b.txt"), "bee\n").expect("write");
    git(dir, &["add", "."]);
    git(dir, &["commit", "-q", "--no-gpg-sign", "-m", "init"]);
}

/// [`GOOD_AGENT`], running `edit` in its shell on every prompt — the shape
/// a transcript fold over edit-tool calls cannot see.
fn editing_agent(edit: &str) -> String {
    format!(
        r#"
while IFS= read -r line; do
  id=$(printf '%s' "$line" | sed -n 's/.*"id":\([0-9]*\).*/\1/p')
  case "$line" in
    *'"method":"initialize"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"protocolVersion":2}}}}\n' "$id" ;;
    *'"method":"session/new"'*)
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"sessionId":"acp-session-1"}}}}\n' "$id" ;;
    *'"method":"session/prompt"'*)
      {edit}
      printf '{{"jsonrpc":"2.0","id":%s,"result":{{"stopReason":"end_turn"}}}}\n' "$id" ;;
  esac
done
"#
    )
}

struct Rig {
    _dir: tempfile::TempDir,
    cwd: std::path::PathBuf,
    channel_id: Uuid,
    provider: Provider,
    target: CodingSessionTarget,
}

/// A provider whose agent appends a line to `a.txt` each turn, with one
/// session created in `cwd` (a repository when `repo`).
async fn rig(repo: bool) -> Rig {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    if repo {
        init_repo(&cwd);
    } else {
        std::fs::create_dir_all(&cwd).expect("mkdir");
    }
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let edit = format!(
        "printf 'shell edit\\n' >> '{}'",
        cwd.join("a.txt").display()
    );
    let agent = fake_agent(dir.path(), "editing-agent", &editing_agent(&edit));
    let mut provider = Provider::new(config_of(
        Keys::generate(),
        &dir.path().join("state"),
        Some(&projects),
        agent,
    ))
    .expect("provider");
    provider
        .handle_command_event(channel_id, &create_event(&provider, channel_id, "create-1"))
        .await
        .expect("create");
    let target = provider
        .state()
        .sessions()
        .next()
        .expect("session")
        .target("instance-1");
    Rig {
        _dir: dir,
        cwd,
        channel_id,
        provider,
        target,
    }
}

impl Rig {
    async fn start_turn(&mut self, command_id: &str) {
        self.provider
            .handle_command_event(
                self.channel_id,
                &turn_event(self.channel_id, command_id, &self.target),
            )
            .await
            .expect("turn");
        pump_until_turn_finished(&mut self.provider).await;
    }

    /// Run a turn to its published checkpoint.
    async fn turn(&mut self, command_id: &str) {
        self.start_turn(command_id).await;
        self.provider.turn_checkpoints.settle().await;
        pump_until(&mut self.provider, |event| {
            matches!(event, SessionEvent::CheckpointCaptured { .. })
        })
        .await;
    }

    async fn flush(&mut self) -> CollectingSink {
        let sink = CollectingSink::new();
        self.provider.flush(&sink).await.expect("flush");
        sink
    }
}

fn checkpoints(sink: &CollectingSink) -> Vec<serde_json::Value> {
    let mut found = sink.contents_of(KIND_CODING_SESSION_CHECKPOINT);
    found.sort_by_key(|checkpoint| checkpoint["coverage"]["throughSeq"].as_u64());
    found
}

fn seq_of(items: &[serde_json::Value], kind: &str) -> Vec<u64> {
    items
        .iter()
        .filter(|item| item["item"]["kind"] == kind)
        .filter_map(|item| item["eventSeq"].as_u64())
        .collect()
}

fn mtime(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .expect("mtime")
}

#[tokio::test]
async fn turn_checkpoint_lists_a_shell_edit_and_leaves_the_repository_alone() {
    let mut rig = rig(true).await;
    let marker = rig.cwd.parent().expect("parent").join("hook-fired");
    for hook in [
        "post-commit",
        "pre-commit",
        "post-checkout",
        "reference-transaction",
    ] {
        let path = rig.cwd.join(".git/hooks").join(hook);
        std::fs::write(&path, format!("#!/bin/sh\ntouch '{}'\n", marker.display())).expect("hook");
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    }
    // Stage a change in the real index: the capture must leave it staged.
    std::fs::write(rig.cwd.join("b.txt"), "staged\n").expect("write");
    git(&rig.cwd, &["add", "b.txt"]);
    let index = rig.cwd.join(".git/index");
    let index_before = (mtime(&index), std::fs::read(&index).expect("index"));
    let head_before = git(&rig.cwd, &["rev-parse", "HEAD"]);
    let branches_before = git(&rig.cwd, &["for-each-ref", "refs/heads"]);

    rig.turn("turn-1").await;
    let sink = rig.flush().await;

    let found = checkpoints(&sink);
    assert_eq!(found.len(), 1, "one checkpoint per turn: {found:#?}");
    let checkpoint = &found[0];
    let items = transcript_items_in_sequence(&sink);
    assert_eq!(
        checkpoint["coverage"]["fromSeq"].as_u64(),
        seq_of(&items, "user_prompt").first().copied(),
        "coverage opens at the turn's prompt"
    );
    assert_eq!(
        checkpoint["coverage"]["throughSeq"].as_u64(),
        seq_of(&items, "result").last().copied(),
        "and closes at its terminal"
    );
    let git_fact = &checkpoint["git"];
    assert!(git_fact["baseTree"].is_string(), "{checkpoint:#}");
    assert_ne!(git_fact["baseTree"], git_fact["tree"]);
    assert_eq!(git_fact["head"], head_before.as_str());
    assert_eq!(git_fact["branch"], "main");
    assert_eq!(
        git_fact["outsideTurn"],
        serde_json::Value::Null,
        "first checkpoint"
    );
    assert_eq!(
        checkpoint["files"],
        serde_json::json!([{
            "path": "a.txt", "status": "modified", "from": null, "additions": 1, "deletions": 0
        }]),
        "the shell-only edit is listed, and the change staged before the turn is not"
    );
    assert_eq!(checkpoint["unavailable"], serde_json::Value::Null);
    // SV-29: restorable exactly when the turn has git facts and a baseline.
    assert_eq!(
        checkpoint["restorable"],
        checkpoint["git"]["baseTree"].is_string()
    );

    // Signed by the key that signs the same generation's 44225 items, and
    // structurally valid for every reader.
    let event = sink
        .all()
        .into_iter()
        .find(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_CHECKPOINT)
        .expect("event");
    beekeeper_sdk::coding_session_checkpoint::parse_coding_session_checkpoint(&event)
        .expect("valid 44231");
    assert_eq!(event.pubkey, rig.provider.config.keys.public_key());

    // The person's repository is exactly as it was.
    assert_eq!(
        (mtime(&index), std::fs::read(&index).expect("index")),
        index_before,
        "the real index is untouched"
    );
    assert_eq!(git(&rig.cwd, &["rev-parse", "HEAD"]), head_before);
    assert_eq!(
        git(&rig.cwd, &["for-each-ref", "refs/heads"]),
        branches_before
    );
    assert_eq!(git(&rig.cwd, &["diff", "--cached", "--name-only"]), "b.txt");
    assert!(!marker.exists(), "no hook ran");
    let pins = git(
        &rig.cwd,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/beekeeper/checkpoints",
        ],
    );
    assert_eq!(pins.lines().count(), 2, "a base and a through ref: {pins}");
}

#[tokio::test]
async fn turn_checkpoint_discloses_edits_between_turns_and_large_files() {
    let mut rig = rig(true).await;
    rig.turn("turn-1").await;
    // A person edits between turns, and drops in a file too large to capture.
    std::fs::write(rig.cwd.join("b.txt"), "edited between turns\n").expect("write");
    std::fs::write(rig.cwd.join("big.bin"), vec![0_u8; 17 * 1024 * 1024]).expect("write");
    rig.turn("turn-2").await;
    rig.turn("turn-3").await;
    let found = checkpoints(&rig.flush().await);
    assert_eq!(found.len(), 3);
    let outside: Vec<_> = found
        .iter()
        .map(|checkpoint| checkpoint["git"]["outsideTurn"].clone())
        .collect();
    assert_eq!(
        outside,
        vec![
            serde_json::Value::Null,
            serde_json::Value::Bool(true),
            serde_json::Value::Bool(false)
        ]
    );
    let second = &found[1]["git"];
    assert_eq!(
        second["omitted"],
        serde_json::json!([{ "path": "big.bin", "reason": "too_large" }])
    );
    assert_eq!(second["complete"], false);
    assert!(
        found[1]["files"]
            .as_array()
            .expect("files")
            .iter()
            .all(|file| file["path"] != "big.bin" && file["path"] != "b.txt"),
        "only the turn's own change is the turn's: {}",
        found[1]["files"]
    );
}

#[tokio::test]
async fn turn_checkpoint_outside_a_repository_says_so() {
    let mut rig = rig(false).await;
    rig.turn("turn-1").await;
    let found = checkpoints(&rig.flush().await);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["git"], serde_json::Value::Null);
    assert_eq!(found[0]["unavailable"]["code"], "NOT_A_REPOSITORY");
    assert_eq!(found[0]["files"], serde_json::json!([]));
}

#[tokio::test]
async fn turn_checkpoint_a_missed_baseline_still_runs_the_turn() {
    let mut rig = rig(true).await;
    rig.provider.turn_checkpoints.baseline_ceiling = Some(Duration::ZERO);
    rig.turn("turn-1").await;
    let sink = rig.flush().await;
    let items = transcript_items_in_sequence(&sink);
    assert_eq!(
        seq_of(&items, "result").len(),
        1,
        "the turn ran to its result"
    );
    assert!(std::fs::read_to_string(rig.cwd.join("a.txt"))
        .expect("read")
        .contains("shell edit"));
    let found = checkpoints(&sink);
    assert_eq!(found.len(), 1);
    assert_eq!(found[0]["git"]["baseTree"], serde_json::Value::Null);
    assert!(found[0]["git"]["tree"].is_string());
    assert_eq!(
        found[0]["files"],
        serde_json::json!([]),
        "no baseline, no range"
    );
    assert_eq!(found[0]["filesNotListed"], 0);
}

#[tokio::test]
async fn turn_checkpoint_a_blocked_capture_does_not_hold_the_terminal() {
    let mut rig = rig(true).await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    rig.provider.turn_checkpoints.hold = Some(hold.clone());
    rig.start_turn("turn-1").await;

    let sink = rig.flush().await;
    let items = transcript_items_in_sequence(&sink);
    assert_eq!(
        seq_of(&items, "result").len(),
        1,
        "the terminal 44225 is out"
    );
    assert!(
        sink.contents_of(KIND_CODING_SESSION_METADATA)
            .iter()
            .any(|metadata| metadata["status"] == "idle"),
        "and the 44223 that says the turn ended"
    );
    assert!(
        checkpoints(&sink).is_empty(),
        "while the capture is still held"
    );

    hold.add_permits(64);
    rig.provider.turn_checkpoints.settle().await;
    pump_until(&mut rig.provider, |event| {
        matches!(event, SessionEvent::CheckpointCaptured { .. })
    })
    .await;
    assert_eq!(checkpoints(&rig.flush().await).len(), 1);
}

#[tokio::test]
async fn turn_checkpoint_a_capture_overlapped_by_the_next_turn_is_not_complete() {
    let mut rig = rig(true).await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    rig.provider.turn_checkpoints.hold = Some(hold.clone());
    // Turn 1's end capture is still running when turn 2 is prompted (a slow
    // repository), so its tree may hold turn 2's edits.
    rig.start_turn("turn-1").await;
    rig.start_turn("turn-2").await;
    hold.add_permits(64);
    rig.provider.turn_checkpoints.settle().await;
    for _ in 0..2 {
        pump_until(&mut rig.provider, |event| {
            matches!(event, SessionEvent::CheckpointCaptured { .. })
        })
        .await;
    }
    // Turn 3 runs alone: nothing overlaps its capture.
    rig.provider.turn_checkpoints.hold = None;
    rig.turn("turn-3").await;
    let found = checkpoints(&rig.flush().await);
    assert_eq!(found.len(), 3);
    assert_eq!(
        found[0]["git"]["complete"], false,
        "the overlapped capture never reads as a complete measurement: {}",
        found[0]
    );
    assert!(found[0]["git"]["tree"].is_string(), "it is still from git");
    assert_eq!(
        found[2]["git"]["complete"], true,
        "a capture nothing overlapped stays complete: {}",
        found[2]
    );
}

#[tokio::test]
async fn turn_checkpoint_a_clean_shutdown_drains_the_captures_in_flight() {
    let mut rig = rig(true).await;
    let hold = Arc::new(tokio::sync::Semaphore::new(0));
    rig.provider.turn_checkpoints.hold = Some(hold.clone());
    rig.start_turn("turn-1").await;
    hold.add_permits(64);
    rig.provider.drain_turn_checkpoints().await;
    assert_eq!(
        checkpoints(&rig.flush().await).len(),
        1,
        "the capture that finished during the drain is queued before exit"
    );
}
