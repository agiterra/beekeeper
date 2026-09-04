//! What the provider will say it watched, and what it refuses to say.
//!
//! Every test here is pure: the observer reads published transcript items and
//! nothing else, which is the point — a record that needed a relay or a
//! cooperating agent to produce would be the weak link the field exists to
//! remove.

use serde_json::json;

use super::*;

/// The `buzz-agent`/L5 harness shape: the command sits on the call itself.
/// `toolKind: "execute"` is what makes a call eligible to be remembered at
/// all now (finding 69) — every real exec call carries it (`transcript.rs`),
/// so every fixture in this module does too.
fn tool_call(tool_id: &str, command: &str) -> Value {
    json!({
        "kind": "tool_call",
        "tool": {
            "toolName": "Bash",
            "toolId": tool_id,
            "toolKind": "execute",
            "input": { "command": command },
        },
    })
}

fn tool_result(tool_id: &str, is_error: bool, content: &str) -> Value {
    json!({
        "kind": "tool_result",
        "toolId": tool_id,
        "toolName": "Bash",
        "toolKind": "execute",
        "content": content,
        "isError": is_error,
    })
}

/// The single gate a composed line resolves to, if it resolves to exactly
/// one — the shape most existing `match_gate` assertions want, kept as a
/// test-only convenience so they read the same as before finding 57 widened
/// [`match_gate_segments`] to return more than one.
fn single_gate(command: &str) -> Option<&'static str> {
    match match_gate_segments(command)?.as_slice() {
        [segment] => Some(segment.gate),
        _ => None,
    }
}

/// Every gate name a composed line resolves to, in order — `None` when the
/// line is refused outright.
fn gate_names(command: &str) -> Option<Vec<&'static str>> {
    Some(
        match_gate_segments(command)?
            .into_iter()
            .map(|segment| segment.gate)
            .collect(),
    )
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
        .is_empty());
    let observed = observer
        .on_item(
            &tool_result(
                "t1",
                true,
                "running 2 tests\ntest result: FAILED. 0 passed; 2 failed; 0 ignored",
            ),
            4_000,
        )
        .pop()
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
        ("nice -n 10 cargo test -p buzz-core", "cargo test"),
        ("env RUST_BACKTRACE=1 cargo test", "cargo test"),
        ("just ci", "just ci"),
        ("just check", "just check"),
        ("just test", "just test"),
        // A `cd`/`echo` wrapper anywhere on the line — before or after the
        // gate — is finding 57: these used to be refused as "composition"
        // and are now the whole point.
        ("cd desktop && pnpm typecheck", "pnpm typecheck"),
        ("cd desktop && pnpm test", "pnpm test"),
        ("cd desktop && nice -n 10 pnpm lint", "pnpm lint"),
        ("cd desktop && pnpm test && echo ok", "pnpm test"),
        ("cargo test -p buzz-core && echo done", "cargo test"),
        (r#"echo "== fmt =="; cargo fmt --all --check"#, "cargo fmt"),
    ] {
        assert_eq!(single_gate(command), Some(gate), "{command}");
    }

    // Every line REVIEW-L5 F1 reproduced as a false positive, plus the ones
    // the old test listed and then skipped. None of these is a `cd`/`echo`
    // wrapper around a gate, so finding 57 does not touch them.
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
        // Still refused outright: a pipe, a redirect, a subshell, a
        // background `&` — none of them is a segment this seam can classify
        // as a gate or a wrapper, so the whole line is refused rather than
        // partly labelled.
        "cargo test | tee out.txt",
        "$(cargo test)",
        "cargo test > out.txt",
        "cargo test &",
        "cargo testify",
        "cargotest",
        "mycargo test",
        // A `&&`/`;` line is still refused when a segment is neither a gate
        // nor a wrapper — `true` and `cd`-to-nowhere-useful are not exempt.
        "true && cargo test",
        "cd desktop && echo hi",
    ] {
        assert_eq!(single_gate(command), None, "{command}");
    }
}

/// Finding 57: a `&&`/`;`-joined line resolves to *every* recognised gate on
/// it, not just one — `single_gate` (the test helper above) only answers
/// `Some` for the one-gate case, so this test reaches for
/// [`match_gate_segments`] directly via `gate_names`.
#[test]
fn a_composed_line_names_every_gate_segment_in_order() {
    assert_eq!(
        gate_names("cargo fmt --all --check; cargo clippy -- -D warnings; cargo test"),
        Some(vec!["cargo fmt", "cargo clippy", "cargo test"])
    );
    assert_eq!(
        gate_names("cd desktop && pnpm typecheck && pnpm lint"),
        Some(vec!["pnpm typecheck", "pnpm lint"])
    );
    // Keystone's own banner-then-gate shape, once per gate: the one recorded
    // in run 4's transcript, minus the redirect-carrying hermit-activation
    // prefix covered separately below.
    assert_eq!(
        gate_names(r#"echo "== fmt =="; cargo fmt --all --check; echo "fmt exit=$?""#),
        Some(vec!["cargo fmt"])
    );
    // A single `; cargo test` is still ambiguous for the one-gate helper —
    // it resolves to two gates, not one — but is not refused.
    assert_eq!(single_gate("cargo test; cargo clippy"), None);
    assert_eq!(
        gate_names("cargo test; cargo clippy"),
        Some(vec!["cargo test", "cargo clippy"])
    );
}

/// A leading, trailing, or doubled separator is an empty segment, and an
/// empty segment is refused rather than skipped.
#[test]
fn an_empty_segment_refuses_the_whole_line() {
    for command in [
        "; cargo test",
        "cargo test;",
        "cargo fmt --check;; cargo test",
        "&& cargo test",
        "cargo test &&",
    ] {
        assert_eq!(gate_names(command), None, "{command}");
    }
}

/// Finding 57's own motivating evidence: Keystone's *exact* command from
/// live run 4 (channel `aa58f6a2-…`, transcript eventSeq 19), captured
/// verbatim from the relay except for the host-redacted hermit-activation
/// path, which the query tool elides but which — crucially — still carries
/// its own trailing redirect either way. Red, and still red after this fix:
/// the leading `. <path> >/dev/null 2>&1;` is a genuine redirect, which this
/// module keeps refusing unconditionally regardless of anything after it.
/// See the module doc, "Composed commands", for why the code fix alone
/// cannot recover this line — only the packs/copy half of finding 57 can.
#[test]
fn keystones_actual_run_4_line_is_still_refused_by_its_own_redirect() {
    let keystones_line = concat!(
        ". /Users/keystone/.hermit/env.bash >/dev/null 2>&1; ",
        r#"echo "== fmt =="; cargo fmt --all --check; echo "fmt exit=$?""#
    );
    assert_eq!(
        gate_names(keystones_line),
        None,
        "a redirect anywhere on the line refuses the whole line, exactly as \
         it did before finding 57"
    );
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
        observer.on_item(&ambiguous, 10).is_empty(),
        "silence about an outcome is not evidence of a good one"
    );
}

#[test]
fn a_call_that_ran_no_gate_produces_no_row_once_its_result_is_in() {
    // Finding 69: which gate(s) a call ran can only be known once the
    // result is in hand (some harnesses never put a command on the call at
    // all), so the call itself is remembered regardless — `git status` is
    // not exempted from opening a pending entry any more than a gate is.
    // What stays true is the observable finding 26 exists to check: no row
    // for a command that was never a gate.
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("t1", "git status --porcelain"), 0);
    assert!(
        !observer.pending.is_empty(),
        "every execute call is remembered, gate or not"
    );
    assert!(observer
        .on_item(&tool_result("t1", false, "clean"), 10)
        .is_empty());
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
            .is_empty(),
        "a call pushed out of the window produces no row rather than a guessed one"
    );
    assert!(!observer
        .on_item(&tool_result("t35", false, "ok"), 1)
        .is_empty());
}

#[test]
fn a_command_longer_than_the_wire_allows_is_dropped_rather_than_shortened() {
    let long = format!("cargo test -p buzz-core {}", "x".repeat(600));
    assert!(long.len() > MAX_OBSERVATION_COMMAND_BYTES);

    // The L5/`buzz-agent` shape: the over-long command is on the call.
    // Finding 69 moved bounds-checking from `open` to `close` (the call is
    // remembered regardless of its command now), so this asserts on the
    // *row*, not on `pending` staying empty — it no longer does.
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("t1", &long), 0);
    assert!(
        observer
            .on_item(&tool_result("t1", false, "ok"), 10)
            .is_empty(),
        "a shortened command line is a different command"
    );

    // Finding 69's own shape: the call names nothing at all, and the
    // over-long command only shows up on the result's `input.command`.
    // The bound applies exactly the same way there.
    let mut observer = GateObserver::default();
    observer.on_item(
        &json!({
            "kind": "tool_call",
            "tool": { "toolId": "t2", "toolKind": "execute", "input": {} },
        }),
        0,
    );
    assert!(
        observer
            .on_item(
                &json!({
                    "kind": "tool_result",
                    "toolId": "t2",
                    "toolKind": "execute",
                    "toolName": "Terminal",
                    "input": { "command": long },
                    "content": "ok",
                    "isError": false,
                }),
                10
            )
            .is_empty(),
        "the same bound applies wherever the command was finally found"
    );
}

#[test]
fn interleaved_calls_close_against_their_own_ids() {
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("a", "cargo fmt --check"), 0);
    observer.on_item(&tool_call("b", "cargo clippy --all-targets"), 100);
    let clippy = observer
        .on_item(&tool_result("b", true, "error: unused variable"), 900)
        .pop()
        .expect("clippy closes first");
    assert_eq!(clippy.row.gate, "cargo clippy");
    assert_eq!(clippy.row.duration_ms, Some(800));
    let fmt = observer
        .on_item(&tool_result("a", false, ""), 1_000)
        .pop()
        .expect("fmt closes second");
    // The gate name is program + subcommand; the *flags* live in `command`,
    // verbatim, so `--check` is never lost — it is simply not part of the name.
    assert_eq!(fmt.row.gate, "cargo fmt");
    assert_eq!(
        fmt.row.summary, None,
        "no output is null, never an empty summary"
    );
}

/// The pre-existing `cd <path> && <gate>` shape, unchanged in spirit: one
/// gate, wrapped, still gets its own row and still trusts the compound's
/// exit as that one gate's own — exactly as it did before finding 57, and
/// for the same reason (nothing else on the line can produce the exit).
#[test]
fn a_single_gate_wrapped_by_cd_still_gets_its_own_row_named_for_itself_alone() {
    let mut observer = GateObserver::default();
    observer.on_item(&tool_call("t1", "cd desktop && pnpm test"), 0);
    let failed = observer
        .on_item(&tool_result("t1", true, "1 failing"), 500)
        .pop()
        .expect("the single gate segment still closes");
    assert_eq!(failed.row.gate, "pnpm test");
    assert_eq!(
        failed.row.command, "pnpm test",
        "the row names the gate's own segment, not the cd prefix ahead of it"
    );
    assert_eq!(
        failed.row.outcome,
        CodingSessionObservationGateOutcome::Failed
    );
}

/// Finding 57's headline case: three gates, one `;`-joined line, one
/// `tool_result`. `isError: false` is sound here — nothing follows the last
/// gate that could itself fail and hide behind it — so every gate segment
/// gets its own `passed` row, sharing the one summary and duration the
/// transcript actually carries.
#[test]
fn a_composed_line_that_passed_publishes_one_row_per_gate_segment() {
    let mut observer = GateObserver::default();
    observer.on_item(
        &tool_call(
            "t1",
            "cargo fmt --all --check; cargo clippy -- -D warnings; cargo test",
        ),
        1_000,
    );
    let mut rows = observer.on_item(&tool_result("t1", false, "test result: ok"), 4_000);
    assert_eq!(rows.len(), 3, "one row per recognised gate segment");
    let test_row = rows.pop().unwrap();
    let clippy_row = rows.pop().unwrap();
    let fmt_row = rows.pop().unwrap();

    assert_eq!(fmt_row.row.gate, "cargo fmt");
    assert_eq!(fmt_row.row.command, "cargo fmt --all --check");
    assert_eq!(clippy_row.row.gate, "cargo clippy");
    assert_eq!(clippy_row.row.command, "cargo clippy -- -D warnings");
    assert_eq!(test_row.row.gate, "cargo test");
    assert_eq!(test_row.row.command, "cargo test");

    for row in [&fmt_row, &clippy_row, &test_row] {
        assert_eq!(row.row.outcome, CodingSessionObservationGateOutcome::Passed);
        assert_eq!(row.row.summary.as_deref(), Some("test result: ok"));
        assert_eq!(row.row.duration_ms, Some(3_000));
    }
}

/// The other half of finding 57's own fallback: a composed call that failed
/// cannot say *which* of several gates produced the exit, so it publishes
/// nothing at all rather than guessing — the same "silence over a guessed
/// gate name" rule the single-command matcher has always followed.
#[test]
fn a_composed_line_that_failed_publishes_nothing_for_more_than_one_gate() {
    let mut observer = GateObserver::default();
    observer.on_item(
        &tool_call("t1", "cargo fmt --all --check; cargo clippy -- -D warnings"),
        0,
    );
    assert!(
        observer
            .on_item(&tool_result("t1", true, "error: ..."), 100)
            .is_empty(),
        "two gates share one exit; neither row can honestly claim it"
    );
}

/// Finding 69, `run5-gate-items.json` eventSeq 36–41 verbatim (the `content`
/// on eventSeq 39 is abbreviated from the ~18.5 KB `cargo` progress bar the
/// live transcript actually carries — nothing the observer reads lives in
/// that noise, and the fields that matter are byte-for-byte the wire's own).
///
/// `claude-agent-acp` never puts a command on the `tool_call` frame at all —
/// `tool.input` arrives as `{}`. The text lands on the paired `tool_result`
/// instead, in `input.command`. Before this fix, [`GateObserver::open`] read
/// the command from the *call's* `tool.input` (`command_of` on `{}` →
/// `None`), so it never opened a pending entry for either of these two
/// commands, and the paired results — both real, both green — closed
/// against nothing and became no rows. Ever, on this harness.
#[test]
fn finding_69_the_command_lands_on_the_result_not_the_call() {
    let items: Vec<Value> = vec![
        // eventSeq 36 — tool_call, empty input, toolKind "execute".
        json!({
            "kind": "tool_call",
            "tool": {
                "input": {},
                "toolId": "toolu_01VSakN3EV2vpDSerszprACX",
                "toolKind": "execute",
                "toolName": "Terminal",
            },
        }),
        // eventSeq 37 — tool_result, command on `input.command`.
        json!({
            "content": "(Bash completed with no output)",
            "input": {
                "command": "cargo fmt --all --check",
                "description": "Gate 1 pre-commit: cargo fmt check",
                "timeout": 600_000,
            },
            "isError": false,
            "kind": "tool_result",
            "toolId": "toolu_01VSakN3EV2vpDSerszprACX",
            "toolKind": "execute",
            "toolName": "cargo fmt --all --check",
        }),
        // eventSeq 38 — tool_call, empty input, toolKind "execute".
        json!({
            "kind": "tool_call",
            "tool": {
                "input": {},
                "toolId": "toolu_01DRVtn2xVqCwuajPFx89376",
                "toolKind": "execute",
                "toolName": "Terminal",
            },
        }),
        // eventSeq 39 — tool_result, command on `input.command`; `content`
        // abbreviated (see the test doc comment above) from the live
        // transcript's own `cargo` build noise, ending in the same tail.
        json!({
            "content": "   Compiling libc v0.2.186\n...\ntest result: ok",
            "input": {
                "command": "cargo clippy -p buzz-cli -- -D warnings",
                "description": "Gate 2 pre-commit: clippy on buzz-cli",
                "timeout": 600_000,
            },
            "isError": false,
            "kind": "tool_result",
            "toolId": "toolu_01DRVtn2xVqCwuajPFx89376",
            "toolKind": "execute",
            "toolName": "cargo clippy -p buzz-cli -- -D warnings",
        }),
        // eventSeq 40 — plain assistant prose, not a tool frame at all.
        json!({
            "kind": "assistant_text",
            "text": "Clippy clean. Third gate:",
        }),
        // eventSeq 41 — a third call opens, unpaired in this fixture: no
        // result for it ships in the live evidence either.
        json!({
            "kind": "tool_call",
            "tool": {
                "input": {},
                "toolId": "toolu_01BeqS27u62DQoGAirnge4Y7",
                "toolKind": "execute",
                "toolName": "Terminal",
            },
        }),
    ];

    let mut observer = GateObserver::default();
    assert!(
        observer.on_item(&items[0], 1_788_486_650_506).is_empty(),
        "a call opens nothing to return"
    );
    let fmt = observer
        .on_item(&items[1], 1_788_486_655_782)
        .pop()
        .expect("the fmt result closes the call it paired with, command and all");
    assert_eq!(fmt.row.gate, "cargo fmt");
    assert_eq!(fmt.row.command, "cargo fmt --all --check");
    assert_eq!(fmt.row.outcome, CodingSessionObservationGateOutcome::Passed);

    assert!(observer.on_item(&items[2], 1_788_486_656_723).is_empty());
    let clippy = observer
        .on_item(&items[3], 1_788_486_686_359)
        .pop()
        .expect("the clippy result closes the call it paired with, command and all");
    assert_eq!(clippy.row.gate, "cargo clippy");
    assert_eq!(
        clippy.row.command,
        "cargo clippy -p buzz-cli -- -D warnings"
    );
    assert_eq!(
        clippy.row.outcome,
        CodingSessionObservationGateOutcome::Passed
    );

    assert!(
        observer.on_item(&items[4], 1_788_486_686_374).is_empty(),
        "prose is not a tool frame"
    );
    assert!(
        observer.on_item(&items[5], 1_788_486_686_374).is_empty(),
        "a third call opens nothing to return, and this fixture never pairs it"
    );
}

/// L5's shape (command on the call, none on the result) still works, in the
/// same observer that also handles finding 69's shape — mixed within one
/// session is exactly what a provider watching real seats sees, since a
/// session's driver does not change mid-turn but the provider itself never
/// assumes a single harness.
#[test]
fn l5s_shape_still_works_alongside_finding_69s() {
    let mut observer = GateObserver::default();

    // finding 69: empty call, command on the result.
    observer.on_item(
        &json!({
            "kind": "tool_call",
            "tool": { "toolId": "acp1", "toolKind": "execute", "input": {} },
        }),
        0,
    );
    // L5: command on the call, nothing on the result.
    observer.on_item(&tool_call("agent1", "cargo test -p buzz-core"), 0);

    let acp_row = observer
        .on_item(
            &json!({
                "kind": "tool_result",
                "toolId": "acp1",
                "toolKind": "execute",
                "toolName": "cargo fmt --all --check",
                "input": { "command": "cargo fmt --all --check" },
                "content": "",
                "isError": false,
            }),
            100,
        )
        .pop()
        .expect("finding 69's shape still closes");
    assert_eq!(acp_row.row.gate, "cargo fmt");
    assert_eq!(acp_row.row.command, "cargo fmt --all --check");

    let agent_row = observer
        .on_item(&tool_result("agent1", true, "test result: FAILED"), 200)
        .pop()
        .expect("L5's shape still closes");
    assert_eq!(agent_row.row.gate, "cargo test");
    assert_eq!(agent_row.row.command, "cargo test -p buzz-core");
    assert_eq!(
        agent_row.row.outcome,
        CodingSessionObservationGateOutcome::Failed
    );
}
