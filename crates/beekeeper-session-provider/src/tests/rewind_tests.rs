//! SV-29 `session.rewind` through the real provider: a scripted agent's three
//! turns in a throwaway repository (add, modify, delete), then a rewind to
//! before turn 2 — chat only, files too, refused, or not restarted.
//!
//! Every repository here is created by the test in its own temporary
//! directory and deleted with it — never this checkout.

use super::turn_checkpoint_tests::{git, init_repo};
use super::*;
use beekeeper_core::coding_session_checkpoint::CodingSessionCheckpointPayload;
use beekeeper_core::coding_session_payload::{
    CHECKPOINT_NOT_THIS_EXECUTION, CHECKPOINT_UNAVAILABLE, NOT_RESTORABLE, REWIND_NOT_RESTARTED,
    SESSION_BUSY, TREE_BUSY,
};
use beekeeper_core::kind::KIND_CODING_SESSION_CHECKPOINT;

/// An agent whose n-th prompt runs the n-th edit: add `new.txt`, change
/// `a.txt`, delete `b.txt`. The counter is an ignored file in the tree.
fn three_edit_agent(cwd: &Path, counter: &Path) -> String {
    let cwd = cwd.display();
    let counter = counter.display();
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
      n=$(cat '{counter}' 2>/dev/null || echo 0); n=$((n+1)); echo "$n" > '{counter}'
      case "$n" in
        1) printf 'added\n' > '{cwd}/new.txt' ;;
        2) printf 'changed\n' >> '{cwd}/a.txt' ;;
        3) rm -f '{cwd}/b.txt' ;;
      esac
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
    /// Every event flushed so far.
    published: Vec<Event>,
}

async fn rig() -> Rig {
    rig_for(None).await
}

/// The rig, its execution seated as `actor` when one is named: the seat is
/// staged under the create's command id, as the desktop stages it.
async fn rig_for(actor: Option<&str>) -> Rig {
    let dir = tempfile::tempdir().expect("tempdir");
    let cwd = dir.path().join("checkout");
    init_repo(&cwd);
    std::fs::write(cwd.join(".gitignore"), "*.log\n").expect("ignore");
    git(&cwd, &["add", ".gitignore"]);
    git(&cwd, &["commit", "-q", "--no-gpg-sign", "-m", "ignore"]);
    let channel_id = Uuid::new_v4();
    let projects = write_projects(dir.path(), channel_id, &cwd);
    let agent = fake_agent(
        dir.path(),
        "three-edit-agent",
        // Inside the tree (the agent's boundary allows no write outside it)
        // and ignored, so it is in no checkpoint.
        &three_edit_agent(&cwd, &cwd.join("turns.log")),
    );
    let mut provider = Provider::new(config_of(
        Keys::generate(),
        &dir.path().join("state"),
        Some(&projects),
        agent,
    ))
    .expect("provider");
    let create = match actor {
        Some(actor) => {
            write_actor_seats(dir.path(), "create-1", actor);
            seated_create_event(&provider, channel_id, "create-1", actor, "builder")
        }
        None => create_event(&provider, channel_id, "create-1"),
    };
    provider
        .handle_command_event(channel_id, &create)
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
        published: Vec::new(),
    }
}

impl Rig {
    async fn turn(&mut self, command_id: &str) {
        self.provider
            .handle_command_event(
                self.channel_id,
                &turn_event(self.channel_id, command_id, &self.target),
            )
            .await
            .expect("turn");
        pump_until_turn_finished(&mut self.provider).await;
        self.provider.turn_checkpoints.settle().await;
        pump_until(&mut self.provider, |event| {
            matches!(event, SessionEvent::CheckpointCaptured { .. })
        })
        .await;
    }

    async fn flush(&mut self) {
        let sink = CollectingSink::new();
        self.provider.flush(&sink).await.expect("flush");
        self.published.extend(sink.all());
    }

    /// Three turns, published; the provider can read their checkpoints.
    async fn three_turns(&mut self) {
        for turn in ["turn-1", "turn-2", "turn-3"] {
            self.turn(turn).await;
        }
        self.flush().await;
        for event in self.checkpoint_events() {
            self.provider
                .rewind_checkpoints
                .insert(event.id.to_hex(), event);
        }
    }

    fn checkpoint_events(&self) -> Vec<Event> {
        let mut found: Vec<Event> = self
            .published
            .iter()
            .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_CHECKPOINT)
            .cloned()
            .collect();
        found.sort_by_key(|event| payload_of(event).coverage.through_seq);
        found
    }

    /// The `n`-th turn's checkpoint (1-based).
    fn turn_checkpoint(&self, n: usize) -> Event {
        self.checkpoint_events()
            .into_iter()
            .filter(|event| payload_of(event).turn_id.is_some())
            .nth(n - 1)
            .expect("checkpoint")
    }

    async fn rewind(&mut self, command_id: &str, checkpoint: &str, files: &str) {
        let event = self.rewind_event(command_id, checkpoint, files);
        self.provider
            .handle_command_event(self.channel_id, &event)
            .await
            .expect("rewind");
        self.flush().await;
    }

    fn rewind_event(&self, command_id: &str, checkpoint: &str, files: &str) -> Event {
        let content = serde_json::json!({
            "schema": "buzz-coding-session-lifecycle-command/v1",
            "commandId": command_id,
            "action": {
                "type": "session.rewind",
                "session": self.target,
                "providerAuthorityPubkey": self.provider.config.pubkey_hex(),
                "checkpoint": checkpoint,
                "files": files,
            },
        })
        .to_string();
        signed_lifecycle_event(self.channel_id, content)
    }

    fn receipts(&self, command_id: &str) -> usize {
        self.published
            .iter()
            .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .filter_map(|event| serde_json::from_str::<serde_json::Value>(&event.content).ok())
            .filter(|receipt| receipt["commandId"] == command_id)
            .count()
    }

    fn receipt(&self, command_id: &str) -> serde_json::Value {
        self.published
            .iter()
            .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_LIFECYCLE_RECEIPT)
            .map(|event| {
                beekeeper_core::coding_session_payload::decode_coding_session_lifecycle_receipt(
                    &event.content,
                )
                .expect("every receipt decodes strictly");
                serde_json::from_str::<serde_json::Value>(&event.content).expect("json")
            })
            .find(|receipt| receipt["commandId"] == command_id)
            .expect("answered")
    }

    fn generation(&self) -> u64 {
        self.provider
            .state()
            .session(&self.target.session_id)
            .expect("record")
            .generation
    }

    fn read(&self, name: &str) -> Option<String> {
        std::fs::read_to_string(self.cwd.join(name)).ok()
    }

    /// The working tree after turn 3: new.txt, a.txt changed, b.txt gone.
    fn assert_after_turn_three(&self) {
        assert_eq!(self.read("new.txt").as_deref(), Some("added\n"));
        assert_eq!(self.read("a.txt").as_deref(), Some("one\nchanged\n"));
        assert_eq!(self.read("b.txt"), None);
    }
}

fn payload_of(event: &Event) -> CodingSessionCheckpointPayload {
    beekeeper_core::coding_session_checkpoint::decode_coding_session_checkpoint(&event.content)
        .expect("checkpoint")
}

#[tokio::test]
async fn rewind_with_files_restores_the_tree_before_the_turn_and_opens_a_new_generation() {
    let mut rig = rig().await;
    rig.three_turns().await;
    rig.assert_after_turn_three();
    std::fs::write(rig.cwd.join("build.log"), "ignored\n").expect("ignored");
    let head = git(&rig.cwd, &["rev-parse", "HEAD"]);
    let branches = git(&rig.cwd, &["for-each-ref", "refs/heads"]);
    let index = std::fs::read(rig.cwd.join(".git/index")).expect("index");
    let turn_two = rig.turn_checkpoint(2);
    let turn_two_payload = payload_of(&turn_two);
    assert!(
        turn_two_payload.restorable,
        "a rewinding build's checkpoint"
    );
    let turn_three_tree = payload_of(&rig.turn_checkpoint(3)).git.expect("git").tree;

    rig.rewind("rewind-1", &turn_two.id.to_hex(), "restore")
        .await;

    let receipt = rig.receipt("rewind-1");
    // No context sidecar in this rig: decision 2, restarted with no memory.
    assert_eq!(receipt["status"], "resumed_without_context", "{receipt:#}");
    assert!(receipt["error"]["message"]
        .as_str()
        .is_some_and(|message| message.starts_with("restarted with no memory")));
    let rewind = &receipt["rewind"];
    assert_eq!(rewind["files"], "restored");
    assert_eq!(rewind["checkpoint"], turn_two.id.to_hex());
    assert_eq!(rewind["cutGeneration"], 1);
    assert_eq!(rewind["previousGeneration"], 1);
    assert_eq!(
        rewind["cutAfterSeq"].as_u64(),
        Some(turn_two_payload.coverage.from_seq - 1)
    );
    assert_eq!(rewind["head"], head.as_str(), "HEAD's unchanged oid");
    assert_eq!(receipt["session"]["generation"], 2);
    assert_eq!(rig.generation(), 2);

    // The tree is turn 2's base: turn 1's addition stays, 2 and 3 are undone.
    assert_eq!(rig.read("new.txt").as_deref(), Some("added\n"));
    assert_eq!(rig.read("a.txt").as_deref(), Some("one\n"));
    assert_eq!(rig.read("b.txt").as_deref(), Some("bee\n"));
    assert_eq!(rig.read("build.log").as_deref(), Some("ignored\n"));
    assert_eq!(git(&rig.cwd, &["rev-parse", "HEAD"]), head);
    assert_eq!(git(&rig.cwd, &["for-each-ref", "refs/heads"]), branches);
    assert_eq!(
        std::fs::read(rig.cwd.join(".git/index")).expect("index"),
        index,
        "the real index is untouched"
    );

    // The pre_rewind checkpoint names the tree as it was before any write —
    // turn 3's — and the receipt names it.
    let pre = rig
        .checkpoint_events()
        .into_iter()
        .find(|event| payload_of(event).turn_id.is_none())
        .expect("a pre_rewind checkpoint");
    let pre_payload = payload_of(&pre);
    assert_eq!(rewind["preRewindCheckpoint"], pre.id.to_hex());
    assert_eq!(pre_payload.git.as_ref().expect("git").tree, turn_three_tree);
    assert!(!pre_payload.restorable);
    let pins = git(
        &rig.cwd,
        &[
            "for-each-ref",
            "--format=%(refname)",
            "refs/beekeeper/checkpoints",
        ],
    );
    assert!(pins.contains("/pre-rewind-"), "{pins}");

    // Generation 2 opens with the session_rewound row.
    let first = rig
        .published
        .iter()
        .filter(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_TRANSCRIPT)
        .map(|event| serde_json::from_str::<serde_json::Value>(&event.content).expect("json"))
        .filter(|envelope| envelope["session"]["generation"] == 2)
        .min_by_key(|envelope| envelope["eventSeq"].as_u64())
        .expect("generation 2 items");
    assert_eq!(first["item"]["status"], "session_rewound", "{first:#}");
    assert_eq!(first["item"]["files"], "restored");
    assert_eq!(first["item"]["memory"], "none");
    assert_eq!(first["item"]["commandId"], "rewind-1");
}

#[tokio::test]
async fn rewind_chat_only_keeps_the_files() {
    let mut rig = rig().await;
    rig.three_turns().await;
    let turn_two = rig.turn_checkpoint(2).id.to_hex();
    rig.rewind("rewind-1", &turn_two, "keep").await;
    let receipt = rig.receipt("rewind-1");
    assert_eq!(receipt["rewind"]["files"], "kept", "{receipt:#}");
    assert!(receipt["rewind"]["preRewindCheckpoint"].is_string());
    assert_eq!(rig.generation(), 2);
    rig.assert_after_turn_three();
}

#[tokio::test]
async fn rewind_refusals_touch_nothing_and_carry_no_rewind_object() {
    let mut rig = rig().await;
    rig.three_turns().await;
    let turn_two = rig.turn_checkpoint(2);

    // CHECKPOINT_UNAVAILABLE: an id nobody published.
    rig.rewind("rw-unknown", &"ef".repeat(32), "restore").await;
    // CHECKPOINT_NOT_THIS_EXECUTION: the same payload signed by a stranger.
    let stranger = beekeeper_sdk::coding_session_checkpoint::build_coding_session_checkpoint(
        rig.channel_id,
        &payload_of(&turn_two),
    )
    .expect("build")
    .sign_with_keys(&Keys::generate())
    .expect("sign");
    rig.provider
        .rewind_checkpoints
        .insert(stranger.id.to_hex(), stranger.clone());
    rig.rewind("rw-stranger", &stranger.id.to_hex(), "restore")
        .await;
    // NOT_RESTORABLE: this provider's own checkpoint saying it cannot be.
    let mut unrestorable = payload_of(&turn_two);
    unrestorable.restorable = false;
    let mine = beekeeper_sdk::coding_session_checkpoint::build_coding_session_checkpoint(
        rig.channel_id,
        &unrestorable,
    )
    .expect("build")
    .custom_created_at(nostr::Timestamp::from_secs(1))
    .sign_with_keys(&rig.provider.config.keys)
    .expect("sign");
    rig.provider
        .rewind_checkpoints
        .insert(mine.id.to_hex(), mine.clone());
    rig.rewind("rw-unrestorable", &mine.id.to_hex(), "restore")
        .await;

    for (command, code) in [
        ("rw-unknown", CHECKPOINT_UNAVAILABLE),
        ("rw-stranger", CHECKPOINT_NOT_THIS_EXECUTION),
        ("rw-unrestorable", NOT_RESTORABLE),
    ] {
        let receipt = rig.receipt(command);
        assert_eq!(receipt["status"], "failed", "{command}: {receipt:#}");
        assert_eq!(receipt["error"]["code"], code, "{command}: {receipt:#}");
        assert!(receipt.get("rewind").is_none(), "{command}");
    }
    assert_eq!(rig.generation(), 1);
    rig.assert_after_turn_three();
    assert!(
        !rig.checkpoint_events()
            .iter()
            .any(|event| payload_of(event).turn_id.is_none()),
        "a refused rewind publishes no pre_rewind checkpoint"
    );
}

#[tokio::test]
async fn rewind_is_refused_while_this_or_a_sibling_turn_is_open() {
    let mut rig = rig().await;
    rig.three_turns().await;
    let turn_two = rig.turn_checkpoint(2).id.to_hex();
    let open = || OpenTurn {
        turn_id: "turn-x".into(),
        command_id: Some("csc-x".into()),
        team_wake_eligible: false,
        started_at_ms: now_ms(),
        operator_pubkey: None,
    };
    rig.provider
        .state
        .update_session(&rig.target.session_id, |record| {
            record.open_turn = Some(open())
        })
        .expect("open");
    rig.rewind("rw-busy", &turn_two, "restore").await;
    rig.provider
        .state
        .update_session(&rig.target.session_id, |record| record.open_turn = None)
        .expect("close");

    // A second execution of this provider in the same working tree, mid-turn.
    rig.provider
        .handle_command_event(
            rig.channel_id,
            &create_event(&rig.provider, rig.channel_id, "create-2"),
        )
        .await
        .expect("second create");
    let sibling = rig
        .provider
        .state()
        .sessions()
        .find(|record| record.session_id != rig.target.session_id)
        .expect("sibling")
        .session_id
        .clone();
    rig.provider
        .state
        .update_session(&sibling, |record| record.open_turn = Some(open()))
        .expect("open sibling");
    rig.rewind("rw-tree", &turn_two, "restore").await;

    let busy = rig.receipt("rw-busy");
    assert_eq!(busy["error"]["code"], SESSION_BUSY, "{busy:#}");
    let tree = rig.receipt("rw-tree");
    assert_eq!(tree["error"]["code"], TREE_BUSY, "{tree:#}");
    let message = tree["error"]["message"].as_str().expect("message");
    assert!(message.contains(&sibling), "names the sibling: {message}");
    assert!(message.contains("this provider only"), "{message}");
    assert_eq!(rig.generation(), 1);
    rig.assert_after_turn_three();
}

#[tokio::test]
async fn rewind_that_cannot_open_the_new_generation_says_so_with_the_files_outcome() {
    let mut rig = rig().await;
    rig.three_turns().await;
    let turn_two = rig.turn_checkpoint(2).id.to_hex();
    rig.provider.rewind_fail_next_open = true;
    rig.rewind("rewind-1", &turn_two, "restore").await;
    let receipt = rig.receipt("rewind-1");
    assert_eq!(receipt["status"], "failed", "{receipt:#}");
    assert_eq!(receipt["error"]["code"], REWIND_NOT_RESTARTED);
    assert_eq!(receipt["session"], serde_json::Value::Null);
    assert_eq!(receipt["rewind"]["files"], "restored");
    let message = receipt["error"]["message"].as_str().expect("message");
    assert!(
        message.contains("still remembers") || message.contains("needs a Restart"),
        "{message}"
    );
    assert_eq!(
        rig.generation(),
        1,
        "generation 1 is detached, never truncated"
    );
    assert_eq!(
        rig.read("a.txt").as_deref(),
        Some("one\n"),
        "files restored"
    );
}

/// Step 1: the restart's authority. A key that may not steer the session is
/// refused before anything is read or touched.
#[tokio::test]
async fn rewind_from_a_stranger_is_refused_as_unauthorized() {
    let mut rig = rig().await;
    rig.three_turns().await;
    let content = serde_json::json!({
        "schema": "buzz-coding-session-lifecycle-command/v1",
        "commandId": "rw-stranger",
        "action": {
            "type": "session.rewind",
            "session": rig.target,
            "providerAuthorityPubkey": rig.provider.config.pubkey_hex(),
            "checkpoint": rig.turn_checkpoint(2).id.to_hex(),
            "files": "restore",
        },
    })
    .to_string();
    let event = signed_lifecycle_event_by(rig.channel_id, content, &Keys::generate());
    rig.provider
        .handle_command_event(rig.channel_id, &event)
        .await
        .expect("rewind");
    rig.flush().await;
    let receipt = rig.receipt("rw-stranger");
    assert_eq!(
        receipt["error"]["code"],
        payload::UNAUTHORIZED_OPERATOR,
        "{receipt:#}"
    );
    assert_eq!(rig.generation(), 1);
    rig.assert_after_turn_three();
}

/// A seated execution's rewind reads its seat under the **rewind's own**
/// command id, exactly like a restart: staged, the new generation opens under
/// the same seat; not staged, the open fails and the answer is
/// `REWIND_NOT_RESTARTED` — never a silent unseated generation.
#[tokio::test]
async fn a_seated_rewind_opens_under_the_seat_staged_for_the_rewind() {
    let actor = "cd".repeat(32);
    let mut rig = rig_for(Some(&actor)).await;
    let record = rig
        .provider
        .state()
        .session(&rig.target.session_id)
        .cloned()
        .expect("record");
    assert_eq!(record.actor.as_deref(), Some(actor.as_str()), "seated");
    rig.three_turns().await;
    let turn_two = rig.turn_checkpoint(2).id.to_hex();

    // Unstaged: the create's seat was consumed; nothing is under "rw-unstaged".
    rig.rewind("rw-unstaged", &turn_two, "keep").await;
    let unstaged = rig.receipt("rw-unstaged");
    assert_eq!(unstaged["status"], "failed", "{unstaged:#}");
    assert_eq!(
        unstaged["error"]["code"], REWIND_NOT_RESTARTED,
        "{unstaged:#}"
    );
    assert_eq!(unstaged["rewind"]["files"], "kept", "{unstaged:#}");

    // Staged under the rewind's own command id, as the desktop does — on a
    // fresh execution, so nothing the refused attempt left behind is read.
    let mut rig = rig_for(Some(&actor)).await;
    rig.three_turns().await;
    let turn_two = rig.turn_checkpoint(2).id.to_hex();
    let seats = write_actor_seats(rig._dir.path(), "rw-staged", &actor);
    let generation = rig.generation();
    rig.rewind("rw-staged", &turn_two, "keep").await;
    let staged = rig.receipt("rw-staged");
    assert_ne!(staged["status"], "failed", "{staged:#}");
    assert_eq!(staged["rewind"]["files"], "kept", "{staged:#}");
    assert_eq!(rig.generation(), generation + 1);
    let record = rig
        .provider
        .state()
        .session(&rig.target.session_id)
        .cloned()
        .expect("record");
    assert_eq!(
        record.actor.as_deref(),
        Some(actor.as_str()),
        "the new generation runs under the same seat"
    );
    let body = std::fs::read_to_string(&seats).expect("read seats");
    assert!(
        !body.contains("rw-staged"),
        "the rewind's seat was consumed: {body}"
    );
}

/// A turn replaced the base's file `notes` with a directory holding an
/// ignored file. A files rewind would write `notes` over the directory and
/// take `notes/x.log` with it — no capture holds it — so it is refused by
/// name and nothing is touched.
#[tokio::test]
async fn rewind_restore_refuses_to_replace_a_directory_that_was_a_file() {
    let mut rig = rig().await;
    std::fs::write(rig.cwd.join("notes"), "a file before the turns\n").expect("notes");
    rig.three_turns().await;
    std::fs::remove_file(rig.cwd.join("notes")).expect("rm notes");
    std::fs::create_dir(rig.cwd.join("notes")).expect("dir");
    std::fs::write(rig.cwd.join("notes/x.log"), "ignored, in no capture\n").expect("x.log");
    let turn_two = rig.turn_checkpoint(2).id.to_hex();

    rig.rewind("rw-dir", &turn_two, "restore").await;

    let receipt = rig.receipt("rw-dir");
    assert_eq!(receipt["status"], "failed", "{receipt:#}");
    assert_eq!(receipt["error"]["code"], NOT_RESTORABLE, "{receipt:#}");
    assert!(receipt.get("rewind").is_none(), "{receipt:#}");
    let message = receipt["error"]["message"].as_str().expect("message");
    assert!(message.contains("\"notes\""), "names the path: {message}");
    assert_eq!(
        rig.read("notes/x.log").as_deref(),
        Some("ignored, in no capture\n"),
        "the ignored file survives"
    );
    assert_eq!(rig.generation(), 1);
    rig.assert_after_turn_three();

    // Chat only is unaffected.
    rig.rewind("rw-dir-keep", &turn_two, "keep").await;
    assert_eq!(rig.receipt("rw-dir-keep")["rewind"]["files"], "kept");
    assert!(rig.cwd.join("notes/x.log").exists());
}

fn case_insensitive(dir: &Path) -> bool {
    let probe = dir.join("CaseProbe");
    std::fs::write(&probe, "x").expect("probe");
    let folded = dir.join("caseprobe").exists();
    std::fs::remove_file(probe).expect("rm probe");
    folded
}

/// On a case-insensitive filesystem a turn's rename of an untracked
/// `Notes.txt` → `NOTES.txt` lists `NOTES.txt` as added. Unlinking it after
/// `Notes.txt` is written back would delete the restored file under a
/// `restored` receipt. A tracked `Readme.md` renamed with `git mv` rides along.
#[tokio::test]
async fn rewind_restore_never_unlinks_a_restored_file_under_another_case() {
    let mut rig = rig().await;
    if !case_insensitive(&rig.cwd) {
        eprintln!("skipped: this filesystem is case-sensitive, so the rename is two files");
        return;
    }
    std::fs::write(rig.cwd.join("Readme.md"), "the base readme\n").expect("readme");
    git(&rig.cwd, &["add", "Readme.md"]);
    git(&rig.cwd, &["commit", "-q", "--no-gpg-sign", "-m", "readme"]);
    std::fs::write(rig.cwd.join("Notes.txt"), "untracked notes\n").expect("notes");
    rig.three_turns().await;
    // The turn's rename, staged as `git mv` stages it, and an untracked
    // file renamed the same way.
    git(&rig.cwd, &["mv", "Readme.md", "README.md"]);
    std::fs::write(rig.cwd.join("README.md"), "renamed and changed\n").expect("change");
    std::fs::rename(rig.cwd.join("Notes.txt"), rig.cwd.join("NOTES.txt")).expect("rename");
    let turn_two = rig.turn_checkpoint(2).id.to_hex();

    rig.rewind("rw-case", &turn_two, "restore").await;

    let receipt = rig.receipt("rw-case");
    assert_eq!(receipt["rewind"]["files"], "restored", "{receipt:#}");
    let names: Vec<String> = std::fs::read_dir(&rig.cwd)
        .expect("ls")
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.eq_ignore_ascii_case("readme.md"))
        .collect();
    assert_eq!(names, vec!["Readme.md".to_owned()], "the base spelling");
    assert_eq!(rig.read("Readme.md").as_deref(), Some("the base readme\n"));
    assert!(
        std::fs::read_dir(&rig.cwd)
            .expect("ls")
            .filter_map(Result::ok)
            .any(|entry| entry.file_name() == "Notes.txt"),
        "the untracked file keeps its base spelling"
    );
    assert_eq!(rig.read("Notes.txt").as_deref(), Some("untracked notes\n"));
}

/// Reconnect replay redelivers a rewind still running off the loop. The
/// redelivery is that rewind: it gets no receipt of its own and leaves the
/// seat staged for it, and the rewind answers once.
#[tokio::test]
async fn a_redelivered_rewind_in_flight_is_dropped_without_an_answer() {
    let actor = "cd".repeat(32);
    let mut rig = rig_for(Some(&actor)).await;
    rig.three_turns().await;
    let turn_two = rig.turn_checkpoint(2).id.to_hex();
    write_actor_seats(rig._dir.path(), "rw-dup", &actor);
    let generation = rig.generation();
    rig.provider.rewind_off_loop = true;
    let mut answers = rig.provider.take_rewind_answers().expect("answers");
    let event = rig.rewind_event("rw-dup", &turn_two, "restore");
    rig.provider
        .handle_command_event(rig.channel_id, &event)
        .await
        .expect("rewind");
    // Once while the checkpoint is verified, once while the files are
    // restored.
    for _ in 0..2 {
        rig.provider
            .handle_command_event(rig.channel_id, &event)
            .await
            .expect("redelivery");
        let done = tokio::time::timeout(Duration::from_secs(60), answers.recv())
            .await
            .expect("a step within the timeout")
            .expect("open");
        rig.provider
            .finish_rewind_step(done, None)
            .await
            .expect("step");
    }
    rig.flush().await;
    assert_eq!(rig.receipts("rw-dup"), 1, "exactly one answer");
    let receipt = rig.receipt("rw-dup");
    assert_ne!(receipt["status"], "failed", "{receipt:#}");
    assert_eq!(receipt["rewind"]["files"], "restored", "{receipt:#}");
    assert_eq!(rig.generation(), generation + 1);
    let record = rig
        .provider
        .state()
        .session(&rig.target.session_id)
        .cloned()
        .expect("record");
    assert_eq!(
        record.actor.as_deref(),
        Some(actor.as_str()),
        "the seat staged for the rewind was not forgotten"
    );
}
