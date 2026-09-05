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
//! transcript items the provider is already publishing, pairs each `execute`
//! `tool_call` with its own `tool_result`, and — once that result names a
//! **recognised gate command** (see "Harness shapes" below for where that
//! name is actually read from) — hands back one [`ObservedGateRow`]. The
//! provider signs it with its own key and publishes it as kind 44246 with
//! `source: "observed"`. A seat's own `bee sessions observe gate` row is
//! `declared` and is never merged with one of these.
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
//!
//! # Harness shapes (finding 69)
//!
//! Live run 5, `claude-agent-acp`: every `tool_call` frame for a shell
//! command carries `tool.input: {}` and `toolName: "Terminal"` — the command
//! text is not on the call at all. It shows up on the paired `tool_result`
//! instead, as `input.command`, with `toolName` echoing the command line
//! itself (`"cargo fmt --all --check"`, not a generic label). The observer of
//! record read the command from the *call's* `tool.input`
//! (`GateObserver::open`, pre-fix), so it never opened a pending entry for
//! either of run 5's two green gates, and the results — real, matched,
//! green — closed against nothing and became no rows. Ever, on this harness.
//!
//! `buzz-agent`'s own driver puts the command on the call, as do every
//! pre-existing fixture in `gate_observer_tests.rs` (predating this
//! finding). Both shapes are real and both must keep working, so **which
//! gate(s) a call ran is no longer decided at `open`** — every `execute`
//! call is remembered regardless of whether (or where) it names a command —
//! **and is decided at `close`**, once the result is in hand, by
//! [`resolve_command`]: the result's own `input.command` first, the call's
//! `tool.input.command` (remembered at `open`) second, and the result's
//! `toolName` last, tried only when it passes the same bounds every other
//! candidate command does. A harness that puts a command nowhere this
//! function looks still produces no row — silence, not a guess, exactly as
//! before.
//!
//! # Composed commands (finding 57)
//!
//! Live run 4 ran all three gates green and produced **zero** observed rows:
//! Keystone ran each one as `echo "== fmt =="; cargo fmt --all --check; echo
//! "fmt exit=$?"`, and the observer of record refused every line with a `;`
//! outright. That is still the right refusal for a line this seam cannot
//! read exactly — but a `;`/`&&`-joined line whose every top-level segment is
//! *itself* either a recognised gate or one of the four wrappers a seat
//! legitimately narrates with (`cd`, `echo`, `nice`, `env`) is not such a
//! line, and is now accepted.
//!
//! One `tool_result` still carries exactly one `isError` for the *whole*
//! line, never one per segment, so a composed call cannot say which segment
//! produced that exit the way a bare one can:
//!
//! * **`isError: true` on more than one gate segment publishes nothing.**
//!   The failure could be any of them (or, for a `&&` line, a wrapper ahead
//!   of all of them, which never let a gate run at all) — a guess here is
//!   exactly the false claim this module exists to refuse.
//! * **`isError: false` publishes a `passed` row for every gate segment.**
//!   Sound for a pure `&&` line (bash's own short-circuit means reaching the
//!   end at all proves every segment succeeded) and for the common
//!   banner-then-gate shape. **Not sound in general** for a `;` line whose
//!   *last* segment is a wrapper that cannot itself fail, such as Keystone's
//!   own `echo "fmt exit=$?"` tail — bash reports the tail's exit, not the
//!   gate's, so a failing gate followed by that tail is indistinguishable
//!   from a passing one at this seam. Two consequences, both deliberate: a
//!   solitary gate segment still gets a `failed` row on `isError: true`
//!   (unchanged from the pre-existing `cd <path> && <gate>` case, which
//!   carries the same "a wrapper ahead of it could have been what failed"
//!   caveat and was never flagged for it); and Keystone's *actual* line is
//!   refused regardless of any of the above, because the hermit-activation
//!   prefix it also carries (`. <path> >/dev/null 2>&1; …`) is a genuine
//!   redirect, which this module keeps refusing unconditionally. The code
//!   fix here widens what a *bare-enough* composed line can prove; it does
//!   not make an embedded redirect provable, and does not make a `;`-tailed
//!   gate's failure provable. The seat-facing fix for both is the same one:
//!   run each gate as its own command (see the persona packs).

use std::collections::VecDeque;

use buzz_core::coding_session_observation::{
    CodingSessionObservationGateOutcome, CodingSessionObservationGateRow,
    MAX_OBSERVATION_COMMAND_BYTES, MAX_OBSERVATION_SUMMARY_BYTES,
};
use serde_json::Value;

/// Execute tool calls remembered while waiting for their results.
///
/// Every `execute` call takes a slot here, not only the ones that turn out to
/// run a gate — see the module doc, "Harness shapes", for why that can no
/// longer be told apart at `open`. A gate command runs for minutes and a seat
/// interleaves reads, edits, and plenty of non-gate shell commands with it, so
/// the window has to be wider than one call regardless — but it is a window,
/// not a map that grows with the session.
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
/// `program` is compared to the **basename** of `argv[0]` (finding 80: a
/// path to the program, `bin/cargo` or an absolute one, is the same gate),
/// and `subcommand` to `argv[1]` exactly.
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

/// One gate segment of a pending call: its name and its own command text.
#[derive(Debug, Clone)]
struct PendingSegment {
    gate: &'static str,
    /// This segment's own text, trimmed, never the whole line — so a row
    /// from a composed call names only the gate that produced it.
    command: String,
}

/// One pending call, remembered while its result is awaited.
///
/// Which gate(s) it names is not decided here — see the module doc, "Harness
/// shapes" — because some harnesses (finding 69) never put a command on the
/// `tool_call` frame at all, so nothing about it can be classified until the
/// paired `tool_result` arrives. `call_command` is the command as it stood on
/// the call, when the call carried one; `None` for exactly the harnesses that
/// don't.
#[derive(Debug, Clone)]
struct PendingGate {
    tool_id: String,
    call_command: Option<String>,
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
    /// Returns one row per gate segment this item closed — zero for anything
    /// that is not the *result* of a call this observer opened, exactly one
    /// for the common bare-command case, and more than one only for an
    /// accepted composed line. Everything else — prose, plans, reads, edits,
    /// a shell command that is not a gate — opens nothing and returns
    /// nothing.
    pub fn on_item(&mut self, item: &Value, now_ms: i64) -> Vec<ObservedGateRow> {
        match item.get("kind").and_then(Value::as_str) {
            Some("tool_call") => {
                // A call that is not `toolKind: "execute"`, or carries no
                // `toolId`, is simply not remembered; opening nothing is
                // never a failure. Every execute call *is* remembered, even
                // one that turns out to run no gate at all — see `open` and
                // the module doc, "Harness shapes".
                let _ = self.open(item, now_ms);
                Vec::new()
            }
            Some("tool_result") => self.close(item, now_ms),
            _ => Vec::new(),
        }
    }

    fn open(&mut self, item: &Value, now_ms: i64) -> Option<()> {
        let tool = item.get("tool")?;
        // Only `execute` calls can ever become a gate row. Restricting the
        // window to them is what keeps it a window at all once opening no
        // longer requires a recognised — or even present — command; see
        // `MAX_PENDING_GATE_CALLS`.
        if tool.get("toolKind").and_then(Value::as_str) != Some("execute") {
            return None;
        }
        let tool_id = string_at(tool, "toolId")?;
        // Some harnesses (finding 69) put the command here; some put it only
        // on the result. Either way it is remembered now, in case the result
        // needs it as a fallback — see `resolve_command`.
        let call_command = command_of(tool.get("input"));
        if self.pending.len() >= MAX_PENDING_GATE_CALLS {
            self.pending.pop_front();
        }
        self.pending.push_back(PendingGate {
            tool_id,
            call_command,
            started_at_ms: now_ms,
        });
        Some(())
    }

    fn close(&mut self, item: &Value, now_ms: i64) -> Vec<ObservedGateRow> {
        let Some(tool_id) = string_at(item, "toolId") else {
            return Vec::new();
        };
        let Some(index) = self.pending.iter().position(|p| p.tool_id == tool_id) else {
            return Vec::new();
        };
        let pending = self.pending.remove(index).expect("index just found");
        // `isError` is the published form of ACP's own terminal status. A
        // result that carries neither is not evidence of a pass, so it is
        // dropped rather than turned into one.
        let Some(is_error) = item.get("isError").and_then(Value::as_bool) else {
            return Vec::new();
        };
        // Which gate(s) this call ran can only be known now — see
        // `resolve_command` and the module doc, "Harness shapes". A call that
        // ran no gate at all, or whose command this seam still cannot read
        // exactly, produces no row, exactly as it always has.
        let Some(command) = resolve_command(item, pending.call_command.as_deref()) else {
            return Vec::new();
        };
        let Some(segments) = match_gate_segments(&command) else {
            return Vec::new();
        };
        // A composed call pairs one exit with several gates. `isError: true`
        // there cannot say which of them produced it — a `&&` line may not
        // even have reached a later gate at all — so more than one gate
        // segment on a failing call publishes nothing rather than a guess.
        // A single segment is unambiguous either way, exactly the bare-gate
        // case this module has always reported.
        if is_error && segments.len() > 1 {
            return Vec::new();
        }
        let duration_ms = now_ms
            .checked_sub(pending.started_at_ms)
            .and_then(|elapsed| u64::try_from(elapsed).ok());
        let summary = summary_of(item.get("content"));
        segments
            .into_iter()
            .map(|segment| ObservedGateRow {
                row: CodingSessionObservationGateRow {
                    gate: segment.gate.to_owned(),
                    outcome: if is_error {
                        CodingSessionObservationGateOutcome::Failed
                    } else {
                        CodingSessionObservationGateOutcome::Passed
                    },
                    command: segment.command,
                    summary: summary.clone(),
                    // A clock that went backwards between the two frames
                    // yields `null`, never a negative or a zero: an
                    // unmeasurable span is not a span of no time (§8 I9).
                    duration_ms,
                    // Resolved by the provider after this returns; see
                    // `ObservedGateRow::row`.
                    head_sha: None,
                    dirty: None,
                },
            })
            .collect()
    }
}

/// The shell command a tool call ran, if it ran one.
///
/// Only the `command` argument, and only a string: a tool whose arguments name
/// a file rather than a command line has not run a gate, and a structured
/// argv would need a quoting rule this seam has no business inventing.
fn command_of(input: Option<&Value>) -> Option<String> {
    valid_command(input?.get("command")?.as_str()?)
}

/// The bounds every candidate command text is held to, wherever it came from.
///
/// A command longer than the wire's own ceiling is not truncated into a row:
/// a *shortened* command line is a different command, and a reader who copied
/// it would run something else. A control character is refused outright
/// rather than stripped, for the same reason.
fn valid_command(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.len() > MAX_OBSERVATION_COMMAND_BYTES {
        return None;
    }
    if raw.chars().any(char::is_control) {
        return None;
    }
    Some(raw.to_owned())
}

/// The command text a gate row should read, tried in the order a real result
/// frame can carry it — see the module doc, "Harness shapes".
///
/// 1. **`input.command` on the result itself.** `claude-agent-acp` (finding
///    69, live run 5) puts the command here and nowhere else — `tool.input`
///    on the call arrives empty (`{}`).
/// 2. **`tool.input.command` on the call**, remembered in `call_command` at
///    [`GateObserver::open`]. `buzz-agent`'s own driver — and every
///    pre-existing fixture in this module's tests — puts it there instead,
///    and a result that repeats it (or omits `input` altogether) still
///    resolves correctly through this fallback.
/// 3. **The result's own `toolName`**, when it looks like a command at all
///    (passes the same [`valid_command`] bounds every other source does).
///    `claude-agent-acp`'s result frame sets `toolName` to the command line
///    itself (`"cargo fmt --all --check"`), not a generic label — unlike the
///    *call's* `toolName` (`"Terminal"`), which this function never reads.
///    A harness whose `toolName` is a generic label falls through to no
///    command at all, exactly as before; [`match_gate_segments`] refusing a
///    label like `"Bash"` is the same silence-over-a-guess rule this module
///    has always followed, not a new check.
fn resolve_command(result: &Value, call_command: Option<&str>) -> Option<String> {
    command_of(result.get("input"))
        .or_else(|| call_command.map(str::to_owned))
        .or_else(|| valid_command(result.get("toolName").and_then(Value::as_str)?))
}

/// One token of a shell command line, as far as this seam reads one.
#[derive(Debug, PartialEq, Eq)]
enum ShellToken {
    /// A word, with any quoting removed.
    Word(String),
    /// A top-level `&&`.
    AndAnd,
    /// A top-level `;`.
    Semicolon,
}

/// One top-level segment of a command line, classified.
enum SegmentKind {
    /// `cd <path>` or `echo …` — narration around a gate, never a gate
    /// itself, and never able to produce the exit status a row would read.
    Wrapper,
    /// A recognised gate, named as the table names it.
    Gate(&'static str),
}

/// Which gate a *single, uncomposed* command line is, or none.
///
/// Three steps, all of them refusals by default:
///
/// 1. **Split into argv.** Anything the split cannot read as plain words — a
///    pipe, a redirect, a `;`, a `&&`, a backtick, a `$(`, an unbalanced
///    quote — is refused outright.
/// 2. **Strip the wrappers a seat legitimately uses**, and only those:
///    `nice [-n N]` and `env [VAR=VALUE …]`. Neither produces an exit status
///    of its own.
/// 3. **Match program and subcommand at the head**, against the closed table.
///
/// This is the single-segment half of [`match_gate_segments`]; a `cd`/`echo`
/// wrapper or a `&&`/`;` composition is that function's job, not this one's.
fn match_gate(command: &str) -> Option<&'static str> {
    let tokens = shell_split(command)?;
    let mut words = Vec::with_capacity(tokens.len());
    for token in tokens {
        match token {
            ShellToken::Word(word) => words.push(word),
            ShellToken::AndAnd | ShellToken::Semicolon => return None,
        }
    }
    let argv = strip_wrappers(words)?;
    let program = program_name(argv.first()?);
    let subcommand = argv.get(1)?.as_str();
    GATE_MATCHERS
        .iter()
        .find(|matcher| matcher.program == program && matcher.subcommand == subcommand)
        .map(|matcher| matcher.gate)
}

/// The program a command position names: the final path component of
/// `argv[0]`, so `bin/cargo`, `./bin/cargo` and `/Users/x/.cargo/bin/cargo`
/// are all `cargo`.
///
/// Finding 80: the table compared `argv[0]` to the bare name, so a seat that
/// ran `bin/cargo fmt --all --check` — the hermit-shim shape this repository's
/// own instructions produce — minted **no row at all**, and only `env
/// PATH=bin:$PATH cargo …` did (Andy's seat measured it). The basename is the
/// same gate; `cargoo`, `mycargo` and a bare `bin/cargo` with no subcommand
/// still name nothing, because the comparison after this is still exact.
fn program_name(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// Which gate(s) a command line names, or none — the composed-command
/// entry point [`GateObserver::open`] actually calls.
///
/// Splits the line into top-level segments at `&&`/`;` (see
/// [`split_top_level_segments`] for exactly what that refuses) and
/// classifies each one as a wrapper (`cd`, `echo`, and — as a prefix inside
/// a gate segment — `nice`/`env`) or a gate via [`match_gate`]. **Any**
/// segment that is neither refuses the whole line: a line this seam cannot
/// classify in full is a line it must not partly label. A line with no `&&`
/// or `;` at all is one segment, so the bare-gate case — the overwhelming
/// common one — goes through exactly the path it always has.
///
/// See the module doc, "Composed commands", for what a caller may and may
/// not conclude from the rows this produces.
fn match_gate_segments(command: &str) -> Option<Vec<PendingSegment>> {
    let segments = split_top_level_segments(command)?;
    let mut gates = Vec::new();
    for segment in segments {
        let kind = if segment_is_cd(segment) || segment_is_echo(segment) {
            SegmentKind::Wrapper
        } else if let Some(gate) = match_gate(segment) {
            SegmentKind::Gate(gate)
        } else {
            return None;
        };
        if let SegmentKind::Gate(gate) = kind {
            gates.push(PendingSegment {
                gate,
                command: segment.to_owned(),
            });
        }
    }
    (!gates.is_empty()).then_some(gates)
}

/// Whether a segment is exactly `cd <path>` — nothing else, nothing more.
fn segment_is_cd(segment: &str) -> bool {
    let Some(tokens) = shell_split(segment) else {
        return false;
    };
    matches!(
        tokens.as_slice(),
        [ShellToken::Word(first), ShellToken::Word(_)] if first == "cd"
    )
}

/// Whether a segment starts with `echo` — any arguments, since none of them
/// can change whether the segment itself produces an exit status.
fn segment_is_echo(segment: &str) -> bool {
    let Some(tokens) = shell_split(segment) else {
        return false;
    };
    matches!(tokens.first(), Some(ShellToken::Word(word)) if word == "echo")
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

/// Split a command line into words, `&&`, and `;`, or refuse it.
///
/// Deliberately small and deliberately strict. It understands single and
/// double quotes and nothing else: any other shell construct — `|`, `<`,
/// `>`, a backtick, `$(`, an unterminated quote — returns `None`, because a
/// line this seam cannot read exactly is a line it must not label.
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
            ';' => {
                if !current.is_empty() || quoted {
                    tokens.push(ShellToken::Word(std::mem::take(&mut current)));
                    quoted = false;
                }
                tokens.push(ShellToken::Semicolon);
            }
            '|' | '<' | '>' | '`' | '\n' => return None,
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

/// Split a command line into its own top-level segments, verbatim.
///
/// Reads the same alphabet [`shell_split`] reads — single/double quotes,
/// `&&`, and `;` — and refuses exactly what `shell_split` refuses: a pipe, a
/// redirect, a lone `&`, a backtick, a `$(`, an unterminated quote. Unlike
/// `shell_split`, this returns the *original* text between separators
/// (trimmed of the whitespace touching the separator, nothing else), so a
/// gate segment's `command` field ends up byte-for-byte what the seat typed
/// for it, never a word list rejoined with normalised spacing.
///
/// A leading, trailing, or doubled separator — an empty segment — is refused
/// rather than skipped: `foo;;bar` and `; foo` are not lines this seam has
/// business guessing the shape of.
fn split_top_level_segments(command: &str) -> Option<Vec<&str>> {
    let mut segments = Vec::new();
    let mut quote: Option<char> = None;
    let mut start = 0usize;
    let mut chars = command.char_indices().peekable();
    while let Some((pos, character)) = chars.next() {
        if let Some(open) = quote {
            if character == open {
                quote = None;
            }
            continue;
        }
        match character {
            '\'' | '"' => quote = Some(character),
            '|' | '<' | '>' | '`' | '\n' => return None,
            '$' if chars.peek().map(|&(_, next)| next) == Some('(') => return None,
            '$' => {}
            ';' => {
                let segment = command[start..pos].trim();
                if segment.is_empty() {
                    return None;
                }
                segments.push(segment);
                start = pos + character.len_utf8();
            }
            '&' => {
                if chars.peek().map(|&(_, next)| next) != Some('&') {
                    return None;
                }
                let (next_pos, next_char) = chars.next().expect("peeked Some above");
                let segment = command[start..pos].trim();
                if segment.is_empty() {
                    return None;
                }
                segments.push(segment);
                start = next_pos + next_char.len_utf8();
            }
            _ => {}
        }
    }
    if quote.is_some() {
        return None;
    }
    let last = command[start..].trim();
    if last.is_empty() {
        return None;
    }
    segments.push(last);
    Some(segments)
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
