//! Gate rows the provider **watched**, derived from a seat's own tool calls.
//!
//! Brian's ruling, 2026-09-02: *"we can achieve this without asking the agents
//! to do it — which I find to always be the weak link."* Every observability
//! failure in live runs 2 and 3 was an agent skipping or mis-stating a
//! reporting step. Live-run finding 26 is the sharpest form of it — a seat
//! reported `cargo test -p buzz-cli` green after running one test *file*, and
//! a verifier reproduced red on the same patch. Prose cannot be checked; a
//! signed row with the command can.
//!
//! So this module writes the row instead of asking for it. It reads the
//! transcript items the provider is already publishing, pairs each `tool_call`
//! that ran a **recognised gate command** with its own `tool_result`, and
//! hands back one [`ObservedGateRow`]. The provider signs it with its own key
//! and publishes it as kind 44246 with `source: "observed"`. A seat's own
//! `bee sessions observe gate` row is `declared` and is never merged with one
//! of these.
//!
//! # What it will not claim
//!
//! * **Only commands it recognises.** An unmatched command produces nothing.
//!   A matcher that guessed would put a gate name on a command that is not
//!   that gate, which is worse than the silence it replaced.
//! * **Only outcomes the transcript carries.** ACP reports a tool result as
//!   `completed` or `failed` (`transcript::terminal_status`), and the
//!   published item carries that as `isError`. That is the strongest signal
//!   available at this seam — the shell's numeric exit status is not on the
//!   wire — so a row says `passed` or `failed` and never `not-run`, which is a
//!   fact only a caller who decided not to run something can state.
//! * **Its own clock, disclosed as its own.** `durationMs` is the provider's
//!   measurement between the two frames, and NIP-CSOB already says every
//!   duration is the author's own; here the author is the provider, which is
//!   exactly what `source: "observed"` says.
//! * **Nothing unbounded.** At most [`MAX_PENDING_GATE_CALLS`] calls are
//!   remembered at once; the oldest is forgotten first, and a call whose
//!   result never arrives simply never becomes a row.

use std::collections::VecDeque;

use buzz_core::coding_session_observation::{
    CodingSessionObservationGateOutcome, CodingSessionObservationGateRow,
    MAX_OBSERVATION_COMMAND_BYTES, MAX_OBSERVATION_SUMMARY_BYTES,
};
use serde_json::Value;

/// Tool calls remembered while waiting for their results.
///
/// A gate command runs for minutes and a seat interleaves reads and edits with
/// it, so the window has to be wider than one call — but it is a window, not a
/// map that grows with the session.
pub const MAX_PENDING_GATE_CALLS: usize = 32;

/// One gate row the provider watched, and the moment it started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedGateRow {
    /// The row, ready for a kind 44246 gate body.
    ///
    /// `head_sha` and `dirty` are **not** filled in here and are `None` on
    /// every row this module returns. They are resolved by
    /// [`crate::Provider::spawn_gate_head_probe`], which runs `git rev-parse
    /// HEAD` and `git status --porcelain` in the seat's own workdir at the
    /// moment this row closes — in the provider process, from the `cwd` the
    /// provider already holds on the session record, never by asking the
    /// agent. This module only reads transcript frames and has no filesystem
    /// of its own; splitting it that way is what keeps it synchronous and
    /// keeps two `git` subprocesses off the provider's event loop.
    pub row: CodingSessionObservationGateRow,
}

/// One recognised gate: a program, its subcommand, and the name a row carries.
///
/// Matched on **argv**, in command position — never on the text of the line.
/// REVIEW-L5 F1 is why: a `contains`-style matcher answered `Some("cargo test")`
/// for `grep -rn 'cargo test' docs/` and `echo "cargo test"`, so the provider
/// signed an `observed` **passed** row for a `grep`, and that false green then
/// displaced a genuine `failed` row as the one both surfaces showed. A seat
/// could bury the exact failure this record exists to expose.
struct GateMatcher {
    program: &'static str,
    subcommand: &'static str,
    gate: &'static str,
}

/// The gates this observer will name, and nothing else.
///
/// A closed table of `(program, subcommand)` pairs. A program not in it, a
/// subcommand not in it, or either of them anywhere but the head of the
/// command produces **no row at all** — silence is the honest answer, and a
/// guessed gate name is worse than the silence it replaced.
const GATE_MATCHERS: &[GateMatcher] = &[
    GateMatcher {
        program: "cargo",
        subcommand: "fmt",
        gate: "cargo fmt",
    },
    GateMatcher {
        program: "cargo",
        subcommand: "clippy",
        gate: "cargo clippy",
    },
    GateMatcher {
        program: "cargo",
        subcommand: "test",
        gate: "cargo test",
    },
    GateMatcher {
        program: "pnpm",
        subcommand: "test",
        gate: "pnpm test",
    },
    GateMatcher {
        program: "pnpm",
        subcommand: "typecheck",
        gate: "pnpm typecheck",
    },
    GateMatcher {
        program: "pnpm",
        subcommand: "lint",
        gate: "pnpm lint",
    },
    GateMatcher {
        program: "just",
        subcommand: "check",
        gate: "just check",
    },
    GateMatcher {
        program: "just",
        subcommand: "test",
        gate: "just test",
    },
    GateMatcher {
        program: "just",
        subcommand: "ci",
        gate: "just ci",
    },
];

/// One pending call: what gate it is, what it ran, and when it opened.
#[derive(Debug, Clone)]
struct PendingGate {
    tool_id: String,
    gate: &'static str,
    command: String,
    /// Milliseconds since the epoch, from the provider's own clock — the same
    /// `now_ms()` every other provider row is stamped with.
    started_at_ms: i64,
}

/// Pairs gate tool calls with their results, one seat at a time.
#[derive(Debug, Default)]
pub struct GateObserver {
    pending: VecDeque<PendingGate>,
}

impl GateObserver {
    /// Feed one published transcript item.
    ///
    /// Returns a row exactly when this item is the *result* of a gate command
    /// this observer opened. Everything else — prose, plans, reads, edits, a
    /// shell command that is not a gate — returns `None` and is not
    /// remembered.
    pub fn on_item(&mut self, item: &Value, now_ms: i64) -> Option<ObservedGateRow> {
        match item.get("kind").and_then(Value::as_str) {
            Some("tool_call") => {
                // A call that names no gate is simply not remembered; the
                // `None` here is "nothing to open", never a failure.
                let _ = self.open(item, now_ms);
                None
            }
            Some("tool_result") => self.close(item, now_ms),
            _ => None,
        }
    }

    fn open(&mut self, item: &Value, now_ms: i64) -> Option<()> {
        let tool = item.get("tool")?;
        let tool_id = string_at(tool, "toolId")?;
        let command = command_of(tool.get("input"))?;
        let gate = match_gate(&command)?;
        if self.pending.len() >= MAX_PENDING_GATE_CALLS {
            self.pending.pop_front();
        }
        self.pending.push_back(PendingGate {
            tool_id,
            gate,
            command,
            started_at_ms: now_ms,
        });
        Some(())
    }

    fn close(&mut self, item: &Value, now_ms: i64) -> Option<ObservedGateRow> {
        let tool_id = string_at(item, "toolId")?;
        let index = self
            .pending
            .iter()
            .position(|pending| pending.tool_id == tool_id)?;
        let pending = self.pending.remove(index)?;
        // `isError` is the published form of ACP's own terminal status. A
        // result that carries neither is not evidence of a pass, so it is
        // dropped rather than turned into one.
        let is_error = item.get("isError").and_then(Value::as_bool)?;
        Some(ObservedGateRow {
            row: CodingSessionObservationGateRow {
                gate: pending.gate.to_owned(),
                outcome: if is_error {
                    CodingSessionObservationGateOutcome::Failed
                } else {
                    CodingSessionObservationGateOutcome::Passed
                },
                command: pending.command,
                summary: summary_of(item.get("content")),
                // A clock that went backwards between the two frames yields
                // `null`, never a negative or a zero: an unmeasurable span is
                // not a span of no time (§8 I9).
                duration_ms: now_ms
                    .checked_sub(pending.started_at_ms)
                    .and_then(|elapsed| u64::try_from(elapsed).ok()),
                // Resolved by the provider after this returns; see
                // `ObservedGateRow::row`.
                head_sha: None,
                dirty: None,
            },
        })
    }
}

/// The shell command a tool call ran, if it ran one.
///
/// Only the `command` argument, and only a string: a tool whose arguments name
/// a file rather than a command line has not run a gate, and a structured
/// argv would need a quoting rule this seam has no business inventing.
fn command_of(input: Option<&Value>) -> Option<String> {
    let raw = input?.get("command")?.as_str()?.trim();
    if raw.is_empty() || raw.len() > MAX_OBSERVATION_COMMAND_BYTES {
        // A command longer than the wire's own ceiling is not truncated into a
        // row: a *shortened* command line is a different command, and a reader
        // who copied it would run something else.
        return None;
    }
    if raw.chars().any(char::is_control) {
        return None;
    }
    Some(raw.to_owned())
}

/// One token of a shell command line, as far as this seam reads one.
#[derive(Debug, PartialEq, Eq)]
enum ShellToken {
    /// A word, with any quoting removed.
    Word(String),
    /// The one separator a gate may hide behind, and only as `cd … && <gate>`.
    AndAnd,
}

/// Which gate a command line is, or none.
///
/// Three steps, all of them refusals by default:
///
/// 1. **Split into argv.** Anything the split cannot read as plain words and at
///    most one `&&` — a pipe, a redirect, a `;`, a backtick, a `$(`, an
///    unbalanced quote — is refused outright. The observer cannot say which
///    segment of a composed line produced the exit it is about to read, so it
///    declines to name any of them.
/// 2. **Strip the wrappers a seat legitimately uses**, and only those: a
///    leading `cd <path> &&`, `nice [-n N]`, and `env [VAR=VALUE …]`. None of
///    them produces an exit status of its own.
/// 3. **Match program and subcommand at the head**, against the closed table.
///
/// What survives all three is a single command whose own exit the paired tool
/// result reports — which is what makes `outcome` a measurement rather than a
/// guess about a pipeline.
fn match_gate(command: &str) -> Option<&'static str> {
    let argv = strip_wrappers(gate_argv(command)?)?;
    let program = argv.first()?.as_str();
    let subcommand = argv.get(1)?.as_str();
    GATE_MATCHERS
        .iter()
        .find(|matcher| matcher.program == program && matcher.subcommand == subcommand)
        .map(|matcher| matcher.gate)
}

/// The argv a gate would run under, or `None` when the line is composed.
///
/// `cd <path> && <rest>` yields `<rest>`; every other use of `&&`, and every
/// other metacharacter, is refused. A `cd` is exempt because it cannot itself
/// produce the exit status the row reports.
fn gate_argv(command: &str) -> Option<Vec<String>> {
    let tokens = shell_split(command)?;
    let separators = tokens
        .iter()
        .filter(|token| **token == ShellToken::AndAnd)
        .count();
    let mut words: Vec<String> = Vec::new();
    match separators {
        0 => {
            for token in tokens {
                match token {
                    ShellToken::Word(word) => words.push(word),
                    ShellToken::AndAnd => return None,
                }
            }
        }
        1 => {
            let mut before: Vec<String> = Vec::new();
            let mut seen = false;
            for token in tokens {
                match token {
                    ShellToken::AndAnd => seen = true,
                    ShellToken::Word(word) => {
                        if seen {
                            words.push(word);
                        } else {
                            before.push(word);
                        }
                    }
                }
            }
            // Exactly `cd <path>`, nothing else and nothing more.
            if before.len() != 2 || before[0] != "cd" {
                return None;
            }
        }
        _ => return None,
    }
    (!words.is_empty()).then_some(words)
}

/// Drop the wrappers that run a gate without being one.
///
/// `nice`/`env` only, and only their own arguments: `nice -n 10 cargo test` and
/// `env RUST_BACKTRACE=1 cargo test` are the same gate as `cargo test`. A
/// wrapper this list does not know leaves the argv alone, so the head no longer
/// matches and no row is minted.
fn strip_wrappers(mut argv: Vec<String>) -> Option<Vec<String>> {
    loop {
        match argv.first().map(String::as_str) {
            Some("nice") => {
                argv.remove(0);
                match argv.first().map(String::as_str) {
                    // `-n 10`
                    Some("-n") => {
                        argv.remove(0);
                        (!argv.is_empty()).then(|| argv.remove(0))?;
                    }
                    // `-n10`
                    Some(flag) if flag.starts_with("-n") => {
                        argv.remove(0);
                    }
                    _ => {}
                }
            }
            Some("env") => {
                argv.remove(0);
                while argv
                    .first()
                    .is_some_and(|word| word.contains('=') && !word.starts_with('-'))
                {
                    argv.remove(0);
                }
            }
            _ => return (!argv.is_empty()).then_some(argv),
        }
    }
}

/// Split a command line into words and `&&`, or refuse it.
///
/// Deliberately small and deliberately strict. It understands single and double
/// quotes and nothing else: any other shell construct — `|`, `;`, `<`, `>`,
/// a backtick, `$(`, an unterminated quote — returns `None`, because a line
/// this seam cannot read exactly is a line it must not label.
fn shell_split(command: &str) -> Option<Vec<ShellToken>> {
    let mut tokens: Vec<ShellToken> = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut chars = command.chars().peekable();
    while let Some(character) = chars.next() {
        match character {
            '\'' | '"' => {
                quoted = true;
                for inner in chars.by_ref() {
                    if inner == character {
                        break;
                    }
                    current.push(inner);
                }
                // An unterminated quote leaves the iterator exhausted; the
                // caller sees a word it never closed, which is refused below.
                if command.matches(character).count() % 2 != 0 {
                    return None;
                }
            }
            '&' => {
                if chars.peek() != Some(&'&') {
                    return None;
                }
                chars.next();
                if !current.is_empty() || quoted {
                    tokens.push(ShellToken::Word(std::mem::take(&mut current)));
                    quoted = false;
                }
                tokens.push(ShellToken::AndAnd);
            }
            '|' | ';' | '<' | '>' | '`' | '\n' => return None,
            '$' => {
                if chars.peek() == Some(&'(') {
                    return None;
                }
                current.push(character);
            }
            character if character.is_whitespace() => {
                if !current.is_empty() || quoted {
                    tokens.push(ShellToken::Word(std::mem::take(&mut current)));
                    quoted = false;
                }
            }
            character => current.push(character),
        }
    }
    if !current.is_empty() || quoted {
        tokens.push(ShellToken::Word(current));
    }
    (!tokens.is_empty()).then_some(tokens)
}

/// The tail a reader wants: the command's last non-blank line, bounded.
///
/// The last line is where `cargo test` and `pnpm test` put their verdict
/// (`test result: ok. 26 passed; …`). Empty output yields `None`, which the
/// wire carries as `null` and every surface renders as absent, never as a
/// blank summary.
fn summary_of(content: Option<&Value>) -> Option<String> {
    let text = content?.as_str()?;
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    let line: String = line
        .chars()
        .filter(|character| !character.is_control())
        .collect();
    if line.is_empty() {
        return None;
    }
    let mut bounded = line;
    while bounded.len() > MAX_OBSERVATION_SUMMARY_BYTES {
        bounded.pop();
    }
    Some(bounded)
}

fn string_at(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_owned())
}

#[cfg(test)]
#[path = "gate_observer_tests.rs"]
mod tests;
