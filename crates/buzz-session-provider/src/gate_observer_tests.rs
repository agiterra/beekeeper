//! What the provider will say it watched, and what it refuses to say.
//!
//! Every test here is pure: the observer reads published transcript items and
//! nothing else, which is the point — a record that needed a relay or a
//! cooperating agent to produce would be the weak link the field exists to
//! remove.

use serde_json::json;

use super::*;

fn tool_call(tool_id: &str, command: &str) -> Value {
    json!({
        "kind": "tool_call",
        "tool": {
            "toolName": "Bash",
            "toolId": tool_id,
            "input": { "command": command },
        },
    })
}

fn tool_result(tool_id: &str, is_error: bool, content: &str) -> Value {
    json!({
        "kind": "tool_result",
        "toolId": tool_id,
        "toolName": "Bash",
        "content": content,
        "isError": is_error,
    })
}

#[test]
fn a_failed_cargo_test_becomes_a_failed_row_with_the_command_verbatim() {
    // Live-run finding 26, on the wire this time: the seat said green, the
    // verifier reproduced red. Nobody had to ask the seat for either.
    let mut observer = GateObserver::default();
    assert!(observer
        .on_item(
            &tool_call("t1", "cargo test -p buzz-cli subcommand_"),
            1_000
        )
        .is_none());
    let observed = observer
        .on_item(
            &tool_result(
                "t1",
                true,
                "running 2 tests\ntest result: FAILED. 0 passed; 2 failed; 0 ignored",
            ),
            4_000,
        )
        .expect("the result closes the call it opened");

    assert_eq!(observed.row.gate, "cargo test");
    assert_eq!(
        observed.row.outcome,
        CodingSessionObservationGateOutcome::Failed
    );
    assert_eq!(
        observed.row.command, "cargo test -p buzz-cli subcommand_",
        "the command is the seat's own, byte for byte"
    );
    assert_eq!(
        observed.row.summary.as_deref(),
        Some("test result: FAILED. 0 passed; 2 failed; 0 ignored")
    );
    assert_eq!(observed.row.duration_ms, Some(3_000));
}

/// REVIEW-L5 **F1**, every line the reviewer reproduced.
///
/// The old matcher asked whether the gate words appeared *anywhere* in the
/// line, so `grep -rn 'cargo test' docs/` and `echo "cargo test"` each minted a
/// provider-signed `observed` **passed** row — and because the fold keys gates
/// on `(author, source, gate)`, that false green then displaced a genuine
/// `failed` row as the one both surfaces show. A seat could bury the exact
/// failure this record exists to expose.
///
/// The command is now parsed as **argv** and matched at the head of a command
/// segment, program then subcommand.
#[test]
fn a_gate_is_recognised_only_in_command_position_never_inside_an_argument() {
    for (command, gate) in [
        ("cargo fmt --check", "cargo fmt"),
        ("cargo clippy --all-targets -- -D warnings", "cargo clippy"),
        ("cargo test -p buzz-core", "cargo test"),
        ("cargo test --test whoami", "cargo test"),
        // Whitespace is normalised by the split, so one gate is one gate
        // however it is typed.
        ("cargo   test    -p buzz-cli", "cargo test"),
        // The only prefixes a gate may hide behind: a directory change, and
        // the two wrappers a seat legitimately uses to run one.
        ("cd desktop && pnpm typecheck", "pnpm typecheck"),
        ("cd desktop && pnpm test", "pnpm test"),
        ("nice -n 10 cargo test -p buzz-core", "cargo test"),
        ("env RUST_BACKTRACE=1 cargo test", "cargo test"),
        ("cd desktop && nice -n 10 pnpm lint", "pnpm lint"),
        ("just ci", "just ci"),
        ("just check", "just check"),
        ("just test", "just test"),
    ] {
        assert_eq!(match_gate(command), Some(gate), "{command}");
    }

    // Every line REVIEW-L5 F1 reproduced as a false positive, plus the ones
    // the old test listed and then skipped.
    for command in [
        r#"echo "cargo test""#,
        "echo 'running cargo test now' > /tmp/x",
        "pnpm add @testing-library/react",
        "pnpm run latest",
        "# pnpm test",
        "cargo test -p buzz-core || true",
        "grep -rn 'cargo test' docs/",
        "git commit -m 'cargo test green'",
        "cargo build",
        "pnpm run build",
        "echo 'cargo test'  is a gate",
        // Shell composition is refused outright rather than labelled: the
        // matcher cannot say which segment produced the exit it is about to
        // read, so it declines to name any of them.
        "cargo test -p buzz-core && echo done",
        "cargo test | tee out.txt",
        "cargo test; cargo clippy",
        "$(cargo test)",
        "cargo test > out.txt",
        "cargo testify",
        "cargotest",
        "mycargo test",
    ] {
        assert_eq!(match_gate(command), None, "{command}");
    }
}

/// `cd … && <gate>` is the one composition allowed, and only in that shape.
#[test]
fn only_a_leading_directory_change_may_precede_a_gate() {
    assert_eq!(match_gate("cd desktop && pnpm test"), Some("pnpm test"));
    // Anything after the gate is still composition, and still refused.
    assert_eq!(match_gate("cd desktop && pnpm test && echo ok"), None);
    // A `cd` that leads somewhere other than a gate is not a gate.
    assert_eq!(match_gate("cd desktop && echo hi"), None);
    // `&&` without a leading `cd` is refused: only the directory change is
    // exempt, because it cannot itself produce an exit status a row would read.
    assert_eq!(match_gate("true && cargo test"), None);
}

#[test]
fn a_result_that_never_says_whether_it_failed_is_not_a_pass() {
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("t1", "cargo fmt --check"), 0);
    let ambiguous = json!({
        "kind": "tool_result",
        "toolId": "t1",
        "content": "",
    });
    assert!(
        observer.on_item(&ambiguous, 10).is_none(),
        "silence about an outcome is not evidence of a good one"
    );
}

#[test]
fn a_call_that_ran_no_gate_is_never_remembered() {
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("t1", "git status --porcelain"), 0);
    assert!(observer.pending.is_empty());
    assert!(observer
        .on_item(&tool_result("t1", false, "clean"), 10)
        .is_none());
}

#[test]
fn the_pending_window_is_bounded_and_forgets_the_oldest_first() {
    let mut observer = GateObserver::default();
    for index in 0..(MAX_PENDING_GATE_CALLS + 4) {
        observer.on_item(
            &tool_call(&format!("t{index}"), "cargo test -p buzz-core"),
            0,
        );
    }
    assert_eq!(observer.pending.len(), MAX_PENDING_GATE_CALLS);
    assert!(
        observer
            .on_item(&tool_result("t0", false, "ok"), 1)
            .is_none(),
        "a call pushed out of the window produces no row rather than a guessed one"
    );
    assert!(observer
        .on_item(&tool_result("t35", false, "ok"), 1)
        .is_some());
}

#[test]
fn a_command_longer_than_the_wire_allows_is_dropped_rather_than_shortened() {
    let long = format!("cargo test -p buzz-core {}", "x".repeat(600));
    assert!(long.len() > MAX_OBSERVATION_COMMAND_BYTES);
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("t1", &long), 0);
    assert!(
        observer.pending.is_empty(),
        "a shortened command line is a different command"
    );
}

#[test]
fn interleaved_calls_close_against_their_own_ids() {
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("a", "cargo fmt --check"), 0);
    observer.on_item(&tool_call("b", "cargo clippy --all-targets"), 100);
    let clippy = observer
        .on_item(&tool_result("b", true, "error: unused variable"), 900)
        .expect("clippy closes first");
    assert_eq!(clippy.row.gate, "cargo clippy");
    assert_eq!(clippy.row.duration_ms, Some(800));
    let fmt = observer
        .on_item(&tool_result("a", false, ""), 1_000)
        .expect("fmt closes second");
    // The gate name is program + subcommand; the *flags* live in `command`,
    // verbatim, so `--check` is never lost — it is simply not part of the name.
    assert_eq!(fmt.row.gate, "cargo fmt");
    assert_eq!(
        fmt.row.summary, None,
        "no output is null, never an empty summary"
    );
}
