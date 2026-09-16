//! The provider's verification-input fence (ledger 133).
//!
//! A verifier or runner turn opens only when the seat's checkout holds the
//! commit its assignment names, cleanly. These tests pin the two halves that
//! can go wrong independently: the wiring in
//! [`crate::Provider::verification_input_refusal`] — which turns it fences,
//! and that an unanswerable question refuses — and the decision in
//! [`crate::verification_input::check_seat_tree`] against a real throwaway
//! repository, so the three reasons are measured rather than asserted.

use super::*;

use crate::verification_input::{
    check_seat_tree, SeatTree, TurnInput, VERIFICATION_INPUT_NOT_PRESENT,
    VERIFICATION_INPUT_TREE_DIRTY, VERIFICATION_INPUT_UNNAMED, VERIFICATION_INPUT_UNOBSERVED,
    VERIFICATION_INPUT_UNVERIFIED,
};

/// The pointer both producers mint for an assignment wake.
fn assignment_pointer(operation_id: &str) -> String {
    serde_json::json!({"operationId": operation_id, "type": "assignment"}).to_string()
}

/// An operation id shaped like the event id one would be.
fn operation_id() -> String {
    "ab".repeat(32)
}

/// A seat record this provider owns, in `cwd`, holding `role`.
///
/// Inserted directly: what is under test is the fence, not session startup,
/// and a live agent behind the record would decide nothing here.
fn seat_record(channel_id: Uuid, cwd: &Path, role: Option<&str>) -> SessionRecord {
    let mut record = governed_record(channel_id, cwd, &"cd".repeat(32));
    record.actor = role.map(|_| "ef".repeat(32));
    record.role = role.map(str::to_owned);
    record
}

/// `git` in `cwd` with the repository-selection variables cleared, so a test
/// running under a hook cannot target the developer's own repository.
fn git(cwd: &Path, args: &[&str]) {
    let mut command = std::process::Command::new("git");
    for var in crate::git_probe::GIT_REPO_SELECTION_VARS {
        command.env_remove(var);
    }
    let status = command
        .arg("-C")
        .arg(cwd)
        .args([
            "-c",
            "user.email=input@example.invalid",
            "-c",
            "user.name=input",
        ])
        .args(args)
        .status()
        .expect("git");
    assert!(status.success(), "git {args:?} failed");
}

/// A throwaway repository with one commit, and that commit's object id.
fn repo_with_commit(cwd: &Path) -> String {
    git(cwd, &["init", "-q", "-b", "trunk", "."]);
    std::fs::write(cwd.join("a.txt"), "a").expect("write");
    git(cwd, &["add", "a.txt"]);
    git(cwd, &["commit", "-q", "--no-gpg-sign", "-m", "one"]);
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(cwd)
        .args(["rev-parse", "HEAD"])
        .output()
        .expect("rev-parse");
    String::from_utf8(out.stdout)
        .expect("utf8")
        .trim()
        .to_owned()
}

/// The fence is on for a verifier, and a relay that cannot answer leaves the
/// turn **undecided** rather than spending it. Unknown is not false: a wake
/// that queued minutes behind a busy seat must not be consumed because one
/// query failed.
#[tokio::test]
async fn a_verifier_assignment_turn_is_undecided_when_the_relay_cannot_answer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let record = seat_record(Uuid::new_v4(), dir.path(), Some("verifier"));
    let target = provider.target_for(&record);
    provider
        .state
        .insert_session(record)
        .expect("insert the seat record");

    assert_eq!(
        provider
            .verification_input_outcome(&target, "wake-1", &assignment_pointer(&operation_id()))
            .await,
        TurnInput::Undecided("relay_query_unavailable")
    );
}

/// A runner is fenced for the same reason a verifier is: its result is a claim
/// about what a particular tree did when the acceptance commands ran.
#[tokio::test]
async fn a_runner_assignment_turn_is_fenced_too() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let record = seat_record(Uuid::new_v4(), dir.path(), Some("runner"));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).expect("insert");

    assert_eq!(
        provider
            .verification_input_outcome(&target, "wake-1", &assignment_pointer(&operation_id()))
            .await,
        TurnInput::Undecided("relay_query_unavailable"),
        "a runner assignment is fenced, and undecidable here for the same reason"
    );
}

/// A builder's assignment says what to build, not what to judge, so it opens
/// exactly as it did before this fence existed — including with no relay.
#[tokio::test]
async fn a_builder_assignment_turn_is_untouched() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let record = seat_record(Uuid::new_v4(), dir.path(), Some("builder"));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).expect("insert");

    assert_eq!(
        provider
            .verification_input_outcome(&target, "wake-1", &assignment_pointer(&operation_id()))
            .await,
        TurnInput::Open
    );
}

/// Prose, READY and anything else that is not the two-key pointer never reach
/// the relay path at all — which is what keeps this fence off every ordinary
/// turn a seat takes.
#[tokio::test]
async fn ordinary_prose_to_a_verifier_never_touches_the_fence() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let record = seat_record(Uuid::new_v4(), dir.path(), Some("verifier"));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).expect("insert");

    for text in [
        "Reply READY when initialized.",
        "{\"operationId\":\"not-hex\",\"type\":\"assignment\"}",
        "{\"operationId\":\"abababababababababababababababababababababababababababababababab\",\"type\":\"report\"}",
        "{\"operationId\":\"abababababababababababababababababababababababababababababababab\",\"type\":\"assignment\",\"extra\":1}",
    ] {
        assert_eq!(
            provider
                .verification_input_outcome(&target, "wake-1", text)
                .await,
            TurnInput::Open,
            "must not fence {text}"
        );
    }
}

/// A command for a generation this record no longer is belongs to the ordinary
/// staleness refusal. Fencing it here would answer it twice, under a code that
/// describes the wrong problem.
#[tokio::test]
async fn a_superseded_generation_is_left_to_the_staleness_refusal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let record = seat_record(Uuid::new_v4(), dir.path(), Some("verifier"));
    let mut stale = provider.target_for(&record);
    stale.generation = record.generation + 1;
    provider.state.insert_session(record).expect("insert");

    assert_eq!(
        provider
            .verification_input_outcome(&stale, "wake-1", &assignment_pointer(&operation_id()))
            .await,
        TurnInput::Open
    );
}

/// The accepting case, against a real repository: HEAD is the named commit and
/// nothing is uncommitted, so the turn opens and the established commit comes
/// back for the caller to record.
#[tokio::test]
async fn a_clean_checkout_at_the_named_commit_is_established() {
    let dir = tempfile::tempdir().expect("tempdir");
    let head = repo_with_commit(dir.path());
    let tree = crate::git_probe::probe_verification_input(dir.path())
        .await
        .expect("observed");
    assert_eq!(
        check_seat_tree(&operation_id(), Some(&head), &tree),
        Ok(head.clone())
    );
    // Case is not identity: the same commit named in upper case is the same
    // commit.
    assert_eq!(
        check_seat_tree(&operation_id(), Some(&head.to_ascii_uppercase()), &tree),
        Ok(head)
    );
}

/// An assignment that names no commit is refused by name rather than run
/// against whatever the seat happens to hold — the commit-less verifier
/// assignment that is storable today, since the relay does not validate 44244
/// content at ingest.
#[tokio::test]
async fn an_assignment_without_a_base_sha_refuses() {
    let dir = tempfile::tempdir().expect("tempdir");
    repo_with_commit(dir.path());
    let tree = crate::git_probe::probe_verification_input(dir.path())
        .await
        .expect("observed");
    let refusal = check_seat_tree(&operation_id(), None, &tree).expect_err("must refuse");
    assert_eq!(refusal.code, VERIFICATION_INPUT_UNNAMED);
    assert!(
        refusal.message.contains(&operation_id()) && refusal.message.contains("baseSha"),
        "the refusal names the assignment and the missing fact; got {}",
        refusal.message
    );
}

/// The seat is at a different commit: both ids are reported, because "not that
/// one" without saying which one it is leaves the lead to guess.
#[tokio::test]
async fn a_checkout_at_another_commit_refuses_naming_both() {
    let dir = tempfile::tempdir().expect("tempdir");
    let first = repo_with_commit(dir.path());
    std::fs::write(dir.path().join("b.txt"), "b").expect("write");
    git(dir.path(), &["add", "b.txt"]);
    git(dir.path(), &["commit", "-q", "--no-gpg-sign", "-m", "two"]);
    let tree = crate::git_probe::probe_verification_input(dir.path())
        .await
        .expect("observed");
    let head = tree.head.clone().expect("head");
    assert_ne!(head, first);

    let refusal = check_seat_tree(&operation_id(), Some(&first), &tree).expect_err("must refuse");
    assert_eq!(refusal.code, VERIFICATION_INPUT_NOT_PRESENT);
    assert!(
        refusal.message.contains(&head) && refusal.message.contains(&first),
        "both commits must appear; got {}",
        refusal.message
    );
}

/// HEAD matches but the tree does not: a verdict here would be about an
/// unpublished tree. The count is in the sentence, so the lead knows whether
/// this is one stray file or a whole unfinished change.
#[tokio::test]
async fn a_dirty_tree_refuses_with_the_line_count() {
    let dir = tempfile::tempdir().expect("tempdir");
    let head = repo_with_commit(dir.path());
    std::fs::write(dir.path().join("a.txt"), "changed").expect("write");
    std::fs::write(dir.path().join("untracked.txt"), "new").expect("write");
    let tree = crate::git_probe::probe_verification_input(dir.path())
        .await
        .expect("observed");
    let refusal = check_seat_tree(&operation_id(), Some(&head), &tree).expect_err("must refuse");
    assert_eq!(refusal.code, VERIFICATION_INPUT_TREE_DIRTY);
    assert!(
        refusal.message.contains("2 uncommitted changes"),
        "the refusal counts the changes; got {}",
        refusal.message
    );
}

/// A checkout that cannot be read is unknown, never clean. Both halves of the
/// observation are refused independently, because a tree whose HEAD matched
/// and whose dirty state was unreadable is exactly as unverified.
#[tokio::test]
async fn an_unreadable_checkout_refuses_rather_than_passing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let plain = crate::git_probe::probe_verification_input(dir.path())
        .await
        .expect("observed");
    assert_eq!(plain, SeatTree::default());
    assert_eq!(
        check_seat_tree(&operation_id(), Some(&"ab".repeat(20)), &plain)
            .expect_err("must refuse")
            .code,
        VERIFICATION_INPUT_UNOBSERVED
    );

    let head_only = SeatTree {
        head: Some("ab".repeat(20)),
        dirty_lines: None,
    };
    assert_eq!(
        check_seat_tree(&operation_id(), Some(&"ab".repeat(20)), &head_only)
            .expect_err("must refuse")
            .code,
        VERIFICATION_INPUT_UNOBSERVED
    );
}

/// One 44220 `thread.turn.start` body addressed at `target`.
fn turn_command_content(command_id: &str, target: &CodingSessionTarget, text: &str) -> String {
    serde_json::json!({
        "schema": "buzz-coding-session-command/v1",
        "commandId": command_id,
        "target": target,
        "action": { "type": "thread.turn.start", "text": text, "deliver": "boundary" },
    })
    .to_string()
}

/// The remedy this fence names has to work, and only one shape of it does.
///
/// A refusal is recorded durably before it is published, and an assignment's
/// wake reuses the assignment's own `deliveryCommandId`, so re-sending *this*
/// assignment's wake is ignored — a hang with a polite message if the refusal
/// had told the lead to do that. A **second** assignment, for the same seat,
/// the same role and the same commit, carries its own operation id and its own
/// delivery command, and is admitted: neither the refusal ledger nor the
/// operation fence in `decide_turn` has anything to match it against.
#[tokio::test]
async fn a_re_issued_assignment_is_admitted_after_a_refusal() {
    let dir = tempfile::tempdir().expect("tempdir");
    let mut provider = provider(&dir.path().join("state"), None);
    let channel_id = Uuid::new_v4();
    let record = seat_record(channel_id, dir.path(), Some("verifier"));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).expect("insert");

    let first = "ab".repeat(32);
    let second = "cd".repeat(32);
    let operator = test_operator_keys().public_key().to_hex();
    let projects = ProjectsFile::default();
    let seats = crate::actor_seats::ActorSeatsFile::default();
    let decide = |provider: &Provider, command_id: &str, operation_id: &str| {
        let context = provider.context(channel_id, &projects, &seats, &operator);
        commands::decide_turn(
            &context,
            now_secs(),
            &turn_command_content(command_id, &target, &assignment_pointer(operation_id)),
        )
    };

    // The first assignment is admitted as far as the fence, which is where it
    // is refused; `apply_turn_decision` records that refusal durably.
    assert!(matches!(
        decide(&provider, "wake-first", &first),
        TurnDecision::Start { .. }
    ));
    provider
        .state
        .record_refusal("wake-first", now_secs())
        .expect("record the fence's refusal");

    // Re-sending the same assignment's wake is ignored, which is why the
    // refusal says "re-issue" rather than "wake the seat again".
    assert!(matches!(
        decide(&provider, "wake-first", &first),
        TurnDecision::Ignore(_)
    ));

    // The re-issued assignment opens: a different operation, a different
    // delivery command, the same seat, role and commit.
    assert!(
        matches!(
            decide(&provider, "wake-second", &second),
            TurnDecision::Start { .. }
        ),
        "a re-issued assignment must not be refused as a duplicate"
    );

    // And the fence itself keeps no memory of the first refusal: with the tree
    // now at the named commit and clean, the second assignment establishes.
    let checkout = dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("mkdir");
    let head = repo_with_commit(&checkout);
    let tree = crate::git_probe::probe_verification_input(&checkout)
        .await
        .expect("observed");
    assert_eq!(check_seat_tree(&second, Some(&head), &tree), Ok(head));
}

/// Every line of an append-only ledger under the state directory.
fn ledger_lines(state_dir: &Path, file: &str) -> Vec<String> {
    std::fs::read_to_string(state_dir.join(file))
        .unwrap_or_default()
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(str::to_owned)
        .collect()
}

/// A relay that cannot answer must cost the assignment nothing.
///
/// The turn is left undecided: no receipt, no refusal ledger entry, no
/// consumption, and — because `handle_command_event` skips the watermark write
/// for this disposition — the same command is re-read on a later pass. So the
/// *same* `commandId` is still decidable afterwards, which the second
/// `decide_turn` proves; when the snapshot and the tree do answer, the fence
/// establishes and the turn opens, which `check_seat_tree` proves here (the
/// provider's own `Open` arm falls straight through to delivery, and driving a
/// full relay snapshot belongs to the live rig rather than a unit test).
#[tokio::test]
async fn an_undecided_wake_stays_decidable_under_the_same_command_id() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state_dir = dir.path().join("state");
    let checkout = dir.path().join("checkout");
    std::fs::create_dir_all(&checkout).expect("mkdir");
    let head = repo_with_commit(&checkout);
    let mut provider = provider(&state_dir, None);
    let channel_id = Uuid::new_v4();
    let record = seat_record(channel_id, &checkout, Some("verifier"));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).expect("insert");

    let operation = "ab".repeat(32);
    let operator = test_operator_keys().public_key().to_hex();
    let projects = ProjectsFile::default();
    let seats = crate::actor_seats::ActorSeatsFile::default();
    let content = turn_command_content("wake-1", &target, &assignment_pointer(&operation));
    let decide = |provider: &Provider| {
        let context = provider.context(channel_id, &projects, &seats, &operator);
        commands::decide_turn(&context, now_secs(), &content)
    };

    let decision = decide(&provider);
    assert!(matches!(decision, TurnDecision::Start { .. }));
    let disposition = provider
        .apply_turn_decision(channel_id, now_secs(), &operator, "event-1", decision)
        .await
        .expect("apply");
    assert_eq!(disposition, TurnDisposition::Undecided);

    assert!(
        !provider.state.is_command_refused("wake-1"),
        "a relay failure must not answer the command"
    );
    assert!(
        !provider.state.is_command_consumed("wake-1"),
        "a relay failure must not spend the command"
    );
    assert!(
        ledger_lines(&state_dir, "refusals.jsonl").is_empty(),
        "nothing durable is written for an undecided turn"
    );
    assert!(
        !ledger_lines(&state_dir, "outbox.jsonl")
            .iter()
            .any(|line| line.contains("wake-1")),
        "nothing terminal is published for an undecided turn"
    );

    // A later pass sees the same command as decidable, not as answered.
    assert!(
        matches!(decide(&provider), TurnDecision::Start { .. }),
        "the same wake must be decidable on the next pass"
    );
    // And once the facts do arrive, the seat's tree establishes the input.
    let tree = crate::git_probe::probe_verification_input(&checkout)
        .await
        .expect("observed");
    assert_eq!(check_seat_tree(&operation, Some(&head), &tree), Ok(head));

    // The control, so the two assertions above are about this fence rather
    // than about an empty state directory: a seat whose record cannot bind an
    // assignment to anybody is a fact, and a fact *does* answer durably and
    // reach the outbox.
    let mut unbindable = seat_record(channel_id, &checkout, Some("verifier"));
    unbindable.actor = None;
    let unbindable_target = provider.target_for(&unbindable);
    provider.state.insert_session(unbindable).expect("insert");
    let content = turn_command_content(
        "wake-2",
        &unbindable_target,
        &assignment_pointer(&operation),
    );
    let context = provider.context(channel_id, &projects, &seats, &operator);
    let decision = commands::decide_turn(&context, now_secs(), &content);
    let disposition = provider
        .apply_turn_decision(channel_id, now_secs(), &operator, "event-2", decision)
        .await
        .expect("apply");
    assert_eq!(
        disposition,
        TurnDisposition::Answered(VERIFICATION_INPUT_UNVERIFIED.to_owned())
    );
    assert!(provider.state.is_command_refused("wake-2"));
    assert!(
        ledger_lines(&state_dir, "outbox.jsonl")
            .iter()
            .any(|line| line.contains("wake-2")),
        "a durable refusal reaches the outbox this test just found empty for wake-1"
    );
}
