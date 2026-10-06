//! Which of the sidecar's own environment variables never reach an adapter.
//!
//! The sidecar's environment is its identity. `BUZZ_PRIVATE_KEY` is the raw
//! nsec the provider signs with, and a consumer *fail-closed trusts* that
//! pubkey for the four provider-authored kinds — 44222 catalog, 44223
//! metadata, 44224 receipts, 44225 transcript. Anything holding that key can
//! mint transcript history the UI renders as genuine provider fact.
//! `BUZZ_AUTH_TAG` is the NIP-OA owner attestation for the same key, so a
//! forgery carrying both also carries the human owner's delegated standing.
//!
//! A coding-session execution is explicitly **not** a managed agent: the
//! metadata it produces always carries `agentRef: null`, and the field's
//! comment in `buzz-core` says exactly that. It is work the provider
//! supervises, not a Buzz participant acting as itself. So it must not hold
//! the provider's identity — and until this module existed it held all of it,
//! because an ACP spawn inherits the parent environment wholesale and the
//! provider passed only additive per-runtime variables on top.
//!
//! # Why a namespace fence and not an allowlist
//!
//! `buzz-terminal`'s `env_fence` clears the child environment and rebuilds it
//! from an eight-key allowlist, on the argument that a denylist is only as
//! current as the last person who remembered to extend it. That argument is
//! right, and it is why [`PREFIXES`] rather than a list of known-bad names
//! carries the weight here: everything spelled `BUZZ_*` is fenced, including
//! keys invented after this was written.
//!
//! The allowlist *shape* does not transfer. A terminal needs eight variables;
//! a coding agent runs the developer's real toolchain in a real checkout and
//! needs `PATH`, `HOME`, `SSH_AUTH_SOCK`, language-runtime configuration, and
//! the adapter's own credential cache. Clearing that wholesale does not
//! produce a safer agent, it produces one that cannot build. So: exhaustive
//! over the namespace Buzz owns, enumerated for the handful of third-party
//! secrets that reach the sidecar from a developer's `.env` without carrying
//! the prefix.
//!
//! # Consequence, intended
//!
//! The `bee` CLI no longer authenticates from inside a coding-session agent's
//! shell. It used to work by accident, by reading the provider's key out of
//! the inherited environment — which made every session's agent
//! indistinguishable from the provider itself on the wire. A coding-session
//! execution that should speak to Buzz gets its **own** identity through
//! `agent_ref`; it does not borrow the provider's.

use std::path::{Component, Path, PathBuf};

use buzz_acp::acp::EnvFence;

use crate::git_exclude::ExcludeOutcome;

/// The namespace Buzz owns end to end.
///
/// Nothing under it is useful to an ACP adapter: the adapter learns what to run
/// from argv and the additive per-runtime environment, and the `BUZZ_CSP_*`
/// surface configures the sidecar, not its children.
const PREFIXES: &[&str] = &["BUZZ_"];

/// Secrets that reach the sidecar without the `BUZZ_` prefix.
///
/// These arrive from a developer's `.env` by way of the launching shell rather
/// than from anything the desktop sets deliberately, and they authorize direct
/// writes to the media store and the search index — a path that bypasses relay
/// authorization entirely.
///
/// The two `BEEKEEPER_HOST_*` key variables are here for the same reason: the
/// agent host reads the provider's key from them and clears them before it
/// spawns this sidecar (`beekeeper_host_core::env::INHERITED_KEYS_TO_CLEAR`),
/// but a provider started any other way could still inherit one. The names are
/// kept byte-for-byte with `beekeeper_host_core::layout`.
const KEYS: &[&str] = &[
    "BEEKEEPER_HOST_PRIVATE_KEY",
    "BEEKEEPER_HOST_KEY_FILE",
    "NOSTR_PRIVATE_KEY",
    "TYPESENSE_API_KEY",
    "S3_ACCESS_KEY",
    "S3_SECRET_KEY",
];

/// Variables under a fenced prefix that are passed through anyway.
///
/// Deliberately empty. It exists so that admitting a non-secret `BUZZ_*` the
/// adapter genuinely needs is a one-line, reviewable exception rather than a
/// reason to weaken [`PREFIXES`]. Do not add a credential here.
const EXEMPT: &[&str] = &[];

/// The tools a seated execution must not use, named in its own briefing.
///
/// Seats run with the operator's `HOME`, so a harness that keeps its session
/// registry there — Claude Code's `~/.claude` is the live example — exposes
/// tools that reach *other* sessions on this machine without going near the
/// relay. `SendMessage` addresses another session directly, and nothing said
/// through it reaches the transcript the provider publishes, so it stays
/// fenced.
///
/// `Task` and `Agent` used to be on this list for the same reason: a
/// subagent's work arrived on the wire but the translator dropped it, so a
/// lead that delegated to one was coordinating off the record (ledger 77).
/// That is no longer true. Every subagent-attributed frame is now published
/// as an ordinary transcript item carrying `parentToolId`, nested under the
/// call that spawned it ([`crate::transcript`], ledger 308), so a subagent's
/// reads, edits and prose are as readable, citable and replayable as the
/// seat's own. A subagent also runs inside the seat — same working
/// directory, same write fence, same identity — so it is the seat's work, not
/// another session's. They are allowed.
///
/// # Enforced on claude-agent-acp; a briefing on codex-acp
///
/// For a seated execution on `claude-agent-acp`, [`crate::session`] passes
/// this list to `AcpClient::set_disallowed_tools`, which writes it to
/// `_meta.claudeCode.options.disallowedTools` on `session/new` (and on
/// `session/resume` / `session/load`, which rebuild the session from the same
/// `_meta`). The adapter merges it into the SDK query's `disallowedTools`
/// (`dist/acp-agent.js:4913` in 0.70.0), so the tools are absent from the
/// model's toolset, not merely discouraged.
///
/// **On codex-acp it is still only a briefing.** `codex-acp` 1.6.2 exposes no
/// per-session tool denial: its argv is `--client-name/--client-title/
/// --client-version` on `login`, it reads `CODEX_PATH` and `CODEX_CONFIG`, and
/// there is no `_meta` option of this shape. There is nothing to enforce with,
/// so a codex seat is held to this list by [`actor_seat_briefing`] alone.
/// Unseated executions are not fenced this way at all, by design.
///
/// # Mechanisms measured and rejected
///
/// Measured against `claude` 2.1.248 and `@agentclientprotocol/claude-agent-acp`
/// 0.70.0 on 2026-08-28. Neither of these works; do not retry them:
///
/// - A per-seat `CLAUDE_CONFIG_DIR` holding a `permissions.deny` settings
///   file. Relocating it makes `claude` answer `Not logged in · Please run
///   /login`, because the credential it reads out of the macOS keychain is
///   keyed by the configuration home. Seeding the new directory with the
///   operator's `.claude.json`, and pointing `CLAUDE_SECURESTORAGE_CONFIG_DIR`
///   back at `~/.claude`, both still fail.
/// - `CLAUDE_CODE_MANAGED_SETTINGS_PATH`. The name exists in the binary but a
///   settings file supplied that way had no effect on the resolved
///   permissions; the same file passed as `--settings` removed the denied tool
///   from the toolset outright. The adapter offers no argv for it.
///
/// So the per-session `_meta` option is the only mechanism that denies these
/// tools without breaking the seat's login, and it is the one wired up.
/// [`actor_seat_briefing`] must keep naming the tools regardless: on codex it
/// is the whole fence, and on claude a named rule beats a tool that has simply
/// vanished without explanation.
pub(crate) const SEAT_OUT_OF_BOUNDS_TOOLS: &[&str] = &["SendMessage"];

/// The fence every adapter this sidecar spawns is subject to.
pub(crate) const FENCE: EnvFence = EnvFence {
    keys: KEYS,
    prefixes: PREFIXES,
    exempt: EXEMPT,
};

/// The fenced briefing's first four paragraphs, shared by both runtimes' variants.
macro_rules! fenced_session_briefing_body {
    () => {
        "Buzz coding-session briefing: you are running inside a Buzz coding session, launched and supervised by the Buzz session provider.\n\nThe provider's Buzz identity is not yours. Every BUZZ_* variable is deliberately removed from this process's environment before you start, so the `bee` CLI cannot authenticate from your shell and this session holds no relay credentials. Do not run `bee` commands that talk to the relay, do not go looking for a key in .env, ~/.config/buzz/, or the environment, and do not report the missing key as a misconfiguration — the absence is the design, not a broken setup.\n\nYou do not need those credentials to be seen. The provider itself observes and publishes this session's state — branch, HEAD commit, dirty worktree, and verified liveness — so when this session's channel belongs to a project, that published state is what the project's Pulse and Beekeeper Desktop show for you. Routine progress needs no post from you.\n\nYou will not receive a Project Pulse digest in this session and you cannot read one from here. If you need to know what other sessions or people are working on before you touch shared code, say so and ask your operator in this conversation: they can see the Pulse and can post an entry on your behalf."
    };
}

/// What a fenced adapter is told about the consequence above, in its own words.
///
/// The fence is invisible from inside the adapter: it sees an environment with
/// no `BUZZ_*` in it and no explanation, so an operator who asks it about Buzz
/// coordination gets "unavailable — nothing is configured", which reads as a
/// broken install rather than a deliberate boundary. That is exactly what
/// happened on 2026-08-21: a session asked to read its Project Pulse reported
/// the empty key files as a misconfiguration.
///
/// So the provider says it. Every rule here is a fact about this process, not
/// an aspiration:
///
/// - It cannot authenticate `bee` — [`FENCE`] removes the whole namespace
///   before the adapter is spawned (`session.rs`, `spawn_with_env_fence`).
/// - Its session state *is* published: the provider observes branch, `HEAD`
///   commit and dirty state itself ([`crate::git_probe`]) and publishes them
///   as kind:44223 session metadata, which is what the Project Pulse digest
///   folds into its session groups (`buzz-acp/src/pulse_fetch.rs`).
/// - It receives no `[Project Pulse]` injection — that is composed in the ACP
///   harness's pool for managed agents (`buzz-acp/src/pool.rs`), a path a
///   coding session never takes.
///
/// Consequently this text must never instruct the adapter to run a relay
/// command, and `buzz-acp`'s base prompt — written for the *unfenced* managed
/// agents, which do inherit the harness's credentials — must keep instructing
/// exactly that. `the_fenced_briefing_never_tells_a_session_to_write_the_pulse`
/// pins both halves.
pub(crate) const FENCED_SESSION_BRIEFING: &str = concat!(
    fenced_session_briefing_body!(),
    "\n\n",
    "Run long work in the foreground and wait for it. Do not detach a build, a test run, or any other command into the background and end your turn promising to report back when it finishes — nothing will wake you to do so, and your operator will be left watching a session that looks busy and has nothing left to say. If something takes a long time, run it in the foreground with an explicit timeout, or run it in pieces you can report on as you go."
);

/// The fenced briefing for a Claude Code execution (SV-115).
///
/// Claude Code wakes itself when a task it started with the Bash tool's
/// background option completes, and since SV-77 the provider reads that
/// wake between prompts and publishes it as its own turn
/// (`session_autonomous.rs`; live proof `buzz-acp/tests/live_background_wake.rs`).
/// Telling a Claude execution that nothing will wake it is therefore false,
/// and on 2026-10-06 it made an agent refuse an explicit instruction to
/// background a job. Other runtimes keep [`FENCED_SESSION_BRIEFING`]: nothing
/// shows that `codex-acp` wakes on a background completion.
const FENCED_CLAUDE_SESSION_BRIEFING: &str = concat!(
    fenced_session_briefing_body!(),
    "\n\n",
    "Prefer the foreground for short work. A command you start with your shell tool's background option (run_in_background) is different: when it finishes, its completion wakes you as a turn of your own, and this session shows the task as running until then, so you may background a long build or test run and report on it when it completes. A process you detach yourself (`&`, `nohup`, `disown`) does not wake you, and neither does anything else outside this session — so never end your turn promising a report that depends on something that will not wake you, or your operator will be left watching a session that looks busy and has nothing left to say."
);

/// The fenced briefing for the runtime `driver` names.
pub(crate) fn fenced_session_briefing(driver: &str) -> &'static str {
    if driver == CLAUDE_DRIVER {
        FENCED_CLAUDE_SESSION_BRIEFING
    } else {
        FENCED_SESSION_BRIEFING
    }
}

/// The same briefing, for an execution that **is** an agent seat.
///
/// The fence still runs — every `BUZZ_*` the sidecar holds is still removed —
/// and then exactly four variables are put back, all of them the *seat's* own
/// ([`crate::actor_seats::ActorSeat::post_fence_env`]). So the unseated
/// briefing above is now a lie for this process, in the specific way this
/// project treats as a bug: it would tell an agent that holds working relay
/// credentials that it holds none, and a competent agent would then either
/// refuse to use them or report a misconfiguration that is not one.
///
/// This text therefore says the true thing instead, and says it narrowly:
///
/// - The identity in this shell is **the seat's**, named by pubkey, and the
///   relay it authenticates against is named too — a seat that does not know
///   which community it is speaking into cannot tell a sibling from a
///   stranger.
/// - It is *not* the provider's identity. The distinction matters because the
///   provider signs the transcript and metadata a human reads as fact; a seat
///   that believed it could sign those would be forging its own record.
/// - The role it holds, so a crew's conventions have something to attach to.
/// - The same background-work rule the unseated briefing carries, per
///   runtime (SV-115): a Claude seat is told its own background task's
///   completion wakes it; any other runtime is told only an addressed relay
///   turn does, because nothing shows it waking any other way.
///
/// - The relay is the only channel out. A seat runs under the operator's
///   `HOME`, so the harness's own cross-session tool
///   ([`SEAT_OUT_OF_BOUNDS_TOOLS`]) can reach other sessions on this machine
///   directly — and on 2026-08-27 a lead used one to dispatch a builder, and
///   the two seats then talked twice outside the relay. Nothing said that way
///   is in the transcript the provider publishes, so it cannot be read, cited
///   or replayed. On codex this sentence is the only thing standing there,
///   and it says so plainly rather than implying the tool is absent.
/// - Subagents (`Task`/`Agent`) are allowed for quick research and side work,
///   because their work is published attributed inside this seat's transcript
///   (ledger 308). Hiring is named as the tool for what a subagent cannot
///   give: independent checking, a different model, its own sandbox, or long
///   parallel work. Without that line a seat would either avoid subagents it
///   may use or reach for one where a separate seat's independence was the
///   point.
/// - After dispatching, end the turn. The report comes back as its own
///   addressed turn.
/// - Reports arrive as turns, so polling the inbox inside a turn reads the
///   same report the relay is about to deliver — which is exactly how the
///   2026-08-27 lead came to read every report twice and call it relay
///   redelivery.
///
/// It deliberately does **not** hand out a task list of `bee` commands. What a
/// seat may usefully do with its identity is a slice-4 question (`bee sessions
/// send`, the inbox, the roster); promising verbs that do not exist yet would
/// reproduce the 2026-08-21 failure in the opposite direction.
pub(crate) fn actor_seat_briefing(
    actor_pubkey: &str,
    role: &str,
    relay_url: &str,
    driver: &str,
) -> String {
    let out_of_bounds = SEAT_OUT_OF_BOUNDS_TOOLS.join(", ");
    // SV-115: a Claude seat is woken by its own background task finishing;
    // no other runtime is shown to be, so they keep the conservative rule.
    let background_work = if driver == CLAUDE_DRIVER {
        "Prefer the foreground for short work. A command you start with your shell tool's background option (run_in_background) is different: when it finishes, its completion wakes you as a turn of your own, and this session shows the task as running until then, so you may background a long build or test run and report on it when it completes. A process you detach yourself (`&`, `nohup`, `disown`) does not wake you, and neither does a relay message you are not addressed in - so never end your turn promising a report that depends on something that will not wake you, or whoever is waiting on you is left watching a seat that looks busy and has nothing left to say."
    } else {
        "Run long work in the foreground and wait for it. Do not detach a build, a test run, or any other command into the background and end your turn promising to report back when it finishes - only an addressed relay turn wakes you, and a background job finishing is not one, so whoever is waiting on you is left watching a seat that looks busy and has nothing left to say. If something takes a long time, run it in the foreground with an explicit timeout, or run it in pieces you can report on as you go."
    };
    format!(
        "Buzz coding-session briefing: you are running inside a Buzz coding session, launched and supervised by the Buzz session provider, and you are seated in it as a Buzz agent.\n\nYou hold your own Buzz identity in this shell: public key {actor_pubkey}, seated with the role \"{role}\", authenticated against the relay at {relay_url}. Run the `bee` CLI as `$BEE` - your host chose one binary, set $BEE to its absolute path, and put its directory first on your PATH, so `$BEE` and a bare `bee` are the same build. Never a path someone typed at you, and never a path from a transcript. It speaks as that identity. Those credentials are yours, not the provider's: the session provider signs this session's transcript, metadata, and receipts with a different key, and nothing you publish can claim to be provider-authored fact.\n\nEvery other Buzz variable is removed from this process's environment before you start, so anything under BUZZ_* that you cannot find is deliberately absent rather than misconfigured. Do not go looking for additional keys in .env, ~/.config/buzz/, or the environment, and never write your own key anywhere - not into a file in the working tree, not into a commit, and not into anything you post.\n\nThe provider itself observes and publishes this session's state - branch, HEAD commit, dirty worktree, and verified liveness - so routine progress needs no post from you. You will not receive a Project Pulse digest in this session.\n\nThe relay is the only channel to other seats and to the operator. Cross-session tools - {out_of_bounds} - are out of bounds in this seat: they reach other sessions on this computer directly, and nothing said through them appears in the transcript this session publishes, so no one can read it, cite it, or replay it. Say it over the relay or it did not happen.\n\nSubagents (the Task/Agent tool) are allowed for quick research and side tasks inside this seat: they run in your working directory under your rules, and their work is published in this session's transcript under the call that spawned them. Hire a seat instead (`bee sessions hire`) when the work needs independent checking, a different model, its own sandbox, or long parallel work.\n\nWrite files only inside your working directory. Everything else on this computer - other projects, the operator's ~/.claude, ~/.codex, ~/.config, ~/.nostr and ~/.ssh, this app's own data, other seats' directories - is somebody else's, and the notes and memory files there are theirs, not a place to record your conclusions. Whether the host enforces that is stated separately in this briefing.\n\nAfter you dispatch work to another seat, end your turn. An addressed relay turn wakes you, so the reply arrives as a turn of its own; holding this turn open to wait for it only leaves the seat busy with nothing to say.\n\nReports arrive as turns. Do not poll `bee sessions inbox` inside a turn looking for one - you will read the same report the relay is about to hand you and count it twice.\n\n{background_work}"
    )
}

// ---------------------------------------------------------------------------
// The write fence: where a seat's file tools may not reach.
// ---------------------------------------------------------------------------

/// The runtime whose file tools honour the write fence.
///
/// The desktop's runtime table names the Claude driver exactly this
/// (`desktop/src-tauri/src/session_provider/runtimes.rs:46`), and it arrives
/// on every create as `CodingSessionTarget::driver`. `codex-acp` is the other
/// driver and gets nothing here, by design: it has no settings file of this
/// shape, so a codex seat is held to the write boundary by
/// [`actor_seat_briefing`] alone, exactly as it is held to
/// [`SEAT_OUT_OF_BOUNDS_TOOLS`].
pub(crate) const CLAUDE_DRIVER: &str = "claude-agent-acp";

/// The file the fence is written to, relative to the seat's working directory.
///
/// # Why a fence at all
///
/// Finding 73 (live run 6, 2026-09-04 10:29): a seated lead running under
/// claude-agent-acp edited a file under the operator's
/// `~/.claude/projects/…/memory/` — the orchestrator's own notes — and asserted
/// a false conclusion there. A seat runs with the operator's `HOME`, so its
/// file tools reach whatever the operator can, and the sidecar auto-approves
/// every `session/request_permission` with `allow_once`
/// (`buzz-acp/src/acp.rs`, the `session/request_permission` arm). Nothing
/// stood between the tool call and the write.
///
/// # What stands there now
///
/// Claude Code's own permission rules. A `permissions.deny` entry of the form
/// `Edit(//absolute/path/**)` is evaluated by the CLI before any
/// `canUseTool` callback, so the sidecar's auto-approval never sees the call:
/// the tool returns `File is in a directory that is denied by your permission
/// settings.` and nothing is written. Measured on 2026-09-05 against `claude`
/// 2.1.232 — the binary `@anthropic-ai/claude-agent-sdk` 0.3.232 bundles
/// under `@agentclientprotocol/claude-agent-acp` 0.70.0, which is what the
/// adapter runs unless the desktop names another — under
/// `--dangerously-skip-permissions`, so a denial could not be mistaken for a
/// prompt nobody answered. What the measurements settled:
///
/// - `Edit(//root/**)` denies `Write` and `Edit` alike, dotfiles included,
///   and files in subdirectories that did not exist yet. `NotebookEdit` hit
///   its read-first precondition before the permission check and is unproven.
/// - `Write(//root/**)` on its own denies **nothing**. The CLI routes every
///   file-editing tool through the `Edit` rules, so `Edit(...)` is the only
///   rule shape emitted here; a `Write(...)` rule would be decoration.
/// - `Edit(//root/*)` denies `root/deep/x.txt` too: a single `*` is not
///   bounded to one path segment. So there is no rule that denies a
///   directory's direct children while leaving a grandchild alone, and an
///   exemption *below* a denied root has to be spelled as rules for the root's
///   other entries ([`write_fence_rules`]). There is no negation either, which
///   is why the fence names concrete roots rather than "outside the worktree".
/// - `//` is the absolute-path prefix; a pattern with no prefix is relative to
///   the project, and a glob segment (`io.agiterra.*/**`) matches.
///
/// # Why this file, and not `_meta`
///
/// The adapter offers two ways in. `_meta.claudeCode.options.settings` is
/// forwarded to the SDK's `settings` option (`dist/acp-agent.js:4829`,
/// `:4839`, `:4871` in 0.70.0) — the flag-settings layer, which the seat cannot
/// reach at all. That transport is measured working and is the better one, but
/// `buzz-acp`'s `AcpClient` has no setter for it (only `set_disallowed_tools`),
/// and that crate is outside this change. The project-local settings file is
/// the other way in: the adapter loads `settingSources: ["user", "project",
/// "local"]` (`dist/acp-agent.js:4868`), the SDK reads `local` as
/// `<cwd>/.claude/settings.local.json` (`sdk.d.ts:1983`; the adapter's own
/// `dist/settings.js:80` reads the same path), and a deny list in that file
/// was measured refusing the same writes the flag layer refused. So the
/// provider writes the file before the child exists.
///
/// # What the file cannot do
///
/// It lives inside the worktree, so the seat can reach it. The file tools are
/// refused by a rule in the file itself (`Edit(.claude/settings.local.json)`,
/// measured), and every spawn rewrites the fence, but `Bash` can edit or
/// delete it — and `Bash` can write anywhere regardless of any of this. The
/// fence governs the file tools, which is where finding 73 happened; the shell
/// is the named gap. Moving the same rules onto `_meta` closes the first half
/// of it and is the intended next step; [`write_fence_rules`] is written so
/// that only the transport changes.
pub(crate) const WRITE_FENCE_SETTINGS_FILE: &str = ".claude/settings.local.json";

/// The line that keeps [`WRITE_FENCE_SETTINGS_FILE`] out of `git status`.
///
/// Same reason [`crate::git_exclude`] exists (finding 76): an untracked file
/// in a seated worktree makes every gate row read `dirty`, and the relay
/// refuses an observed-dirty push. Claude Code does not exclude the file
/// itself — measured: `info/exclude` was untouched after a run that read it.
pub(crate) const WRITE_FENCE_EXCLUDE_LINE: &str = ".claude/settings.local.json";

/// The tool every write-fence rule is written against; see
/// [`WRITE_FENCE_SETTINGS_FILE`] for why it is the only one that works.
const WRITE_FENCE_RULE_TOOL: &str = "Edit";

/// The file-editing tools whose stale rules are pruned when they would fence
/// the seat out of its own tree. Claude Code applies `Edit` rules to all four.
const FILE_EDIT_TOOLS: &[&str] = &["Edit", "Write", "MultiEdit", "NotebookEdit"];

/// Directories under the operator's home that no seat may write into.
///
/// The concrete dangerous roots, because "everything outside the worktree"
/// has no rule shape. `~/.claude` is where finding 73 happened; `~/.codex`
/// and `~/.config` are the other harness's home and `bee`'s; `~/.nostr` and
/// `~/.ssh` hold keys; `~/.beekeeper` and `~/.beekeeper-dev` are the shared
/// nests every managed agent used to run in.
const HOME_DENIED_ROOTS: &[&str] = &[
    ".claude",
    ".codex",
    ".config",
    ".nostr",
    ".ssh",
    ".beekeeper",
    ".beekeeper-dev",
];

/// macOS application-support directory, relative to the home.
const APP_SUPPORT_DIR: &str = "Library/Application Support";

/// The identifier prefix of every data directory this app has had —
/// `io.agiterra.beekeeper.app`, its `.dev` sibling, and the renamed copies
/// beside them. Denied as a glob unless the seat's own tree is under one of
/// them, in which case the matching directories are enumerated instead.
const APP_IDENTIFIER_PREFIX: &str = "io.agiterra.";

/// The app-support directory the desktop keeps its node tools and runtimes
/// in — the adapter this very fence is read by lives there.
const APP_SUPPORT_TOOLS_DIR: &str = "Beekeeper";

/// `<app data dir>/agents/nests` — kept byte-for-byte in step with
/// `AGENT_NESTS_DIR` in `desktop/src-tauri/src/managed_agents/agent_nest.rs`.
const AGENT_NESTS_DIR: &str = "agents/nests";

/// A nest is named by the first 8 hex of the agent's pubkey
/// (`agent_nest.rs`, `NEST_KEY_PREFIX_LEN`).
const NEST_NAME_LEN: usize = 8;

/// The state directory's parent, as the desktop lays it out:
/// `<app data dir>/session-provider/<provider pubkey>` is `BUZZ_CSP_STATE_DIR`
/// (`desktop/src-tauri/src/session_provider/mod.rs`, "Layout on disk").
pub(crate) const SESSION_PROVIDER_DIR: &str = "session-provider";

/// What the fence is computed from: the seat's own directories, and the
/// operator's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WriteFenceLayout {
    /// The seat's working directory. Never denied, in whole or in part.
    pub cwd: PathBuf,
    /// The git worktree the cwd sits in — the nearest ancestor holding a
    /// `.git` entry, below the home — when that is not the cwd itself. Never
    /// denied either: a seat started in a subdirectory of its own tree was
    /// otherwise fenced *around* that subdirectory, which denies every other
    /// entry of the seat's own worktree (control run 7).
    pub seat_tree: Option<PathBuf>,
    /// The operator's home.
    pub home: PathBuf,
    /// `<app data dir>`, when `BUZZ_CSP_STATE_DIR` had the shape the desktop
    /// gives it; `None` when the provider was launched some other way.
    pub app_data_dir: Option<PathBuf>,
    /// The seat's own nest, `<app data dir>/agents/nests/<first 8 hex>`. Never
    /// denied, whether or not it exists yet.
    pub nest: Option<PathBuf>,
}

impl WriteFenceLayout {
    /// The layout for this host: `HOME` and `BUZZ_CSP_STATE_DIR` from the
    /// environment, exactly the way [`crate::session::shared_workdir_roots`]
    /// reads the home.
    ///
    /// Errors when `HOME` is unset — a fence that cannot name the operator's
    /// directories is not a fence, and the seat must not launch as if it had
    /// one.
    pub(crate) fn from_host(cwd: &Path, actor_pubkey: &str) -> std::io::Result<Self> {
        let home = std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .filter(|home| !home.as_os_str().is_empty())
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    "HOME is not set, so the operator's directories cannot be named",
                )
            })?;
        let state_dir = std::env::var_os("BUZZ_CSP_STATE_DIR").map(PathBuf::from);
        Ok(Self::new(cwd, &home, state_dir.as_deref(), actor_pubkey))
    }

    /// The layout as data, so it can be proved against directories a test
    /// owns.
    pub(crate) fn new(
        cwd: &Path,
        home: &Path,
        state_dir: Option<&Path>,
        actor_pubkey: &str,
    ) -> Self {
        let app_data_dir = state_dir.and_then(app_data_dir_from_state_dir);
        let nest = app_data_dir.as_ref().and_then(|app| {
            nest_name(actor_pubkey).map(|name| app.join(AGENT_NESTS_DIR).join(name))
        });
        Self {
            cwd: cwd.to_path_buf(),
            seat_tree: enclosing_git_tree(cwd, home),
            home: home.to_path_buf(),
            app_data_dir,
            nest,
        }
    }

    /// The directories no rule may cover: the seat's own.
    fn protected(&self) -> Vec<&Path> {
        let mut protected = vec![self.cwd.as_path()];
        if let Some(tree) = &self.seat_tree {
            protected.push(tree.as_path());
        }
        if let Some(nest) = &self.nest {
            protected.push(nest.as_path());
        }
        protected
    }
}

/// The nearest strict ancestor of `cwd` holding a `.git` entry (a directory
/// for a checkout, a file for a linked worktree), searched no higher than
/// below `home` — the home itself is never a seat's tree. `None` when `cwd`
/// is itself the tree's root, or sits in no tree.
fn enclosing_git_tree(cwd: &Path, home: &Path) -> Option<PathBuf> {
    if cwd.join(".git").exists() {
        return None;
    }
    cwd.ancestors()
        .skip(1)
        .take_while(|ancestor| *ancestor != home && ancestor.starts_with(home))
        .find(|ancestor| ancestor.join(".git").exists())
        .map(Path::to_path_buf)
}

/// `<app data dir>` from `BUZZ_CSP_STATE_DIR`, or `None` when the directory
/// is not shaped `<app data dir>/session-provider/<pubkey>`.
///
/// Shared with [`crate::session::seat_bundle_dir`]: a seat's nest and a seat's
/// skill bundle are both app data, and deriving the same root twice from the
/// same string is how two directories that must agree stop agreeing.
pub(crate) fn app_data_dir_from_state_dir(state_dir: &Path) -> Option<PathBuf> {
    let provider_root = state_dir.parent()?;
    (provider_root.file_name()? == SESSION_PROVIDER_DIR)
        .then(|| provider_root.parent().map(Path::to_path_buf))
        .flatten()
}

/// The nest directory name for a pubkey, or `None` when it is not a
/// 64-character lowercase hex key.
fn nest_name(pubkey: &str) -> Option<&str> {
    (pubkey.len() == 64
        && pubkey
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
    .then(|| &pubkey[..NEST_NAME_LEN])
}

/// The deny rules for a layout, in a stable order, deduplicated.
///
/// Every root in [`HOME_DENIED_ROOTS`], the app-support directories, and the
/// app data dir is denied whole — unless the seat's working directory or nest
/// lies under it, in which case the root's other entries are denied one by
/// one and the root itself is never named. That is the only exemption shape
/// Claude Code's rules can express (see [`WRITE_FENCE_SETTINGS_FILE`]), and
/// it is a snapshot: an entry created beside the nest after this ran is not
/// covered until the next spawn.
///
/// Reads the filesystem only when an exemption forces the enumeration; a root
/// that is absent is denied anyway, so it stays denied when it appears.
pub(crate) fn write_fence_rules(layout: &WriteFenceLayout) -> std::io::Result<Vec<String>> {
    let protected = layout.protected();
    let mut rules = Vec::new();
    for name in HOME_DENIED_ROOTS {
        deny_root(&layout.home.join(name), &protected, &mut rules)?;
    }
    let support = layout.home.join(APP_SUPPORT_DIR);
    deny_root(&support.join(APP_SUPPORT_TOOLS_DIR), &protected, &mut rules)?;
    let seat_under_an_app_home = protected.iter().any(|path| {
        path.strip_prefix(&support)
            .ok()
            .and_then(|rest| rest.components().next())
            .is_some_and(|first| {
                first
                    .as_os_str()
                    .to_string_lossy()
                    .starts_with(APP_IDENTIFIER_PREFIX)
            })
    });
    if seat_under_an_app_home {
        for (entry, _) in read_dir_sorted(&support)? {
            let is_app_home = entry
                .file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(APP_IDENTIFIER_PREFIX));
            if is_app_home {
                deny_root(&entry, &protected, &mut rules)?;
            }
        }
    } else {
        push_rule(
            &mut rules,
            format!(
                "{WRITE_FENCE_RULE_TOOL}(//{}/{APP_IDENTIFIER_PREFIX}*/**)",
                absolute_pattern_path(&support)
            ),
        );
    }
    if let Some(app_data_dir) = &layout.app_data_dir {
        deny_root(app_data_dir, &protected, &mut rules)?;
    }
    Ok(rules)
}

/// Deny `root`, or — when a protected directory lies under it — its other
/// entries, recursively. A protected directory itself yields nothing.
fn deny_root(root: &Path, protected: &[&Path], rules: &mut Vec<String>) -> std::io::Result<()> {
    if protected.contains(&root) {
        return Ok(());
    }
    if protected.iter().any(|path| path.starts_with(root)) {
        for (entry, is_dir) in read_dir_sorted(root)? {
            if is_dir {
                deny_root(&entry, protected, rules)?;
            } else {
                push_rule(rules, rule_for(&entry, false));
            }
        }
        return Ok(());
    }
    push_rule(rules, rule_for(root, true));
    Ok(())
}

/// The entries of `dir` with whether each is a directory, sorted by path so
/// the rule list is stable. An absent `dir` has no entries; a symlink counts
/// as a file, so it is denied by name rather than followed.
fn read_dir_sorted(dir: &Path) -> std::io::Result<Vec<(PathBuf, bool)>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut listed = Vec::new();
    for entry in entries {
        let entry = entry?;
        let is_dir = entry.file_type()?.is_dir();
        listed.push((entry.path(), is_dir));
    }
    listed.sort();
    Ok(listed)
}

/// `Edit(//path/**)` for a directory, `Edit(//path)` for a file.
fn rule_for(path: &Path, is_dir: bool) -> String {
    let suffix = if is_dir { "/**" } else { "" };
    format!(
        "{WRITE_FENCE_RULE_TOOL}(//{}{suffix})",
        absolute_pattern_path(path)
    )
}

/// The path as it appears after the `//` prefix: no leading separator.
fn absolute_pattern_path(path: &Path) -> String {
    path.to_string_lossy().trim_start_matches('/').to_owned()
}

fn push_rule(rules: &mut Vec<String>, rule: String) {
    if !rules.contains(&rule) {
        rules.push(rule);
    }
}

/// The absolute pattern inside a file-tool rule, or `None` for any other rule
/// (another tool, or a project-relative pattern).
fn file_tool_absolute_pattern(rule: &str) -> Option<&str> {
    FILE_EDIT_TOOLS.iter().find_map(|tool| {
        rule.strip_prefix(tool)?
            .strip_prefix("(//")?
            .strip_suffix(')')
    })
}

/// Does this rule deny `path` — the path itself, or a directory above it?
///
/// A rule for something *under* `path` does not cover it. Glob segments are
/// matched conservatively (a `*` matches any run of characters within one
/// segment), so a stale `io.agiterra.*/**` is recognized as covering a nest
/// under `io.agiterra.beekeeper.app`. Used for the two things that must never
/// happen: emitting such a rule, and leaving one behind from an earlier seat.
pub(crate) fn rule_covers(rule: &str, path: &Path) -> bool {
    let Some(pattern) = file_tool_absolute_pattern(rule) else {
        return false;
    };
    let pattern = pattern
        .strip_suffix("/**")
        .or_else(|| pattern.strip_suffix("/*"))
        .unwrap_or(pattern);
    let pattern_segments: Vec<&str> = pattern.split('/').filter(|s| !s.is_empty()).collect();
    let path_segments: Vec<String> = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(segment) => Some(segment.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect();
    if pattern_segments.len() > path_segments.len() {
        return false;
    }
    pattern_segments
        .iter()
        .zip(&path_segments)
        .all(|(pattern, segment)| segment_matches(pattern, segment))
}

/// One path segment against one pattern segment.
fn segment_matches(pattern: &str, segment: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == segment,
        Some((prefix, rest)) => {
            let Some(after) = segment.strip_prefix(prefix) else {
                return false;
            };
            if rest.contains('*') {
                // More than one wildcard: assume it matches. Conservative in
                // the only direction that matters — a rule is *pruned* or
                // *refused* on a match, never admitted.
                true
            } else {
                after.ends_with(rest)
            }
        }
    }
}

/// What [`install_write_fence`] did to the settings file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WriteFenceOutcome {
    /// The file already said exactly this; nothing was written.
    Unchanged,
    /// The file was created or rewritten.
    Written,
}

/// Write `rules` into `<cwd>/.claude/settings.local.json`.
///
/// Idempotent, and never a clobber: the file's other keys, `permissions`'s
/// other keys, and every existing `deny` entry survive — except a file-tool
/// rule that covers the seat's working directory or nest, which is dropped.
/// (A worktree handed from one seat to another carries the first seat's
/// fence, and that fence denied the second seat's nest.) The self-protection
/// rule for the file itself is added alongside. A file that is not a JSON
/// object, or whose `permissions` / `permissions.deny` are not an object /
/// an array, is refused rather than overwritten.
///
/// The rewrite is a temp file and a rename; a file whose rendered content is
/// already identical is not touched at all.
pub(crate) fn install_write_fence(
    layout: &WriteFenceLayout,
    rules: &[String],
) -> std::io::Result<WriteFenceOutcome> {
    use serde_json::{Map, Value};

    let settings_file = layout.cwd.join(WRITE_FENCE_SETTINGS_FILE);
    let existing = match std::fs::read_to_string(&settings_file) {
        Ok(content) => Some(content),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
        Err(error) => return Err(error),
    };
    let malformed = |what: &str| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "{} {what}; refusing to overwrite it",
                settings_file.display()
            ),
        )
    };
    let mut root: Map<String, Value> = match &existing {
        None => Map::new(),
        Some(content) => match serde_json::from_str::<Value>(content) {
            Ok(Value::Object(map)) => map,
            Ok(_) => return Err(malformed("is not a JSON object")),
            Err(error) => return Err(malformed(&format!("is not JSON ({error})"))),
        },
    };
    let mut permissions = match root.remove("permissions") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(map)) => map,
        Some(_) => return Err(malformed("has a `permissions` that is not an object")),
    };
    let existing_deny = match permissions.remove("deny") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items,
        Some(_) => return Err(malformed("has a `permissions.deny` that is not an array")),
    };

    let protected = layout.protected();
    let mut deny: Vec<Value> = existing_deny
        .into_iter()
        .filter(|item| {
            !item
                .as_str()
                .is_some_and(|rule| protected.iter().any(|path| rule_covers(rule, path)))
        })
        .collect();
    let self_protection = format!("{WRITE_FENCE_RULE_TOOL}({WRITE_FENCE_SETTINGS_FILE})");
    for rule in rules.iter().chain(std::iter::once(&self_protection)) {
        let value = Value::String(rule.clone());
        if !deny.contains(&value) {
            deny.push(value);
        }
    }
    permissions.insert("deny".to_owned(), Value::Array(deny));
    root.insert("permissions".to_owned(), Value::Object(permissions));

    let mut rendered = serde_json::to_string_pretty(&Value::Object(root))
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
    rendered.push('\n');
    if existing.as_deref() == Some(rendered.as_str()) {
        return Ok(WriteFenceOutcome::Unchanged);
    }
    let dir = settings_file
        .parent()
        .ok_or_else(|| std::io::Error::other("settings file has no parent directory"))?;
    std::fs::create_dir_all(dir)?;
    let temp = dir.join(format!(".settings.local.json.{}.tmp", std::process::id()));
    std::fs::write(&temp, rendered.as_bytes())?;
    std::fs::rename(&temp, &settings_file)?;
    Ok(WriteFenceOutcome::Written)
}

/// Keep [`WRITE_FENCE_SETTINGS_FILE`] out of `git status` in `cwd`.
///
/// One line through the shared appender in [`crate::git_exclude`], which owns
/// the `git rev-parse --git-path info/exclude` resolution and the repo-
/// selection variables that make it answer for the right repository. This was
/// a second copy of that resolution until the seat's skills moved out of the
/// worktree and left this as the only caller.
///
/// Non-fatal for its caller: a fence that cannot be excluded is a fence that
/// works and a gate row that reads dirty, not a failed spawn.
fn exclude_write_fence_file(cwd: &Path) -> std::io::Result<ExcludeOutcome> {
    crate::git_exclude::append_exclude_line(cwd, WRITE_FENCE_EXCLUDE_LINE)
}

/// What installing a seat's write fence produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstalledWriteFence {
    /// The settings file the rules were written to.
    pub settings_file: PathBuf,
    /// The rules, in the order written.
    pub rules: Vec<String>,
    /// Whether the file changed.
    pub outcome: WriteFenceOutcome,
    /// What happened to the worktree's git exclude.
    pub exclude: ExcludeOutcome,
}

/// Install the write fence for a Claude seat in `cwd`, and say what was done.
///
/// The one call the session path makes: layout from the host, rules from the
/// layout, the settings file written, the file excluded from `git status`.
/// An exclude failure is logged, not fatal — the fence is on disk regardless,
/// and a seat whose pushes are refused with a reason is not a seat that lies.
/// The rules that deny a directory the seat may read but not write — its
/// read-only agents clone (spec § 4.11) — whole, like a protected root.
pub(crate) fn read_only_root_rules(roots: &[PathBuf]) -> Vec<String> {
    roots.iter().map(|root| rule_for(root, true)).collect()
}

pub(crate) fn install_seat_write_fence(
    cwd: &Path,
    actor_pubkey: &str,
    read_only_roots: &[PathBuf],
) -> std::io::Result<InstalledWriteFence> {
    let layout = WriteFenceLayout::from_host(cwd, actor_pubkey)?;
    let mut rules = write_fence_rules(&layout)?;
    for rule in read_only_root_rules(read_only_roots) {
        push_rule(&mut rules, rule);
    }
    let outcome = install_write_fence(&layout, &rules)?;
    let settings_file = layout.cwd.join(WRITE_FENCE_SETTINGS_FILE);
    let exclude = match exclude_write_fence_file(cwd) {
        Ok(exclude) => exclude,
        Err(error) => {
            tracing::warn!(
                target: "csp::session",
                cwd = %cwd.display(),
                "could not add `{WRITE_FENCE_EXCLUDE_LINE}` to the worktree's git exclude — \
                 every gate row for this seat will read dirty: {error}"
            );
            ExcludeOutcome::NotARepository
        }
    };
    tracing::info!(
        target: "csp::session",
        settings_file = %settings_file.display(),
        rules = rules.len(),
        written = outcome == WriteFenceOutcome::Written,
        nest = ?layout.nest,
        app_data_dir = ?layout.app_data_dir,
        excluded = ?exclude,
        "seat write fence installed"
    );
    Ok(InstalledWriteFence {
        settings_file,
        rules,
        outcome,
        exclude,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor_seats::ActorSeat;
    use crate::git_probe::GIT_REPO_SELECTION_VARS;

    /// The credentials named in the finding, one assertion each so a
    /// regression names the variable it re-exposed.
    #[test]
    fn the_provider_identity_and_infrastructure_secrets_are_fenced() {
        for key in [
            "BUZZ_PRIVATE_KEY",
            "BUZZ_ACP_PRIVATE_KEY",
            "BUZZ_AUTH_TAG",
            "BUZZ_API_TOKEN",
            "BUZZ_S3_ACCESS_KEY",
            "BUZZ_S3_SECRET_KEY",
            "BUZZ_DEV_KEYRING_SERVICE",
            "BEEKEEPER_HOST_PRIVATE_KEY",
            "BEEKEEPER_HOST_KEY_FILE",
            "NOSTR_PRIVATE_KEY",
            "TYPESENSE_API_KEY",
            "S3_ACCESS_KEY",
            "S3_SECRET_KEY",
        ] {
            assert!(FENCE.covers(key), "{key} escaped the fence");
        }
    }

    /// The property an enumerated denylist cannot offer: a credential invented
    /// tomorrow is fenced for where it lives, not because someone listed it.
    #[test]
    fn an_unknown_buzz_variable_is_fenced_on_its_prefix_alone() {
        assert!(FENCE.covers("BUZZ_SOME_FUTURE_CREDENTIAL"));
    }

    /// The rule the 2026-08-21 finding cost us: never instruct an agent to do
    /// something its own environment forbids.
    ///
    /// Both halves are asserted together, in one test, because the defect was
    /// the *pair* drifting apart — a write instruction that is correct for the
    /// unfenced managed agents leaking into the fenced coding-session path.
    #[test]
    fn the_fenced_briefing_never_tells_a_session_to_write_the_pulse() {
        for forbidden in [
            "bee pulse update",
            "bee pulse digest",
            "bee pulse list",
            "bee pulse sessions",
            "BUZZ_PULSE_PROJECT",
        ] {
            assert!(
                !FENCED_SESSION_BRIEFING.contains(forbidden),
                "the fenced briefing tells a credential-less session to run `{forbidden}`"
            );
        }
        assert!(
            FENCED_SESSION_BRIEFING.contains("cannot authenticate"),
            "the fenced briefing must say why `bee` will not work here"
        );
        assert!(
            FENCED_SESSION_BRIEFING.contains("ask your operator"),
            "the fenced briefing must name the path that does work"
        );
        // Same rule, second instance: the provider cannot deliver anything the
        // agent produces after a turn ends — nothing reads the adapter's
        // output between turns — so an agent must never be left believing it
        // can detach work and report back later.
        assert!(
            FENCED_SESSION_BRIEFING.contains("foreground"),
            "the fenced briefing must tell a session not to detach long work it \
             cannot be woken to report on"
        );

        // The unfenced audience keeps the instruction: a managed ACP agent
        // inherits the harness's credentials and is the only writer Pulse has.
        assert!(
            buzz_acp::BASE_PROMPT.contains("post it yourself with `bee pulse update`"),
            "the managed-agent base prompt lost its Pulse write instruction"
        );
    }

    /// The seated half of the same rule: a briefing must describe the process
    /// it is actually installed in.
    ///
    /// The unseated text says `bee` cannot authenticate, which is true for a
    /// fenced execution and false for a seat that was handed its own key — so
    /// the two variants are asserted against each other here, not separately,
    /// because the defect this pins is the pair drifting into agreement.
    #[test]
    fn the_actor_seat_briefing_states_the_identity_the_seat_actually_holds() {
        let actor = "cd".repeat(32);
        let briefing = actor_seat_briefing(&actor, "lead", "wss://relay.example", CLAUDE_DRIVER);

        assert!(briefing.contains(&actor), "the seat is not named");
        assert!(briefing.contains("wss://relay.example"), "no relay named");
        assert!(briefing.contains("\"lead\""), "no role named");
        assert!(
            briefing.contains("You hold your own Buzz identity"),
            "the seated briefing must say the shell is authenticated"
        );

        // The exact sentence the unseated briefing exists to deliver must not
        // survive into the seated one: it is false here.
        assert!(
            !briefing.contains("cannot authenticate"),
            "the seated briefing repeats the fenced briefing's claim"
        );
        assert!(
            FENCED_SESSION_BRIEFING.contains("cannot authenticate"),
            "the unseated briefing must still say why `bee` will not work there"
        );

        // Neither variant promises Pulse writes, and neither leaks a key.
        for text in [FENCED_SESSION_BRIEFING.to_owned(), briefing.clone()] {
            for forbidden in ["bee pulse update", "bee pulse digest", "nsec1"] {
                assert!(
                    !text.contains(forbidden),
                    "a session briefing contains `{forbidden}`"
                );
            }
        }

        // The seat is told the boundary that makes the provider's record
        // trustworthy: its key is not the provider's.
        assert!(
            briefing.contains("not the provider's"),
            "the seated briefing must separate the seat from the provider"
        );

        // The one rule that is identical for both variants, asserted on both
        // in the same test for the same reason as the pair above: nothing
        // reads the adapter's output between turns, seated or not, so an
        // agent must never be left believing it can detach work and be woken
        // to report on it. A seat is the *more* dangerous case — a sibling or
        // a lead may be blocked waiting on the report that never comes.
        for (label, text) in [
            ("fenced", FENCED_SESSION_BRIEFING.to_owned()),
            ("seated", briefing.clone()),
        ] {
            assert!(
                text.contains("foreground"),
                "the {label} briefing lost the do-not-detach rule"
            );
        }
    }

    /// Ledger 77 (*Fence*, a): a lead dispatched a builder through a local
    /// tool and the two seats then talked twice outside the relay. The
    /// briefing must name the tool rather than leave the seat to infer it.
    /// Ledger 308: subagents are allowed now that their work is published
    /// attributed, so `SendMessage` is the one tool still fenced.
    #[test]
    fn the_seat_briefing_names_the_tools_that_bypass_the_relay() {
        let briefing = actor_seat_briefing(
            &"cd".repeat(32),
            "lead",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        for tool in SEAT_OUT_OF_BOUNDS_TOOLS {
            assert!(
                briefing.contains(tool),
                "the seated briefing does not name `{tool}`"
            );
        }
        assert_eq!(
            SEAT_OUT_OF_BOUNDS_TOOLS,
            &["SendMessage"],
            "only the cross-session tool is out of bounds for a seat"
        );
        assert!(
            briefing.contains("The relay is the only channel to other seats"),
            "the seated briefing must say where coordination happens"
        );
        assert!(
            briefing.contains("out of bounds"),
            "the seated briefing must say the tools are forbidden, not absent"
        );
    }

    /// Ledger 308: the seat is told subagents are allowed, what for, and when
    /// to hire instead — and is not told they are out of bounds.
    #[test]
    fn the_seat_briefing_allows_subagents_and_says_when_to_hire() {
        let briefing = actor_seat_briefing(
            &"cd".repeat(32),
            "lead",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        assert!(briefing.contains("Subagents (the Task/Agent tool) are allowed"));
        assert!(briefing.contains("quick research and side tasks"));
        assert!(briefing.contains("`bee sessions hire`"));
        for reason in [
            "independent checking",
            "a different model",
            "its own sandbox",
            "long parallel work",
        ] {
            assert!(briefing.contains(reason), "hire reason missing: {reason}");
        }
        assert!(
            briefing.contains("Cross-session tools - SendMessage - are out of bounds"),
            "SendMessage must still be named as out of bounds"
        );
    }

    /// Ledger 77 (*Lead pack*, c): after dispatching, end the turn — the
    /// report arrives as its own addressed turn.
    #[test]
    fn the_seat_briefing_tells_a_dispatcher_to_end_its_turn() {
        let briefing = actor_seat_briefing(
            &"cd".repeat(32),
            "lead",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        assert!(
            briefing.contains("end your turn"),
            "the seated briefing must tell a dispatching seat to end the turn"
        );
        assert!(
            briefing.contains("An addressed relay turn wakes you"),
            "the seated briefing must say what does wake a seat"
        );
    }

    /// Same finding, second half: polling the inbox inside a turn reads the
    /// report the relay is about to deliver, which is how the 2026-08-27 lead
    /// counted every report twice.
    #[test]
    fn the_seat_briefing_forbids_polling_the_inbox_inside_a_turn() {
        let briefing = actor_seat_briefing(
            &"cd".repeat(32),
            "lead",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        assert!(
            briefing.contains("Reports arrive as turns"),
            "the seated briefing must say how a report arrives"
        );
        assert!(
            briefing.contains("Do not poll `bee sessions inbox` inside a turn"),
            "the seated briefing must forbid polling the inbox inside a turn"
        );
    }

    /// Ledger 77 (*Seat briefing*): "nothing wakes you between turns" is false
    /// for a seat, and the do-not-detach rule must survive without it.
    #[test]
    fn the_seated_do_not_detach_rule_no_longer_rests_on_a_falsehood() {
        let briefing = actor_seat_briefing(
            &"cd".repeat(32),
            "lead",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        assert!(
            briefing.contains("foreground"),
            "the seated briefing lost the do-not-detach rule"
        );
        for falsehood in [
            "nothing will wake you",
            "nothing reads this process between turns, so nothing will wake you",
            "Nothing wakes you between turns",
        ] {
            assert!(
                !briefing.contains(falsehood),
                "the seated briefing still claims `{falsehood}`, which an \
                 addressed relay turn disproves"
            );
        }
        let conservative =
            actor_seat_briefing(&"cd".repeat(32), "lead", "wss://relay.example", "codex-acp");
        assert!(
            conservative.contains("a background job finishing is not one"),
            "a non-Claude seat must still be told why detaching fails it"
        );
    }

    /// The sentence a Claude seat is told about background work (SV-115).
    const CLAUDE_BACKGROUND_WAKE: &str = "when it finishes, its completion wakes you as a turn of your own, and this session shows the task as running until then";

    /// SV-115: since SV-77 a Claude Code execution wakes on its own background
    /// task's completion and the provider publishes that turn live, so telling
    /// it "a background job finishing is not one" made an agent refuse an
    /// explicit instruction to background a job. Both Claude variants say the
    /// true thing; every other runtime keeps the conservative rule, because
    /// nothing shows `codex-acp` waking on a background completion.
    #[test]
    fn a_claude_briefing_says_its_background_task_wakes_it() {
        let seated = actor_seat_briefing(
            &"cd".repeat(32),
            "lead",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        let fenced = fenced_session_briefing(CLAUDE_DRIVER);
        for (label, text) in [("seated", seated.as_str()), ("fenced", fenced)] {
            assert!(
                text.contains(CLAUDE_BACKGROUND_WAKE),
                "the Claude {label} briefing must say a background completion wakes it"
            );
            for falsehood in [
                "a background job finishing is not one",
                "nothing will wake you to do so",
            ] {
                assert!(
                    !text.contains(falsehood),
                    "the Claude {label} briefing still claims `{falsehood}`"
                );
            }
            assert!(
                text.contains("Prefer the foreground for short work"),
                "the Claude {label} briefing must still prefer the foreground"
            );
            assert!(
                text.contains("never end your turn promising a report that depends on something that will not wake you"),
                "the Claude {label} briefing must still forbid an unkeepable promise"
            );
        }
        assert!(
            seated.contains("a relay message you are not addressed in"),
            "the Claude seat must be told an unaddressed relay reply does not wake it"
        );
    }

    #[test]
    fn a_non_claude_briefing_keeps_the_conservative_background_rule() {
        for driver in ["codex-acp", "some-future-runtime"] {
            let seated =
                actor_seat_briefing(&"cd".repeat(32), "lead", "wss://relay.example", driver);
            assert!(
                seated.contains("only an addressed relay turn wakes you, and a background job finishing is not one"),
                "the {driver} seat lost the conservative rule"
            );
            assert!(!seated.contains(CLAUDE_BACKGROUND_WAKE));
            let fenced = fenced_session_briefing(driver);
            assert_eq!(fenced, FENCED_SESSION_BRIEFING);
            assert!(fenced.contains("nothing will wake you to do so"));
            assert!(!fenced.contains(CLAUDE_BACKGROUND_WAKE));
        }
        // The two fenced variants differ only in the background paragraph.
        let claude = fenced_session_briefing(CLAUDE_DRIVER);
        let split = FENCED_SESSION_BRIEFING
            .find("\n\nRun long work in the foreground")
            .expect("conservative paragraph");
        assert!(claude.starts_with(&FENCED_SESSION_BRIEFING[..split + 2]));
    }

    /// The fence itself is unchanged by seating: `EXEMPT` stays empty, so a
    /// seat's variables arrive by explicit post-fence injection and never by
    /// weakening the namespace rule.
    #[test]
    fn seating_an_agent_never_widens_the_fence() {
        assert!(
            EXEMPT.is_empty(),
            "an exemption was added instead of a post-fence injection"
        );
        for key in ["BUZZ_PRIVATE_KEY", "BUZZ_RELAY_URL", "BUZZ_AUTH_TAG"] {
            assert!(FENCE.covers(key), "{key} is no longer fenced");
        }
    }

    /// `BEE` and `PATH` are outside the `BUZZ_` namespace on purpose, and the
    /// fence must not start covering either.
    ///
    /// Fencing `BEE` would strip the host's choice back out; fencing `PATH`
    /// would leave the seat with no binaries at all. Both are supplied
    /// **post-fence** ([`crate::actor_seats::ActorSeat::post_fence_env_with_bee`]),
    /// which is also the only reason an operator's ambient `BEE` cannot
    /// survive: the fence would have left it alone.
    #[test]
    fn the_bee_the_host_chose_is_outside_the_fence_and_injected_over_it() {
        for key in [crate::seat_bee::BEE_ENV, "PATH"] {
            assert!(
                !FENCE.covers(key),
                "{key} was fenced, so the host's chosen bee could never reach the seat"
            );
        }
        let seat = ActorSeat {
            pubkey: "ab".repeat(32),
            nsec: "nsec1secret".to_owned(),
            auth_tag: None,
            relay_url: "wss://relay.example".to_owned(),
            display_name: None,
            pack_dir: None,
            persona_id: None,
            pack_ref: None,
            agents_checkout: None,
        };
        let bee = crate::seat_bee::SeatBee {
            path: std::path::PathBuf::from("/Applications/Beekeeper.app/Contents/MacOS/bee"),
            source: buzz_core::coding_session_payload::BeeStampSource::Bundled,
        };
        let inherited = std::env::join_paths([std::path::Path::new("/usr/bin")]).expect("a PATH");
        let env = seat.post_fence_env_with_bee(Some("builder"), None, Some(&bee), Some(&inherited));
        assert_eq!(
            env.iter()
                .find(|(key, _)| key == crate::seat_bee::BEE_ENV)
                .map(|(_, value)| value.as_str()),
            Some("/Applications/Beekeeper.app/Contents/MacOS/bee"),
            "the post-fence list must carry the binary the host chose: {env:?}"
        );
        assert!(
            env.iter().any(|(key, value)| key == "PATH"
                && value.starts_with("/Applications/Beekeeper.app/Contents/MacOS:")),
            "the chosen directory must lead the seat's PATH: {env:?}"
        );
        // A host with no `bee` adds nothing: the seat keeps exactly the
        // environment it has today rather than an invented one.
        assert_eq!(
            seat.post_fence_env_with_bee(Some("builder"), None, None, Some(&inherited)),
            seat.post_fence_env_in_project(Some("builder"), None),
        );
    }

    /// The briefing cannot be skipped, so it is the half of the `$BEE` rule
    /// that ships regardless of whether a pack repeats it.
    #[test]
    fn the_seated_briefing_names_the_variable_rather_than_a_bare_command() {
        let briefing = actor_seat_briefing(
            &"ab".repeat(32),
            "builder",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        assert!(
            briefing.contains("Run the `bee` CLI as `$BEE`"),
            "the briefing must name the variable the host actually sets"
        );
        assert!(
            briefing.contains("never a path from a transcript"),
            "the briefing must refuse the prose path that was honoured only sometimes"
        );
    }

    /// Over-fencing is the other failure mode: an agent that cannot find its
    /// toolchain is as broken as one that can sign as the provider.
    #[test]
    fn the_developer_toolchain_environment_survives() {
        for key in [
            "PATH",
            "HOME",
            "SHELL",
            "SSH_AUTH_SOCK",
            "TMPDIR",
            "CLAUDE_CODE_EXECUTABLE",
            "ANTHROPIC_API_KEY",
            "CODEX_CONFIG",
            "HERMES_ACP_SKIP_CONFIGURED_MCP",
        ] {
            assert!(
                !FENCE.covers(key),
                "{key} was fenced but the agent needs it"
            );
        }
    }

    // ----------------------------------------------------------------------
    // The write fence (finding 73).
    // ----------------------------------------------------------------------

    /// A pubkey whose nest is `aaaaaaaa`.
    fn seat_pubkey() -> String {
        "aa".repeat(32)
    }

    /// The rule that denies `path` whole, as the fence spells it.
    fn dir_rule(path: &Path) -> String {
        rule_for(path, true)
    }

    /// Every rule must leave the seat's own directories alone: nothing may
    /// cover them, and nothing may name them.
    fn assert_seat_tree_untouched(rules: &[String], protected: &[&Path]) {
        for rule in rules {
            for path in protected {
                assert!(
                    !rule_covers(rule, path),
                    "rule `{rule}` covers the seat's own {}",
                    path.display()
                );
                assert!(
                    !rule.contains(&path.to_string_lossy().into_owned()),
                    "rule `{rule}` names the seat's own {}",
                    path.display()
                );
            }
        }
    }

    /// The operator's home as finding 73 found it, plus the app's data dir,
    /// with two nests in it: this seat's and a sibling's.
    fn operator_layout(dir: &Path) -> (WriteFenceLayout, PathBuf) {
        let home = dir.join("home");
        for path in [".claude/projects/x/memory", ".config/buzz", ".ssh"] {
            std::fs::create_dir_all(home.join(path)).expect("home dir");
        }
        let app = home.join(APP_SUPPORT_DIR).join("io.agiterra.beekeeper.app");
        for path in [
            "agents/nests/aaaaaaaa",
            "agents/nests/bbbbbbbb",
            "agents/logs",
            "session-provider/provider-pk",
            "shell-sessions",
        ] {
            std::fs::create_dir_all(app.join(path)).expect("app dir");
        }
        std::fs::write(app.join("agents/managed-agents.json"), "{}").expect("store");
        std::fs::write(app.join("coding-session-workdirs.json"), "{}").expect("workdirs");
        std::fs::create_dir_all(
            home.join(APP_SUPPORT_DIR)
                .join("io.agiterra.beekeeper.app.dev"),
        )
        .expect("dev app dir");
        let cwd = dir.join("work");
        std::fs::create_dir_all(&cwd).expect("cwd");
        let state_dir = app.join("session-provider/provider-pk");
        let layout = WriteFenceLayout::new(&cwd, &home, Some(&state_dir), &seat_pubkey());
        (layout, app)
    }

    /// Finding 73's own path is denied, every other operator root with it,
    /// and the seat's worktree and nest are never covered — the nest by
    /// enumerating what sits beside it rather than by naming its parents.
    #[test]
    fn the_write_fence_denies_the_operator_roots_and_never_the_seat_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (layout, app) = operator_layout(dir.path());
        assert_eq!(layout.app_data_dir.as_deref(), Some(app.as_path()));
        assert_eq!(
            layout.nest.as_deref(),
            Some(app.join("agents/nests/aaaaaaaa").as_path())
        );

        let rules = write_fence_rules(&layout).expect("rules");
        let home = &layout.home;

        // Where the lead wrote on 2026-09-04, and the rest of the named roots
        // — present on disk or not.
        for root in HOME_DENIED_ROOTS {
            assert!(
                rules.contains(&dir_rule(&home.join(root))),
                "`{root}` is not denied: {rules:#?}"
            );
        }
        assert!(
            rules.contains(&dir_rule(&home.join(APP_SUPPORT_DIR).join("Beekeeper"))),
            "the node-tools directory is not denied: {rules:#?}"
        );
        // The seat's nest is under `io.agiterra.beekeeper.app`, so the glob is
        // replaced by the app homes one by one: the `.dev` sibling whole, this
        // one by its entries.
        assert!(
            !rules.iter().any(|rule| rule.contains("io.agiterra.*")),
            "the app-home glob would deny the nest: {rules:#?}"
        );
        assert!(rules.contains(&dir_rule(
            &home
                .join(APP_SUPPORT_DIR)
                .join("io.agiterra.beekeeper.app.dev")
        )));
        for entry in [
            "session-provider",
            "shell-sessions",
            "agents/logs",
            "agents/nests/bbbbbbbb",
        ] {
            assert!(
                rules.contains(&dir_rule(&app.join(entry))),
                "`{entry}` beside the nest is not denied: {rules:#?}"
            );
        }
        for file in ["coding-session-workdirs.json", "agents/managed-agents.json"] {
            assert!(
                rules.contains(&rule_for(&app.join(file), false)),
                "the store file `{file}` is not denied: {rules:#?}"
            );
        }
        // Never the parents of the nest, and never the nest.
        for parent in ["", "agents", "agents/nests"] {
            assert!(
                !rules.contains(&dir_rule(&app.join(parent))),
                "`{parent}` above the nest is denied whole: {rules:#?}"
            );
        }
        assert_seat_tree_untouched(&rules, &layout.protected());

        // Stable and duplicate-free, so two spawns write the same file.
        let again = write_fence_rules(&layout).expect("rules again");
        assert_eq!(rules, again);
        let mut deduped = rules.clone();
        deduped.dedup();
        assert_eq!(rules, deduped);
    }

    /// Mutation test: a seat whose worktree sits *inside* a denied root is
    /// fenced around, never out. No rule names the worktree, its parents, or
    /// the root above it; the root's other entries are what gets denied.
    #[test]
    fn a_seat_working_inside_a_denied_root_is_fenced_around_not_out() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let cwd = home.join(".config/checkouts/feature");
        std::fs::create_dir_all(&cwd).expect("cwd");
        std::fs::create_dir_all(home.join(".config/checkouts/other")).expect("sibling");
        std::fs::create_dir_all(home.join(".config/buzz")).expect("buzz");
        std::fs::write(home.join(".config/checkouts/notes.md"), "").expect("file");
        let layout = WriteFenceLayout::new(&cwd, &home, None, &seat_pubkey());

        let rules = write_fence_rules(&layout).expect("rules");
        assert_seat_tree_untouched(&rules, &[cwd.as_path()]);
        for never in [
            dir_rule(&cwd),
            dir_rule(&home.join(".config/checkouts")),
            dir_rule(&home.join(".config")),
            dir_rule(&home),
        ] {
            assert!(!rules.contains(&never), "`{never}` was emitted: {rules:#?}");
        }
        for still in [
            dir_rule(&home.join(".config/buzz")),
            dir_rule(&home.join(".config/checkouts/other")),
            rule_for(&home.join(".config/checkouts/notes.md"), false),
            dir_rule(&home.join(".claude")),
        ] {
            assert!(rules.contains(&still), "`{still}` is missing: {rules:#?}");
        }
        // No state dir: no app data dir, no nest, and the app-home glob is
        // safe to emit whole.
        assert_eq!(layout.app_data_dir, None);
        assert_eq!(layout.nest, None);
        assert!(rules.iter().any(|rule| rule.contains("io.agiterra.*/**")));
    }

    /// Control run 7: a seat under `~/.beekeeper-dev/REPOS/<x>-wt-*` must get
    /// no rule over its own tree — not when it runs at the tree's root, and
    /// not when it runs in a subdirectory of it, where fencing "around" the
    /// cwd used to deny the rest of the seat's own worktree.
    #[test]
    fn a_seat_under_a_repos_worktree_is_never_fenced_out_of_its_own_tree() {
        let dir = tempfile::tempdir().expect("tempdir");
        let home = dir.path().join("home");
        let repos = home.join(".beekeeper-dev/REPOS");
        let root = repos.join("kettle");
        let tree = repos.join("kettle-wt-coding-session-builder-1");
        std::fs::create_dir_all(root.join(".git")).expect("root");
        std::fs::create_dir_all(tree.join("src")).expect("tree");
        std::fs::write(tree.join(".git"), "gitdir: ../kettle/.git/worktrees/b\n").expect("gitfile");
        std::fs::write(tree.join("Cargo.toml"), "").expect("file");
        std::fs::create_dir_all(repos.join("kettle-wt-coding-session-verifier-1")).expect("peer");

        for cwd in [tree.clone(), tree.join("src")] {
            let layout = WriteFenceLayout::new(&cwd, &home, None, &seat_pubkey());
            let rules = write_fence_rules(&layout).expect("rules");
            for own in [&tree, &tree.join("Cargo.toml"), &tree.join("src/main.rs")] {
                assert!(
                    !rules.iter().any(|rule| rule_covers(rule, own)),
                    "cwd {} is fenced out of {}: {rules:#?}",
                    cwd.display(),
                    own.display()
                );
            }
            // Around it, not open: the project's checkout and a peer seat's
            // tree stay denied to this seat's file tools.
            assert!(rules.contains(&dir_rule(&root)), "{rules:#?}");
            assert!(
                rules.contains(&dir_rule(
                    &repos.join("kettle-wt-coding-session-verifier-1")
                )),
                "{rules:#?}"
            );
        }
    }

    /// The layout's derivation from what the desktop hands the provider.
    #[test]
    fn the_app_data_dir_and_nest_are_derived_from_the_state_dir() {
        let app = Path::new("/data/io.agiterra.beekeeper.app");
        let layout = WriteFenceLayout::new(
            Path::new("/work"),
            Path::new("/home/op"),
            Some(&app.join("session-provider/3728312c")),
            &seat_pubkey(),
        );
        assert_eq!(layout.app_data_dir.as_deref(), Some(app));
        assert_eq!(
            layout.nest.as_deref(),
            Some(app.join("agents/nests/aaaaaaaa").as_path())
        );
        // A state dir laid out some other way names no app data dir, and a
        // pubkey that is not one names no nest.
        let odd = WriteFenceLayout::new(
            Path::new("/work"),
            Path::new("/home/op"),
            Some(Path::new("/var/lib/csp/state")),
            &seat_pubkey(),
        );
        assert_eq!(
            odd,
            WriteFenceLayout::new(Path::new("/work"), Path::new("/home/op"), None, "x")
        );
        let not_hex = WriteFenceLayout::new(
            Path::new("/work"),
            Path::new("/home/op"),
            Some(&app.join("session-provider/pk")),
            "not-a-pubkey",
        );
        assert_eq!(not_hex.app_data_dir.as_deref(), Some(app));
        assert_eq!(not_hex.nest, None);
    }

    /// `rule_covers` is the guard both the emitter and the pruner rely on.
    #[test]
    fn rule_covers_reads_claude_code_patterns_the_way_the_fence_needs() {
        let nest = Path::new(
            "/h/Library/Application Support/io.agiterra.beekeeper.app/agents/nests/aaaaaaaa",
        );
        for (rule, covers) in [
            ("Edit(//h/Library/Application Support/io.agiterra.*/**)", true),
            ("Edit(//h/Library/Application Support/io.agiterra.beekeeper.app/**)", true),
            ("Write(//h/Library/Application Support/io.agiterra.beekeeper.app/agents/*)", true),
            ("Edit(//h/Library/Application Support/io.agiterra.beekeeper.app/agents/nests/aaaaaaaa/**)", true),
            ("Edit(//h/Library/Application Support/io.agiterra.beekeeper.app/agents/nests/aaaaaaaa)", true),
            ("Edit(//)", true),
            ("Edit(//h/Library/Application Support/io.agiterra.beekeeper.app/agents/nests/bbbbbbbb/**)", false),
            ("Edit(//h/Library/Application Support/io.agiterra.beekeeper.app/agents/nests/aaaaaaaa/sub/**)", false),
            ("Edit(//h/Library/Application Support/io.agiterra.beekeeper.app.dev/**)", false),
            ("Bash(rm:*)", false),
            ("Edit(.claude/settings.local.json)", false),
            ("Read(//h/**)", false),
        ] {
            assert_eq!(rule_covers(rule, nest), covers, "{rule}");
        }
    }

    /// The settings file is well-formed JSON, keeps every key it had, drops
    /// only a stale rule that would fence this seat out of its own tree, and
    /// is byte-identical after a second install.
    #[test]
    fn the_settings_file_is_well_formed_idempotent_and_keeps_other_keys() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (layout, _app) = operator_layout(dir.path());
        let settings_file = layout.cwd.join(WRITE_FENCE_SETTINGS_FILE);
        std::fs::create_dir_all(settings_file.parent().expect("parent")).expect(".claude");
        let stale_nest_rule = dir_rule(layout.nest.as_deref().expect("nest"));
        let stale_cwd_rule = format!("Write(//{}/**)", absolute_pattern_path(&layout.cwd));
        std::fs::write(
            &settings_file,
            format!(
                r#"{{"other": "kept", "permissions": {{"allow": ["Bash(ls:*)"], "deny": ["Bash(rm:*)", "{stale_nest_rule}", "{stale_cwd_rule}", "Edit(//{home}/.claude/**)"]}}}}"#,
                home = absolute_pattern_path(&layout.home)
            ),
        )
        .expect("seed");

        let rules = write_fence_rules(&layout).expect("rules");
        assert_eq!(
            install_write_fence(&layout, &rules).expect("install"),
            WriteFenceOutcome::Written
        );
        let first = std::fs::read_to_string(&settings_file).expect("read");
        let parsed: serde_json::Value = serde_json::from_str(&first).expect("well-formed JSON");
        assert_eq!(parsed["other"], "kept");
        assert_eq!(
            parsed["permissions"]["allow"],
            serde_json::json!(["Bash(ls:*)"])
        );
        let deny: Vec<&str> = parsed["permissions"]["deny"]
            .as_array()
            .expect("deny array")
            .iter()
            .map(|v| v.as_str().expect("string rule"))
            .collect();
        assert_eq!(deny[0], "Bash(rm:*)", "a foreign rule must keep its place");
        assert!(
            !deny.contains(&stale_nest_rule.as_str()),
            "stale nest rule kept: {deny:?}"
        );
        assert!(
            !deny.contains(&stale_cwd_rule.as_str()),
            "stale cwd rule kept: {deny:?}"
        );
        for rule in &rules {
            assert!(deny.contains(&rule.as_str()), "`{rule}` missing: {deny:?}");
        }
        assert_eq!(
            deny.iter().filter(|r| r.ends_with(".claude/**)")).count(),
            1,
            "a rule already present must not be duplicated: {deny:?}"
        );
        assert!(
            deny.contains(&"Edit(.claude/settings.local.json)"),
            "the file must protect itself from the file tools: {deny:?}"
        );
        assert!(first.ends_with('\n'));

        assert_eq!(
            install_write_fence(&layout, &rules).expect("install again"),
            WriteFenceOutcome::Unchanged
        );
        assert_eq!(
            std::fs::read_to_string(&settings_file).expect("read"),
            first
        );
        assert!(
            !std::fs::read_dir(settings_file.parent().expect("parent"))
                .expect("dir")
                .any(|entry| entry
                    .expect("entry")
                    .file_name()
                    .to_string_lossy()
                    .ends_with(".tmp")),
            "no temp file may be left behind"
        );
    }

    /// A file the fence cannot read as settings is refused, not replaced.
    #[test]
    fn a_malformed_settings_file_is_refused_not_overwritten() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (layout, _app) = operator_layout(dir.path());
        let settings_file = layout.cwd.join(WRITE_FENCE_SETTINGS_FILE);
        std::fs::create_dir_all(settings_file.parent().expect("parent")).expect(".claude");
        let rules = write_fence_rules(&layout).expect("rules");
        for (content, why) in [
            ("{not json", "is not JSON"),
            ("[]", "is not a JSON object"),
            (r#"{"permissions": "deny all"}"#, "not an object"),
            (r#"{"permissions": {"deny": "Edit(**)"}}"#, "not an array"),
        ] {
            std::fs::write(&settings_file, content).expect("seed");
            let error = install_write_fence(&layout, &rules).expect_err(why);
            assert_eq!(error.kind(), std::io::ErrorKind::InvalidData, "{content}");
            assert!(error.to_string().contains(why), "{error}");
            assert_eq!(
                std::fs::read_to_string(&settings_file).expect("read"),
                content,
                "the malformed file was overwritten"
            );
        }
    }

    /// `git` in `cwd`, identity forced and the repo-selection variables
    /// cleared, so a test never touches the developer's own repository.
    fn git(cwd: &Path, args: &[&str]) -> String {
        let mut command = std::process::Command::new("git");
        for var in GIT_REPO_SELECTION_VARS {
            command.env_remove(var);
        }
        let output = command
            .arg("-C")
            .arg(cwd)
            .args(args)
            .env("GIT_AUTHOR_NAME", "fence")
            .env("GIT_AUTHOR_EMAIL", "fence@example.invalid")
            .env("GIT_COMMITTER_NAME", "fence")
            .env("GIT_COMMITTER_EMAIL", "fence@example.invalid")
            // A developer's global ignore may already hide the file (this
            // machine's `~/.config/git/ignore` does), which would make the
            // "reads dirty first" precondition vacuous here and the fence
            // untested for the seat's machine. Git reads that file with or
            // without `core.excludesFile`, so both are pointed away.
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("XDG_CONFIG_HOME", cwd.join(".no-xdg-config"))
            .output()
            .expect("git");
        assert!(output.status.success(), "git {args:?} failed");
        String::from_utf8(output.stdout).expect("utf-8")
    }

    /// Finding 76 must not come back through this file: after the fence is
    /// installed the worktree is still clean by `git status --porcelain`.
    #[test]
    fn the_fence_file_is_kept_out_of_git_status() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (layout, _app) = operator_layout(dir.path());
        git(&layout.cwd, &["init", "-q"]);
        std::fs::write(layout.cwd.join("README"), "x").expect("tracked file");
        git(&layout.cwd, &["add", "README"]);
        git(&layout.cwd, &["commit", "-q", "-m", "seed"]);

        let rules = write_fence_rules(&layout).expect("rules");
        install_write_fence(&layout, &rules).expect("install");
        assert!(
            git(&layout.cwd, &["status", "--porcelain"]).contains(".claude/"),
            "the unexcluded file must read dirty for this test to mean anything"
        );
        let outcome = exclude_write_fence_file(&layout.cwd).expect("exclude");
        assert!(
            matches!(outcome, ExcludeOutcome::Added { .. }),
            "{outcome:?}"
        );
        assert_eq!(git(&layout.cwd, &["status", "--porcelain"]), "");
        assert!(matches!(
            exclude_write_fence_file(&layout.cwd).expect("exclude again"),
            ExcludeOutcome::AlreadyExcluded { .. }
        ));
        // Outside any repository there is nothing to exclude and no error.
        assert_eq!(
            exclude_write_fence_file(dir.path().join("home").as_path()).expect("no repo"),
            ExcludeOutcome::NotARepository
        );
    }

    /// What the fence would be on *this* machine — a diagnostic, not a gate.
    ///
    /// Ignored, so it runs only when asked:
    ///
    /// ```text
    /// FENCE_DEMO_CWD=/tmp/demo-worktree BUZZ_CSP_STATE_DIR=<app data>/session-provider/<pk> \
    ///   cargo test -p buzz-session-provider print_the_write_fence -- --ignored --nocapture
    /// ```
    ///
    /// Prints the rules for the real `HOME` and `BUZZ_CSP_STATE_DIR`, with a
    /// tempdir as the working directory. With `FENCE_DEMO_CWD` set it also
    /// installs the fence there, which is how the mechanism was proved against
    /// the real `claude` binary. Never writes anywhere else.
    #[test]
    #[ignore = "reads the real HOME; run by hand to see this host's fence"]
    fn print_the_write_fence_for_this_host() {
        let dir = tempfile::tempdir().expect("tempdir");
        let cwd = std::env::var_os("FENCE_DEMO_CWD")
            .map(PathBuf::from)
            .unwrap_or_else(|| dir.path().join("work"));
        std::fs::create_dir_all(&cwd).expect("cwd");
        let installed = install_seat_write_fence(&cwd, &seat_pubkey(), &[]).expect("install");
        println!("settings file: {}", installed.settings_file.display());
        println!(
            "outcome: {:?}, exclude: {:?}",
            installed.outcome, installed.exclude
        );
        for rule in &installed.rules {
            println!("{rule}");
        }
    }

    /// A read-only agents clone (spec § 4.11) is denied whole, in the same
    /// spelling as a protected root; a writable one adds no rule.
    #[test]
    fn a_read_only_agents_clone_is_fenced_whole() {
        let clone = PathBuf::from("/src/proj.worktrees/lane-agents");
        assert_eq!(
            read_only_root_rules(std::slice::from_ref(&clone)),
            vec!["Edit(//src/proj.worktrees/lane-agents/**)".to_owned()]
        );
        assert!(read_only_root_rules(&[]).is_empty());
    }

    /// The seated briefing states the write boundary in words, and defers
    /// the question of enforcement to the paragraph built from the actual
    /// execution plan (`session::boundary_briefing`) rather than claiming a
    /// mechanism it cannot know is in place.
    #[test]
    fn the_seat_briefing_states_the_write_boundary() {
        let briefing = actor_seat_briefing(
            &"cd".repeat(32),
            "lead",
            "wss://relay.example",
            CLAUDE_DRIVER,
        );
        assert!(
            briefing.contains("Write files only inside your working directory"),
            "the seated briefing does not state the write boundary"
        );
        for root in [
            "~/.claude",
            "~/.codex",
            "~/.config",
            "~/.nostr",
            "~/.ssh",
            "other projects",
        ] {
            assert!(briefing.contains(root), "the briefing does not name {root}");
        }
        assert!(
            !briefing.contains("permission rule") && !briefing.contains("whole fence"),
            "the seated briefing must not assert an enforcement mechanism by itself"
        );
        assert_eq!(CLAUDE_DRIVER, "claude-agent-acp");
    }
}
