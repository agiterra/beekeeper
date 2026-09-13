pub mod agent_management;
/// The build script's own commit-stamp resolution, compiled here only for its
/// tests — `build.rs` `include!`s the same file, and `cargo test` never runs a
/// build script.
#[cfg(test)]
mod build_provenance;
mod client;
mod commands;
mod error;
mod links;
mod validate;

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use client::BuzzClient;
use error::CliError;
use nostr::Keys;
use uuid::Uuid;

/// Run the Buzz CLI from raw arguments (including `argv[0]`).
///
/// Returns a process exit code (0 = success).
///
/// # Example
///
/// ```ignore
/// let code = buzz_cli::run_from_args(std::env::args()).await;
/// std::process::exit(code);
/// ```
pub async fn run_from_args<I, S>(args: I) -> i32
where
    I: IntoIterator<Item = S>,
    S: Into<std::ffi::OsString> + Clone,
{
    // Install ring as the process-level rustls CryptoProvider. Required because the
    // release workflow builds all binaries in one cargo invocation, which unifies
    // features across the workspace and enables *both* ring (from buzz-acp/buzz-dev-mcp)
    // and aws-lc-rs (from reqwest's rustls feature via hyper-rustls). With both on,
    // rustls cannot auto-select a provider, and any code that reaches
    // ClientConfig::builder() — specifically the WSS path in publish_ephemeral_event
    // used by `agents draft-create`, `agents draft-update`, and `users set-presence`
    // — panics at rustls crypto/mod.rs. The `let _ =` swallow is intentional: when
    // buzz-dev-mcp delegates to run_from_args, it has already installed ring; the
    // double-install returns Err and is harmless.
    let _ = rustls::crypto::ring::default_provider().install_default();

    // Parsed in two steps rather than one `Cli::try_parse_from`, for one fact
    // that the built `Cli` cannot carry: `--format` has a default, so
    // `cli.format` reads `Json` whether it was named or fell through, and
    // `sessions status` needs the difference (a named format suppresses the
    // NDJSON that a pipe otherwise gets — see `crew_cmds::resolve_json_lines`).
    // `ArgMatches::value_source` is the only thing that knows. Scanning
    // `std::env::args()` for `--format` would be wrong: `sessions transcript`
    // has its own unrelated `--format md|jsonl`.
    //
    // The error handling below is unchanged from the single-step form, and
    // must stay so: `--help`/`--version` print and exit 0, everything else is
    // a usage error on stderr and exit 1.
    let matches = match <Cli as clap::CommandFactory>::command().try_get_matches_from(args) {
        Ok(matches) => matches,
        Err(e) => {
            if e.use_stderr() {
                error::print_error(&CliError::Usage(e.to_string()));
                return 1;
            } else {
                // --help and --version: print normally (intentional human output)
                let _ = e.print();
                return 0;
            }
        }
    };
    let format_explicit =
        matches.value_source("format") == Some(clap::parser::ValueSource::CommandLine);
    let mut cli = match <Cli as clap::FromArgMatches>::from_arg_matches(&matches) {
        Ok(cli) => cli,
        Err(e) => {
            error::print_error(&CliError::Usage(e.to_string()));
            return 1;
        }
    };
    if let Cmd::Sessions(SessionsCmd::Status {
        format_explicit: explicit,
        ..
    }) = &mut cli.command
    {
        *explicit = format_explicit;
    }

    match run(cli).await {
        Ok(()) => 0,
        Err(e) => {
            error::print_error(&e);
            error::exit_code(&e)
        }
    }
}

/// This build's own provenance: crate version, the commit it was built from,
/// and when it was built.
///
/// A seat reaches whichever `bee` its `PATH` finds first — on 2026-09-01 that
/// was the desktop app's bundled sidecar, so a CLI fix that has landed in the
/// repo may still not be the one running. This is how a seat says which one it
/// ran, and since 2026-09-05 *when* it was built, which is the question a
/// bundle installed by `just app-from` raises and a commit alone cannot answer.
///
/// Both come from `buzz_core::build_info`, the same resolution the relay uses
/// for NIP-11 `software_commit` — so a client stamp and a relay stamp are
/// comparable field by field, and both disclose `unknown` rather than guess.
///
/// Two lines, and the split is load-bearing:
///
/// ```text
/// bee 0.1.0 (f723824d)
/// built 2026-09-05T14:02:11Z
/// ```
///
/// The first line's shape — `{crate version} ({stamp})`, the stamp being a
/// short hex commit, `<sha>-dirty`, or the literal `unknown` — is a contract
/// with `parse_bee_version` (`crates/buzz-session-provider/src/seat_bee.rs`),
/// which reads `output.lines().next()` and records "unknown" for anything it
/// does not recognise. Putting the build time inside those parentheses, as the
/// P1 brief asked, makes that parser return `None` and the beeStamp surface
/// silently lose the sha it has. So the time goes on its own line, where the
/// existing parser never looks, and the provider can start reading it whenever
/// its own lane chooses to.
pub static VERSION: std::sync::LazyLock<String> = std::sync::LazyLock::new(|| {
    let info = buzz_core::build_info::build_info();
    format!(
        "{} ({})\nbuilt {}",
        env!("CARGO_PKG_VERSION"),
        info.short_commit(),
        info.build_time
    )
});

#[derive(Parser)]
#[command(
    name = "bee",
    version = VERSION.as_str(),
    about = "Beekeeper CLI — interact with a Beekeeper relay",
    long_about = "\
Beekeeper CLI — interact with a Beekeeper relay

Configuration (flags override env vars):
  BUZZ_RELAY_URL     Relay base URL        [default: http://localhost:3000]
  BUZZ_PRIVATE_KEY   Nostr private key (hex or nsec)  [required]
  BUZZ_AUTH_TAG      NIP-OA auth tag JSON  [optional]

The 'pack' subcommand runs locally and does not require a relay connection.

Exit codes: 0=ok  1=bad input  2=relay/network error  3=auth error  4=other  5=write conflict
Errors are JSON on stderr: {\"error\": \"<category>\", \"message\": \"<detail>\"}"
)]
struct Cli {
    /// Relay URL (http:// or https://). Overrides BUZZ_RELAY_URL env var.
    #[arg(long, env = "BUZZ_RELAY_URL", default_value = "http://localhost:3000")]
    relay: String,

    /// Nostr private key (hex or nsec). This is the CLI's identity.
    #[arg(long, env = "BUZZ_PRIVATE_KEY", hide_env_values = true)]
    private_key: Option<String>,

    /// NIP-OA auth tag JSON (owner attestation). Injected into every signed event.
    #[arg(long, env = "BUZZ_AUTH_TAG", hide_env_values = true)]
    auth_tag: Option<String>,

    /// Output format: 'json' (default, full fields) or 'compact' (reduced fields).
    #[arg(long, value_enum, default_value = "json")]
    format: OutputFormat,

    #[command(subcommand)]
    command: Cmd,
}

#[derive(Clone, clap::ValueEnum)]
pub enum ChannelType {
    #[value(name = "stream")]
    Stream,
    #[value(name = "forum")]
    Forum,
}

impl std::fmt::Display for ChannelType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Stream => write!(f, "stream"),
            Self::Forum => write!(f, "forum"),
        }
    }
}

#[derive(Clone, clap::ValueEnum)]
pub enum ChannelVisibility {
    #[value(name = "open")]
    Open,
    #[value(name = "private")]
    Private,
}

impl std::fmt::Display for ChannelVisibility {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Open => write!(f, "open"),
            Self::Private => write!(f, "private"),
        }
    }
}

#[derive(Clone, clap::ValueEnum)]
pub enum PresenceStatus {
    #[value(name = "online")]
    Online,
    #[value(name = "away")]
    Away,
    #[value(name = "offline")]
    Offline,
}

#[derive(Clone, clap::ValueEnum)]
pub enum EmojiScope {
    #[value(name = "own")]
    Own,
    #[value(name = "workspace")]
    Workspace,
}

impl std::fmt::Display for PresenceStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Online => write!(f, "online"),
            Self::Away => write!(f, "away"),
            Self::Offline => write!(f, "offline"),
        }
    }
}

/// Output format for `sessions transcript`.
///
/// Distinct from [`OutputFormat`] because a transcript has two useful shapes
/// and neither is "the same JSON with fewer fields": `jsonl` is the archival
/// one (whole signed events, one per line, verifiable offline) and `md` is the
/// one a person reads.
#[derive(Clone, Copy, clap::ValueEnum, Default)]
pub enum TranscriptFormat {
    /// Rendered turns, tool calls, and results as markdown (default)
    #[default]
    #[value(name = "md")]
    Md,
    /// One raw signed event per line, signature included
    #[value(name = "jsonl")]
    Jsonl,
}

/// Output format for read commands.
#[derive(Clone, clap::ValueEnum, Default)]
pub enum OutputFormat {
    /// Full normalized JSON (default)
    #[default]
    #[value(name = "json")]
    Json,
    /// Reduced fields for agent scanning
    #[value(name = "compact")]
    Compact,
}

#[derive(Subcommand)]
enum Cmd {
    /// Draft owner-reviewed agent creation and updates
    #[command(subcommand)]
    Agents(AgentsCmd),
    /// Wait for exact, relay-confirmed CI results; register and inspect
    /// CI-managed turn continuations
    #[command(subcommand)]
    Ci(CiCmd),
    /// Send, read, search, and manage messages
    #[command(subcommand)]
    Messages(MessagesCmd),
    /// Create, configure, and manage channels
    #[command(subcommand)]
    Channels(ChannelsCmd),
    /// Get and set channel canvas documents
    #[command(subcommand)]
    Canvas(CanvasCmd),
    /// Add, remove, and list emoji reactions
    #[command(subcommand)]
    Reactions(ReactionsCmd),
    /// Manage your custom emoji set (workspace palette is the union of all members' sets)
    #[command(subcommand)]
    Emoji(EmojiCmd),
    /// List, open, and manage direct messages
    #[command(subcommand)]
    Dms(DmsCmd),
    /// Look up users and manage profiles and presence
    #[command(subcommand)]
    Users(UsersCmd),
    /// Create, trigger, and manage workflows
    #[command(subcommand)]
    Workflows(WorkflowsCmd),
    /// Read the activity feed
    #[command(subcommand)]
    Feed(FeedCmd),
    /// Publish notes and manage the social graph (NIP-01/02)
    #[command(subcommand)]
    Social(SocialCmd),
    /// Publish and edit long-form NIP-23 notes — team knowledge base
    #[command(subcommand)]
    Notes(NotesCmd),
    /// Announce and discover git repositories (NIP-34)
    #[command(subcommand)]
    Repos(ReposCmd),
    /// Create and manage multi-repo projects (NIP-MP)
    #[command(subcommand)]
    Projects(ProjectsCmd),
    /// Send, get, list, and set status on git patches (NIP-34)
    #[command(subcommand)]
    Patches(PatchesCmd),
    /// Create, get, list, and set status on git issues (NIP-34)
    #[command(subcommand)]
    Issues(IssuesCmd),
    /// Open, update, list, and set status on git pull requests (NIP-34)
    #[command(subcommand)]
    Pr(PrCmd),
    /// Upload and download relay Blossom media
    #[command(subcommand)]
    Media(MediaCmd),
    /// Upload files to the relay's Blossom store
    #[command(subcommand)]
    Upload(UploadCmd),
    /// Agent engram management — persistent memory per NIP-AE
    #[command(subcommand)]
    Mem(MemCmd),
    /// Persona pack operations (local, no relay connection needed)
    #[command(subcommand)]
    Pack(PackCmd),
    /// Say where a project's persona packs live, and what this machine would
    /// stage from them (NIP-PK, kind 30624)
    #[command(subcommand)]
    Packs(PacksCmd),
    /// Configure terminal git access to the relay's git hosting
    /// (local, no relay connection needed)
    #[command(subcommand)]
    Git(GitCmd),
    /// Read and drive interactive sessions on this machine (local; via the
    /// desktop session broker, gated by per-session agent consent)
    #[command(subcommand)]
    Session(commands::session::SessionCmd),
    /// Community moderation — reports queue, bans, timeouts, audit trail
    #[command(subcommand)]
    Moderation(ModerationCmd),
    /// Read and analyze recorded coding sessions (NIP-CSL/CSM/CST)
    #[command(subcommand)]
    Sessions(SessionsCmd),
    /// List, share, and drive shared terminals (NIP-ST)
    #[command(subcommand)]
    Terminals(TerminalsCmd),
    /// Read and write a project's Pulse — explicit coordination entries
    /// (kind 44240) folded with observed coding-session facts
    #[command(subcommand)]
    Pulse(PulseCmd),
    /// Run raw Nostr filters against the relay — the debugging verb
    #[command(subcommand)]
    Events(EventsCmd),
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum CiPhaseArg {
    Build,
    Deploy,
}

impl From<CiPhaseArg> for buzz_core::ci_result::CiPhase {
    fn from(value: CiPhaseArg) -> Self {
        match value {
            CiPhaseArg::Build => Self::Build,
            CiPhaseArg::Deploy => Self::Deploy,
        }
    }
}

// Parsed once per CLI invocation; the exact CI identity and target account
// for this variant's size. This enum is not stored in a queue or hot collection.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum CiCmd {
    /// Block until the exact CI run has a durable terminal result
    Wait {
        /// Full NIP-MP project coordinate (30621:<owner>:<slug>)
        #[arg(long)]
        project: String,
        /// Full NIP-34 repository coordinate (30617:<owner>:<id>)
        #[arg(long)]
        repo: String,
        /// Exact lowercase 40-hex commit
        #[arg(long)]
        commit: String,
        /// Configured CI check name
        #[arg(long)]
        check: String,
        /// External CI run identifier
        #[arg(long)]
        run: String,
        /// External CI attempt number
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        attempt: u32,
        /// Workflow UUID authorized to record this result
        #[arg(long)]
        workflow: String,
        /// Completion phase; build completion never satisfies a deploy wait
        #[arg(long, value_enum)]
        phase: CiPhaseArg,
        /// Overall wait limit. Omit to wait until cancelled.
        #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..))]
        timeout: Option<u64>,
    },
    /// Register a turn to run when one exact CI result is recorded.
    ///
    /// The provider reads the recorded result with its own relay key at
    /// delivery time: a private project must explicitly admit that identity,
    /// or a real result is reported the same as "not finished" until the
    /// registration expires. Durable, at-most-once admission: an exact retry
    /// with the printed absolute --expires-at plus the same target, provider,
    /// identity, and continuation text reproduces the same commandId.
    #[command(after_help = r#"Examples:
  bee ci continue --channel <uuid> --provider <provider-pubkey> \
    --driver claude-code --instance-id i-1 --session-id s-1 --generation 1 \
    --project '30621:<owner>:<id>' --repository '30617:<owner>:<id>' \
    --commit <40-hex> --check main-validation --run 136 --attempt 1 \
    --workflow <uuid> --phase build \
    --continuation 'Inspect the CI result; if it passed, open the PR; otherwise diagnose it.' \
    --expires-in 86400 --ack-timeout 60"#)]
    Continue {
        /// Channel UUID the registration is published into
        #[arg(long)]
        channel: String,
        /// Signing pubkey of the provider whose receipts this command trusts
        #[arg(long)]
        provider: String,
        /// Full `cs-target` key, in place of the four target flags below
        #[arg(long, conflicts_with_all = ["driver", "instance_id", "session_id", "generation"])]
        target: Option<String>,
        /// Provider driver slug (with --instance-id/--session-id/--generation)
        #[arg(long)]
        driver: Option<String>,
        /// Provider instance identifier (with --driver/--session-id/--generation)
        #[arg(long = "instance-id")]
        instance_id: Option<String>,
        /// Provider session identifier (with --driver/--instance-id/--generation)
        #[arg(long = "session-id")]
        session_id: Option<String>,
        /// Session generation (with --driver/--instance-id/--session-id)
        #[arg(long)]
        generation: Option<u64>,
        /// Full NIP-MP project coordinate (30621:<owner>:<slug>)
        #[arg(long)]
        project: String,
        /// Full NIP-34 repository coordinate (30617:<owner>:<id>)
        #[arg(long)]
        repository: String,
        /// Exact lowercase 40-hex commit
        #[arg(long)]
        commit: String,
        /// Configured CI check name
        #[arg(long)]
        check: String,
        /// External CI run identifier
        #[arg(long)]
        run: String,
        /// External CI attempt number
        #[arg(long, value_parser = clap::value_parser!(u32).range(1..))]
        attempt: u32,
        /// Workflow UUID authorized to record this result
        #[arg(long)]
        workflow: String,
        /// Completion phase; build completion never satisfies a deploy wait
        #[arg(long, value_enum)]
        phase: CiPhaseArg,
        /// Text delivered alongside the verified result, or @path to read it
        /// from a file (@- reads stdin). Must not be empty.
        #[arg(long)]
        continuation: String,
        /// Registration horizon in seconds; a result recorded after this
        /// many seconds have elapsed is refused rather than delivered. Defaults
        /// to 86400 when neither expiry flag is supplied.
        #[arg(long, value_name = "SECONDS", value_parser = clap::value_parser!(u64).range(1..), conflicts_with = "expires_at")]
        expires_in: Option<u64>,
        /// Absolute Unix expiry printed by an earlier attempt. Use this for an
        /// exact retry that must reproduce the same commandId.
        #[arg(long, value_name = "UNIX_SECONDS", value_parser = clap::value_parser!(u64).range(1..), conflicts_with = "expires_in")]
        expires_at: Option<u64>,
        /// How long to wait for the relay's registration or refusal receipt
        /// before reporting unconfirmed.
        #[arg(long, value_name = "SECONDS", default_value_t = 60)]
        ack_timeout: u64,
    },
    /// Inspect a CI-managed continuation registration (read-only)
    #[command(subcommand)]
    Continuation(CiContinuationCmd),
}

/// `bee ci continuation` subcommands.
#[derive(Subcommand)]
pub enum CiContinuationCmd {
    /// Read the latest receipt stage for one registration commandId.
    ///
    /// Reads stored replay only — no wait, no write. Accepts only the named
    /// provider's signed receipts for the exact target and reports conflicting
    /// target or terminal facts as an error.
    Status {
        /// Channel UUID the registration was published into
        #[arg(long)]
        channel: String,
        /// The registration's commandId (`cic-<64 hex>`)
        #[arg(long = "command-id")]
        command_id: String,
        /// Exact `cs-target` key named by the registration
        #[arg(long)]
        target: String,
        /// Signing pubkey of the provider whose receipts this read trusts
        #[arg(long)]
        provider: String,
    },
}

/// Raw relay queries — no contract decoding, no writes.
#[derive(Subcommand)]
pub enum EventsCmd {
    /// Run a raw authenticated REQ against the relay. --kinds is required;
    /// the relay's p-gate refuses a filter without it.
    #[command(
        about = "Run a raw authenticated REQ against the relay. --kinds is required; \
                 the relay's p-gate refuses a filter without it."
    )]
    Query {
        /// Event kinds to match, comma-separated. Required: the relay's
        /// p-gate answers 403 to a filter that names none.
        #[arg(long)]
        kinds: Option<String>,
        /// Channel UUID to scope to; written to the filter's `#h` key
        #[arg(long, conflicts_with = "h")]
        channel: Option<String>,
        /// Raw `#h` tag value, for an h-scope that is not a channel UUID
        #[arg(long)]
        h: Option<String>,
        /// Author pubkeys, comma-separated 64-char lowercase hex
        #[arg(long)]
        authors: Option<String>,
        /// Event ids, comma-separated 64-char lowercase hex
        #[arg(long)]
        ids: Option<String>,
        /// Lower time bound: RFC 3339 or Unix seconds
        #[arg(long)]
        since: Option<String>,
        /// Upper time bound: RFC 3339 or Unix seconds
        #[arg(long)]
        until: Option<String>,
        /// Stop after this many events; absent pages the whole result
        #[arg(long)]
        limit: Option<u32>,
    },
}

#[derive(Clone, Copy, clap::ValueEnum)]
pub enum RespondToArg {
    #[value(name = "owner-only")]
    OwnerOnly,
    #[value(name = "anyone")]
    Anyone,
}

impl RespondToArg {
    fn to_wire(self) -> String {
        match self {
            Self::OwnerOnly => "owner-only",
            Self::Anyone => "anyone",
        }
        .to_string()
    }
}

#[derive(Subcommand)]
pub enum AgentsCmd {
    /// Open a prefilled create-agent form in the owner's Beekeeper Desktop
    DraftCreate {
        /// Current channel UUID; the new agent is added here after save
        #[arg(long)]
        channel: String,
        /// Proposed agent name
        #[arg(long)]
        display_name: String,
        /// Proposed instructions; use '-' to read from stdin
        #[arg(long)]
        system_prompt: String,
    },
    /// Open a prefilled edit-agent form in the owner's Beekeeper Desktop
    DraftUpdate {
        /// Current channel UUID
        #[arg(long)]
        channel: String,
        /// Current name of the personal agent to update
        #[arg(long)]
        agent_name: String,
        #[arg(long)]
        display_name: Option<String>,
        /// Replacement instructions; use '-' to read from stdin
        #[arg(long)]
        system_prompt: Option<String>,
        #[arg(long)]
        runtime: Option<String>,
        #[arg(long)]
        provider: Option<String>,
        #[arg(long)]
        model: Option<String>,
        #[arg(long, value_enum)]
        respond_to: Option<RespondToArg>,
    },
    /// Submit a NIP-IA archive request for an identity (kind 9035)
    #[command(
        after_help = "Auth flow: when target != signer, the CLI fetches the target's kind:0 and \
attaches its owner-auth tag. On extraction failure it retries once (common cause: profile \
republish in progress). If the retry also fails, the command exits with an error — use \
--admin to bypass this guard when your key is a relay admin.\n\n\
Suggested --reason codes (unknown values are allowed): rotated, retired, \
bot-rebuilt, left-organization, spam\n\n\
Archiving a third-party identity is a human owner/admin action: an agent \
running under BUZZ_AUTH_TAG signs as itself, so it can only ever satisfy \
the self path (target == signer) — not the owner-of-agent path for another \
identity.\n\n\
Examples:\n  \
bee agents archive <PUBKEY> --reason retired\n  \
bee agents archive <PUBKEY> --reason bot-rebuilt --replaced-by <NEW_PUBKEY>"
    )]
    Archive {
        /// Target identity pubkey (hex)
        target_pubkey: String,
        /// Machine-readable reason code, max 64 UTF-8 bytes
        #[arg(long)]
        reason: Option<String>,
        /// Rotation pointer pubkey (hex); must differ from the target
        #[arg(long)]
        replaced_by: Option<String>,
        /// Optional human-readable note (not parsed for authorization)
        #[arg(long, default_value = "")]
        content: String,
        /// Allow sending without owner-auth attestation after extraction fails
        /// (relay-admin path). Use only when your key is a relay admin; ordinary
        /// owners do not need this flag. Without it, auth-extraction failure after
        /// one automatic retry is a hard error rather than a silent bare send.
        #[arg(long, default_value_t = false)]
        admin: bool,
    },
    /// Submit a NIP-IA unarchive request for an identity (kind 9036)
    #[command(
        after_help = "Auth flow: same as `archive` — retries kind:0 fetch once on \
extraction failure, then exits with an error if still unresolvable. Use --admin to bypass \
for relay-admin callers.\n\n\
Examples:\n  \
bee agents unarchive <PUBKEY> --reason returned"
    )]
    Unarchive {
        /// Target identity pubkey (hex)
        target_pubkey: String,
        /// Machine-readable reason code, max 64 UTF-8 bytes
        #[arg(long)]
        reason: Option<String>,
        /// Optional human-readable note (not parsed for authorization)
        #[arg(long, default_value = "")]
        content: String,
        /// Allow sending without owner-auth attestation after extraction fails
        /// (relay-admin path). Use only when your key is a relay admin; ordinary
        /// owners do not need this flag. Without it, auth-extraction failure after
        /// one automatic retry is a hard error rather than a silent bare send.
        #[arg(long, default_value_t = false)]
        admin: bool,
    },
    /// Read the relay's current NIP-IA archive snapshot (kind 13535)
    #[command(
        after_help = "Verifies the snapshot's NIP-11 `self` authorship, event id, signature, \
and NIP-70 `-` protection tag before trusting it. Any trust failure is a \
nonzero-exit error, never a false-empty success — this command's whole \
purpose is verification.\n\n\
Examples:\n  \
bee agents archived"
    )]
    Archived,
}

#[derive(Subcommand)]
pub enum MessagesCmd {
    /// Send a message to a channel
    #[command(
        after_help = "Examples:\n  bee messages send --channel <UUID> --content \"hello\"\n  bee messages send --channel <UUID> --content \"@alice check this\"\n  echo \"hello from stdin\" | bee messages send --channel <UUID> --content -"
    )]
    Send {
        /// Channel UUID (from 'bee channels list')
        #[arg(long)]
        channel: String,
        /// Message text — supports @mentions and markdown. Use '-' to read from stdin.
        #[arg(long)]
        content: String,
        /// Nostr event kind (default: channel default)
        #[arg(long)]
        kind: Option<u16>,
        /// Event ID to reply to (creates a thread)
        #[arg(long)]
        reply_to: Option<String>,
        /// Also publish to the Nostr network
        #[arg(long, default_value_t = false)]
        broadcast: bool,
        /// Attach file(s) — uploads and includes as imeta tags
        #[arg(long = "file")]
        files: Vec<String>,
        /// Pubkey to mention (hex or npub; repeatable). Supplying any explicit identity permits unresolved or ambiguous @Name text as presentation-only; uniquely resolved member names still notify.
        #[arg(long = "mention")]
        mentions: Vec<String>,
    },
    /// Send a code diff / patch to a channel
    SendDiff {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// Diff/patch content (use '-' to read from stdin)
        #[arg(long)]
        diff: String,
        /// Repository URL (e.g. https://github.com/org/repo)
        #[arg(long)]
        repo: String,
        /// Commit SHA
        #[arg(long)]
        commit: String,
        /// Single file path within the repo
        #[arg(long)]
        file: Option<String>,
        /// Parent commit SHA for three-way diff context
        #[arg(long)]
        parent_commit: Option<String>,
        /// Source branch name
        #[arg(long)]
        source_branch: Option<String>,
        /// Target branch name
        #[arg(long)]
        target_branch: Option<String>,
        /// Pull request number
        #[arg(long)]
        pr: Option<u32>,
        /// Language hint (auto-detected from file extension if omitted)
        #[arg(long)]
        lang: Option<String>,
        /// Human-readable description of the change
        #[arg(long)]
        description: Option<String>,
        /// Event ID to reply to (creates a thread)
        #[arg(long)]
        reply_to: Option<String>,
    },
    /// Edit a previously sent message
    Edit {
        /// Event ID of the message to edit (64-char hex)
        #[arg(long)]
        event: String,
        /// New message content
        #[arg(long)]
        content: String,
    },
    /// Delete a message by event ID
    Delete {
        /// Event ID to delete (64-char hex)
        #[arg(long)]
        event: String,
        /// Optional moderation audit action UUID for the public tombstone
        #[arg(long)]
        action_id: Option<Uuid>,
        /// Optional machine-readable public reason code for the tombstone
        #[arg(long)]
        reason_code: Option<String>,
        /// Optional human-readable public reason for the tombstone
        #[arg(long)]
        public_reason: Option<String>,
    },
    /// Retrieve messages from a channel
    #[command(
        after_help = "Examples:\n  bee messages get --channel <UUID>\n  bee messages get --channel <UUID> --limit 50 --kinds 1,1984"
    )]
    Get {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// Maximum number of results to return
        #[arg(long)]
        limit: Option<u32>,
        /// Unix timestamp — return messages before this time
        #[arg(long)]
        before: Option<i64>,
        /// Unix timestamp — return messages after this time
        #[arg(long)]
        since: Option<i64>,
        /// Comma-separated event kinds to filter (e.g. 1,1984)
        #[arg(long)]
        kinds: Option<String>,
    },
    /// Get a message thread (replies to a root message)
    Thread {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// Root message event ID (64-char hex)
        #[arg(long)]
        event: String,
        /// Maximum number of results to return
        #[arg(long)]
        limit: Option<u32>,
        /// Maximum reply nesting depth to include
        #[arg(long)]
        depth_limit: Option<u32>,
    },
    /// Full-text search across messages
    #[command(
        after_help = "Examples:\n  bee messages search --query checkout\n  bee messages search --author npub1... --since 1783497600\n  bee messages search --author Aaron --query checkout --limit 20"
    )]
    Search {
        /// Search query string (optional when --author is given)
        #[arg(long)]
        query: Option<String>,
        /// Filter by author: 64-char hex pubkey, npub, or display name
        #[arg(long)]
        author: Option<String>,
        /// Unix timestamp — return messages after this time
        #[arg(long)]
        since: Option<i64>,
        /// Maximum number of results to return
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Upvote or downvote a forum post
    Vote {
        /// Event ID of the post to vote on (64-char hex)
        #[arg(long)]
        event: String,
        /// Vote direction: "up" or "down"
        #[arg(long)]
        direction: String,
    },
}

#[derive(Subcommand)]
pub enum ChannelsCmd {
    /// List channels visible to the current identity
    #[command(
        after_help = "Examples:\n  bee channels list\n  bee channels list --visibility open"
    )]
    List {
        /// Filter by visibility
        #[arg(long, value_enum)]
        visibility: Option<ChannelVisibility>,
        /// Only show channels where the current identity is a member
        #[arg(long, default_value_t = false)]
        member: bool,
        /// Maximum number of channels to return [default: 500]
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Get details for a single channel
    Get {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Search channels by human-readable name
    #[command(
        after_help = "Examples:\n  bee channels search --query composer\n  bee channels search --query buzz-chat-composer --exact\n  bee channels search --query design --include-archived"
    )]
    Search {
        /// Search query (case-insensitive substring of channel name)
        #[arg(long)]
        query: String,
        /// Require an exact case-insensitive match instead of substring
        #[arg(long, default_value_t = false)]
        exact: bool,
        /// Include archived channels in results
        #[arg(long, default_value_t = false)]
        include_archived: bool,
        /// Maximum number of channel-metadata events to fetch from the relay
        #[arg(long, default_value_t = 1000)]
        limit: u32,
    },
    /// Create a new channel
    #[command(
        after_help = "Examples:\n  bee channels create --name general --type stream --visibility open\n  bee channels create --name design --type forum --visibility open --description \"Design discussions\"\n  bee channels create --name standup --type stream --visibility open --ttl 3600  # ephemeral, archived after 1h idle\n  bee channels create --name project-x --template \"Buzz Team\"  # type/visibility/canvas/roster from the template; explicit flags override"
    )]
    Create {
        /// Channel name
        #[arg(long)]
        name: String,
        /// Channel type. Required unless --template supplies one.
        #[arg(long = "type", value_enum, required_unless_present = "template")]
        channel_type: Option<ChannelType>,
        /// Channel visibility. Required unless --template supplies one.
        #[arg(long, value_enum, required_unless_present = "template")]
        visibility: Option<ChannelVisibility>,
        /// Channel description
        #[arg(long)]
        description: Option<String>,
        /// Make the channel ephemeral: lifetime in seconds. The relay archives
        /// it once this many seconds pass without a new message.
        #[arg(long, value_name = "SECONDS")]
        ttl: Option<i64>,
        /// Apply a desktop-local channel template by name (case-insensitive):
        /// supplies default type/visibility/description/canvas, and resolves
        /// its agent roster against the relay to add as members.
        #[arg(long)]
        template: Option<String>,
        /// Override the channel-templates.json path (default: the desktop
        /// app's prod app-data dir). Mainly for the dev store or testing.
        #[arg(long, value_name = "PATH")]
        templates_file: Option<String>,
    },
    /// Update channel name, description, visibility, or ephemeral TTL
    #[command(
        after_help = "Examples:\n  bee channels update --channel <uuid> --name general\n  bee channels update --channel <uuid> --visibility open\n  bee channels update --channel <uuid> --visibility private"
    )]
    Update {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// New channel name
        #[arg(long)]
        name: Option<String>,
        /// New channel description
        #[arg(long)]
        description: Option<String>,
        /// New channel visibility
        #[arg(long, value_enum)]
        visibility: Option<ChannelVisibility>,
        /// Make the channel ephemeral (or change its lifetime): seconds until
        /// the relay archives it after the last message. Conflicts with --no-ttl.
        #[arg(long, value_name = "SECONDS", conflicts_with = "no_ttl")]
        ttl: Option<i64>,
        /// Clear an existing TTL, making the channel permanent.
        #[arg(long)]
        no_ttl: bool,
    },
    /// Set the channel topic
    Topic {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// New topic text
        #[arg(long)]
        topic: String,
    },
    /// Set the channel purpose
    Purpose {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// New purpose text
        #[arg(long)]
        purpose: String,
    },
    /// Join a channel
    Join {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Leave a channel
    Leave {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Archive a channel
    Archive {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Unarchive a channel
    Unarchive {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Delete a channel permanently
    Delete {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// List members of a channel
    Members {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Add a member to a channel
    #[command(name = "add-member")]
    AddMember {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// Member pubkey (64-char hex)
        #[arg(long)]
        pubkey: String,
        /// Member role (owner, admin, member, guest, bot)
        #[arg(long)]
        role: Option<String>,
    },
    /// Remove a member from a channel
    #[command(name = "remove-member")]
    RemoveMember {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// Member pubkey (64-char hex)
        #[arg(long)]
        pubkey: String,
    },
    /// Set your channel addition policy
    #[command(name = "set-add-policy")]
    SetAddPolicy {
        /// Policy: anyone | owner_only | nobody
        #[arg(long)]
        policy: String,
    },
}

#[derive(Subcommand)]
pub enum CanvasCmd {
    /// Get the canvas document for a channel
    Get {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Set (replace) the canvas document for a channel
    Set {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// Canvas content (markdown; use '-' to read from stdin)
        #[arg(long)]
        content: String,
    },
}

#[derive(Subcommand)]
pub enum ReactionsCmd {
    /// Add an emoji reaction to a message
    Add {
        /// Event ID (64-char hex)
        #[arg(long)]
        event: String,
        /// Emoji character (e.g. '👍') or custom emoji shortcode
        #[arg(long)]
        emoji: String,
        /// Image URL for a custom emoji reaction; when set, content becomes `:shortcode:`
        #[arg(long = "emoji-url")]
        emoji_url: Option<String>,
    },
    /// Remove an emoji reaction from a message
    Remove {
        /// Event ID (64-char hex)
        #[arg(long)]
        event: String,
        /// Emoji character to remove
        #[arg(long)]
        emoji: String,
    },
    /// List reactions on a message
    Get {
        /// Event ID (64-char hex)
        #[arg(long)]
        event: String,
    },
}

#[derive(Subcommand)]
pub enum EmojiCmd {
    /// List the workspace custom emoji palette (union of every member's set)
    List,
    /// Add or update a custom emoji in your own set
    Set {
        /// Emoji shortcode, without surrounding colons
        #[arg(long)]
        shortcode: String,
        /// Image URL for the emoji
        #[arg(long)]
        url: String,
    },
    /// Remove a custom emoji from your own set
    Rm {
        /// Emoji shortcode, without surrounding colons
        #[arg(long)]
        shortcode: String,
    },
    /// Export custom emojis to stdout or a file
    Export {
        /// Write JSON to this file path instead of stdout
        #[arg(long)]
        file: Option<String>,
        /// Export your own set (default) or the full workspace palette
        #[arg(long, value_enum, default_value = "own")]
        scope: EmojiScope,
    },
    /// Import custom emojis from stdin or a file into your own set
    Import {
        /// Read JSON from this file path instead of stdin
        #[arg(long)]
        file: Option<String>,
        /// Replace your entire set instead of merging
        #[arg(long, default_value_t = false)]
        replace: bool,
        /// Print what would be published without writing
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
}

#[derive(Subcommand)]
pub enum DmsCmd {
    /// List direct message conversations
    List {
        /// Maximum number of results to return
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Open a new direct message with one or more users
    Open {
        /// User pubkey(s) to DM (64-char hex, 1-8)
        #[arg(long = "pubkey")]
        pubkeys: Vec<String>,
    },
    /// Add a member to an existing DM conversation
    AddMember {
        /// DM conversation UUID
        #[arg(long)]
        channel: String,
        /// User pubkey to add (64-char hex)
        #[arg(long)]
        pubkey: String,
    },
    /// Hide a DM conversation from your DM list
    Hide {
        /// DM conversation UUID
        #[arg(long)]
        channel: String,
    },
}

#[derive(Subcommand)]
pub enum UsersCmd {
    /// Look up user profiles by pubkey or name
    Get {
        /// User pubkey(s) to look up (64-char hex). Omit for your own profile
        #[arg(long = "pubkey")]
        pubkeys: Vec<String>,
        /// Search by display name (case-insensitive substring match)
        #[arg(long = "name")]
        name: Option<String>,
        /// Scope an exact-name agent lookup to its owner (`me`, hex, or npub)
        #[arg(long = "owner", requires = "name")]
        owner: Option<String>,
    },
    /// Update the current identity's profile
    #[command(name = "set-profile")]
    SetProfile {
        /// Display name
        #[arg(long)]
        name: Option<String>,
        /// Avatar URL
        #[arg(long)]
        avatar: Option<String>,
        /// Bio / about text
        #[arg(long)]
        about: Option<String>,
        /// NIP-05 identifier (e.g. user@example.com)
        #[arg(long)]
        nip05: Option<String>,
    },
    /// Get presence status for users
    Presence {
        /// Comma-separated pubkeys (64-char hex)
        #[arg(long)]
        pubkeys: String,
    },
    /// Set your presence status (online/away/offline)
    #[command(name = "set-presence")]
    SetPresence {
        /// Presence status
        #[arg(long, value_enum)]
        status: PresenceStatus,
    },
    /// Set your user status (NIP-38 kind:30315 — the "status" line on your profile)
    #[command(name = "set-status")]
    SetStatus {
        /// Status text (required unless --clear)
        #[arg(long, required_unless_present = "clear")]
        text: Option<String>,
        /// Optional emoji shown before the status text
        #[arg(long)]
        emoji: Option<String>,
        /// Remove your status entirely
        #[arg(long, conflicts_with_all = ["text", "emoji"])]
        clear: bool,
    },
}

#[derive(Subcommand)]
pub enum WorkflowsCmd {
    /// List workflows in a channel
    List {
        /// Channel UUID
        #[arg(long)]
        channel: String,
    },
    /// Get details for a single workflow
    Get {
        /// Workflow UUID
        #[arg(long)]
        workflow: String,
    },
    /// Create a workflow from a YAML definition
    Create {
        /// Channel UUID
        #[arg(long)]
        channel: String,
        /// Workflow YAML definition
        #[arg(long)]
        yaml: String,
    },
    /// Update a workflow's YAML definition
    Update {
        /// Channel UUID the workflow belongs to
        #[arg(long)]
        channel: String,
        /// Workflow UUID
        #[arg(long)]
        workflow: String,
        /// Updated workflow YAML definition
        #[arg(long)]
        yaml: String,
    },
    /// Delete a workflow
    Delete {
        /// Workflow UUID
        #[arg(long)]
        workflow: String,
    },
    /// Trigger a workflow run
    #[command(
        after_help = "Examples:\n  bee workflows trigger --workflow <UUID>\n  bee workflows trigger --workflow <UUID> --inputs '{\"key\":\"value\"}'"
    )]
    Trigger {
        /// Workflow UUID
        #[arg(long)]
        workflow: String,
        /// JSON object of input variables passed to the workflow as event content
        #[arg(long)]
        inputs: Option<String>,
    },
    /// List runs for a workflow
    Runs {
        /// Workflow UUID
        #[arg(long)]
        workflow: String,
        /// Maximum number of results to return
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Approve or deny a workflow step
    #[command(
        after_help = "Examples:\n  bee workflows approve --token <UUID>\n  bee workflows approve --token <UUID> --approved false --note \"needs revision\""
    )]
    Approve {
        /// The approval token UUID (from the approval request)
        #[arg(long)]
        token: String,
        /// Approve (true) or deny (false) the step
        #[arg(long, default_value_t = true, action = clap::ArgAction::Set)]
        approved: bool,
        /// Optional note to include with the approval/denial
        #[arg(long)]
        note: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum FeedCmd {
    /// Get recent activity feed entries
    Get {
        /// Unix timestamp — return entries after this time
        #[arg(long)]
        since: Option<i64>,
        /// Maximum number of results to return
        #[arg(long)]
        limit: Option<u32>,
        /// Comma-separated feed types to include: mentions, needs_action, activity, agent_activity
        #[arg(long)]
        types: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum SocialCmd {
    /// Publish a text note (NIP-01 kind:1)
    #[command(name = "publish")]
    PublishNote {
        /// Text content of the note.
        #[arg(long)]
        content: String,
        /// 64-char hex event ID to reply to.
        #[arg(long)]
        reply_to: Option<String>,
    },
    /// Set your contact list (NIP-02 kind:3)
    #[command(name = "set-contacts")]
    SetContactList {
        /// JSON array of contacts: [{"pubkey":"hex","relay_url":"...","petname":"..."}]
        #[arg(long)]
        contacts: String,
    },
    /// Get a single event by ID
    #[command(name = "event")]
    GetEvent {
        /// 64-char hex event ID.
        #[arg(long)]
        event: String,
    },
    /// Get recent notes published by a user
    #[command(name = "notes")]
    GetUserNotes {
        /// 64-char hex pubkey of the author.
        #[arg(long)]
        pubkey: String,
        /// Maximum number of notes to return (default 50, max 100).
        #[arg(long)]
        limit: Option<u32>,
        /// Unix timestamp cursor — return notes created before this time.
        #[arg(long)]
        before: Option<i64>,
        /// Event ID cursor — return notes created before this event (composite pagination with --before).
        #[arg(long)]
        before_id: Option<String>,
    },
    /// Get a user's contact list
    #[command(name = "contacts")]
    GetContactList {
        /// 64-char hex pubkey.
        #[arg(long)]
        pubkey: String,
    },
    /// Publish a NIP-51/NIP-65 social list or set.
    #[command(name = "set-list")]
    SetList {
        /// Supported kind: 10000, 10001, 10002, 10003, 30000, or 30003.
        #[arg(long)]
        kind: u16,
        /// JSON array of Nostr tags, e.g. [["p","<hex>"],["d","friends"]].
        #[arg(long)]
        tags: String,
        /// Event content.
        #[arg(long, default_value = "")]
        content: String,
    },
    /// Get NIP-51/NIP-65 social lists or sets by author and kind.
    #[command(name = "list")]
    GetList {
        /// 64-char hex pubkey of the author.
        #[arg(long)]
        pubkey: String,
        /// Supported kind: 10000, 10001, 10002, 10003, 30000, or 30003.
        #[arg(long)]
        kind: u32,
        /// Optional d-tag for parameterized replaceable sets.
        #[arg(long)]
        d_tag: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum NotesCmd {
    /// Create or update a note. Idempotent upsert keyed by `(me, --name)`.
    ///
    /// `published_at` is preserved on edits (only set on first create).
    /// `--title` is required on first create; on subsequent edits the existing
    /// title is carried forward when `--title` is omitted, and `--title ""`
    /// explicitly clears it.
    #[command(
        after_help = "Examples:\n  echo '# Hello' | bee notes set --name hello --title 'Hello' --content -\n  bee notes set --name hello --tag onboarding --content - < draft.md"
    )]
    Set {
        /// Slug — becomes the `d` tag. `[a-z0-9._-]{1,80}`.
        #[arg(long)]
        name: String,
        /// Note title (NIP-23 `title` tag). Required on first create; omit to carry; `""` to clear.
        #[arg(long)]
        title: Option<String>,
        /// Short summary (NIP-23 `summary` tag). Omit to carry; `""` to clear.
        #[arg(long)]
        summary: Option<String>,
        /// Topic tag (NIP-23 `t` tag). May be repeated. Replaces (not merges) existing tags on edit; omit to carry forward.
        #[arg(long = "tag")]
        tags: Vec<String>,
        /// Clear all `t` tags on update. Mutually exclusive with `--tag`.
        /// Without this and without `--tag`, existing tags are carried forward.
        #[arg(long, default_value_t = false)]
        clear_tags: bool,
        /// Markdown body. Use `-` to read from stdin.
        #[arg(long)]
        content: String,
        /// Allow committing an empty body (refused by default to catch upstream pipeline failures).
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
    },
    /// Read a note by `--naddr` (exact) or `--name <slug>` (cross-author lookup).
    Get {
        /// NIP-19 `naddr1…` or `30023:<pubkey>:<slug>` coordinate. Mutually exclusive with `--name`.
        #[arg(long)]
        naddr: Option<String>,
        /// Slug to look up across authors. Mutually exclusive with `--naddr`.
        #[arg(long)]
        name: Option<String>,
        /// Disambiguate `--name` to a specific author (hex pubkey, display name, or `me`).
        #[arg(long)]
        author: Option<String>,
        /// On an ambiguous `--name` (multiple authors), pick the most recently updated note
        /// instead of erroring. Mutually exclusive with `--author` and `--naddr`.
        #[arg(long, default_value_t = false)]
        latest: bool,
        /// Print only the markdown body, not the full event JSON.
        #[arg(long, default_value_t = false)]
        content_only: bool,
    },
    /// List notes. Defaults to your own.
    Ls {
        /// Hex pubkey, display name, `me`, or `all`.
        #[arg(long, default_value = "me")]
        author: Option<String>,
        /// Filter by NIP-23 `t` tag.
        #[arg(long)]
        tag: Option<String>,
        /// Max results (default 50, hard cap 200).
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Delete one of your own notes via NIP-09 (kind:5).
    ///
    /// Emits an a-tag-only deletion targeting the addressable coordinate
    /// `30023:<pubkey>:<slug>` (no `e` tag — an `e` tag would route around the
    /// relay's coordinate soft-delete and leave the note alive). Read-before-
    /// write gives a clean NotFound when there's nothing to delete.
    Rm {
        /// Slug of the note to delete. Only your own notes can be removed.
        #[arg(long)]
        name: String,
    },
}

#[derive(Subcommand)]
pub enum ReposCmd {
    /// Announce a git repository (NIP-34)
    Create {
        /// Repository identifier: [a-zA-Z0-9._-]{1,64}
        #[arg(long)]
        id: String,
        /// Human-readable display name
        #[arg(long)]
        name: Option<String>,
        /// Repository description
        #[arg(long)]
        description: Option<String>,
        /// Clone URL(s) — can be specified multiple times
        #[arg(long = "clone")]
        clone_urls: Vec<String>,
        /// Web browsing URL
        #[arg(long)]
        web: Option<String>,
        /// Preferred Nostr relay(s) for repo discovery — can be specified multiple times
        #[arg(long = "nostr-relay")]
        relays: Vec<String>,
        /// Channel UUID to bind the repo to. Members of this channel get git
        /// access at their channel role. Optional when `--project` is given.
        #[arg(long)]
        channel: Option<String>,
        /// Project coordinate (`30621:<owner-hex>:<project-d>`) to announce
        /// the repo into. The project's roster gets git access — owners push
        /// as owners, collaborators as members, viewers read only.
        ///
        /// A repo with neither `--project` nor `--channel` has no ACL, so the
        /// relay 404s every clone/fetch/push for everyone but its owner until
        /// the author runs `bee repos bind` (issue #3527).
        #[arg(long)]
        project: Option<String>,
        /// A co-founder's pubkey (64-char lowercase hex). Repeatable.
        ///
        /// Written as the NIP-34 `maintainers` tag. Everyone listed is a
        /// **founder** of the repository alongside you: their missions can
        /// rule on it, and they may land a `require-verdict` ref. Rules
        /// themselves stay yours — only the announcement's signer can rewrite
        /// a `buzz-protect` tag in v1.
        #[arg(long = "maintainer")]
        maintainers: Vec<String>,
    },
    /// Change who co-founds one of your repositories.
    ///
    /// `--maintainer` replaces the NIP-34 `maintainers` tag whole; it is never
    /// merged with what is already there, so the printed founder set is what
    /// the announcement now says. This is an authority change: a founder's
    /// missions can rule on this repository and a founder may land a
    /// `require-verdict` ref.
    #[command(
        after_help = "Examples:\n  bee repos update --id myrepo --maintainer <hex>\n  bee repos update --id myrepo --maintainer <hex> --maintainer <hex>\n  bee repos update --id myrepo --clear-maintainers"
    )]
    Update {
        /// Repository identifier (d-tag).
        #[arg(long)]
        id: String,
        /// A co-founder's pubkey (64-char lowercase hex). Repeatable.
        #[arg(long = "maintainer")]
        maintainers: Vec<String>,
        /// Remove the `maintainers` tag entirely, leaving you the only founder
        /// the announcement names.
        #[arg(long, default_value_t = false)]
        clear_maintainers: bool,
    },
    /// Get a repository announcement
    Get {
        /// Repository identifier (d-tag)
        #[arg(long)]
        id: String,
        /// Owner pubkey (64-char hex). Omit to match any owner.
        #[arg(long)]
        owner: Option<String>,
    },
    /// List repository announcements
    List {
        /// Owner pubkey (64-char hex). Omit for your repos.
        #[arg(long)]
        owner: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Give one of your repositories an ACL — a project, a channel, or both.
    ///
    /// A repository is reachable through either its project's roster or its
    /// bound channel's membership, and the relay grants whichever is more
    /// permissive. A repo announced with neither (e.g. by a vanilla NIP-34
    /// client) returns 404 for everyone until its author fixes it here.
    Bind {
        /// Repository identifier (d-tag).
        #[arg(long)]
        id: String,
        /// Channel UUID to bind. Replaces any existing binding.
        #[arg(long)]
        channel: Option<String>,
        /// Project coordinate (`30621:<owner-hex>:<project-d>`) to link into.
        /// Replaces any existing link.
        #[arg(long)]
        project: Option<String>,
    },
    /// Delete a repository (kind:5 tombstone of its kind:30617 announce).
    ///
    /// The repository stops being listed and stops being cloneable: the
    /// relay soft-deletes the announcement and its kind:30618 ref state,
    /// and removes the object-store pointer every read path resolves.
    ///
    /// Two things deliberately survive. The name stays reserved to its
    /// owner — deletion never frees a name for somebody else to squat — so
    /// this cannot be used to take a name over. And the repository's packed
    /// objects are content-addressed and shared with any fork or repo that
    /// has the same content, so they are left for an operator sweep rather
    /// than deleted from under a neighbour.
    ///
    /// Signed by the repo's owner, or by an Owner of the project it is in.
    #[command(
        after_help = "Examples:\n  bee repos delete --id myrepo\n  bee repos delete --id myrepo --owner <hex>"
    )]
    Delete {
        /// Repository identifier (d-tag).
        #[arg(long)]
        id: String,
        /// Repo owner pubkey (64-char hex). Defaults to the current identity;
        /// pass it to delete a repository you did not announce but whose
        /// project you own.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Manage branch and tag protection rules on one of your repositories.
    #[command(subcommand)]
    Protect(ReposProtectCmd),
}

/// Commands for inspecting and changing repository protection rules.
#[derive(Subcommand)]
pub enum ReposProtectCmd {
    /// List the repository's protection rules.
    List {
        /// Repository identifier (d-tag).
        #[arg(long)]
        id: String,
    },
    /// Create or replace the rule for an exact ref pattern.
    Set {
        /// Repository identifier (d-tag).
        #[arg(long)]
        id: String,
        /// Full ref pattern, such as refs/heads/main or refs/heads/*.
        #[arg(long = "ref")]
        ref_pattern: String,
        /// Minimum role allowed to push.
        #[arg(long)]
        push: Option<RepoPushRole>,
        /// Reject non-fast-forward updates.
        #[arg(long, default_value_t = false)]
        no_force_push: bool,
        /// Reject deletion of matching refs.
        #[arg(long, default_value_t = false)]
        no_delete: bool,
        /// Require the NIP-34 patch workflow instead of direct pushes.
        #[arg(long, default_value_t = false)]
        require_patch: bool,
        /// Admit a push two ways only: a founder's, with no verdict at all;
        /// or a seat's, behind a verifier's `not-refuted` refutation of an
        /// approved report naming the pushed commit. Enforced by the relay
        /// serving the repository — a relay predating the rule parses the
        /// token and ignores it. Does not read any session policy:
        /// `gates.verifierRequired` governs mission completion, not this.
        #[arg(long, default_value_t = false)]
        require_verdict: bool,
    },
    /// Remove every protection rule for an exact ref pattern.
    Remove {
        /// Repository identifier (d-tag).
        #[arg(long)]
        id: String,
        /// Full ref pattern to remove.
        #[arg(long = "ref")]
        ref_pattern: String,
    },
}

/// Minimum channel role accepted by a repository push rule.
#[derive(Clone, Copy, clap::ValueEnum)]
pub enum RepoPushRole {
    /// Repository owner only.
    Owner,
    /// Repository owner or channel admin.
    Admin,
    /// Any channel member.
    Member,
}

/// Access level of a multi-repo project container (`buzz-access`,
/// NIP-MP Buzz access extension).
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ProjectAccess {
    /// Community-readable (the pre-extension default).
    Public,
    /// Withheld from everyone except the author and invited members.
    Private,
}

impl ProjectAccess {
    /// The `buzz-access` tag value this variant serializes to.
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectAccess::Public => "public",
            ProjectAccess::Private => "private",
        }
    }
}

/// Project member role (NIP-MP roles extension: owner/collaborator/viewer).
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ProjectRoleArg {
    /// Full rights plus roster management.
    Owner,
    /// Read everything, write into project contents; no roster management.
    Collaborator,
    /// Read-only across the project and its contents.
    Viewer,
}

impl ProjectRoleArg {
    /// The role vocabulary string this variant serializes to.
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectRoleArg::Owner => "owner",
            ProjectRoleArg::Collaborator => "collaborator",
            ProjectRoleArg::Viewer => "viewer",
        }
    }
}

/// Grant tier for coding-session authority grants and shared-terminal
/// rosters: collaborator (may steer / type) or viewer (read-only).
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum GrantRoleArg {
    /// May steer the session / type into the terminal.
    Collaborator,
    /// Read-only access.
    Viewer,
}

impl GrantRoleArg {
    /// The roster role string this variant serializes to.
    pub fn as_str(self) -> &'static str {
        match self {
            GrantRoleArg::Collaborator => "collaborator",
            GrantRoleArg::Viewer => "viewer",
        }
    }
}

/// Shared-terminal roster role (NIP-ST): collaborator (watch + type) or
/// viewer (watch only).
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ShellRoleArg {
    /// May watch and type into the owner's PTY.
    Collaborator,
    /// Watch-only access.
    Viewer,
}

impl ShellRoleArg {
    /// The roster role string this variant serializes to.
    pub fn as_str(self) -> &'static str {
        match self {
            ShellRoleArg::Collaborator => "collaborator",
            ShellRoleArg::Viewer => "viewer",
        }
    }
}

/// Visibility of a multi-repo project listing.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum ProjectVisibility {
    /// Project appears in public listings (default).
    Listed,
    /// Project is hidden from public listings.
    Unlisted,
}

impl ProjectVisibility {
    pub fn as_str(self) -> &'static str {
        match self {
            ProjectVisibility::Listed => "listed",
            ProjectVisibility::Unlisted => "unlisted",
        }
    }
}

#[derive(Subcommand)]
pub enum ProjectsCmd {
    /// Create a new multi-repo project (NIP-MP kind:30621)
    ///
    /// Requires at least one --repo. Fails with Conflict if the project already exists.
    Create {
        /// Project identifier (slug), up to 1024 bytes
        slug: String,
        /// Member repository coordinate: bare Buzz repo id (e.g. `buzz`) or full
        /// `30617:<owner-hex>:<repo-d>` for cross-owner or colon-bearing repo ids.
        /// At least one --repo is required.
        #[arg(long = "repo", required = true)]
        repo: Vec<String>,
        /// Display name (≤256 bytes)
        #[arg(long)]
        name: Option<String>,
        /// Description (≤2048 bytes)
        #[arg(long)]
        description: Option<String>,
        /// Associated Buzz channel UUID
        #[arg(long)]
        channel: Option<String>,
        /// Visibility: `listed` (default) or `unlisted`
        #[arg(long)]
        visibility: Option<ProjectVisibility>,
        /// Access level: `private` (default) or `public`. Private restricts
        /// the container and its contents to you plus invited members.
        #[arg(long, value_enum, default_value = "private")]
        access: ProjectAccess,
        /// Invited member as `<pubkey>[:role]` where role is owner,
        /// collaborator (default), or viewer. Repeatable. Members are
        /// meaningful with `--access private`; agents are invited exactly
        /// like users — by pubkey.
        #[arg(long = "member")]
        member: Vec<String>,
    },
    /// Get a project by slug
    Get {
        /// Project slug
        slug: String,
        /// Owner pubkey (64-char hex). Defaults to the current identity.
        #[arg(long)]
        owner: Option<String>,
    },
    /// List projects
    List {
        /// Owner pubkey (64-char hex). Defaults to the current identity.
        #[arg(long)]
        owner: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Add one or more member repositories to a project
    #[command(name = "add-repo")]
    AddRepo {
        /// Project slug
        slug: String,
        /// Member repository coordinate (bare id or full `30617:<owner-hex>:<repo-d>`)
        #[arg(long = "repo", required = true)]
        repo: Vec<String>,
    },
    /// Remove one or more member repositories from a project
    #[command(name = "remove-repo")]
    RemoveRepo {
        /// Project slug
        slug: String,
        /// Member repository coordinate to remove (bare id or full `30617:<owner-hex>:<repo-d>`)
        #[arg(long = "repo", required = true)]
        repo: Vec<String>,
    },
    /// Update project metadata (at least one setter or clearer required)
    #[command(group = clap::ArgGroup::new("mutation").required(true).multiple(true))]
    Update {
        /// Project slug
        slug: String,
        /// Set the display name
        #[arg(long, group = "mutation")]
        name: Option<String>,
        /// Remove the display name
        #[arg(long, group = "mutation", conflicts_with = "name")]
        clear_name: bool,
        /// Set the description
        #[arg(long, group = "mutation")]
        description: Option<String>,
        /// Remove the description
        #[arg(long, group = "mutation", conflicts_with = "description")]
        clear_description: bool,
        /// Set the associated Buzz channel UUID
        #[arg(long, group = "mutation")]
        channel: Option<String>,
        /// Remove the associated channel
        #[arg(long, group = "mutation", conflicts_with = "channel")]
        clear_channel: bool,
        /// Set visibility: `listed` or `unlisted`
        #[arg(long, group = "mutation")]
        visibility: Option<ProjectVisibility>,
        /// Remove the visibility tag (absence defaults to `listed`)
        #[arg(long, group = "mutation", conflicts_with = "visibility")]
        clear_visibility: bool,
        /// Set the access level: `public` or `private`. There is no clear
        /// variant — an absent tag means public, so flipping is explicit.
        #[arg(long, group = "mutation", value_enum)]
        access: Option<ProjectAccess>,
    },
    /// Delete a project (head-based tombstone; verified after submit).
    ///
    /// By default this deletes the kind:30621 event ONLY — repositories,
    /// channels, workflows, and messages are untouched (NIP-MP: "there is no
    /// cascade, in either direction"). `--cascade` opts in to also deleting
    /// the project's channels and your own workflows in them.
    Delete {
        /// Project slug
        slug: String,
        /// Also delete the project's channels (kind:9008, transport channels
        /// included) and your own workflow definitions in them, then the
        /// project itself. Repositories are detached, never deleted.
        #[arg(long)]
        cascade: bool,
        /// Print the cascade plan (counts per child type plus warnings) and
        /// exit without publishing anything.
        #[arg(long, requires = "cascade")]
        dry_run: bool,
        /// Confirm a cascade delete. Without it, `--cascade` prints the plan
        /// and exits with a usage error rather than deleting anything.
        #[arg(long, requires = "cascade")]
        yes: bool,
    },
    /// Add a project member or change their role (kind 9010 membership op).
    ///
    /// Requires the signer to be the project creator or a roster owner. The
    /// first accepted op flips the roster to relay-managed: head `p` tags
    /// are thereafter ignored.
    #[command(name = "add-member")]
    AddMember {
        /// Project slug
        slug: String,
        /// Member pubkey (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
        /// Member role
        #[arg(long, value_enum)]
        role: ProjectRoleArg,
        /// Project owner pubkey (64-char hex) for a co-owned project you
        /// manage. Defaults to the current identity.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Remove a project member (kind 9011 membership op)
    #[command(name = "remove-member")]
    RemoveMember {
        /// Project slug
        slug: String,
        /// Member pubkey (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
        /// Project owner pubkey (64-char hex). Defaults to the current identity.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Change an existing member's role (kind 9010 re-put)
    #[command(name = "set-role")]
    SetRole {
        /// Project slug
        slug: String,
        /// Member pubkey (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
        /// New member role
        #[arg(long, value_enum)]
        role: ProjectRoleArg,
        /// Project owner pubkey (64-char hex). Defaults to the current identity.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Print a project's member roster as `[{pubkey, role}]`.
    ///
    /// Reads the relay-signed kind:39010 roster projection; falls back to
    /// the head's `p` tags when no projection exists yet (head-sourced
    /// roster).
    Members {
        /// Project slug
        slug: String,
        /// Project owner pubkey (64-char hex). Defaults to the current identity.
        #[arg(long)]
        owner: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum PatchesCmd {
    /// Send a git patch (NIP-34 kind:1617)
    #[command(
        after_help = "Examples:\n  git format-patch -1 HEAD --stdout | bee patches send --repo-owner <hex> --repo-id myrepo --patch-file - --root\n  bee patches send --repo-owner <hex> --repo-id myrepo --patch-file 0001-fix.patch --reply-to <prev-patch-id>"
    )]
    Send {
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Path to a `git format-patch` file, or '-' to read from stdin
        #[arg(long)]
        patch_file: String,
        /// Earliest-unique-commit of the repo
        #[arg(long)]
        euc: Option<String>,
        /// Additional recipient pubkey(s) — can be specified multiple times
        #[arg(long = "to")]
        to: Vec<String>,
        /// Previous patch event id (series) or original root (revision)
        #[arg(long)]
        reply_to: Option<String>,
        /// Mark as the first patch of a new series
        #[arg(long, default_value_t = false)]
        root: bool,
        /// Mark as the first patch of a new revision of an existing series
        #[arg(long, default_value_t = false)]
        root_revision: bool,
        /// Commit ID this patch produces when applied
        #[arg(long)]
        commit: Option<String>,
        /// Parent commit ID
        #[arg(long)]
        parent_commit: Option<String>,
        /// PGP signature of the commit
        #[arg(long)]
        commit_pgp_sig: Option<String>,
        /// Committer identity: 'name|email|timestamp|tz-offset-minutes'
        #[arg(long)]
        committer: Option<String>,
    },
    /// Get a patch by event id
    Get {
        /// Patch event id (64-char hex)
        #[arg(long)]
        event: String,
    },
    /// List patches for a repo
    List {
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Filter by patch author pubkey
        #[arg(long)]
        author: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Set status on a patch (open/merged/closed/draft — NIP-34 kind:1630-1633)
    Status {
        /// Root patch event id (first patch of the series/revision)
        #[arg(long)]
        root: String,
        /// New status
        #[arg(long, value_parser = ["open", "merged", "closed", "draft"])]
        status: String,
        /// Markdown context for the status change ('-' to read from stdin)
        #[arg(long)]
        content: Option<String>,
        /// Repo owner pubkey — requires --repo-id
        #[arg(long, requires = "repo_id")]
        repo_owner: Option<String>,
        /// Repo identifier (d-tag) — requires --repo-owner
        #[arg(long, requires = "repo_owner")]
        repo_id: Option<String>,
        /// Earliest-unique-commit of the repo
        #[arg(long)]
        euc: Option<String>,
        /// Root id of the revision that was accepted (status=merged only)
        #[arg(long)]
        revision: Option<String>,
        /// Additional recipient pubkey(s) for the status event (besides the
        /// repo owner, which is tagged automatically when --repo-owner is
        /// given) — e.g. root/revision author. Can be specified multiple times.
        #[arg(long = "to")]
        to: Vec<String>,
        /// Applied patch event id — can be specified multiple times (status=merged only).
        /// Accepts `<id>`, `<id>:<relay-url>`, or `<id>:<relay-url>:<pubkey>`.
        #[arg(long = "q")]
        q: Vec<String>,
        /// Merge commit id (status=merged only)
        #[arg(long)]
        merge_commit: Option<String>,
        /// Commit id applied to the target branch — can be specified multiple times (status=merged only)
        #[arg(long = "applied-as-commit")]
        applied_as_commit: Vec<String>,
    },
}

#[derive(Subcommand)]
pub enum PrCmd {
    /// Open a git pull request (NIP-34 kind:1618)
    #[command(
        after_help = "Examples:\n  bee pr open --repo-owner <hex> --repo-id myrepo --subject 'Fix bug' --body-file - --commit $(git rev-parse HEAD) --clone https://relay/git/owner/myrepo --branch-name fix-bug\n  bee pr update --repo-owner <hex> --repo-id myrepo --pr <event> --pr-author <hex> --commit $(git rev-parse HEAD) --clone https://relay/git/owner/myrepo"
    )]
    Open {
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Pull request subject/header
        #[arg(long, alias = "title")]
        subject: String,
        /// Pull request body markdown. Use '-' to read from stdin.
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Path to pull request body markdown, or '-' to read from stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Tip commit of the PR branch
        #[arg(long)]
        commit: String,
        /// Clone URL where the tip commit can be fetched — can be specified multiple times
        #[arg(long = "clone", required = true)]
        clone: Vec<String>,
        /// Recommended branch name
        #[arg(long)]
        branch_name: Option<String>,
        /// Most recent common ancestor with the target branch
        #[arg(long)]
        merge_base: Option<String>,
        /// Earliest-unique-commit of the repo
        #[arg(long)]
        euc: Option<String>,
        /// Label — can be specified multiple times
        #[arg(long = "label")]
        label: Vec<String>,
        /// Additional recipient pubkey(s) — can be specified multiple times
        #[arg(long = "to")]
        to: Vec<String>,
        /// Channel where this pull request originated (NIP-29 h-tag)
        #[arg(long)]
        channel: Option<String>,
        /// Root patch event id this PR revises
        #[arg(long)]
        revision_of: Option<String>,
    },
    /// Update a git pull request tip (NIP-34 kind:1619)
    Update {
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Pull request event id being updated
        #[arg(long)]
        pr: String,
        /// Pull request author's pubkey
        #[arg(long)]
        pr_author: String,
        /// Updated tip commit of the PR branch
        #[arg(long)]
        commit: String,
        /// Clone URL where the updated tip commit can be fetched — can be specified multiple times
        #[arg(long = "clone", required = true)]
        clone: Vec<String>,
        /// Markdown context for the update. Use '-' to read from stdin.
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Path to markdown context for the update, or '-' to read from stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Most recent common ancestor with the target branch
        #[arg(long)]
        merge_base: Option<String>,
        /// Earliest-unique-commit of the repo
        #[arg(long)]
        euc: Option<String>,
        /// Additional recipient pubkey(s) — can be specified multiple times
        #[arg(long = "to")]
        to: Vec<String>,
    },
    /// Get a PR by event id
    Get {
        /// PR event id (64-char hex)
        #[arg(long)]
        event: String,
    },
    /// List PRs for a repo
    List {
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Filter by PR author pubkey
        #[arg(long)]
        author: Option<String>,
        /// Filter by label
        #[arg(long)]
        label: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Set status on a PR (open/merged/closed/draft — NIP-34 kind:1630-1633)
    Status {
        /// Pull request event id
        #[arg(long)]
        pr: String,
        /// New status
        #[arg(long, value_parser = ["open", "merged", "closed", "draft"])]
        status: String,
        /// Markdown context for the status change. Use '-' to read from stdin.
        #[arg(long, conflicts_with = "body_file")]
        body: Option<String>,
        /// Path to markdown context for the status change, or '-' to read from stdin.
        #[arg(long, conflicts_with = "body")]
        body_file: Option<String>,
        /// Repo owner pubkey — requires --repo-id
        #[arg(long, requires = "repo_id")]
        repo_owner: Option<String>,
        /// Repo identifier (d-tag) — requires --repo-owner
        #[arg(long, requires = "repo_owner")]
        repo_id: Option<String>,
        /// Earliest-unique-commit of the repo
        #[arg(long)]
        euc: Option<String>,
        /// Additional recipient pubkey(s) for the status event (besides the
        /// repo owner, which is tagged automatically when --repo-owner is
        /// given) — e.g. PR author/reviewers. Can be specified multiple times.
        #[arg(long = "to")]
        to: Vec<String>,
        /// Merge commit id (status=merged only)
        #[arg(long)]
        merge_commit: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum IssuesCmd {
    /// Create a git issue (NIP-34 kind:1621)
    Create {
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Issue title
        #[arg(long, alias = "subject")]
        title: String,
        /// Issue body, markdown. Use '-' to read from stdin.
        #[arg(long)]
        content: String,
        /// Label — can be specified multiple times
        #[arg(long = "label")]
        label: Vec<String>,
        /// Additional recipient pubkey(s) — can be specified multiple times
        #[arg(long = "to")]
        to: Vec<String>,
    },
    /// Get an issue by event id
    Get {
        /// Issue event id (64-char hex)
        #[arg(long)]
        event: String,
    },
    /// List issues for a repo
    List {
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Filter by issue author pubkey
        #[arg(long)]
        author: Option<String>,
        /// Filter by label
        #[arg(long)]
        label: Option<String>,
        /// Maximum number of results
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Set status on an issue (open/resolved/closed/draft — NIP-34 kind:1630-1633)
    Status {
        /// Issue event id
        #[arg(long)]
        issue: String,
        /// New status
        #[arg(long, value_parser = ["open", "resolved", "closed", "draft"])]
        status: String,
        /// Markdown context for the status change ('-' to read from stdin)
        #[arg(long)]
        content: Option<String>,
        /// Repo owner pubkey — requires --repo-id
        #[arg(long, requires = "repo_id")]
        repo_owner: Option<String>,
        /// Repo identifier (d-tag) — requires --repo-owner
        #[arg(long, requires = "repo_owner")]
        repo_id: Option<String>,
        /// Earliest-unique-commit of the repo
        #[arg(long)]
        euc: Option<String>,
        /// Additional recipient pubkey(s) for the status event (besides the
        /// repo owner, which is tagged automatically when --repo-owner is
        /// given) — e.g. the issue author. Can be specified multiple times.
        #[arg(long = "to")]
        to: Vec<String>,
    },
    /// Assign an issue to one or more people or agents. Only assignments
    /// signed by the issue author or repo owner are trusted by clients;
    /// anyone may assign themselves (sole assignee = your own pubkey).
    Assign {
        /// Issue event id (64-char hex)
        #[arg(long)]
        issue: String,
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Assignee pubkey (64-char hex) — can be specified multiple times
        #[arg(long = "assignee", required = true)]
        assignee: Vec<String>,
        /// Human-readable assignee name(s) for the note body, e.g. "Thomas".
        /// Defaults to the truncated assignee pubkeys.
        #[arg(long)]
        label: Option<String>,
    },
    /// Remove one or more assignees from an issue. Issue authors and repo
    /// owners may remove anyone; other users may remove only themselves.
    Unassign {
        /// Issue event id (64-char hex)
        #[arg(long)]
        issue: String,
        /// Repo owner pubkey (64-char hex)
        #[arg(long)]
        repo_owner: String,
        /// Repo identifier (d-tag)
        #[arg(long)]
        repo_id: String,
        /// Assignee pubkey to remove — can be specified multiple times
        #[arg(long = "assignee", required = true)]
        assignee: Vec<String>,
        /// Human-readable assignee name(s) for the note body.
        /// Defaults to the truncated assignee pubkeys.
        #[arg(long)]
        label: Option<String>,
    },
}

#[derive(Subcommand)]
pub enum UploadCmd {
    /// Upload a file to the relay's Blossom store
    File {
        /// Path to the file to upload
        #[arg(long)]
        file: String,
    },
}

#[derive(Subcommand)]
pub enum MediaCmd {
    /// Download relay media with Blossom get auth
    Get {
        /// Relay media URL or sha256[.ext] path segment
        input: String,
        /// Output path. Omit or use '-' to write raw bytes to stdout.
        #[arg(short, long)]
        output: Option<String>,
    },
}

/// Subcommands for `bee mem`.
#[derive(Subcommand)]
pub enum MemCmd {
    /// List non-tombstoned memory entries
    Ls {
        /// Owner pubkey (hex). Overrides BUZZ_AUTH_TAG.
        #[arg(long)]
        owner: Option<String>,
        /// Agent pubkey (hex) to read as this key's owner.
        #[arg(long)]
        agent: Option<String>,
        /// Emit JSON instead of tab-delimited lines.
        #[arg(long, default_value_t = false)]
        json: bool,
    },
    /// Print the value of a slug to stdout (no trailing newline)
    Get {
        slug: String,
        #[arg(long)]
        owner: Option<String>,
        /// Agent pubkey (hex) to read as this key's owner.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Print sha256(value) in hex (use as `--base-hash` for `mem patch`).
    Hash {
        slug: String,
        #[arg(long)]
        owner: Option<String>,
        /// Agent pubkey (hex) to read as this key's owner.
        #[arg(long)]
        agent: Option<String>,
    },
    /// Set a slug's value. Pass `-` to read the value from stdin.
    Set {
        slug: String,
        value: String,
        #[arg(long)]
        owner: Option<String>,
        /// Allow committing an empty value. Without this, a zero-byte stdin
        /// read is rejected to prevent silent data loss from upstream
        /// pipeline failures.
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
    },
    /// Apply a unified diff to a slug's current value (safer than set).
    ///
    /// Reads the diff from stdin or `--patch-file`. Refuses to apply if the
    /// slug has changed since `--base-hash` was captured, and refuses
    /// hunks whose context doesn't match the current value verbatim.
    Patch {
        slug: String,
        /// Read the patch from a file instead of stdin.
        #[arg(long)]
        patch_file: Option<String>,
        /// sha256 hex digest (lowercase) of the value the patch was generated
        /// against. Hashes the exact UTF-8 bytes returned by `bee mem get`,
        /// not normalized lines. Run `bee mem hash <slug>` to capture this
        /// before editing.
        #[arg(long)]
        base_hash: Option<String>,
        /// Skip the base-hash check. Unsafe if concurrent edits are possible —
        /// the patch will be applied against whatever the current value is,
        /// even if another agent rewrote it after the patch was generated.
        #[arg(long, default_value_t = false)]
        no_base_hash: bool,
        /// Echo the input patch + resulting sha256 and exit without writing.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        /// Allow committing an empty result.
        #[arg(long, default_value_t = false)]
        allow_empty: bool,
        #[arg(long)]
        owner: Option<String>,
    },
    /// Publish a tombstone for a slug (cannot be used on `core`).
    Rm {
        slug: String,
        #[arg(long)]
        owner: Option<String>,
    },
}

/// Subcommands for `bee pack`.
#[derive(Subcommand)]
pub enum PackCmd {
    /// Validate a persona pack directory
    Validate {
        /// Path to the pack directory
        path: String,
    },
    /// Inspect a persona pack — show metadata and effective config
    Inspect {
        /// Path to the pack directory
        path: String,
    },
}

pub use commands::packs_cli::PacksCmd;

/// Terminal git access to the relay's own git hosting.
///
/// The relay speaks NIP-98 over git's `authtype` credential capability, which
/// `git-credential-nostr` answers. These commands write the git config that
/// points git at it — scoped to the relay's `/git` path so `osxkeychain` (or
/// whatever serves GitHub) is left alone.
#[derive(Subcommand)]
pub enum GitCmd {
    /// Write the git config for terminal push/clone against the relay
    Setup {
        /// Path to git-credential-nostr. Defaults to finding it on PATH.
        #[arg(long)]
        helper: Option<PathBuf>,
        /// Key file git-credential-nostr reads. Defaults to ~/.nostr/key.
        #[arg(long)]
        keyfile: Option<PathBuf>,
        /// Which git config to write.
        #[arg(long, value_enum, default_value = "global")]
        scope: commands::git_setup::ConfigScope,
        /// Also write the identity to the key file at mode 0600.
        /// Requires BUZZ_PRIVATE_KEY; never overwrites a different identity.
        #[arg(long)]
        write_key: bool,
        /// Print the commands instead of running them; changes nothing.
        #[arg(long)]
        print: bool,
    },
    /// Report whether terminal git access is configured locally.
    ///
    /// Local only — it cannot say whether the relay accepts the key. Use
    /// `bee git check` for that.
    Status {
        /// Key file to check. Defaults to whatever `nostr.keyfile` names.
        #[arg(long)]
        keyfile: Option<PathBuf>,
    },
    /// Ask the relay's git transport whether it accepts this key — the same
    /// request `git clone` makes, signed the same way.
    ///
    /// Uses the key `git-credential-nostr` itself would use
    /// (`$NOSTR_PRIVATE_KEY`, else `git config nostr.keyfile`) — not
    /// `BUZZ_PRIVATE_KEY` — so the verdict is about the identity git presents.
    /// Exit 0 when the transport accepts, 3 when it denies.
    Check {
        /// Key file to test. Defaults to whatever `nostr.keyfile` names.
        #[arg(long)]
        keyfile: Option<PathBuf>,
        /// Also probe `git-receive-pack` — the request `git push` makes first.
        #[arg(long)]
        push: bool,
        /// Predict what the pre-receive hook would decide for this ref.
        ///
        /// A prediction, never a promise: the hook decides at push time, on
        /// the commit actually sent.
        #[arg(long = "ref")]
        ref_name: Option<String>,
        /// The commit the prediction is about. Defaults to `HEAD` in this
        /// checkout.
        #[arg(long)]
        sha: Option<String>,
    },
}

/// Community moderation commands.
///
/// The community (tenant) is selected by the relay host in `--relay` /
/// `BUZZ_RELAY_URL` — moderation commands are community-global and carry no
/// channel scope. The signing key must be a community owner/admin; the relay
/// authorizes every command.
#[derive(Subcommand)]
pub enum ModerationCmd {
    /// List reports in the moderation queue (newest first)
    #[command(
        after_help = "Examples:\n  bee moderation reports\n  bee moderation reports --status open --limit 20"
    )]
    Reports {
        /// Filter by status: open | resolved | dismissed | escalated (default: all)
        #[arg(long)]
        status: Option<String>,
        /// Maximum number of reports to return
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
    /// Resolve or dismiss a report (kind 9044)
    #[command(
        after_help = "Examples:\n  bee moderation resolve --report <REPORT_EVENT_ID> --status dismissed --action dismiss\n  bee moderation resolve --report <REPORT_EVENT_ID> --status resolved --action ban --reason \"rule 3\""
    )]
    Resolve {
        /// Hex event id of the kind:1984 report being resolved
        #[arg(long)]
        report: String,
        /// Resolution status: resolved | dismissed
        #[arg(long)]
        status: String,
        /// Action taken: delete | kick | ban | timeout | dismiss | escalate
        #[arg(long)]
        action: String,
        /// Optional reason — relayed to the reporter, so keep it tombstone-safe
        #[arg(long)]
        reason: Option<String>,
    },
    /// Ban a member from the community (kind 9040)
    #[command(
        after_help = "Examples:\n  bee moderation ban --pubkey <HEX>\n  bee moderation ban --pubkey <HEX> --expires-in 604800 --reason \"repeated spam\""
    )]
    Ban {
        /// Target member pubkey (hex)
        #[arg(long)]
        pubkey: String,
        /// Ban duration in seconds from now (omit for a permanent ban)
        #[arg(long, conflicts_with = "expires_at")]
        expires_in: Option<u64>,
        /// Absolute ban expiry as a unix timestamp (seconds)
        #[arg(long)]
        expires_at: Option<u64>,
        /// Optional private ban reason (audit only)
        #[arg(long)]
        reason: Option<String>,
    },
    /// Lift a member's ban (kind 9041)
    Unban {
        /// Target member pubkey (hex)
        #[arg(long)]
        pubkey: String,
    },
    /// Time out a member — a write-block, not a disconnect (kind 9042)
    #[command(
        after_help = "Examples:\n  bee moderation timeout --pubkey <HEX> --expires-in 3600\n  bee moderation timeout --pubkey <HEX> --expires-at 1783500000 --reason \"cool off\""
    )]
    Timeout {
        /// Target member pubkey (hex)
        #[arg(long)]
        pubkey: String,
        /// Timeout duration in seconds from now
        #[arg(long, conflicts_with = "expires_at")]
        expires_in: Option<u64>,
        /// Absolute timeout expiry as a unix timestamp (seconds)
        #[arg(long)]
        expires_at: Option<u64>,
        /// Optional private timeout reason (audit only)
        #[arg(long)]
        reason: Option<String>,
    },
    /// Clear a member's timeout early (kind 9043)
    Untimeout {
        /// Target member pubkey (hex)
        #[arg(long)]
        pubkey: String,
    },
    /// List currently-restricted members (active ban or timeout)
    Restricted,
    /// Read the moderation audit trail (newest first)
    Audit {
        /// Maximum number of audit rows to return
        #[arg(long, default_value_t = 50)]
        limit: i64,
    },
}

/// Read-side analysis over recorded coding sessions.
///
/// The relay's stored 442xx rows are the analysis database; these commands are
/// the shortest path into them. `docs/coding-session-analysis.md` covers the
/// direct-SQL equivalents for questions this surface does not answer.
#[derive(Subcommand)]
pub enum SessionsCmd {
    /// List the coding-session generations recorded in a channel
    #[command(
        after_help = "Examples:\n  bee sessions list --channel <uuid>\n  bee --format compact sessions list --channel <uuid>\n\nRecipe:\n  bee sessions list --channel <uuid>"
    )]
    List {
        /// Channel UUID the sessions were published into
        #[arg(long)]
        channel: String,
    },
    /// Print one generation's transcript in sequence order
    #[command(
        after_help = "Examples:\n  bee sessions transcript --channel <uuid> --session <session-id>\n  bee sessions transcript --channel <uuid> --target '<cs-target>' --format jsonl\n\nRecipe:\n  bee sessions transcript --channel <uuid> --session <session-id>"
    )]
    Transcript {
        /// Channel UUID the session was published into
        #[arg(long)]
        channel: String,
        /// Exact `cs-target` key (from `sessions list`)
        #[arg(long, conflicts_with = "session", required_unless_present = "session")]
        target: Option<String>,
        /// Provider-minted session id; resolved through `sessions list`
        #[arg(long)]
        session: Option<String>,
        /// Transcript shape: 'md' (default, rendered) or 'jsonl' (raw signed events)
        #[arg(long, value_enum, default_value = "md")]
        format: TranscriptFormat,
    },
    /// Delete a coding session outright (one kind:5 over its whole chain).
    ///
    /// The relay refuses a genesis or a closure deleted on its own — the
    /// first would strand the session's closure revisions, the second would
    /// roll shared state back with no counter-revision. So this assembles
    /// the whole session in one deletion: its genesis, every closure, and
    /// its metadata, transcript, goal, name and team records.
    ///
    /// The session's reference is **never released**. A deleted session's
    /// `sessionRef` stays claimed for good, exactly as a deleted repository
    /// keeps its name, so this can never be used to re-found a session under
    /// an identity that already existed.
    ///
    /// Signed by the session's founder, or by an Owner of the project the
    /// session's channel belongs to.
    ///
    /// This does not stop a running execution on its host. Close or stop the
    /// session first if one is still live; a closure frees the host slot.
    #[command(
        after_help = "Examples:\n  bee sessions delete --channel <uuid> --session-ref <uuid>\n  bee sessions delete --channel <uuid> --session-ref <uuid> --dry-run\n\nRecipe:\n  bee sessions delete --channel <uuid> --session-ref <uuid> --dry-run"
    )]
    Delete {
        /// Channel UUID the session was published into
        #[arg(long)]
        channel: String,
        /// The umbrella session's reference (its genesis `d` tag)
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Print what would be deleted and exit without publishing.
        #[arg(long, default_value_t = false)]
        dry_run: bool,
    },
    /// Diagnose how each turn ended — spans, unterminated tools, stalled prompts
    #[command(
        after_help = "Examples:\n  bee sessions doctor --channel <uuid>\n  bee --format compact sessions doctor --channel <uuid> --target '<cs-target>'\n\nReports, per turn: wall span, unterminated tool calls, the terminal result's\ntoken counts, and the `turn_wire` row when the producer published one. A\nfailed turn whose result carries no usage was never resolved by the agent.\n\nRecipe:\n  bee sessions doctor --channel <uuid>"
    )]
    Doctor {
        /// Channel UUID the session was published into
        #[arg(long)]
        channel: String,
        /// Restrict to one generation's `cs-target` key
        #[arg(long)]
        target: Option<String>,
    },
    /// Per-turn usage and waste for a channel's coding sessions.
    ///
    /// Reads only what the transcript (kind 44225) already carries: the
    /// terminal `result` item's `usage` block, its `costUsd` and duration, and
    /// the `tool_call`/`tool_result` pairs around it. Every number is measured
    /// — an absent measurement prints `null`, never `0`, and no cost is ever
    /// computed here against a price list. At most 4,096 items are folded per
    /// execution; the report discloses when it stopped, and a turn whose tool
    /// count had to be taken from a clipped stream carries
    /// `toolCallsTruncated: true`.
    ///
    /// Beyond the per-turn table it reports three kinds of waste: the same
    /// file or command handed to one seat more than once (`handedTwice`, with
    /// the published byte count, how many of those calls were answered, and
    /// whether the provider clipped a result), the
    /// `bee sessions status|inbox|send|operation` reads a seat pulled into its
    /// own context (`roomDownloads`), and three or more consecutive identical
    /// commands (`retryLoops`, whose `identicalResults` is `null` when the
    /// results were never published).
    #[command(
        after_help = "Examples:\n  bee sessions audit --channel <uuid>\n  bee sessions audit --channel <uuid> --session-ref <uuid>\n  bee --format compact sessions audit --channel <uuid>   # one JSON row per turn\n\nRecipe:\n  bee sessions audit --channel <uuid> --session-ref <uuid>"
    )]
    Audit {
        /// Channel UUID to audit
        #[arg(long)]
        channel: String,
        /// Restrict the audit to one umbrella's executions
        #[arg(long = "session-ref")]
        session_ref: Option<String>,
    },
    /// Aggregate tool usage and error rates across transcripts
    #[command(
        after_help = "Examples:\n  bee sessions tools --channel <uuid>\n  bee sessions tools --channel <uuid> --target '<cs-target>'\n\nRecipe:\n  bee sessions tools --channel <uuid>"
    )]
    Tools {
        /// Channel UUID to aggregate over
        #[arg(long)]
        channel: String,
        /// Restrict the aggregate to one generation's `cs-target` key
        #[arg(long)]
        target: Option<String>,
    },
    /// Write every generation's raw events to a directory, with a manifest
    #[command(
        after_help = "Examples:\n  bee sessions export --channel <uuid> --out ./session-archive\n\nThe directory must be absent or empty — an export never overwrites.\n\nRecipe:\n  bee sessions export --channel <uuid> --out <dir>"
    )]
    Export {
        /// Channel UUID to export
        #[arg(long)]
        channel: String,
        /// Destination directory; created if absent, refused if non-empty
        #[arg(long)]
        out: String,
    },
    /// Grant a pubkey authority over a coding session (NIP-CSAT kind 44228).
    ///
    /// Two tiers, and the set is closed. `collaborator` maps to
    /// `grant-operator` (may steer) and `viewer` to `grant-viewer`
    /// (read-only): both say what a human may do to a session, and only the
    /// session owner (the genesis signer) may extend that chain.
    ///
    /// Who an actor *is* inside an umbrella — `lead`, `builder`, `verifier`, …
    /// — is a **role seat**, and it has its own verb, `grant-seat`. Writing
    /// authority is opted into, never reached by mistyping a tier here.
    #[command(
        after_help = "Examples:\n  bee sessions grant --channel <uuid> --genesis <64-hex> --pubkey <64-hex> --role collaborator\n\nA role seat is granted with `bee sessions grant-seat` and withdrawn with\n`bee sessions revoke-seat`; `revoke` only clears these two tiers.\n\nRecipe:\n  bee sessions grant --channel <uuid> --genesis <hex64> --pubkey <hex64> --role collaborator"
    )]
    Grant {
        /// Channel UUID the session's authority chain lives in
        #[arg(long)]
        channel: String,
        /// Genesis event id (64-char hex) the chain roots at
        #[arg(long)]
        genesis: String,
        /// Grantee pubkey (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
        /// Grant tier: `collaborator` (may steer) or `viewer` (read-only)
        #[arg(long, value_enum)]
        role: GrantRoleArg,
    },
    /// Seat one actor in a role inside an umbrella (NIP-CSAT kind 44228
    /// `grant-seat`).
    ///
    /// A role seat says who an actor *is* — the fact the typed team fold reads
    /// for verifier standing — as opposed to what a human may do to a session,
    /// which is `grant`. It may be written by the founder, an active steering
    /// operator, or an active lead (a lead may not grant `lead`), and it is
    /// idempotent: an actor already holding that exact role reports
    /// `already_granted` with no write. An actor holding a *different* role is
    /// refused — withdraw the held seat with `revoke-seat` first, because a
    /// seated actor is never silently re-roled.
    #[command(
        name = "grant-seat",
        after_help = "Examples:\n  bee sessions grant-seat --channel <uuid> --genesis <64-hex> --pubkey <64-hex> --role builder\n\nExit codes: 0 granted or already granted, 1 refused, 5 submitted but\nunconfirmed by the accepted chain.\n\nRecipe:\n  bee sessions grant-seat --channel <uuid> --genesis <hex64> --pubkey <hex64> --role builder"
    )]
    GrantSeat {
        /// Channel UUID the session's authority chain lives in
        #[arg(long)]
        channel: String,
        /// Genesis event id (64-char hex) the chain roots at
        #[arg(long)]
        genesis: String,
        /// Actor pubkey taking the seat (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
        /// Role slug the actor is seated in (`[a-z0-9-]`, 1-64 bytes)
        #[arg(long)]
        role: String,
        /// Umbrella session reference; resolved from the signed genesis when
        /// omitted
        #[arg(long = "session-ref")]
        session_ref: Option<String>,
    },
    /// Withdraw one actor's exact role seat (NIP-CSAT kind 44228 `revoke-seat`).
    ///
    /// The counterpart of `grant-seat`, and the write that settles a
    /// disputed role: two founder-signed seated creates are history and
    /// neither can be withdrawn, so which role an actor holds is decided only
    /// on the accepted authority chain. Refused unless the pubkey holds that
    /// exact role — the relay's transition matrix remains the gate; this
    /// refuses first, naming the role the actor really holds.
    #[command(
        name = "revoke-seat",
        after_help = "Examples:\n  bee sessions revoke-seat --channel <uuid> --genesis <64-hex> --pubkey <64-hex> --role builder\n\nExit codes: 0 revoked, 1 refused (no seat, or a different role), 5 submitted\nbut unconfirmed by the accepted chain.\n\nRecipe:\n  bee sessions revoke-seat --channel <uuid> --genesis <hex64> --pubkey <hex64> --role builder"
    )]
    RevokeSeat {
        /// Channel UUID the session's authority chain lives in
        #[arg(long)]
        channel: String,
        /// Genesis event id (64-char hex) the chain roots at
        #[arg(long)]
        genesis: String,
        /// Actor pubkey losing its seat (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
        /// The exact role slug it holds; a mismatch is refused
        #[arg(long)]
        role: String,
        /// Umbrella session reference; resolved from the signed genesis when
        /// omitted
        #[arg(long = "session-ref")]
        session_ref: Option<String>,
    },
    /// Revoke a pubkey's live coding-session grant (NIP-CSAT kind 44228).
    ///
    /// The relay refuses a revoke naming a pubkey with no live grant.
    #[command(
        after_help = "Recipe:\n  bee sessions revoke --channel <uuid> --genesis <hex64> --pubkey <hex64>"
    )]
    Revoke {
        /// Channel UUID the session's authority chain lives in
        #[arg(long)]
        channel: String,
        /// Genesis event id (64-char hex) the chain roots at
        #[arg(long)]
        genesis: String,
        /// Pubkey losing its grant (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
    },
    /// Print a session's folded grant map plus pending transitions.
    ///
    /// Grants are folded from relay acceptance receipts (kind 40099) in
    /// sequence order; transitions with no matching receipt are listed as
    /// pending.
    #[command(after_help = "Recipe:\n  bee sessions roster --channel <uuid> --genesis <hex64>")]
    Roster {
        /// Channel UUID the session's authority chain lives in
        #[arg(long)]
        channel: String,
        /// Genesis event id (64-char hex) the chain roots at
        #[arg(long)]
        genesis: String,
    },
    /// Publish a signed team assignment (kind 44244).
    #[command(
        after_help = "Recipe:\n  bee sessions assign --channel <uuid> --session-ref <uuid> --genesis <hex64> --body @assignment.json --wake-to builder"
    )]
    Assign(TeamTransactionWriteArgs),
    /// Publish a signed assignment report (kind 44244).
    #[command(
        after_help = "Rule:\n  reports **included** by assignee-equality whose author holds no active seat for the assignment's `assignee_role`.\n\nRecipe:\n  bee sessions report --channel <uuid> --session-ref <uuid> --genesis <hex64> --body @report.json --wake-to lead"
    )]
    Report(TeamTransactionWriteArgs),
    /// Publish a signed refutation or disposition (kind 44244).
    #[command(
        after_help = "Recipe:\n  bee sessions verdict --channel <uuid> --session-ref <uuid> --genesis <hex64> --body @disposition.json --wake-to builder"
    )]
    Verdict(TeamTransactionWriteArgs),
    /// Acknowledge receipt of a governing disposition (kind 44244).
    #[command(
        after_help = "Recipe:\n  bee sessions acknowledge --channel <uuid> --session-ref <uuid> --genesis <hex64> --body @acknowledgement.json"
    )]
    Acknowledge(TeamTransactionWriteArgs),
    /// Publish mission completion after locally verifying every approval chain.
    #[command(
        after_help = "Recipe:\n  bee sessions complete --channel <uuid> --session-ref <uuid> --genesis <hex64> --body @completion.json"
    )]
    Complete(TeamTransactionWriteArgs),
    /// Publish an explicit terminal blocker (kind 44244).
    #[command(
        after_help = "Recipe:\n  bee sessions block --channel <uuid> --session-ref <uuid> --genesis <hex64> --body @blocked.json"
    )]
    Block(TeamTransactionWriteArgs),
    /// Say something without changing any mission state (kind 44244).
    ///
    /// A note is listed in the fold and settles nothing: it is not a phase,
    /// not a terminal, and can neither correct nor be corrected. Reach for it
    /// instead of a `mission.blocked` whenever the mission has not actually
    /// stopped.
    #[command(
        after_help = "Examples:\n  bee sessions note --channel <uuid> --session-ref <uuid> --genesis <hex64> --text 'lane B is rebasing, nothing is blocked'\n  bee sessions note --channel <uuid> --session-ref <uuid> --genesis <hex64> --text 'context for the ruling' --ref <event-id> --ref <event-id>\n\nRecipe:\n  bee sessions note --channel <uuid> --session-ref <uuid> --genesis <hex64> --text <note-text>"
    )]
    Note(TeamNoteArgs),
    /// Ask for, or give, a ruling the mission needs (kind 44244).
    #[command(
        subcommand,
        after_help = "Recipe:\n  bee sessions decide answer --channel <uuid> --session-ref <uuid> --genesis <hex64> --request <hex64> --choice <chosen-option>"
    )]
    Decide(TeamDecisionCmd),
    /// Read signed team operations and their deterministic fold.
    #[command(
        subcommand,
        after_help = "Recipe:\n  bee sessions operation list --channel <uuid> --session-ref <uuid> --genesis <hex64>"
    )]
    Operation(TeamOperationCmd),
    /// Publish one observation: something you saw while working (kind 44246).
    ///
    /// An observation **settles nothing**. It grants nothing, blocks nothing
    /// and excludes nothing — a mission's state is decided entirely by kind
    /// 44244. Any active seat or the founder may observe; the relay validates
    /// structure only, and it is the reader that says who wrote what.
    ///
    /// Every duration and `--started-at-ms` you pass is **your own
    /// measurement**. It is printed as your claim and is never used for
    /// ordering, discovery or dedupe.
    #[command(
        subcommand,
        after_help = "Rule:\n  Unknown tokens are refused, never ignored.\n\nRecipe:\n  bee sessions observe gate --channel <uuid> --session-ref <uuid> --gate <gate:outcome:command>"
    )]
    Observe(SessionObserveCmd),
    /// What this machine's session worktrees hold, and what may be removed
    ///
    /// L11. Reads the desktop host's own record of the worktrees it cut. A
    /// tree the host never recorded is listed and removed by nothing here.
    #[command(
        subcommand,
        after_help = "Recipe:\n  bee sessions worktree status --session <uuid>"
    )]
    Worktree(SessionWorktreeCmd),
    /// Print this umbrella's bounded observation fold (kind 44246).
    ///
    /// One row per checkpoint and phase, and one per `(author, gate)` and
    /// `(author, findingId)` — a later observation by the same author about the
    /// same gate or finding is the newer statement, and both event ids are
    /// listed because the older one is still on the wire. `--format compact`
    /// prints one line each.
    #[command(
        after_help = "Examples:\n  bee sessions observations --channel <uuid> --session-ref <uuid>\n  bee --format compact sessions observations --channel <uuid> --session-ref <uuid>\n\nRule:\n  Unknown tokens are refused, never ignored.\n\nRecipe:\n  bee sessions observations --channel <uuid> --session-ref <uuid>"
    )]
    Observations {
        /// Channel UUID containing the session.
        #[arg(long)]
        channel: String,
        /// Canonical umbrella session UUID.
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Session genesis event id. Resolved from the relay when omitted.
        #[arg(long)]
        genesis: Option<String>,
    },
    /// Continue another participant's work: check point it, claim it, pick it up.
    ///
    /// Three mechanisms that already exist, wired together. A **checkpoint**
    /// (kind 44247) says what the work is, durably enough for somebody else to
    /// pick it up — including what could **not** be preserved. A **claim** is
    /// one more link on the session's accepted 44228 chain, so racing claims
    /// are resolved by the relay and every consumer folds one answer. A
    /// **continuation** records what the claimant then did, labelled
    /// `native-resume` or `reconstructed` and never conflated.
    ///
    /// A claim hands over the **whole session**: it is rooted at the genesis,
    /// so every execution and every assignment under the umbrella moves and is
    /// fenced together. There is no per-execution handover in this version.
    #[command(
        subcommand,
        after_help = "Examples:\n  bee sessions handover status --channel <uuid> --session-ref <uuid>\n  bee sessions handover checkpoint --channel <uuid> --session-ref <uuid> --task '<what the work is>' --next '<next action>'\n\nNote:\n  A claim hands over the whole session, not one execution.\n\nRecipe:\n  bee sessions handover status --channel <uuid> --session-ref <uuid>"
    )]
    Handover(HandoverCmd),
    /// Set, read, or withdraw this umbrella's session policy (kind 44245).
    ///
    /// One signed record saying how a mission is meant to be run: posture,
    /// budgets, what the founder wants to be told, what a lane owes before its
    /// work counts, who may be benched, which acts stay the founder's, and
    /// when to stop.
    ///
    /// **Two fields are enforced**: `budget.turns`, at the provider's turn
    /// gate, and `gates.verifierRequired`, at the 44244 fold's completion
    /// check (so `bee sessions complete` refuses a completion no verifier has
    /// ruled on). Every other field is read and shown, never counted, and both
    /// `set` and `get` say exactly that in their own output.
    #[command(
        subcommand,
        after_help = "Recipe:\n  bee sessions policy get --channel <uuid> --session-ref <uuid> --genesis <hex64>"
    )]
    Policy(SessionPolicyCmd),
    /// Send a turn to a coding-session execution (kind 44220).
    ///
    /// `--to` names one execution three ways, tried in that order: an exact
    /// `cs-target` key, a provider session id, or a role slug. A role only
    /// resolves inside one umbrella — pass `--session-ref`, or run as a
    /// seated actor whose own umbrella scopes the lookup — and an ambiguous
    /// name is an error listing every candidate, never a guess.
    ///
    /// `accepted` reports the relay storing the command. Delivery is a
    /// separate fact and is read from the provider's first receipt for this
    /// `commandId`: `delivered` (true/false/null), `deliveryStatus` (the
    /// receipt's own word, or `unconfirmed`), and `delivery` (one sentence).
    /// A `--deliver steer` a runtime cannot honour reports `turn_degraded`;
    /// one the runtime injected into its running turn reports
    /// `turn_injected`; one written to the runtime and never acknowledged
    /// reports `turn_delivery_unknown` (`delivered: null` — the provider will
    /// not resend it, and neither will this command).
    #[command(
        after_help = "Examples:\n  echo 'rebase and re-run the gate' | bee sessions send --channel <uuid> --to builder --session-ref <uuid> --content -\n  bee sessions send --channel <uuid> --to '<cs-target>' --deliver interrupt --content 'stop'\n  bee sessions send --channel <uuid> --readdress <commandId>\n\nRecipe:\n  bee sessions send --channel <uuid> --session-ref <uuid> --to builder --content <turn-text>"
    )]
    Send {
        /// Channel UUID the session lives in
        #[arg(long)]
        channel: String,
        /// Addressee: a `cs-target` key, a provider session id, or a role slug
        #[arg(long, required_unless_present = "readdress")]
        to: Option<String>,
        /// Umbrella session reference (lowercase UUID) scoping a role lookup
        #[arg(long = "session-ref")]
        session_ref: Option<String>,
        /// Delivery class: boundary (default), steer, or interrupt
        #[arg(long, value_enum, default_value = "boundary")]
        deliver: DeliveryArg,
        /// Turn text, or `-` to read it from stdin
        #[arg(long, required_unless_present = "readdress")]
        content: Option<String>,
        /// Re-send an owed turn: the `commandId` of a 44220 answered
        /// `turn_dropped`/NO_LIVE_EXECUTION or `turn_refused`/STALE_GENERATION
        #[arg(long, conflicts_with_all = ["to", "content"])]
        readdress: Option<String>,
        /// Refused: 44220 carries no reply reference (see the error text)
        #[arg(long = "reply-to")]
        reply_to: Option<String>,
        /// Attach an image to the turn; repeatable. Uploaded to the relay's
        /// Blossom store and delivered to the agent as an ACP image block.
        /// Only reaches runtimes that advertise image prompts.
        #[arg(long = "image")]
        image: Vec<String>,
        /// Print the relay's acceptance without waiting for the provider's
        /// first turn receipt; delivery is then reported as unconfirmed
        #[arg(long = "no-wait")]
        no_wait: bool,
    },
    /// Create a coding-session execution (kind 44221 `session.create`).
    ///
    /// The brief becomes the create's `initialTurn`. Seated (agent) creates
    /// are refused here: an actor's key material is host-local custody the
    /// CLI does not hold — see `--actor`. A bare create under a verified
    /// NIP-OA identity first founds a new umbrella with the agent as immutable
    /// founder and grants its attested owner collaborator authority. Passing
    /// session coordinates joins that exact umbrella and never infers a grant.
    #[command(
        after_help = "Examples:\n  bee sessions create --channel <uuid> --session-ref <uuid> --genesis <hex> --provider-instance <ref> --provider-authority <hex> --model <id> --brief -\n  bee sessions create --channel <uuid> --provider-instance <ref> --provider-authority <hex> --brief 'start' --wait --timeout-secs 60\n\nRecipe:\n  bee sessions create --channel <uuid> --session-ref <uuid> --genesis <hex64> --provider-instance <ref> --provider-authority <hex64> --brief -\n\nFor a bare managed-agent create, the CLI first publishes a fresh genesis and a collaborator grant for the cryptographically verified NIP-OA owner; setup failure prevents dispatch. With --wait, the command publishes the create once and waits for the named provider's signed receipt. A confirmed receipt adds the exact cs-target; timeout preserves the commandId and never retries."
    )]
    Create {
        /// Channel UUID to publish the create into
        #[arg(long)]
        channel: String,
        /// Umbrella session reference (lowercase UUID) this execution joins
        #[arg(long = "session-ref")]
        session_ref: Option<String>,
        /// Genesis event id (64-char hex) founding the umbrella
        #[arg(long)]
        genesis: Option<String>,
        /// Capability-advertised provider instance reference
        #[arg(long = "provider-instance")]
        provider_instance: String,
        /// Signing pubkey (64-char lowercase hex) of the provider catalog authority
        #[arg(long = "provider-authority")]
        provider_authority: String,
        /// Provider-neutral model identifier
        #[arg(long)]
        model: Option<String>,
        /// Operator-facing session title
        #[arg(long)]
        title: Option<String>,
        /// NIP-MP project coordinate (`30621:<owner>:<d>`)
        #[arg(long)]
        project: Option<String>,
        /// Repository coordinate within the project
        #[arg(long)]
        repo: Option<String>,
        /// First turn to deliver after creation, or `-` to read it from stdin
        #[arg(long)]
        brief: Option<String>,
        /// Refused: actor custody is host-local (see the error text)
        #[arg(long)]
        actor: Option<String>,
        /// Refused: a role is half of the actor/role pair (see the error text)
        #[arg(long)]
        role: Option<String>,
        /// Refused: the driver slug is minted by the provider (see the error text)
        #[arg(long)]
        driver: Option<String>,
        /// Wait for the named provider to confirm this create and return its target.
        #[arg(long)]
        wait: bool,
        /// Maximum seconds to wait after relay acceptance (1..=300).
        #[arg(long = "timeout-secs")]
        timeout_secs: Option<u64>,
    },
    /// Ask an umbrella's host to seat a new agent on a role (kind 44221
    /// `session.hire`).
    ///
    /// A hire is a *request*, not a create. The signer must be the umbrella's
    /// founder, hold a live operator grant on it, or hold its active `lead`
    /// seat. A lead may hire only non-lead roles. The founder's host then
    /// applies its own standing policy — hiring on/off, allowed roles, a
    /// maximum number of live seats, allowed providers — chooses an installed
    /// identity whose home role matches, cuts that seat its own worktree, and
    /// publishes an ordinary seated create. That create's signed provider
    /// receipt and metadata are this hire's execution proof. The hiring CLI
    /// then uses its own signer key to append an accepted NIP-CSAT `grant-seat`
    /// for the exact actor-role pair; the provider never grants authority.
    ///
    /// The brief becomes the seat's first turn verbatim (the host prefixes
    /// it), so it is required: a seat hired with nothing to do is a bug.
    ///
    /// `accepted` reports the relay storing the request. `outcome` is a
    /// separate fact and reports the host: `created`, `failed` (the provider
    /// refused the seat), `refused` (the host's own policy refused the hire,
    /// with a `code`), `seating` (a seat was published but no provider
    /// receipt answered inside the wait), or `unconfirmed` (nothing answered
    /// at all). `created_ungranted` is a partial outcome: the execution is
    /// live, but the role-seat grant failed, so callers must not hire again.
    /// Exit codes follow: 0 created with accepted seat authority, 1
    /// refused/failed/created_ungranted, 2 relay error, 5
    /// seating/unconfirmed.
    ///
    /// ROUTING — the hire asks, the host answers. Pass `--class` and
    /// `--risk i,u,i` and the hire carries a routing REQUEST: the class, the
    /// risk triple, and whatever you asked for with `--profile`,
    /// `--review-flags` and `--challenger-sample`. It carries no model, no
    /// provider and no effort, because only the founder's host can see its own
    /// live kind:44222 catalog, and only the host may therefore decide. The
    /// host routes, writes the decision as a `routing` RECORD on the seat's
    /// create and its kind:44223 metadata, and the seat can then always be
    /// asked why it is the model it is.
    ///
    /// This command still runs the router locally — reading
    /// `team/model-registry.yaml` and the catalog it can see — but only for
    /// disclosure: the answer is attached to the request as `proposed`, and
    /// the host must say so on the create (`routing.proposedDisagreement`) if
    /// it lands somewhere else. A local router that cannot answer is reported
    /// as `proposedUnavailable` and does NOT block the hire: refusing here for
    /// a target this machine cannot see would be a refusal nobody asked for.
    /// Read the object a hire attaches with
    /// `bee --format json sessions route … | jq .proposed` — never `.routing`,
    /// which is the host's answer shape and is refused on a hire.
    ///
    /// `--override-model` with `--because` is the one way a hire dictates a
    /// target. It is checked against the catalog, recorded as an override, and
    /// never silently substituted; the host's own pick survives on the create
    /// as `runnerUp`. Only an override sets the hire's top-level `model` and
    /// `providerInstanceRef`. `--model` without `--class` is still an unrouted
    /// hire and carries no `routing` at all.
    ///
    /// A refusal's `code` is one of HIRE_OFF, HIRE_ROLE_NOT_ALLOWED,
    /// HIRE_LIMIT, HIRE_NO_IDENTITY, HIRE_ROLE_BUSY,
    /// HIRE_PROVIDER_NOT_ALLOWED, HIRE_MODEL_NOT_OFFERED, HIRE_NO_ROUTE,
    /// HIRE_MALFORMED or
    /// HIRE_STALE, and
    /// the detail carries the remedy for it. HIRE_NO_IDENTITY and
    /// HIRE_ROLE_BUSY are two different facts: the first means the host holds
    /// no identity for that role and only its operator can fix it; the second
    /// means it holds the role and every identity that is it is already
    /// seated in this umbrella, so the way forward is to brief the seat the
    /// reason names rather than to hire again. Model ids are the provider
    /// catalog's own ids — read
    /// them from `bee sessions status` (the `model` a live seat runs) or the
    /// runtime's kind:44222 catalog. There are no aliases or translations in
    /// this path: an id not offered exactly is refused HIRE_MODEL_NOT_OFFERED,
    /// with the offered ids in the reason.
    /// HIRE_NO_ROUTE means nothing the catalog offers clears the class gate at
    /// that risk tier; the reason names the binding trait and the best score
    /// available, and the answer is a different class, a different risk
    /// assessment, or an explicit override — never a quiet demotion.
    /// HIRE_MALFORMED means the hire's `routing` did not parse, and the reason
    /// names the failing key. It exists because on 2026-08-30 a routed hire the
    /// relay had accepted was classified malformed by the host and dropped with
    /// no answer at all, and a request that gets no answer is a crash with
    /// better manners.
    #[command(
        after_help = "Examples:\n  bee sessions hire --channel <uuid> --session-ref <uuid> --role builder --brief ./briefs/lane-c.md\n  bee sessions hire --channel <uuid> --session-ref <uuid> --role architect --model <id> --content 'Read §3 and report' --no-wait\n\nA relay that predates session.hire refuses the request as malformed; the\ncommand says so in those words rather than blaming the request.\n\nRule:\n  coding-session lifecycle command action.brief exceeds 12272 bytes (got <n>): a hire's brief becomes the seat's first turn behind the host's 16-byte \"[From the lead] \" prefix, so its ceiling is the initial-turn ceiling minus that prefix\n\nRecipe:\n  bee sessions hire --channel <uuid> --session-ref <uuid> --role builder --brief <path>"
    )]
    Hire {
        /// Channel UUID the umbrella lives in
        #[arg(long)]
        channel: String,
        /// Umbrella session reference (lowercase UUID) to hire into
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Genesis event id (64-char hex) founding the umbrella. Resolved from
        /// the channel when omitted; required when two geneses claim the label.
        #[arg(long)]
        genesis: Option<String>,
        /// Role slug to seat: `[a-z0-9-]`, 1-64 bytes
        #[arg(long)]
        role: String,
        /// Provider instance the seat should run on; the host's policy default
        /// when omitted
        #[arg(long = "provider-instance")]
        provider_instance: Option<String>,
        /// Model the seat should run on an UNROUTED hire; the chosen
        /// identity's own when omitted. To override a routed hire, pass
        /// --override-model with --because instead.
        #[arg(long)]
        model: Option<String>,
        /// Capability class to route for: lead, architect, builder, runner,
        /// ui_designer, researcher, verifier, poker. With --risk, the hire
        /// carries the routing request and the host decides.
        #[arg(long, requires = "risk")]
        class: Option<String>,
        /// Risk as `impact,uncertainty,irreversibility`, each 1-5. The tier
        /// and the effort are derived from it; there is no --tier.
        #[arg(long, requires = "class")]
        risk: Option<String>,
        /// Extra trait minimums as JSON, e.g. '{"taste":4.6}'
        #[arg(long, requires = "class")]
        profile: Option<String>,
        /// Spec §6 review triggers, comma-separated
        #[arg(long = "review-flags", requires = "class")]
        review_flags: Option<String>,
        /// Deliberately route a challenger for this class and mark the record
        #[arg(long = "challenger-sample", requires = "class")]
        challenger_sample: bool,
        /// Catalog id to run instead of whatever the host's router chooses.
        /// The one way a hire dictates a target; needs --class, --risk and
        /// --because.
        #[arg(long = "override-model", requires = "class", requires = "because")]
        override_model: Option<String>,
        /// Why you are overriding the router. Required with --override-model:
        /// an unexplained override is indistinguishable from a bug.
        #[arg(long, requires = "class")]
        because: Option<String>,
        /// File holding the brief, or `-` to read it from stdin
        #[arg(long, conflicts_with = "content")]
        brief: Option<String>,
        /// The brief as literal text, or `-` to read it from stdin
        #[arg(long)]
        content: Option<String>,
        /// Print the relay's acceptance without waiting for the host to
        /// answer; the outcome is then reported as unconfirmed
        #[arg(long = "no-wait")]
        no_wait: bool,
        /// Validate the brief and routing locally, print the facts the hire
        /// would carry, and publish NOTHING. Exit 0 when the payload is one
        /// the relay would accept, 1 when it is not. Use it for acceptance
        /// tests: a test that seats a live agent is not a test.
        #[arg(long, conflicts_with = "no_wait")]
        check: bool,
    },
    /// Grant the role seat a receipt-backed hire created but never got.
    ///
    /// The recovery path for the one failure `bee sessions hire` cannot undo:
    /// the host seated the role, the provider answered `created`, and the
    /// accepted kind:44228 authority chain never learned about it — because
    /// the receipt landed after the hire's window closed, or the grant write
    /// failed. The seat then runs, its report is folded in by assignee
    /// identity and disclosed as `unseatedReports`, and nothing grants it
    /// verifier authority. **Re-hiring cannot fix this** — a second hire
    /// carries a fresh `since` cutoff that excludes the create that already
    /// exists — and this command never publishes a hire.
    ///
    /// It reads the channel exactly once (no wait, no poll) and lets signed
    /// evidence — never a self-asserted `created_at` — choose which create to
    /// repair: candidates are the founder-signed seated creates for `--actor`,
    /// each judged on receipts that are cryptographically bound to it, and the
    /// one whose whole genesis → create → receipt → provider-metadata chain
    /// verifies is repaired with one `grant-seat` transition. It writes nothing
    /// else, ever.
    ///
    /// Outcomes and exit codes: `granted` and `already_granted` exit 0
    /// (running it twice is a no-op, by design); `no_receipt_yet` exits 5 — no
    /// candidate has a bound provider receipt yet, so there is nothing to
    /// grant against; `refused` exits 1 — nothing names the actor, a bound
    /// receipt failed the chain, the provider refused the create, or the actor
    /// already holds a different role; `ambiguous` exits 1 — the verifying
    /// creates disagree about the **role**, which only the founder can settle.
    /// Creates that imply the same `(actor, role)` write are never ambiguous,
    /// however many there are, so an umbrella that hired the same actor twice
    /// still reports `already_granted` on a second run. Every non-granting
    /// outcome names every candidate it considered, and none of them writes.
    #[command(
        after_help = "Examples:\n  bee sessions seat-repair --channel <uuid> --session-ref <uuid> --actor <64-hex>\n  bee --format compact sessions seat-repair --channel <uuid> --session-ref <uuid> --genesis <64-hex> --actor <64-hex>\n\nSafe to re-run: a seat that already holds the exact role is reported\n`already_granted` with no write. Never hire again to recover a seat.\n\nRule:\n  execution has `agentRef` and `role` (a receipt-backed create) and no live seat for that exact pair\n\nRecipe:\n  bee sessions seat-repair --channel <uuid> --session-ref <uuid> --actor <hex64>"
    )]
    SeatRepair {
        /// Channel UUID the umbrella lives in
        #[arg(long)]
        channel: String,
        /// Umbrella session reference (lowercase UUID) the seat joined
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Genesis event id (64-char hex) founding the umbrella. Resolved from
        /// the channel when omitted; required when two geneses claim the label.
        #[arg(long)]
        genesis: Option<String>,
        /// Pubkey (64-char lowercase hex) the host seated
        #[arg(long)]
        actor: String,
    },
    /// List turns addressed to executions this identity is seated on.
    ///
    /// Oldest first, each row carrying the newest receipt stage its command
    /// has been answered with.
    #[command(
        after_help = "Examples:\n  bee sessions inbox --channel <uuid>\n  bee sessions inbox --channel <uuid> --since <event-id>\n\nRecipe:\n  bee sessions inbox --channel <uuid>"
    )]
    Inbox {
        /// Channel UUID to read
        #[arg(long)]
        channel: String,
        /// Exclusive cursor: the event id of the last row already handled
        #[arg(long)]
        since: Option<String>,
    },
    /// Per-execution liveness, seat, and open-turn state for a channel.
    ///
    /// The output shape follows stdout unless you say otherwise: a terminal
    /// gets the single document, a pipe or a file gets NDJSON. See
    /// `--json-lines` / `--no-json-lines`, and the rule spelled out below the
    /// examples.
    #[command(
        after_help = "Examples:\n  bee sessions status --channel <uuid>\n  bee sessions status --channel <uuid> --json-lines\n  bee sessions status --channel <uuid> --no-json-lines\n\nOutput shape, when neither flag is given and --format is not named:\nstdout decides. A terminal gets the single document (the --format json\nenvelope, or the --format compact array); a pipe or a file gets NDJSON --\none JSON object per execution, one per line. Naming --format explicitly\nalways gets that format's document, terminal or pipe. This is a different\nthing from `sessions transcript --format jsonl`, which is whole signed\nevents rather than these rows.\n\nThe context field: how full this seat's model context is, from the wire\nonly. Two sources, in order. (1) The driver's own context_window_updated\nitem (used/size) \u{2014} occupancy, measured by the driver against the prompt\nit was about to send, so it never exceeds the window. (2) Failing that, the\nturn's result usage block: inputTokens + cacheReadTokens + cacheWriteTokens,\nthe three disjoint prompt-side counts. That second number is the turn's\nconsumption across every model call the turn made, so on a multi-call turn\nit is larger than the context the model held. The cell reads '\u{2014}' (an em\ndash) when nothing on the wire has said, and --format json prints null\nthere; '<n> (window unknown)' means tokens are known and the window is\nnot \u{2014} never a percentage of a guess.\n\nRecipe:\n  bee sessions status --channel <uuid>"
    )]
    Status {
        /// Channel UUID to read
        #[arg(long)]
        channel: String,
        /// Print one JSON object per execution, one per line (NDJSON), with the
        /// same fields as a `--format json` row. The envelope keys (`channel`,
        /// `founders`, `leaseSnapshotRecords`) are not printed. Overrides
        /// `--format`: a compact request still gets the JSON row's fields.
        /// This is already what a pipe gets when neither flag is given and
        /// `--format` is not named, so the flag is only needed to force NDJSON
        /// onto a terminal, or alongside an explicit `--format`.
        #[arg(long = "json-lines")]
        json_lines: bool,
        /// Print the single document (the `--format json` envelope, or the
        /// `--format compact` array) even when stdout is not a terminal —
        /// the escape hatch from the automatic NDJSON above. Naming `--format`
        /// explicitly has the same effect. Conflicts with `--json-lines`.
        #[arg(long = "no-json-lines", conflicts_with = "json_lines")]
        no_json_lines: bool,
        /// Not a flag. The entry point sets this after parsing, because
        /// `--format` has a default and so `Cli.format` alone cannot say
        /// whether a format was named or fell through — and only a named one
        /// suppresses the automatic NDJSON. See `crew_cmds::resolve_json_lines`.
        #[arg(skip)]
        format_explicit: bool,
    },
    /// Print the live provider catalog (kind 44222) — every model on offer.
    ///
    /// This is the *only* list of models this product offers. A create, a
    /// hire, or a registry row naming an id that is not here is naming something
    /// nobody is serving: the answer is to say so and refuse, never to
    /// translate the id onto a neighbouring one that happens to be offered.
    ///
    /// One row per provider instance and model. `contextWindow`, `family`,
    /// `vendor` and `deprecated` are per-model metadata the publisher carries
    /// only where it knows the fact — `null` there means **nobody said**, not
    /// a default a caller may assume.
    ///
    /// Each provider host signs its own catalog with its own revision counter,
    /// so nothing is reconciled here: each signer's newest catalog (highest
    /// revision, ties broken on created_at then event id) contributes its rows,
    /// and `--format json` names the signer and revision on every row. A
    /// catalog whose body does not parse is listed under `malformed` rather
    /// than skipped, because a provider whose models cannot be read is a
    /// provider whose models are invisible.
    #[command(
        after_help = "Examples:\n  bee sessions catalog --channel <uuid>\n  bee --format compact sessions catalog --channel <uuid>\n\nRecipe:\n  bee sessions catalog --channel <uuid>"
    )]
    Catalog {
        /// Channel UUID the providers publish their catalogs into
        #[arg(long)]
        channel: String,
    },
    /// The model registry, checked against the live catalog.
    #[command(
        subcommand,
        after_help = "Recipe:\n  bee sessions registry check --channel <uuid>"
    )]
    Registry(RegistryCmd),
    /// Choose an execution target for a class at a risk tier, and say why.
    ///
    /// Brian's ruling of 2026-08-30: "The lead chooses the capability
    /// required. The router chooses the execution target." You name a CLASS
    /// and a RISK triple; this names the provider, the model and the effort.
    /// You never name a model — `--model` is not an option here.
    ///
    /// THE ORDER. Live catalog, then hard requirements (modality, tools,
    /// context window, known failure modes, per-target constraints), then the
    /// class gate (every numeric minimum), then the risk tier, then the
    /// effort, then — among what is left — the cheapest expected accepted
    /// completion. Cost and speed never compensate for a capability deficit,
    /// because they are consulted only after every gate has already passed.
    /// There is no weighted product anywhere in it.
    ///
    /// THE TIER IS DERIVED. Risk = impact x uncertainty x irreversibility,
    /// each 1-5, so 1-125: 1-8 FAST (effort low), 9-39 STANDARD (medium),
    /// 40-125 DEEP (high). `--tier` is refused with that explanation rather
    /// than accepted, because a tier a caller can set is a risk assessment
    /// nobody made. The router never purchases xhigh, max or ultra, and never
    /// escalates effort after a failure — a failed high goes to a different
    /// target or to a reviewer.
    ///
    /// REVIEW IS NOT DEEP. Independent review is required by the spec's own
    /// trigger list: risk >= 40, irreversibility >= 4, or any of the flags
    /// under `--review-flags`. A cheap fast job that touches an auth boundary
    /// needs a reviewer; an expensive deep job need not on that ground alone.
    ///
    /// Exit 0 with the decision, 1 for a malformed request or an unknown
    /// class, 4 when nothing clears the bar — and that refusal names the
    /// binding trait, the minimum it wanted, and the best score anything
    /// available actually has. It never falls back to the smartest model.
    #[command(
        after_help = "Examples:\n  bee sessions route --channel <uuid> --class builder --risk 3,3,2\n  bee sessions route --channel <uuid> --class runner --risk 1,1,1 --scope bounded\n  bee sessions route --channel <uuid> --class builder --risk 3,3,2 --challenger-sample\n  bee sessions route --channel <uuid> --class verifier --risk 3,3,2 --counterpart-provider codex-primary\n  bee --format compact sessions route --channel <uuid> --class architect --risk 5,4,4 --review-flags contractChange\n\n--format json prints the whole table: every candidate with the gate it\ncleared or the reason it did not, the cost/latency/retry numbers behind the\ncomparison, the formula itself, and the provenance of any fact that gated a\ncandidate. --format compact prints the routing record alone -- the same\nobject that rides on a hire.\n\nRecipe:\n  bee sessions route --channel <uuid> --class builder --risk 3,3,2"
    )]
    Route {
        /// Channel UUID the providers publish their catalogs into
        #[arg(long)]
        channel: String,
        /// Capability class: lead, architect, builder, runner, ui_designer,
        /// researcher, verifier, poker
        #[arg(long)]
        class: String,
        /// Risk as `impact,uncertainty,irreversibility`, each 1-5. The tier
        /// and the effort are derived from it. Required — but deliberately not
        /// enforced by the parser, so a caller reaching for `--tier` is told
        /// why the tier is derived rather than told a flag is missing.
        #[arg(long)]
        risk: Option<String>,
        /// Extra trait minimums as JSON, e.g. '{"taste":4.6}'. A profile may
        /// tighten a class gate; it can never loosen one.
        #[arg(long)]
        profile: Option<String>,
        /// Spec §6 review triggers, comma-separated: securityBoundary,
        /// contractChange, outsidePlan, builderUncertain, testsInsufficient,
        /// leadRequests
        #[arg(long = "review-flags")]
        review_flags: Option<String>,
        /// Deliberately route a challenger for this class and mark the record,
        /// so its result can be attributed later
        #[arg(long = "challenger-sample")]
        challenger_sample: bool,
        /// The task's scope, e.g. `bounded`. A target constrained to a scope
        /// is refused when nobody states one — unstated is not bounded.
        #[arg(long)]
        scope: Option<String>,
        /// Tokens of context this task needs. A target whose window nobody
        /// published cannot be shown to satisfy it, and is refused.
        #[arg(long = "context-need")]
        context_need: Option<u64>,
        /// For a cross-provider class (verifier): the provider the class it
        /// reviews is running on. Its provider's targets are removed when an
        /// eligible cross-provider target exists.
        #[arg(long = "counterpart-provider")]
        counterpart_provider: Option<String>,
        /// Path to the model registry; resolved from the working directory
        /// upwards when omitted
        #[arg(long)]
        registry: Option<String>,
        /// Refused: the tier is derived from --risk (see the error text)
        #[arg(long)]
        tier: Option<String>,
        /// Accepted and has no effect: `route` never writes anything. It
        /// exists so a caller that habitually passes it is not refused.
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Print this signer's identity as one JSON object: pubkey, relay
    /// display name, relay URL, and any active team seat role.
    #[command(
        after_help = "Examples:\n  bee sessions whoami\n  bee --format compact sessions whoami\n\nRecipe:\n  bee sessions whoami"
    )]
    Whoami,
    /// Define one word this session's fold prints — and say where it is shown.
    ///
    /// Every word a team fold puts in front of a seat — `unseated`, `dangling`,
    /// `waiting`, `superseded`, and every exclusion code — is carried in this
    /// binary as data, not in the source tree. With a word, this prints that
    /// one entry: what it means, what causes it, and the one command that shows
    /// it. With no word, it lists every word once. An unknown word exits 1
    /// naming the closest spelling.
    ///
    /// It reaches no relay and needs no key. On 2026-09-01 a lead spent its own
    /// context grepping this repository's Rust source for the string
    /// `"unseated"`, because that was the only place the word its own tool had
    /// just printed was defined. This command is the answer to that.
    #[command(
        after_help = "Examples:\n  bee sessions explain unseated\n  bee sessions explain dangling_reference\n  bee sessions explain           # every word, once\n\nRecipe:\n  bee sessions explain <word>"
    )]
    Explain {
        /// The word to define. Omit to list every word once.
        word: Option<String>,
    },
}

/// Common envelope and JSON-body input for one typed team transaction.
#[derive(clap::Args, Clone)]
pub struct TeamTransactionWriteArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id.
    #[arg(long)]
    pub genesis: String,
    /// Exact operation body as JSON, `@path`, or `-` for stdin.
    #[arg(long)]
    pub body: String,
    /// Same-author correction event id; never a causal workflow reference.
    #[arg(long)]
    pub supersedes: Option<String>,
    /// Existing 44220 command id used to correlate provider delivery.
    #[arg(long = "delivery-command-id")]
    pub delivery_command_id: Option<String>,
    /// Execution target or role to wake after the transaction is stored.
    #[arg(long = "wake-to")]
    pub wake_to: Option<String>,
}

/// Input for one signed `note` — the state-free verb.
///
/// There is deliberately no `--supersedes` and no `--wake-to`: a note can
/// never correct another record, and a record that changes no state has no
/// business spending a seat's turn.
#[derive(clap::Args, Clone)]
pub struct TeamNoteArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id.
    #[arg(long)]
    pub genesis: String,
    /// The complete note text (at most 8 KiB).
    #[arg(long)]
    pub text: String,
    /// Event id this note points at; repeatable, at most 16.
    #[arg(long = "ref")]
    pub refs: Vec<String>,
}

/// Signed decision verbs: ask one named party for a ruling, or give it.
#[derive(Subcommand)]
pub enum TeamDecisionCmd {
    /// Ask the founder or one actor for a ruling the mission needs.
    ///
    /// An unanswered request naming active assignments in `--blocks` puts the
    /// mission in a waiting-on-a-person state **without** publishing a
    /// terminal — the state `mission.blocked` was previously used to fake.
    #[command(
        after_help = "Examples:\n  bee sessions decide request --channel <uuid> --session-ref <uuid> --genesis <hex64> --question 'ship the CLI fix now or after the rebuild?' --option 'now' --option 'after' --held-on founder --blocks <assignment-id>"
    )]
    Request {
        /// Channel UUID containing the session.
        #[arg(long)]
        channel: String,
        /// Canonical umbrella session UUID.
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Session genesis event id.
        #[arg(long)]
        genesis: String,
        /// The exact question needing a ruling (at most 8 KiB).
        #[arg(long)]
        question: String,
        /// One closed option; repeatable, at most 8, each at most 512 bytes.
        #[arg(long = "option")]
        options: Vec<String>,
        /// Who holds this decision: `founder`, or an actor pubkey (64-hex).
        #[arg(long = "held-on")]
        held_on: String,
        /// Assignment event id this question blocks; repeatable, at most 16.
        #[arg(long = "blocks")]
        blocks: Vec<String>,
        /// Optional recommendation from the asker (at most 2 KiB).
        #[arg(long)]
        recommendation: Option<String>,
        /// Same-author correction event id for an earlier request.
        #[arg(long)]
        supersedes: Option<String>,
        /// Execution target or role to wake once the request is stored.
        ///
        /// The wake's 44220 command id is derived from the stored request and
        /// that exact target, so there is deliberately no
        /// `--delivery-command-id`: an inherited id is fenced as
        /// `AlreadyConsumed` and the wake never lands.
        #[arg(long = "wake-to")]
        wake_to: Option<String>,
    },
    /// Answer one open decision request.
    ///
    /// Only the party the request named — or the founder, always — can answer.
    /// An answer from anyone else is excluded `Unauthorized` by the fold.
    #[command(
        after_help = "Examples:\n  bee sessions decide answer --channel <uuid> --session-ref <uuid> --genesis <hex64> --request <request-id> --choice-index 0\n  bee sessions decide answer --channel <uuid> --session-ref <uuid> --genesis <hex64> --request <request-id> --choice 'neither; hold until the rebuild' --note 'the sidecar is stale'\n  bee sessions decide answer --channel <uuid> --session-ref <uuid> --genesis <hex64> --request <request-id> --choice-index 0 --condition 'any SHA whose buzz-acp diff against origin/main is empty'"
    )]
    Answer {
        /// Channel UUID containing the session.
        #[arg(long)]
        channel: String,
        /// Canonical umbrella session UUID.
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Session genesis event id.
        #[arg(long)]
        genesis: String,
        /// Event id of the `decision.request` being answered.
        #[arg(long)]
        request: String,
        /// Zero-based index of the chosen declared option.
        #[arg(long = "choice-index", conflicts_with = "choice")]
        choice_index: Option<u32>,
        /// Free-text answer when no declared option fits (at most 2 KiB).
        #[arg(long, required_unless_present = "choice_index")]
        choice: Option<String>,
        /// Optional bounded reasoning recorded with the answer.
        #[arg(long)]
        note: Option<String>,
        /// The class of case this ruling covers, at most 512 bytes.
        ///
        /// Text a person reads, never a predicate: nothing evaluates it and
        /// the fold neither reads nor enforces it. Use it when the honest
        /// answer would have to be given again for the next commit — a per-SHA
        /// ruling is a question you have agreed to ask again (live run 2,
        /// finding 21).
        #[arg(long)]
        condition: Option<String>,
        /// Same-author correction event id for an earlier answer.
        #[arg(long)]
        supersedes: Option<String>,
        /// Execution target or role to wake once the answer is stored.
        ///
        /// Defaults to the seat role of the actor that asked. An answer nobody
        /// is told about is an answer that never lands.
        #[arg(long = "wake-to")]
        wake_to: Option<String>,
    },
}

/// `bee sessions worktree` — the host's record of the trees it cut, and what
/// may be done with each one.
#[derive(Subcommand)]
pub enum SessionWorktreeCmd {
    /// Every recorded worktree for a session, and what may be done with it
    #[command(
        after_help = "Examples:\n  bee sessions worktree status --session <uuid>\n  bee --format compact sessions worktree status --all"
    )]
    Status {
        /// Canonical umbrella session UUID.
        #[arg(long)]
        session: Option<String>,
        /// Every session the host recorded a worktree for.
        #[arg(long, conflicts_with = "session")]
        all: bool,
        /// Host record path. Defaults to the desktop app's own file.
        #[arg(long)]
        store: Option<String>,
        /// Treat this session's executions as still running.
        ///
        /// The host stops a settled session's executions, so this is an
        /// operator's observation rather than something the record can prove.
        #[arg(long = "execution-live")]
        execution_live: bool,
    },
    /// Remove the worktrees that are prunable. Without --confirm, prints only
    #[command(
        after_help = "Examples:\n  bee sessions worktree prune --session <uuid>\n  bee sessions worktree prune --session <uuid> --seat builder-1 --confirm"
    )]
    Prune {
        /// Canonical umbrella session UUID.
        #[arg(long)]
        session: String,
        /// One seat's worktree rather than every seat's.
        #[arg(long)]
        seat: Option<String>,
        /// Actually remove. Without it nothing is removed.
        #[arg(long)]
        confirm: bool,
        /// Host record path. Defaults to the desktop app's own file.
        #[arg(long)]
        store: Option<String>,
        /// Treat this session's executions as still running.
        #[arg(long = "execution-live")]
        execution_live: bool,
    },
    /// Remove target/ and desktop/node_modules. Without --confirm, prints only
    #[command(
        after_help = "Examples:\n  bee sessions worktree reclaim --session <uuid>\n  bee sessions worktree reclaim --session <uuid> --confirm"
    )]
    Reclaim {
        /// Canonical umbrella session UUID.
        #[arg(long)]
        session: String,
        /// One seat's worktree rather than every seat's.
        #[arg(long)]
        seat: Option<String>,
        /// Actually remove the build directories.
        #[arg(long)]
        confirm: bool,
        /// Host record path. Defaults to the desktop app's own file.
        #[arg(long)]
        store: Option<String>,
        /// Treat this session's executions as still running.
        #[arg(long = "execution-live")]
        execution_live: bool,
    },
}

/// `bee sessions handover` — continue another participant's work.
#[derive(Subcommand)]
pub enum HandoverCmd {
    /// Publish what this work is, durably enough to be picked up (kind 44247).
    ///
    /// Reads the worktree, pushes HEAD to `refs/heads/wip/<role-or-owner>/<session8>`
    /// without forcing, captures the whole working tree as one patch against
    /// HEAD, and carries that patch as a NIP-34 patch event — or, above the
    /// relay's own message limit, as a Blossom blob. The capture is complete
    /// or enumerated: any file too large for the bounds is omitted **by path**
    /// and listed under `missing`, and `revision.preserved` says all, partial
    /// or none accordingly. A failed push is a `missing` line and no wip
    /// artifact.
    #[command(
        after_help = "Examples:\n  bee sessions handover checkpoint --channel <uuid> --session-ref <uuid> --task 'finish the fold' --next 'run just ci'\n  bee sessions handover checkpoint --channel <uuid> --session-ref <uuid> --cwd /path/to/worktree --json\n\nNote:\n  This publishes a record. It fences nothing and steers nothing — run `handover claim` for that."
    )]
    Checkpoint(HandoverCheckpointArgs),
    /// Take the whole session over: publish a `takeover` on its 44228 chain.
    ///
    /// Waits, bounded, for the relay's signed acceptance receipt. On a
    /// stale-head refusal it re-reads the chain **once**, names the claimant
    /// who won, and exits 5 — it never retries blindly into a race it lost.
    #[command(
        after_help = "Examples:\n  bee sessions handover claim --channel <uuid> --session-ref <uuid> --body <providerAuthorityPubkey>\n  bee sessions handover claim --channel <uuid> --session-ref <uuid> --body-self --json\n\nNote:\n  --body names the provider authority of the execution body the claim fences to, not a person."
    )]
    Claim(HandoverClaimArgs),
    /// Claim the session and pick the work up, natively or by reconstruction.
    #[command(
        after_help = "Examples:\n  bee sessions handover continue --channel <uuid> --session-ref <uuid> --cwd /path/to/checkout --provider-instance <alias> --body-self\n  bee sessions handover continue --channel <uuid> --session-ref <uuid> --native\n\nNote:\n  Reachability is a live kind-24223 lease on the candidate execution's current generation; nothing weaker counts."
    )]
    Continue(HandoverContinueArgs),
    /// Print the handover fold: claim, checkpoint, continuations, fences.
    #[command(
        after_help = "Examples:\n  bee sessions handover status --channel <uuid> --session-ref <uuid>\n  bee sessions handover status --channel <uuid> --session-ref <uuid> --json"
    )]
    Status(HandoverStatusArgs),
}

/// Arguments for `bee sessions handover checkpoint`.
#[derive(clap::Args, Clone)]
pub struct HandoverCheckpointArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// The accepted task, in your own words.
    #[arg(long)]
    pub task: Option<String>,
    /// The next useful action for whoever picks this up.
    #[arg(long = "next")]
    pub next: Option<String>,
    /// One open question the next participant inherits. Repeatable.
    #[arg(long)]
    pub unresolved: Vec<String>,
    /// A decision already taken, as `<eventId>` or `<eventId>:<summary>`.
    /// Repeatable. A bare id records that no summary was stated.
    #[arg(long)]
    pub decision: Vec<String>,
    /// A kind-44244 assignment id this work answers. Repeatable.
    #[arg(long)]
    pub assignment: Vec<String>,
    /// One test row as `name:outcome:command`, outcome in passed|failed|not-run.
    /// Repeatable; split on the first two colons so a command may contain colons.
    #[arg(long)]
    pub test: Vec<String>,
    /// Worktree to read. Defaults to the current directory.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Repository coordinate. Derived from the checkout's remote when omitted.
    #[arg(long)]
    pub repo: Option<String>,
    /// Repository owner pubkey for the NIP-34 patch coordinate. Defaults to
    /// this caller's own pubkey, which is printed so it is never a silent guess.
    #[arg(long = "repo-owner")]
    pub repo_owner: Option<String>,
    /// Seat role for the wip ref's first segment. Defaults to `owner`.
    #[arg(long)]
    pub role: Option<String>,
    /// Remote to push the wip ref to. Resolved from git's own push
    /// configuration when omitted; never a hard-coded name.
    #[arg(long)]
    pub remote: Option<String>,
    /// Do not push HEAD. The commits are then listed under `missing`.
    #[arg(long = "no-push", default_value_t = false)]
    pub no_push: bool,
    /// Print one JSON object instead of lines.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Arguments for `bee sessions handover claim`.
#[derive(clap::Args, Clone)]
pub struct HandoverClaimArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// Provider authority pubkey of the execution body this claim fences to.
    #[arg(long)]
    pub body: Option<String>,
    /// Use the provider authority this caller's own session creates have named
    /// in this channel. Refused when that is ambiguous.
    #[arg(long = "body-self", default_value_t = false, conflicts_with = "body")]
    pub body_self: bool,
    /// Seconds to wait for the relay's signed acceptance receipt.
    #[arg(long = "wait-secs", default_value_t = 30)]
    pub wait_secs: u64,
    /// Print one JSON object instead of lines.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Arguments for `bee sessions handover continue`.
#[derive(clap::Args, Clone)]
pub struct HandoverContinueArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// Checkout to reconstruct into. Without it nothing is fetched or applied,
    /// and every artifact is listed as not fetched.
    #[arg(long)]
    pub cwd: Option<PathBuf>,
    /// Provider authority pubkey of the execution body this claim fences to.
    #[arg(long)]
    pub body: Option<String>,
    /// Use the provider authority this caller's own session creates have named.
    #[arg(long = "body-self", default_value_t = false, conflicts_with = "body")]
    pub body_self: bool,
    /// Require a native resume on the original provider.
    #[arg(long, default_value_t = false)]
    pub native: bool,
    /// Require a reconstruction, skipping the reachability read.
    #[arg(long, default_value_t = false, conflicts_with = "native")]
    pub reconstruct: bool,
    /// Continue even though no authorized checkpoint exists, and label it so.
    #[arg(long = "allow-no-checkpoint", default_value_t = false)]
    pub allow_no_checkpoint: bool,
    /// Fetch a wip ref the relay's kind-30618 state does not name, and say so.
    #[arg(long = "allow-no-artifact", default_value_t = false)]
    pub allow_no_artifact: bool,
    /// Provider instance alias to reconstruct on. Required for a reconstruction.
    #[arg(long = "provider-instance")]
    pub provider_instance: Option<String>,
    /// Remote to fetch the wip ref from. Resolved from git's configuration
    /// when omitted; never a hard-coded name.
    #[arg(long)]
    pub remote: Option<String>,
    /// The provider's projects file, whose sibling `pending-hints/` directory
    /// receives the binding. Defaults to $BUZZ_CSP_PROJECTS_FILE. A create
    /// carries no directory field, so a reconstruction into --cwd writes a
    /// one-shot hint naming it; the projects file itself is generated and is
    /// never modified.
    #[arg(long = "projects-file")]
    pub projects_file: Option<PathBuf>,
    /// One sentence to put on the continuation record.
    #[arg(long)]
    pub note: Option<String>,
    /// Seconds to wait for each receipt.
    #[arg(long = "wait-secs", default_value_t = 60)]
    pub wait_secs: u64,
    /// Print one JSON object instead of lines.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// Arguments for `bee sessions handover status`.
#[derive(clap::Args, Clone)]
pub struct HandoverStatusArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// Print the HandoverFold plus the execution list as one JSON object.
    #[arg(long, default_value_t = false)]
    pub json: bool,
}

/// `bee sessions observe` — publish one NIP-CSOB observation (44246).
#[derive(Subcommand)]
pub enum SessionObserveCmd {
    /// Where you are in your own loop, and what you have written so far.
    Checkpoint(SessionObserveCheckpointArgs),
    /// What one or more named gates said, and what command said it.
    #[command(
        after_help = "Examples:\n  bee sessions observe gate --channel <uuid> --session-ref <uuid> --gate 'just ci:passed:just ci'\n  bee sessions observe gate --channel <uuid> --session-ref <uuid> --gate 'cargo clippy:failed:cargo clippy --all-targets -- -D warnings' --summary '3 warnings'"
    )]
    Gate(SessionObserveGateArgs),
    /// One finding and what you did about it.
    Finding(SessionObserveFindingArgs),
    /// One phase's own measured span. Every number here is your own claim.
    Phase(SessionObservePhaseArgs),
}

/// Coordinates every `observe` subcommand shares.
#[derive(clap::Args, Clone)]
pub struct SessionObserveCheckpointArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// The assignment this is about. A pointer, never a causal reference.
    #[arg(long)]
    pub assignment: Option<String>,
    /// planning | red | green | gates | reporting
    #[arg(long)]
    pub phase: String,
    /// Tests written so far in this lane.
    #[arg(long = "tests-written", default_value_t = 0)]
    pub tests_written: u32,
    /// Of those, how many you have observed failing first.
    #[arg(long = "tests-red", default_value_t = 0)]
    pub tests_red: u32,
    /// Of those, how many now pass.
    #[arg(long = "tests-green", default_value_t = 0)]
    pub tests_green: u32,
    /// The last command you ran.
    #[arg(long = "last-command")]
    pub last_command: Option<String>,
    /// Its summary line, on one line.
    #[arg(long = "last-summary")]
    pub last_summary: Option<String>,
    /// Free prose.
    #[arg(long)]
    pub note: Option<String>,
}

/// Flags for `bee sessions observe gate`.
#[derive(clap::Args, Clone)]
pub struct SessionObserveGateArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// The assignment this is about. A pointer, never a causal reference.
    #[arg(long)]
    pub assignment: Option<String>,
    /// One row, as `NAME:OUTCOME:COMMAND`. Repeatable, up to 32, unique by name.
    #[arg(long)]
    pub gate: Vec<String>,
    /// Summary line for the row at the same position. Repeatable.
    #[arg(long)]
    pub summary: Vec<String>,
    /// Your own measured duration for the row at the same position, in ms.
    #[arg(long = "duration-ms")]
    pub duration_ms: Vec<u64>,
    /// The commit the row at the same position ran against, as
    /// `SHA:clean` or `SHA:dirty`. Repeatable.
    ///
    /// Cleanliness is not optional here because a commit named without it is
    /// not evidence about that commit. A row published by this command is
    /// `declared` whatever it names, and a declared row never admits a push.
    #[arg(long = "head-sha")]
    pub head_sha: Vec<String>,
}

/// Flags for `bee sessions observe finding`.
#[derive(clap::Args, Clone)]
pub struct SessionObserveFindingArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// The assignment this is about. A pointer, never a causal reference.
    #[arg(long)]
    pub assignment: Option<String>,
    /// Your own stable id for this finding, unique within your own findings.
    #[arg(long = "finding-id")]
    pub finding_id: String,
    /// One line naming the finding.
    #[arg(long)]
    pub title: String,
    /// found | fixed | cross-lane | needs-ruling | wont-fix
    #[arg(long)]
    pub disposition: String,
    /// Free prose.
    #[arg(long)]
    pub detail: Option<String>,
    /// An event id a reader can follow. Repeatable, up to 16, unique.
    #[arg(long = "ref")]
    pub reference: Vec<String>,
    /// The `decision.request` this finding is waiting on.
    #[arg(long)]
    pub decision: Option<String>,
}

/// Flags for `bee sessions observe phase`.
#[derive(clap::Args, Clone)]
pub struct SessionObservePhaseArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id. Resolved from the relay when omitted.
    #[arg(long)]
    pub genesis: Option<String>,
    /// The assignment this is about. A pointer, never a causal reference.
    #[arg(long)]
    pub assignment: Option<String>,
    /// The phase's name, in your own words.
    #[arg(long)]
    pub phase: String,
    /// When you say it started, in ms since the Unix epoch. Your own claim.
    #[arg(long = "started-at-ms")]
    pub started_at_ms: u64,
    /// When you say it ended. Your own claim.
    #[arg(long = "ended-at-ms")]
    pub ended_at_ms: Option<u64>,
    /// Your own measured duration, in ms.
    #[arg(long = "duration-ms")]
    pub duration_ms: Option<u64>,
}

/// `bee sessions policy` — set, read, or withdraw a session policy (44245).
///
/// `Set` is much larger than `Get`/`Clear` because NIP-CSP has eighteen fields
/// and each is a flag. Boxing it is not available here — `clap`'s `Subcommand`
/// derive flattens a variant's single `Args` field and cannot see through a
/// `Box` — and a parsed command is constructed exactly once per process.
#[allow(clippy::large_enum_variant)]
#[derive(Subcommand)]
pub enum SessionPolicyCmd {
    /// Publish a session policy for this umbrella.
    ///
    /// Every flag is optional and every unset field is **omitted** from the
    /// record, never written as an explicit `null`. A `set` that passes no
    /// policy flag at all is refused: the record it would publish is the
    /// withdrawal, and withdrawing a policy is a decision with its own verb
    /// (`policy clear`).
    #[command(
        after_help = "Examples:\n  bee sessions policy set --channel <uuid> --session-ref <uuid> --genesis <hex64> --posture overnight --budget-turns 240 --red-first true --required-gate 'just ci'\n  bee sessions policy set --channel <uuid> --session-ref <uuid> --genesis <hex64> --irreversible push --irreversible deploy --time-box-secs 28800"
    )]
    Set(SessionPolicySetArgs),
    /// Print the newest accepted policy for this umbrella, or `null`.
    ///
    /// `null` means nobody set one. A record that sets nothing is a
    /// **withdrawal** somebody published, and prints as a real record with
    /// `setsAnyPolicy: false` — those are different facts.
    Get {
        /// Channel UUID containing the session.
        #[arg(long)]
        channel: String,
        /// Canonical umbrella session UUID.
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Session genesis event id.
        #[arg(long)]
        genesis: String,
    },
    /// Publish the empty withdrawal record, taking any policy back.
    Clear {
        /// Channel UUID containing the session.
        #[arg(long)]
        channel: String,
        /// Canonical umbrella session UUID.
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Session genesis event id.
        #[arg(long)]
        genesis: String,
    },
}

/// Flags for `bee sessions policy set`.
#[derive(clap::Args, Clone)]
pub struct SessionPolicySetArgs {
    /// Channel UUID containing the session.
    #[arg(long)]
    pub channel: String,
    /// Canonical umbrella session UUID.
    #[arg(long = "session-ref")]
    pub session_ref: String,
    /// Session genesis event id.
    #[arg(long)]
    pub genesis: String,
    /// How this mission is being run: spike, ship, investigate, overnight.
    #[arg(long)]
    pub posture: Option<String>,
    /// Ceiling on turns across the umbrella. **Enforced**, at the provider's
    /// turn gate.
    #[arg(long = "budget-turns")]
    pub budget_turns: Option<u32>,
    /// Ceiling on tokens any one seat may spend (read and shown only).
    #[arg(long = "tokens-per-seat")]
    pub tokens_per_seat: Option<u64>,
    /// Ceiling on tokens the umbrella may spend (read and shown only).
    #[arg(long = "tokens-per-session")]
    pub tokens_per_session: Option<u64>,
    /// Ceiling on dollars the umbrella may spend (read and shown only).
    #[arg(long = "cost-usd")]
    pub cost_usd: Option<f64>,
    /// Which context window seats run in: standard or long.
    #[arg(long = "context-tier")]
    pub context_tier: Option<String>,
    /// What a person is shown: decisions, decisions-and-milestones, everything.
    #[arg(long)]
    pub attention: Option<String>,
    /// Whether acceptance tests are written failing first.
    #[arg(long = "red-first")]
    pub red_first: Option<bool>,
    /// Whether every lane is reviewed by somebody who did not write it.
    #[arg(long = "review-every-lane")]
    pub review_every_lane: Option<bool>,
    /// A gate every lane must run; repeatable, at most 32, unique.
    #[arg(long = "required-gate")]
    pub required_gate: Vec<String>,
    /// Whether a verifier must rule before the mission may settle.
    ///
    /// **Enforced**, at the 44244 fold's completion check: with this set, a
    /// `mission.completed` whose settled assignments carry no active
    /// verifier's ruling is excluded `CompletionNotVerified`, and
    /// `bee sessions complete` refuses to sign one.
    #[arg(long = "verifier-required")]
    pub verifier_required: Option<bool>,
    /// Pubkey eligible for the bench (64-hex); repeatable, at most 64.
    #[arg(long = "bench-identity")]
    pub bench_identity: Vec<String>,
    /// Provider **alias** eligible for the bench; repeatable, at most 16.
    ///
    /// An alias (`claude-primary`), never an instance id
    /// (`1958c6c448e05eed`): they are different names for different things.
    #[arg(long = "bench-provider")]
    pub bench_provider: Vec<String>,
    /// Fraction of eligible jobs given to a challenger, 0.0..=1.0.
    #[arg(long = "challenger-sample-rate")]
    pub challenger_sample_rate: Option<f64>,
    /// An act that needs the founder's word: push, deploy, delete,
    /// external-message. Repeatable.
    #[arg(long)]
    pub irreversible: Vec<String>,
    /// Wall-clock seconds after which the lead stops opening work.
    #[arg(long = "time-box-secs")]
    pub time_box_secs: Option<u64>,
    /// The milestone whose arrival ends the mission (at most 8 KiB).
    #[arg(long = "on-milestone")]
    pub on_milestone: Option<String>,
}

/// Signed team-operation read commands.
#[derive(Subcommand)]
pub enum TeamOperationCmd {
    /// Fetch one signed operation plus its canonical fold status. The signed
    /// event supplies channel/session/genesis when all three scope flags are
    /// omitted, which is the form used for a kind-44220 operation wake.
    Get {
        /// Channel UUID containing the session. Optional only when all three
        /// scope flags are omitted; then scope is verified from the signed event.
        #[arg(long, requires_all = ["session_ref", "genesis"])]
        channel: Option<String>,
        /// Canonical umbrella session UUID. Optional with --channel/--genesis.
        #[arg(long = "session-ref", requires_all = ["channel", "genesis"])]
        session_ref: Option<String>,
        /// Session genesis event id. Optional with --channel/--session-ref.
        #[arg(long, requires_all = ["channel", "session_ref"])]
        genesis: Option<String>,
        /// Exact operation event id.
        #[arg(long)]
        id: String,
    },
    /// List signed operations with exclusions, conflicts, and terminal state.
    List {
        /// Channel UUID containing the session.
        #[arg(long)]
        channel: String,
        /// Canonical umbrella session UUID.
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Session genesis event id.
        #[arg(long)]
        genesis: String,
    },
}

/// `bee sessions registry` — keep the written registry honest about the offer.
#[derive(Subcommand)]
pub enum RegistryCmd {
    /// Compare the model registry to the live kind:44222 catalog.
    ///
    /// A registry is a good instrument and a stale one is a quiet lie. This
    /// command never edits the registry and never translates an id — it prints
    /// three lists:
    ///
    ///   `stale: <ids>`    — execution targets the catalog offers today that
    ///   no registry row covers. THIS is staleness, and it is the only list
    ///   that fails: exit 4.
    ///
    ///   `dormant: <ids>`  — registry rows the catalog does not offer today.
    ///   LEGAL, and reported rather than counted: the registry is allowed to
    ///   hold an opinion about a model this host is not serving right now.
    ///
    ///   `variants: <ids>` — offered ids a row covers by the base rule without
    ///   naming literally. Informational.
    ///
    /// A bracket suffix is a variant of its base: `gpt-5.6-sol[high]` and
    /// `gpt-5.6-sol[max]` are one model at two effort levels, and a row naming
    /// `gpt-5.6-sol` has decided about both. `default` is a provider's pointer
    /// at whatever the host is set to, never a row and never a gap.
    ///
    /// `--format json` carries all three lists, the registry's version and
    /// date, the offered pairs, and the catalog revision (`null` when more
    /// than one signer published, because two hosts share no revision counter).
    #[command(
        after_help = "Examples:\n  bee sessions registry check --channel <uuid>\n  bee --format compact sessions registry check --channel <uuid> --registry ./team/model-registry.yaml\n\nWith no --registry, the nearest ancestor of the working directory holding\nteam/model-registry.yaml is used, and a failure to find one names every\ndirectory that was tried."
    )]
    Check {
        /// Channel UUID the providers publish their catalogs into
        #[arg(long)]
        channel: String,
        /// Path to the model registry; resolved from the working directory
        /// upwards when omitted
        #[arg(long)]
        registry: Option<String>,
    },
    /// Run a role's bench and leave a signed row for every task.
    ///
    /// Every score in `team/model-registry.yaml` today is an opinion: ten 1-5
    /// priors under `rating: { status: operational_opinion, confidence: low }`.
    /// Live run 3 is what that cost — the router disclosed that an incumbent
    /// "cleared the verifier gates (reasoning>=4.5, judgment>=4.5,
    /// verification>=4.7) ... nothing else cleared them" while a benched model
    /// sat there uncompared, and every number in that sentence was a guess.
    ///
    /// This runs `team/registry-bench/<role>` through the real runtime and
    /// publishes, per task per run: one 44246 gate row carrying the argv it
    /// spawned, one finding per failed criterion, and one checkpoint carrying
    /// the bench manifest. Scoring is mechanical — a closed set of checks read
    /// off the run's artifacts, no model judging anything — which is the only
    /// reason two runs of it agree.
    ///
    /// It writes NOTHING to the registry. `propose` does that, from the rows
    /// this leaves on the relay.
    #[command(
        after_help = "Examples:\n  bee sessions registry measure --role verifier --runtime claude-primary --model 'opus[1m]' --channel <uuid> --session-ref <uuid> --repeat 3\n  bee sessions registry measure --role verifier --runtime claude-primary --model 'opus[1m]' --channel <uuid> --session-ref <uuid> --dry-run\n\n--dry-run resolves and prints the real spawn argv, runs the bench, scores it,\nand publishes nothing."
    )]
    Measure {
        /// Registry class to measure, e.g. `verifier`
        #[arg(long)]
        role: String,
        /// `providerInstanceRef` from BUZZ_CSP_RUNTIMES
        #[arg(long)]
        runtime: String,
        /// Model id the runtime offers
        #[arg(long)]
        model: String,
        /// Channel UUID containing the coding session
        #[arg(long)]
        channel: String,
        /// Canonical umbrella session UUID the rows are published into
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Runs per task. Minimum and default 3 — fewer cannot produce a
        /// median that means anything.
        #[arg(long, default_value_t = 3)]
        repeat: u32,
        /// Path to the model registry; the bench is its sibling
        #[arg(long)]
        registry: Option<String>,
        /// Wall-clock budget per task, in seconds. Past it, the task fails
        /// every criterion.
        #[arg(long = "task-timeout", default_value_t = 900)]
        task_timeout: u64,
        /// Run and score, publish nothing.
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Turn the rows a measurement left on the relay into one registry row.
    ///
    /// Reads the 44246 rows BACK OFF THE RELAY — never the measuring run's own
    /// memory — and refuses, exit 4, naming which check failed: fewer than
    /// `--repeat` runs for any task, rows from more than one signing key, a
    /// benchHash differing from the tree's, any trait the role's gate reads
    /// that the bench does not evidence, or an unstable set (a spread of 1.0
    /// or more on any trait).
    ///
    /// Traits the bench did not evidence stay OPINIONS, and the disclosure
    /// says which are which per trait. A half-measured row reading as measured
    /// is the same lie as a badge with no event behind it.
    ///
    /// Brian's ruling of 2026-09-01: a seat may `measure`; only the founder's
    /// key may `propose`. One proposal, one signer.
    ///
    /// The founder is the author of the session's own genesis event — a fact on
    /// the wire, not a flag — and it is checked against the key THIS command is
    /// invoked with. The proposal is signed by that key (and published
    /// nowhere), so `proposedBy` is provable rather than asserted.
    #[command(
        after_help = "Examples:\n  bee sessions registry propose --role verifier --runtime claude-primary --model 'opus[1m]' --channel <uuid> --session-ref <uuid>\n  bee sessions registry propose ... --write\n\nWithout --write it prints a YAML fragment and changes nothing. Run it with the\nfounder's key: a seat may measure, the founder proposes."
    )]
    Propose {
        /// Registry class the rows measured
        #[arg(long)]
        role: String,
        /// `providerInstanceRef` of the row to amend
        #[arg(long)]
        runtime: String,
        /// Model id of the row to amend
        #[arg(long)]
        model: String,
        /// Channel UUID holding the measuring session
        #[arg(long)]
        channel: String,
        /// Umbrella session UUID the rows were published into
        #[arg(long = "session-ref")]
        session_ref: String,
        /// Runs the proposal requires per task
        #[arg(long, default_value_t = 3)]
        repeat: u32,
        /// Path to the model registry
        #[arg(long)]
        registry: Option<String>,
        /// Apply the fragment in place, refusing if the row already carries a
        /// measurement.
        #[arg(long)]
        write: bool,
    },
}

/// Delivery class for `bee sessions send`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, clap::ValueEnum)]
pub enum DeliveryArg {
    /// Hold the turn and start it when the current one settles (default).
    Boundary,
    /// Inject mid-turn where the runtime advertises native steering.
    Steer,
    /// Cancel the running turn, then deliver. Founder-only.
    Interrupt,
}

/// Shared-terminal commands (NIP-ST kind 30623 announces + kind 24312 input).
#[derive(Subcommand)]
pub enum TerminalsCmd {
    /// List shared-terminal session announces (kind 30623)
    List {
        /// Restrict to one project: a `30621:<owner>:<dtag>` coordinate, or
        /// a bare slug (expanded with your own pubkey as owner).
        #[arg(long)]
        project: Option<String>,
    },
    /// Add a pubkey to your own session's roster, or change their role.
    ///
    /// Read-modify-writes your own kind:30623 announce; errors if you have
    /// no announce for the session id. The owner host independently
    /// re-verifies roster membership before input reaches the PTY.
    Invite {
        /// Session id (`d` tag of your announce)
        session_id: String,
        /// Invitee pubkey (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
        /// Roster role: collaborator (watch + type) or viewer (watch only)
        #[arg(long, value_enum)]
        role: ShellRoleArg,
    },
    /// Remove a pubkey from your own session's roster
    Revoke {
        /// Session id (`d` tag of your announce)
        session_id: String,
        /// Pubkey to remove (64-char lowercase hex)
        #[arg(long)]
        pubkey: String,
    },
    /// Delete a shared-terminal announce (kind:5 tombstone of its 30623).
    ///
    /// The announce is what makes a terminal discoverable, watchable and
    /// typeable; deleting it drops the roster projection with it, so the
    /// watch and input gates fall back to project access alone. It does
    /// not reach into the owner's machine — a PTY that is still running
    /// keeps running, unattached.
    ///
    /// Signed by the announce's owner, or by an Owner of the project the
    /// announce is bound to.
    #[command(
        after_help = "Examples:\n  bee terminals delete <session-id>\n  bee terminals delete <session-id> --owner <hex>"
    )]
    Delete {
        /// Session id (`d` tag of the announce)
        session_id: String,
        /// Announce owner pubkey (64-char hex). Defaults to the current
        /// identity; pass it to delete a terminal you did not announce but
        /// whose project you own.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Print a session announce's roster as `[{pubkey, role}]`
    Roster {
        /// Session id (`d` tag of the announce)
        session_id: String,
        /// Session owner pubkey (64-char hex). Defaults to the current identity.
        #[arg(long)]
        owner: Option<String>,
    },
    /// Send raw input bytes to a shared terminal (kind 24312, ephemeral).
    ///
    /// Accepted by the relay only from the session owner or a roster
    /// collaborator; delivered only to the owner. Content is chunked when
    /// it exceeds 6 KiB raw.
    #[command(name = "send-input")]
    SendInput {
        /// Session id (`d` tag of the owner's announce)
        session_id: String,
        /// Session owner pubkey (64-char hex)
        #[arg(long)]
        owner: String,
        /// Input text to send. Use --stdin to send raw stdin bytes instead.
        #[arg(long, conflicts_with = "stdin", required_unless_present = "stdin")]
        text: Option<String>,
        /// Read the input bytes from stdin
        #[arg(long, default_value_t = false)]
        stdin: bool,
    },
}

/// The claim a Pulse entry makes — the `pu-type` tag and the content `type`,
/// which are always the same value.
#[derive(Clone, Copy, Debug, clap::ValueEnum)]
pub enum PulseKindArg {
    /// What the author intends to do next.
    Plan,
    /// A completed step worth recording.
    Milestone,
    /// Context that is neither a plan nor a blocker.
    Note,
    /// Work being passed to somebody else.
    Handoff,
    /// Something another worker should not walk into.
    Blocker,
}

impl PulseKindArg {
    /// The wire value this variant carries in the `pu-type` tag.
    pub fn as_str(self) -> &'static str {
        match self {
            PulseKindArg::Plan => "plan",
            PulseKindArg::Milestone => "milestone",
            PulseKindArg::Note => "note",
            PulseKindArg::Handoff => "handoff",
            PulseKindArg::Blocker => "blocker",
        }
    }
}

/// `bee pulse` — per-project coordination.
///
/// Every subcommand takes `--project`, which accepts a full
/// `30621:<owner-hex>:<dtag>` coordinate or a bare dtag that resolves only
/// when exactly one *visible* project matches. `BUZZ_PULSE_PROJECT` supplies
/// the same value when the flag is absent; the ACP harness sets it per
/// channel session.
#[derive(Subcommand)]
pub enum PulseCmd {
    /// Publish one Pulse entry (kind 44240)
    Update {
        /// Project coordinate `30621:<owner-hex>:<dtag>`, or a bare dtag
        #[arg(long, env = "BUZZ_PULSE_PROJECT")]
        project: Option<String>,
        /// What the entry claims
        #[arg(long, value_enum)]
        kind: PulseKindArg,
        /// Repository-relative paths you claim to be working in, comma-separated
        #[arg(long)]
        areas: Option<String>,
        /// Branch the claim applies to
        #[arg(long)]
        branch: Option<String>,
        /// Coding-session `sessionRef` UUID this entry belongs to (not a genesis event id)
        #[arg(long)]
        session: Option<String>,
        /// Event id of your own earlier entry this one revises
        #[arg(long)]
        supersedes: Option<String>,
        /// Attach what the work cost, folded from signed turn usage in
        /// `<channel-uuid>` (optionally narrowed to `:<session-ref-uuid>`)
        #[arg(long)]
        cost_from: Option<String>,
        /// Restrict --cost-from to one seat: 4-64 lowercase hex characters of its pubkey
        #[arg(long)]
        cost_seat: Option<String>,
        /// Entry text, taken verbatim; use '-' to read stdin to EOF
        #[arg(long)]
        content: String,
    },
    /// List a project's Pulse entries, unfolded and newest first
    List {
        /// Project coordinate `30621:<owner-hex>:<dtag>`, or a bare dtag
        #[arg(long, env = "BUZZ_PULSE_PROJECT")]
        project: Option<String>,
        /// Only entries created at or after this Unix timestamp
        #[arg(long)]
        since: Option<u64>,
        /// Only entries of this type
        #[arg(long, value_enum)]
        kind: Option<PulseKindArg>,
        /// Only entries on this branch; the reserved value '-' selects entries with no branch
        #[arg(long)]
        branch: Option<String>,
        /// Maximum entries to return
        #[arg(long)]
        limit: Option<u32>,
    },
    /// List the coding sessions observed in the project's channels
    Sessions {
        /// Project coordinate `30621:<owner-hex>:<dtag>`, or a bare dtag
        #[arg(long, env = "BUZZ_PULSE_PROJECT")]
        project: Option<String>,
    },
    /// Print the project's Pulse digest — entries and sessions, folded
    Digest {
        /// Project coordinate `30621:<owner-hex>:<dtag>`, or a bare dtag
        #[arg(long, env = "BUZZ_PULSE_PROJECT")]
        project: Option<String>,
        /// Only rows on this branch; the reserved value '-' selects rows with no branch
        #[arg(long)]
        branch: Option<String>,
        /// Maximum entries to fold; a truncated read is reported as incomplete
        #[arg(long)]
        limit: Option<u32>,
    },
    /// Print one umbrella's mission row — the same sentences Desktop renders
    ///
    /// Composed entirely from mechanism: the signed 44244/44245 records, the
    /// hooks' 44246 observations, and the relay's own 30618 ref state. Nobody
    /// is asked to report.
    Missions {
        /// Channel UUID the umbrella's records live in
        #[arg(long)]
        channel: String,
        /// Umbrella `sessionRef` UUID
        #[arg(long)]
        session_ref: String,
        /// Immutable session-genesis event id
        #[arg(long)]
        genesis: String,
        /// Repository id (the kind:30617 `d` tag) whose ref state to read
        #[arg(long)]
        repo: Option<String>,
    },
    /// Plan the prune of shared wip refs; prints what it would delete
    ///
    /// Deletes nothing outside `refs/heads/wip/`, and keeps any ref whose
    /// state carries no readable date — unknown is not old.
    PruneWip {
        /// Repository id (the kind:30617 `d` tag) whose ref state to read
        #[arg(long)]
        repo: String,
        /// Treat these commits as merged, comma-separated
        #[arg(long)]
        merged: Option<String>,
    },
}

/// Normalize hand-authored `BUZZ_AUTH_TAG` input to strict JSON.
///
/// `.env` files and shell exports sometimes carry the tag in the unquoted
/// shorthand `[auth,<hex>,<conditions>,<hex>]` (quotes dropped by hand).
/// When the input is not valid JSON but is bracket-delimited, rewrite it as
/// a JSON array of the comma-separated fields (an empty field `,,` becomes
/// `""`, matching the canonical form `["auth","hex","","hex"]`).
///
/// This is presentation-layer leniency at the configuration edge only: the
/// output is always fed through the SDK's strict `parse_auth_tag` /
/// `verify_auth_tag`, which enforce structure, hex, the conditions grammar,
/// and the BIP-340 signature. Inputs that are already valid JSON — or not
/// recognizable as the shorthand — are returned unchanged so the strict
/// parser reports the error on the original bytes.
fn normalize_auth_tag_input(input: &str) -> String {
    let trimmed = input.trim();
    if serde_json::from_str::<serde_json::Value>(trimmed).is_ok() {
        return trimmed.to_owned();
    }
    if trimmed.starts_with('[') && trimmed.ends_with(']') {
        let fields: Vec<&str> = trimmed[1..trimmed.len() - 1]
            .split(',')
            .map(str::trim)
            .collect();
        // Only a plausible 4-field auth tag is rewritten; anything else is
        // passed through untouched for the strict parser to reject with an
        // error that references the caller's original input.
        if fields.len() == 4 && !fields.iter().any(|f| f.contains('"')) {
            // serde_json cannot fail serializing a Vec<&str>.
            return serde_json::to_string(&fields).expect("string array serializes");
        }
    }
    trimmed.to_owned()
}

async fn run(cli: Cli) -> Result<(), CliError> {
    let relay_url = client::normalize_relay_url(&cli.relay);

    // Pack commands are local-only — no relay connection needed.
    if let Cmd::Pack(ref sub) = cli.command {
        return match sub {
            PackCmd::Validate { path } => commands::pack::cmd_validate(path),
            PackCmd::Inspect { path } => commands::pack::cmd_inspect(path),
        };
    }

    // Git setup is local-only — it writes git config and, on request, a key
    // file. It deliberately runs before the key check below: `bee git setup`
    // without --write-key needs no identity at all, and `bee git status` must
    // stay usable on exactly the machine where nothing is configured yet.
    if let Cmd::Git(ref sub) = cli.command {
        let keys = cli
            .private_key
            .as_ref()
            .map(|k| Keys::parse(k))
            .transpose()
            .map_err(|e| CliError::Key(format!("invalid BUZZ_PRIVATE_KEY: {e}")))?;
        return match sub {
            GitCmd::Setup {
                helper,
                keyfile,
                scope,
                write_key,
                print,
            } => commands::git_setup::cmd_setup(commands::git_setup::SetupRequest {
                relay_url: &relay_url,
                helper: helper.clone(),
                keyfile: keyfile.clone(),
                scope: *scope,
                write_key: *write_key,
                print_only: *print,
                keys,
            }),
            GitCmd::Status { keyfile } => {
                commands::git_setup::cmd_status(&relay_url, keyfile.clone())
            }
            GitCmd::Check {
                keyfile,
                push,
                ref_name,
                sha,
            } => {
                commands::git_setup::cmd_check(
                    &relay_url,
                    keyfile.clone(),
                    *push,
                    ref_name.clone(),
                    sha.clone(),
                    matches!(cli.format, OutputFormat::Compact),
                )
                .await
            }
        };
    }

    // Session commands are local-only — they call the desktop session broker,
    // not the relay. No key/relay is required; when BUZZ_PRIVATE_KEY is present
    // the caller pubkey is passed to the broker for its audit log.
    if let Cmd::Session(ref sub) = cli.command {
        let caller = cli
            .private_key
            .as_ref()
            .and_then(|k| Keys::parse(k).ok())
            .map(|keys| keys.public_key().to_hex());
        return commands::session::dispatch(sub, caller).await;
    }

    // `sessions registry measure --dry-run` scores a bench and publishes
    // nothing, so it must not demand an identity (F16). It is dispatched here,
    // ahead of the key gate, with no client at all — which is also how the
    // command proves it signs nothing on this path: there is nothing to sign
    // with.
    if let Cmd::Sessions(SessionsCmd::Registry(RegistryCmd::Measure {
        ref role,
        ref runtime,
        ref model,
        ref channel,
        ref session_ref,
        repeat,
        ref registry,
        task_timeout,
        dry_run: true,
    })) = cli.command
    {
        return commands::sessions::registry_measure::cmd_registry_measure(
            None,
            role,
            runtime,
            model,
            channel,
            session_ref,
            repeat,
            registry.as_deref(),
            task_timeout,
            true,
            &cli.format,
        )
        .await;
    }

    // `sessions explain` answers from a data file compiled into this binary.
    // It is handled here, before the key requirement below, on purpose: a seat
    // that has to ask what a word its own tool printed means should not have to
    // be authenticated, on a network, or in a checkout to find out.
    if let Cmd::Sessions(SessionsCmd::Explain { ref word }) = cli.command {
        return commands::sessions::explain::cmd_explain(word.as_deref(), &cli.format);
    }

    // Auth: private key is required for all relay operations.
    // The keypair IS the identity — no tokens, no other auth.
    let private_key_str = cli.private_key.ok_or_else(|| {
        CliError::Auth("BUZZ_PRIVATE_KEY is required (use --private-key or set env var)".into())
    })?;
    let keys = Keys::parse(&private_key_str)
        .map_err(|e| CliError::Key(format!("invalid BUZZ_PRIVATE_KEY: {e}")))?;

    // NIP-OA: parse and verify the auth tag if provided.
    //
    // `BUZZ_AUTH_TAG` is hand-authored configuration, so the unquoted raw
    // shorthand `[auth,hex,,hex]` is normalized to JSON here — at this input
    // edge only. The SDK grammar and the `x-auth-tag` wire format stay strict
    // JSON; all validation and signature verification happen on the strict
    // path below, unchanged.
    let (auth_tag, auth_tag_json) = match cli.auth_tag {
        Some(ref input) if !input.is_empty() => {
            let json = normalize_auth_tag_input(input);
            let tag = buzz_sdk::nip_oa::parse_auth_tag(&json)
                .map_err(|e| CliError::Auth(format!("BUZZ_AUTH_TAG is malformed: {e}")))?;
            buzz_sdk::nip_oa::verify_auth_tag(&json, &keys.public_key()).map_err(|e| {
                CliError::Auth(format!(
                    "BUZZ_AUTH_TAG verification failed for pubkey {}: {e}",
                    keys.public_key().to_hex()
                ))
            })?;
            // Canonical wire form derives from the parsed-and-verified tag
            // (same shape as buzz-acp's RestClient), never from raw input.
            let canonical = serde_json::to_string(tag.as_slice())
                .map_err(|e| CliError::Auth(format!("BUZZ_AUTH_TAG serialization failed: {e}")))?;
            (Some(tag), Some(canonical))
        }
        _ => (None, None),
    };

    let client = BuzzClient::new(relay_url, keys, auth_tag, auth_tag_json)?;

    match cli.command {
        Cmd::Agents(sub) => commands::agents::dispatch(sub, &client).await,
        Cmd::Ci(sub) => commands::ci::dispatch(sub, &client, &cli.format).await,
        Cmd::Messages(sub) => commands::messages::dispatch(sub, &client, &cli.format).await,
        Cmd::Channels(sub) => commands::channels::dispatch(sub, &client, &cli.format).await,
        Cmd::Canvas(sub) => commands::channels::dispatch_canvas(sub, &client).await,
        Cmd::Reactions(sub) => commands::reactions::dispatch(sub, &client).await,
        Cmd::Emoji(sub) => commands::emoji::dispatch(sub, &client).await,
        Cmd::Dms(sub) => commands::dms::dispatch(sub, &client).await,
        Cmd::Users(sub) => commands::users::dispatch(sub, &client, &cli.format).await,
        Cmd::Workflows(sub) => commands::workflows::dispatch(sub, &client).await,
        Cmd::Feed(sub) => commands::feed::dispatch(sub, &client, &cli.format).await,
        Cmd::Social(sub) => commands::social::dispatch(sub, &client).await,
        Cmd::Notes(sub) => commands::notes::dispatch(sub, &client).await,
        Cmd::Repos(sub) => commands::repos::dispatch(sub, &client).await,
        Cmd::Projects(sub) => commands::projects::dispatch(sub, &client).await,
        Cmd::Patches(sub) => commands::patches::dispatch(sub, &client).await,
        Cmd::Issues(sub) => commands::issues::dispatch(sub, &client).await,
        Cmd::Pr(sub) => commands::pr::dispatch(sub, &client).await,
        Cmd::Media(sub) => commands::upload::dispatch_media(sub, &client).await,
        Cmd::Upload(sub) => commands::upload::dispatch(sub, &client).await,
        Cmd::Mem(sub) => commands::mem::dispatch(sub, &client).await,
        Cmd::Moderation(sub) => commands::moderation::dispatch(sub, &client, &cli.format).await,
        Cmd::Sessions(sub) => commands::sessions::dispatch(sub, &client, &cli.format).await,
        Cmd::Terminals(sub) => commands::terminals::dispatch(sub, &client).await,
        Cmd::Pulse(sub) => commands::pulse::dispatch(sub, &client, &cli.format).await,
        Cmd::Events(sub) => commands::events::dispatch(sub, &client, &cli.format).await,
        Cmd::Packs(sub) => commands::packs_cli::dispatch(sub, &client).await,
        Cmd::Pack(_) => unreachable!("handled above"),
        Cmd::Git(_) => unreachable!("handled above"),
        Cmd::Session(_) => unreachable!("handled above"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    /// Raw shorthand `[auth,hex,,hex]` normalizes to strict JSON; the empty
    /// conditions field becomes `""`.
    #[test]
    fn normalize_auth_tag_raw_shorthand() {
        let owner = "a".repeat(64);
        let sig = "b".repeat(128);

        let raw = format!("[auth,{owner},,{sig}]");
        let json = normalize_auth_tag_input(&raw);
        let parsed: Vec<String> = serde_json::from_str(&json).expect("output must be JSON");
        assert_eq!(parsed, vec!["auth", &owner, "", &sig]);

        // With conditions and surrounding whitespace (shell/.env artifacts).
        let raw = format!("  [auth, {owner} , kind=9, {sig}]  \n");
        let json = normalize_auth_tag_input(&raw);
        let parsed: Vec<String> = serde_json::from_str(&json).expect("output must be JSON");
        assert_eq!(parsed, vec!["auth", &owner, "kind=9", &sig]);
    }

    /// Valid JSON input passes through byte-identical (modulo outer trim) —
    /// the normalizer must never rewrite well-formed input.
    #[test]
    fn normalize_auth_tag_json_passthrough() {
        let owner = "a".repeat(64);
        let sig = "b".repeat(128);
        let json_in = serde_json::json!(["auth", owner, "kind=9", sig]).to_string();
        assert_eq!(normalize_auth_tag_input(&json_in), json_in);
    }

    /// Inputs that are neither JSON nor a plausible 4-field shorthand pass
    /// through unchanged, so the strict parser rejects the original bytes.
    #[test]
    fn normalize_auth_tag_leaves_garbage_untouched() {
        for garbage in [
            "not a tag",
            "[auth,too,few]",
            "[a,b,c,d,e]",
            r#"[auth,"quoted",x,y]"#, // quote chars => not the shorthand
            "[]",
            "{\"auth\":1}",
        ] {
            assert_eq!(normalize_auth_tag_input(garbage), garbage.trim());
        }
    }

    /// `bee sessions status --help` explains the `context` column.
    ///
    /// The column reports two different measurements depending on what the
    /// wire carried — the driver's own occupancy, or the turn's prompt-side
    /// consumption — and the second can exceed the window on a multi-call
    /// turn. A reader who is not told which one they are looking at will read
    /// the larger number as a full context (item 89).
    #[test]
    fn sessions_status_help_explains_the_context_field() {
        let cmd = Cli::command();
        let sessions = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "sessions")
            .expect("sessions command");
        let status = sessions
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "status")
            .expect("sessions status command");
        let help = status.clone().render_long_help().to_string();
        assert!(help.contains("context field"), "help:\n{help}");
    }

    /// Smoke test: CLI definition is valid and parseable.
    #[test]
    fn cli_definition_is_valid() {
        Cli::command().debug_assert();
    }

    /// The four `observe` verbs and the reader parse the forms NIP-CSOB
    /// documents, and every closed word is refused at the wire rather than by
    /// clap — so the CLI can never accept a token the relay will not store.
    #[test]
    fn the_observation_verbs_parse_the_forms_the_nip_documents() {
        let channel = "d3e440ea-89f8-4aee-8a02-17edc3e7272e";
        let session = "dc580cfb-6c80-4fc2-8f4e-dfc328acf222";
        for args in [
            vec![
                "bee",
                "sessions",
                "observe",
                "checkpoint",
                "--channel",
                channel,
                "--session-ref",
                session,
                "--phase",
                "red",
                "--tests-written",
                "4",
                "--tests-red",
                "4",
            ],
            vec![
                "bee",
                "sessions",
                "observe",
                "gate",
                "--channel",
                channel,
                "--session-ref",
                session,
                "--gate",
                "just ci:passed:just ci",
            ],
            vec![
                "bee",
                "sessions",
                "observe",
                "finding",
                "--channel",
                channel,
                "--session-ref",
                session,
                "--finding-id",
                "16",
                "--title",
                "the waiting state",
                "--disposition",
                "fixed",
            ],
            vec![
                "bee",
                "sessions",
                "observe",
                "phase",
                "--channel",
                channel,
                "--session-ref",
                session,
                "--phase",
                "red",
                "--started-at-ms",
                "1756800000000",
            ],
            vec![
                "bee",
                "sessions",
                "observations",
                "--channel",
                channel,
                "--session-ref",
                session,
            ],
        ] {
            assert!(Cli::try_parse_from(args.clone()).is_ok(), "{args:?}");
        }

        // `--gate` is repeatable, and so are its positional companions.
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "observe",
            "gate",
            "--channel",
            channel,
            "--session-ref",
            session,
            "--gate",
            "just ci:passed:just ci",
            "--gate",
            "cargo fmt:failed:cargo fmt --check",
            "--summary",
            "ok",
            "--summary",
            "1 file",
            "--duration-ms",
            "10",
            "--duration-ms",
            "20",
        ])
        .is_ok());

        // A phase timing without its own start is refused: the whole record is
        // a measurement, and a measurement with no beginning is not one.
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "observe",
            "phase",
            "--channel",
            channel,
            "--session-ref",
            session,
            "--phase",
            "red",
        ])
        .is_err());
    }

    #[test]
    fn operation_get_accepts_a_wake_pointer_or_one_complete_explicit_scope() {
        let id = "ab".repeat(32);
        assert!(Cli::try_parse_from(["bee", "sessions", "operation", "get", "--id", &id,]).is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "operation",
            "get",
            "--id",
            &id,
            "--channel",
            "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
            "--session-ref",
            "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
            "--genesis",
            &id,
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "operation",
            "get",
            "--id",
            &id,
            "--channel",
            "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86",
        ])
        .is_err());
    }

    /// `bee sessions note` and `bee sessions decide` — the two verbs the lead
    /// lacked on 2026-09-01, when four `mission.blocked` records were used to
    /// say things instead.
    #[test]
    fn note_and_decide_parse_their_exact_flags() {
        let id = "ab".repeat(32);
        let channel = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
        let session = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
        let scope = [
            "--channel",
            channel,
            "--session-ref",
            session,
            "--genesis",
            &id,
        ];

        let note = ["bee", "sessions", "note"].into_iter().chain(scope).chain([
            "--text",
            "nothing is blocked",
            "--ref",
            &id,
        ]);
        assert!(Cli::try_parse_from(note).is_ok());
        // A note can never correct another record, so the flag must not exist.
        let superseding_note = ["bee", "sessions", "note"].into_iter().chain(scope).chain([
            "--text",
            "correction",
            "--supersedes",
            &id,
        ]);
        assert!(Cli::try_parse_from(superseding_note).is_err());
        let textless = ["bee", "sessions", "note"].into_iter().chain(scope);
        assert!(Cli::try_parse_from(textless).is_err());

        let request = ["bee", "sessions", "decide", "request"]
            .into_iter()
            .chain(scope)
            .chain([
                "--question",
                "ship now or after the rebuild?",
                "--option",
                "now",
                "--option",
                "after",
                "--held-on",
                "founder",
                "--blocks",
                &id,
            ]);
        assert!(Cli::try_parse_from(request).is_ok());
        let unheld = ["bee", "sessions", "decide", "request"]
            .into_iter()
            .chain(scope)
            .chain(["--question", "ship now?"]);
        assert!(Cli::try_parse_from(unheld).is_err());

        for choice in [
            vec!["--choice-index", "0"],
            vec!["--choice", "neither; hold"],
        ] {
            let answer = ["bee", "sessions", "decide", "answer"]
                .into_iter()
                .chain(scope)
                .chain(["--request", &id])
                .chain(choice);
            assert!(Cli::try_parse_from(answer).is_ok());
        }
        // F6: the wake's command id is derived, so there is nothing to inherit.
        let inherited = ["bee", "sessions", "decide", "request"]
            .into_iter()
            .chain(scope)
            .chain([
                "--question",
                "ship?",
                "--held-on",
                "founder",
                "--delivery-command-id",
                "reused",
            ]);
        assert!(Cli::try_parse_from(inherited).is_err());
        // F4: an answer can wake the asker.
        let waking = ["bee", "sessions", "decide", "answer"]
            .into_iter()
            .chain(scope)
            .chain(["--request", &id, "--choice-index", "0", "--wake-to", "lead"]);
        assert!(Cli::try_parse_from(waking).is_ok());
        // Exactly one choice, never both and never neither.
        let both = ["bee", "sessions", "decide", "answer"]
            .into_iter()
            .chain(scope)
            .chain(["--request", &id, "--choice-index", "0", "--choice", "no"]);
        assert!(Cli::try_parse_from(both).is_err());
        let neither = ["bee", "sessions", "decide", "answer"]
            .into_iter()
            .chain(scope)
            .chain(["--request", &id]);
        assert!(Cli::try_parse_from(neither).is_err());
    }

    #[test]
    fn every_shipped_role_persona_can_consume_a_signed_operation_wake() {
        let roles = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../personas/roles");
        let mut checked = 0;
        for role in std::fs::read_dir(&roles).expect("read shipped role packs") {
            let persona_dir = role.expect("role directory").path().join("personas");
            for persona in std::fs::read_dir(persona_dir).into_iter().flatten() {
                let path = persona.expect("persona file").path();
                if path.extension().and_then(|extension| extension.to_str()) != Some("md") {
                    continue;
                }
                let body = std::fs::read_to_string(&path).expect("read role persona");
                let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
                assert!(
                    body.contains("$BEE sessions operation get --id <operationId>")
                        || body.contains("bee sessions operation get --id <operationId>"),
                    "{} does not teach the exact pointer fetch command",
                    path.display()
                );
                assert!(
                    body.contains("`operations[0].canonical` is `true`"),
                    "{} does not fail closed on a noncanonical operation",
                    path.display()
                );
                checked += 1;
            }
        }
        assert!(checked > 0, "no shipped role personas were checked");
    }

    #[test]
    fn set_status_clear_rejects_text_and_emoji() {
        for extra in [["--text", "busy"], ["--emoji", "🎶"]] {
            let args = ["buzz", "users", "set-status", "--clear"]
                .into_iter()
                .chain(extra);
            assert!(
                Cli::try_parse_from(args).is_err(),
                "--clear must conflict with {}",
                extra[0]
            );
        }
    }

    #[test]
    fn set_status_requires_text_or_clear() {
        assert!(Cli::try_parse_from(["buzz", "users", "set-status"]).is_err());
        assert!(
            Cli::try_parse_from(["buzz", "users", "set-status", "--emoji", "🎶"]).is_err(),
            "--emoji alone must not imply a status"
        );
        assert!(Cli::try_parse_from(["buzz", "users", "set-status", "--clear"]).is_ok());
    }

    /// A seat must be able to say which `bee` it ran. `bee --version` is that
    /// answer, and it names the commit, not just the crate version.
    #[test]
    fn version_names_the_build_commit() {
        let version = Cli::command()
            .get_version()
            .expect("bee must answer --version")
            .to_owned();
        assert_eq!(
            version,
            VERSION.as_str(),
            "--version must print the stamped build"
        );
        let first_line = version.lines().next().expect("a first line");
        assert!(
            first_line.starts_with(env!("CARGO_PKG_VERSION")),
            "got {version}"
        );
        let commit = first_line
            .rsplit_once(" (")
            .and_then(|(_, tail)| tail.strip_suffix(')'))
            .unwrap_or_else(|| panic!("no commit in {version}"));
        let (sha, dirty) = commit
            .strip_suffix("-dirty")
            .map_or((commit, false), |sha| (sha, true));
        assert!(
            commit == "unknown"
                || (sha.len() >= 7
                    && sha
                        .chars()
                        .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase())),
            "the commit is a short hex sha, optionally `-dirty`, or the literal `unknown`, never \
             invented: {commit:?}"
        );
        assert!(
            !(commit == "unknown" && dirty),
            "`unknown` names no commit, so there is nothing for `-dirty` to qualify: {commit:?}"
        );

        // The build time is a second line, never inside the parentheses:
        // `parse_bee_version` (buzz-session-provider) reads the first line and
        // refuses a stamp that is not a commit, so a time in there would cost
        // the beeStamp surface the sha it already has.
        let built = version
            .lines()
            .nth(1)
            .unwrap_or_else(|| panic!("no build time in {version}"));
        let built = built
            .strip_prefix("built ")
            .unwrap_or_else(|| panic!("the second line names the build time: {built:?}"));
        assert!(
            built == "unknown"
                || (built.len() == "2026-09-05T12:00:00Z".len() && built.ends_with('Z')),
            "an RFC 3339 UTC stamp, or the disclosed `unknown` — never a guess: {built:?}"
        );
        assert_eq!(version.lines().count(), 2, "exactly two lines: {version:?}");
    }

    /// The seat verbs must exist and must take the flags the seat-repair
    /// remedy tells an operator to type.
    #[test]
    fn the_seat_verbs_parse_the_forms_the_remedy_prints() {
        let genesis = "a".repeat(64);
        let pubkey = "b".repeat(64);
        let channel = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";

        // A role seat, on its own verb — writing seat authority is opted into,
        // never reached by mistyping a tier.
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "grant-seat",
            "--channel",
            channel,
            "--genesis",
            &genesis,
            "--pubkey",
            &pubkey,
            "--role",
            "builder",
        ])
        .is_ok());
        // `grant` keeps its closed tier set: a typo is a parse error naming
        // the tiers, not an accepted 44228 seating role `colaborator` that
        // then blocks every legitimate grant for that actor (REVIEW-A1 F5).
        let rendered = match Cli::try_parse_from([
            "bee",
            "sessions",
            "grant",
            "--channel",
            channel,
            "--genesis",
            &genesis,
            "--pubkey",
            &pubkey,
            "--role",
            "colaborator",
        ]) {
            Ok(_) => panic!("a mistyped tier must not parse"),
            Err(error) => error.to_string(),
        };
        assert!(
            rendered.contains("collaborator") && rendered.contains("viewer"),
            "the parse error names the tiers that exist: {rendered}"
        );
        assert!(
            Cli::try_parse_from([
                "bee",
                "sessions",
                "grant",
                "--channel",
                channel,
                "--genesis",
                &genesis,
                "--pubkey",
                &pubkey,
                "--role",
                "builder",
            ])
            .is_err(),
            "a role seat is `grant-seat`, not a fourth tier on `grant`"
        );
        // The two operator tiers keep working, unchanged.
        for tier in ["collaborator", "viewer"] {
            assert!(
                Cli::try_parse_from([
                    "bee",
                    "sessions",
                    "grant",
                    "--channel",
                    channel,
                    "--genesis",
                    &genesis,
                    "--pubkey",
                    &pubkey,
                    "--role",
                    tier,
                ])
                .is_ok(),
                "{tier} must still parse"
            );
        }
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "revoke-seat",
            "--channel",
            channel,
            "--genesis",
            &genesis,
            "--pubkey",
            &pubkey,
            "--role",
            "builder",
        ])
        .is_ok());
        // Every flag of revoke-seat but --session-ref is required.
        assert!(
            Cli::try_parse_from([
                "bee",
                "sessions",
                "revoke-seat",
                "--channel",
                channel,
                "--genesis",
                &genesis,
                "--pubkey",
                &pubkey,
            ])
            .is_err(),
            "revoke-seat must name the exact role it withdraws"
        );
    }

    /// `--check` is an acceptance-test flag; waiting for a host that will
    /// never be asked is a contradiction, so the two are mutually exclusive.
    #[test]
    fn hire_check_and_no_wait_are_mutually_exclusive() {
        let channel = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
        let session = "6a8f1b2c-0000-4000-8000-000000000001";
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "hire",
            "--channel",
            channel,
            "--session-ref",
            session,
            "--role",
            "builder",
            "--content",
            "go",
            "--check",
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "hire",
            "--channel",
            channel,
            "--session-ref",
            session,
            "--role",
            "builder",
            "--content",
            "go",
            "--check",
            "--no-wait",
        ])
        .is_err());
    }

    #[test]
    fn sessions_audit_takes_a_channel_and_an_optional_umbrella() {
        let channel = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
        assert!(Cli::try_parse_from(["bee", "sessions", "audit", "--channel", channel]).is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "sessions",
            "audit",
            "--channel",
            channel,
            "--session-ref",
            "6a8f1b2c-0000-4000-8000-000000000001",
        ])
        .is_ok());
        assert!(
            Cli::try_parse_from(["bee", "sessions", "audit"]).is_err(),
            "an audit with no channel has nothing to read"
        );
    }

    #[test]
    fn command_inventory_is_stable() {
        let expected_groups: Vec<&str> = vec![
            "agents",
            "canvas",
            "channels",
            "ci",
            "dms",
            "emoji",
            "events",
            "feed",
            "git",
            "issues",
            "media",
            "mem",
            "messages",
            "moderation",
            "notes",
            "pack",
            "packs",
            "patches",
            "pr",
            "projects",
            "pulse",
            "reactions",
            "repos",
            "session",
            "sessions",
            "social",
            "terminals",
            "upload",
            "users",
            "workflows",
        ];

        let cmd = Cli::command();
        let mut actual: Vec<String> = cmd
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .filter(|n| n != "help")
            .collect();
        actual.sort();

        assert_eq!(
            actual.len(),
            expected_groups.len(),
            "Expected {} groups, got {}. Actual: {:?}",
            expected_groups.len(),
            actual.len(),
            actual
        );
        assert_eq!(
            actual, expected_groups,
            "Command group inventory drift detected"
        );
    }

    /// `bee repos update` takes repeatable `--maintainer` and a
    /// `--clear-maintainers` switch, and `bee repos create` takes the same
    /// repeatable flag. Finding 33: without a writer, the `maintainers` tag
    /// the gate now reads could only be produced by hand.
    #[test]
    fn repos_maintainer_flags_parse() {
        let a = "aa".repeat(32);
        let b = "bb".repeat(32);
        assert!(Cli::try_parse_from([
            "bee",
            "repos",
            "create",
            "--id",
            "demo",
            "--maintainer",
            &a,
            "--maintainer",
            &b,
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "repos",
            "update",
            "--id",
            "demo",
            "--maintainer",
            &a
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "repos",
            "update",
            "--id",
            "demo",
            "--clear-maintainers"
        ])
        .is_ok());
        // `--id` is not optional: an update with no repository names nothing.
        assert!(Cli::try_parse_from(["bee", "repos", "update", "--maintainer", &a]).is_err());
    }

    #[test]
    fn subcommand_names_are_stable() {
        fn names(cmd: &clap::Command, group: &str) -> Vec<String> {
            let group_cmd = cmd
                .get_subcommands()
                .find(|s| s.get_name() == group)
                .unwrap_or_else(|| panic!("group '{}' not found", group));
            let mut names: Vec<String> = group_cmd
                .get_subcommands()
                .map(|s| s.get_name().to_string())
                .filter(|n| n != "help")
                .collect();
            names.sort();
            names
        }

        let cmd = Cli::command();
        assert_eq!(
            names(&cmd, "agents"),
            vec![
                "archive",
                "archived",
                "draft-create",
                "draft-update",
                "unarchive"
            ]
        );
        assert_eq!(
            names(&cmd, "messages"),
            vec![
                "delete",
                "edit",
                "get",
                "search",
                "send",
                "send-diff",
                "thread",
                "vote"
            ]
        );
        assert_eq!(
            names(&cmd, "channels"),
            vec![
                "add-member",
                "archive",
                "create",
                "delete",
                "get",
                "join",
                "leave",
                "list",
                "members",
                "purpose",
                "remove-member",
                "search",
                "set-add-policy",
                "topic",
                "unarchive",
                "update"
            ]
        );
        assert_eq!(names(&cmd, "canvas"), vec!["get", "set"]);
        assert_eq!(names(&cmd, "reactions"), vec!["add", "get", "remove"]);
        assert_eq!(
            names(&cmd, "emoji"),
            vec!["export", "import", "list", "rm", "set"]
        );
        assert_eq!(
            names(&cmd, "dms"),
            vec!["add-member", "hide", "list", "open"]
        );
        assert_eq!(
            names(&cmd, "users"),
            vec![
                "get",
                "presence",
                "set-presence",
                "set-profile",
                "set-status"
            ]
        );
        assert_eq!(
            names(&cmd, "workflows"),
            vec!["approve", "create", "delete", "get", "list", "runs", "trigger", "update"]
        );
        assert_eq!(names(&cmd, "feed"), vec!["get"]);
        assert_eq!(
            names(&cmd, "social"),
            vec![
                "contacts",
                "event",
                "list",
                "notes",
                "publish",
                "set-contacts",
                "set-list"
            ]
        );
        assert_eq!(
            names(&cmd, "repos"),
            vec!["bind", "create", "delete", "get", "list", "protect", "update"]
        );
        let repos = cmd
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "repos")
            .expect("repos command");
        let protect = repos
            .get_subcommands()
            .find(|subcommand| subcommand.get_name() == "protect")
            .expect("repos protect command");
        let mut protect_names: Vec<String> = protect
            .get_subcommands()
            .map(|subcommand| subcommand.get_name().to_string())
            .filter(|name| name != "help")
            .collect();
        protect_names.sort();
        assert_eq!(protect_names, vec!["list", "remove", "set"]);
        assert_eq!(
            names(&cmd, "pr"),
            vec!["get", "list", "open", "status", "update"]
        );
        assert_eq!(
            names(&cmd, "patches"),
            vec!["get", "list", "send", "status"]
        );
        assert_eq!(
            names(&cmd, "projects"),
            vec![
                "add-member",
                "add-repo",
                "create",
                "delete",
                "get",
                "list",
                "members",
                "remove-member",
                "remove-repo",
                "set-role",
                "update"
            ]
        );
        assert_eq!(
            names(&cmd, "pulse"),
            // 4 on the base tree, plus L9's `missions` and `prune-wip`.
            vec![
                "digest",
                "list",
                "missions",
                "prune-wip",
                "sessions",
                "update"
            ]
        );
        assert_eq!(
            names(&cmd, "issues"),
            vec!["assign", "create", "get", "list", "status", "unassign"]
        );
        assert_eq!(
            names(&cmd, "sessions"),
            vec![
                "acknowledge",
                "assign",
                "audit",
                "block",
                "catalog",
                "complete",
                "create",
                "decide",
                "delete",
                "doctor",
                "explain",
                "export",
                "grant",
                "grant-seat",
                "handover",
                "hire",
                "inbox",
                "list",
                "note",
                "observations",
                "observe",
                "operation",
                "policy",
                "registry",
                "report",
                "revoke",
                "revoke-seat",
                "roster",
                "route",
                "seat-repair",
                "send",
                "status",
                "tools",
                "transcript",
                "verdict",
                "whoami",
                "worktree"
            ]
        );
        assert_eq!(
            names(&cmd, "terminals"),
            vec!["delete", "invite", "list", "revoke", "roster", "send-input"]
        );
        assert_eq!(names(&cmd, "media"), vec!["get"]);
        assert_eq!(names(&cmd, "upload"), vec!["file"]);
        assert_eq!(names(&cmd, "pack"), vec!["inspect", "validate"]);
        assert_eq!(
            names(&cmd, "moderation"),
            vec![
                "audit",
                "ban",
                "reports",
                "resolve",
                "restricted",
                "timeout",
                "unban",
                "untimeout"
            ]
        );
    }

    /// The `registry` group's own inventory.
    ///
    /// Deliberately its **own** test rather than another arm inside
    /// `subcommand_names_are_stable`: item 108's trap is two lanes appending
    /// to one exact-count assertion in `subcommand_names_are_stable` /
    /// `subcommand_counts_are_stable` and taking main red between them. These
    /// three names are nested one level below `sessions`, so they move neither
    /// of those numbers, and this assertion collides with nothing.
    #[test]
    fn registry_subcommand_names_are_stable() {
        let cmd = Cli::command();
        let sessions = cmd
            .get_subcommands()
            .find(|group| group.get_name() == "sessions")
            .expect("sessions group");
        let registry = sessions
            .get_subcommands()
            .find(|group| group.get_name() == "registry")
            .expect("registry group");
        let mut names: Vec<String> = registry
            .get_subcommands()
            .map(|sub| sub.get_name().to_string())
            .filter(|name| name != "help")
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "check".to_owned(),
                "measure".to_owned(),
                "propose".to_owned()
            ],
            "one noun, one home: measure and propose live under `registry`, not at top level"
        );
    }

    #[test]
    fn subcommand_counts_are_stable() {
        let expected: Vec<(&str, usize)> = vec![
            ("agents", 5),
            ("canvas", 2),
            ("channels", 16),
            ("dms", 4),
            ("emoji", 5),
            ("feed", 1),
            ("issues", 6),
            ("media", 1),
            ("messages", 8),
            ("pack", 2),
            ("patches", 4),
            ("pr", 5),
            ("projects", 11),
            // 4 on the base tree, plus L9's `missions` and `prune-wip`.
            ("pulse", 6),
            ("reactions", 3),
            // 5 on the base tree, plus `delete`.
            ("repos", 7),
            // 24 on the base tree, plus A1's `audit`, `grant-seat` and
            // `revoke-seat` (batch 2 A), B1c's `decide` and `note`, B2's
            // `policy` (batch 2 B), `delete`, `whoami`, batch 3 L1's
            // `observe` and `observations`, batch 3 L11's `worktree` and
            // L13's `explain`.
            // `subcommand_names_are_stable` above names all thirty-six, so this
            // count and that list cannot drift apart. Five lanes of this wave
            // appended here and two of them independently wrote 35 (item 108's
            // exact-count trap); the finalizer set it once, after every lane,
            // and both tests re-run green.
            ("sessions", 37),
            ("social", 7),
            ("terminals", 6),
            ("upload", 1),
            ("users", 5),
            ("workflows", 8),
        ];

        let cmd = Cli::command();
        for (group_name, expected_count) in &expected {
            let group = cmd
                .get_subcommands()
                .find(|s| s.get_name() == *group_name)
                .unwrap_or_else(|| panic!("group '{}' not found", group_name));
            let actual_count = group
                .get_subcommands()
                .filter(|s| s.get_name() != "help")
                .count();
            assert_eq!(
                actual_count, *expected_count,
                "Group '{}': expected {} subcommands, got {}",
                group_name, expected_count, actual_count
            );
        }
    }

    /// The `sessions` verbs whose `--help` carries a **Rule** block, and the
    /// label of the frozen sentence it must quote.
    ///
    /// The set is exhaustive on purpose: a verb the specification froze no
    /// sentence for gets its recipe and nothing else. Inventing a rule to fill
    /// the gap would put a sentence nobody ruled on in front of every seat.
    const SESSIONS_RULE_SOURCES: &[(&str, &str)] = &[
        ("report", "batch vocabulary, fold disclosure"),
        ("hire", "batch vocabulary, hire brief ceiling"),
        ("observe", "batch vocabulary, observation tokens"),
        ("observations", "batch vocabulary, observation tokens"),
        ("seat-repair", "batch vocabulary, seat authority remedy"),
    ];

    /// The `after_help` text of one `bee sessions` verb.
    ///
    /// Read from the built `Command` rather than from rendered help: clap wraps
    /// rendered output at the terminal width, and a wrapped line cannot be
    /// compared byte-for-byte against a frozen sentence.
    fn sessions_after_help(name: &str) -> String {
        let cmd = Cli::command();
        let sessions = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "sessions")
            .unwrap_or_else(|| panic!("the sessions group exists"));
        let verb = sessions
            .get_subcommands()
            .find(|s| s.get_name() == name)
            .unwrap_or_else(|| panic!("sessions verb '{name}' exists"));
        verb.get_after_help()
            .map(|help| help.to_string())
            .unwrap_or_default()
    }

    /// Every `bee sessions` verb, `help` excluded.
    fn sessions_verb_names() -> Vec<String> {
        let cmd = Cli::command();
        let sessions = cmd
            .get_subcommands()
            .find(|s| s.get_name() == "sessions")
            .unwrap_or_else(|| panic!("the sessions group exists"));
        sessions
            .get_subcommands()
            .map(|s| s.get_name().to_string())
            .filter(|n| n != "help")
            .collect()
    }

    /// The line under `Recipe:`, or `None` when the verb carries no block.
    fn recipe_line(after_help: &str) -> Option<String> {
        let start = after_help.find("Recipe:\n  ")?;
        let rest = &after_help[start + "Recipe:\n  ".len()..];
        Some(rest.lines().next().unwrap_or(rest).to_string())
    }

    /// The line under `Rule:`, or `None` when the verb carries no block.
    fn rule_line(after_help: &str) -> Option<String> {
        let start = after_help.find("Rule:\n  ")?;
        let rest = &after_help[start + "Rule:\n  ".len()..];
        Some(rest.lines().next().unwrap_or(rest).to_string())
    }

    /// Batch 3 L13.1. A seat should learn a verb from one `--help`, not from a
    /// file read. The next verb added without a recipe fails here rather than
    /// in a live run.
    #[test]
    fn every_sessions_verb_help_carries_a_runnable_recipe() {
        for name in sessions_verb_names() {
            let after_help = sessions_after_help(&name);
            let recipe = recipe_line(&after_help).unwrap_or_else(|| {
                panic!(
                    "`bee sessions {name} --help` carries no Recipe block. Add one to the \
                     variant's after_help: a verb whose help does not show how to run it \
                     sends the reader to the source."
                )
            });
            assert!(
                recipe.starts_with("bee sessions "),
                "sessions {name}: the recipe must be a runnable `bee sessions ...` line, got {recipe:?}"
            );
        }
    }

    /// A recipe that names a flag the verb does not have is worse than none:
    /// it is a confident wrong answer. Every recipe is parsed as the command
    /// it claims to be.
    #[test]
    fn every_recipe_line_parses_as_the_command_it_shows() {
        for name in sessions_verb_names() {
            let after_help = sessions_after_help(&name);
            let Some(recipe) = recipe_line(&after_help) else {
                continue;
            };
            let argv: Vec<&str> = recipe.split_whitespace().collect();
            let parsed = Cli::try_parse_from(&argv);
            assert!(
                parsed.is_ok(),
                "sessions {name}: the recipe in --help does not parse.\n  recipe: {recipe}\n  clap: {}",
                parsed.err().map(|e| e.to_string()).unwrap_or_default()
            );
        }
    }

    /// A Rule block quotes a frozen sentence verbatim, and only the verbs the
    /// specification froze one for carry a Rule at all.
    #[test]
    fn only_the_verbs_with_a_frozen_sentence_carry_a_rule() {
        let mut with_rule: Vec<String> = Vec::new();
        for name in sessions_verb_names() {
            if rule_line(&sessions_after_help(&name)).is_some() {
                with_rule.push(name);
            }
        }
        with_rule.sort();
        let mut expected: Vec<String> = SESSIONS_RULE_SOURCES
            .iter()
            .map(|(verb, _)| (*verb).to_string())
            .collect();
        expected.sort();
        assert_eq!(
            with_rule, expected,
            "a Rule block appeared on a verb the specification froze no sentence for, or \
             vanished from one it did. Never invent a rule to fill a gap; write the recipe \
             alone and say so."
        );

        for (verb, label) in SESSIONS_RULE_SOURCES {
            let frozen = buzz_core::team_vocabulary::frozen_sentence(label)
                .expect("the frozen-sentence snapshot parses")
                .unwrap_or_else(|| panic!("no snapshot block is labelled {label:?}"));
            let rule = rule_line(&sessions_after_help(verb))
                .unwrap_or_else(|| panic!("sessions {verb} carries no Rule block"));
            assert_eq!(
                rule, frozen,
                "sessions {verb}: the Rule in --help has drifted from the checked-in copy \
                 of the frozen sentence"
            );
        }
    }

    /// `bee packs` parses its four verbs, and clap refuses two pins before a
    /// key is ever loaded — the wire rule stated where a person can still fix
    /// it.
    #[test]
    fn packs_parses_its_four_verbs_and_refuses_two_pins() {
        let project = format!("30621:{}:agiterra", "6c".repeat(32));
        let repo = format!("30617:{}:agiterra-packs", "6c".repeat(32));
        assert!(Cli::try_parse_from([
            "bee",
            "packs",
            "set-source",
            "--project",
            &project,
            "--repo",
            &repo,
            "--ref",
            "refs/heads/main"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "packs",
            "set-source",
            "--project",
            &project,
            "--repo",
            &repo,
            "--sha",
            &"a".repeat(40),
            "--path",
            "packs/roles",
            "--note",
            "pinned"
        ])
        .is_ok());
        assert!(
            Cli::try_parse_from([
                "bee",
                "packs",
                "set-source",
                "--project",
                &project,
                "--repo",
                &repo,
                "--ref",
                "refs/heads/main",
                "--sha",
                &"a".repeat(40)
            ])
            .is_err(),
            "--ref and --sha must conflict at the parser, not at the relay"
        );
        assert!(Cli::try_parse_from(["bee", "packs", "get-source", "--project", &project]).is_ok());
        assert!(Cli::try_parse_from(["bee", "packs", "init", "--project", &project]).is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "packs",
            "init",
            "--project",
            &project,
            "--repo-id",
            "agiterra-packs",
            "--from",
            "/tmp/packs",
            "--path",
            "packs/roles",
            "--dry-run"
        ])
        .is_ok());
        assert!(Cli::try_parse_from([
            "bee",
            "packs",
            "status",
            "--project",
            &project,
            "--role",
            "builder"
        ])
        .is_ok());
    }

    /// `sessions explain` is the verb the lane exists for; it must be listed
    /// and it must be reachable with no argument.
    #[test]
    fn sessions_explain_takes_an_optional_word() {
        assert!(Cli::try_parse_from(["bee", "sessions", "explain"]).is_ok());
        assert!(Cli::try_parse_from(["bee", "sessions", "explain", "unseated"]).is_ok());
    }

    /// Collect all args (recursing into subcommands) whose env var name looks
    /// like a credential but does NOT have `hide_env_values` set.
    fn collect_unhidden_secret_args(cmd: &clap::Command) -> Vec<(String, String)> {
        const SECRET_PATTERNS: &[&str] = &["KEY", "SECRET", "TOKEN", "PASSWORD", "CRED", "AUTH"];

        let mut violations: Vec<(String, String)> = Vec::new();

        for arg in cmd.get_arguments() {
            if let Some(env_key) = arg.get_env() {
                let env_name = env_key.to_string_lossy().to_uppercase();
                let is_secret = SECRET_PATTERNS.iter().any(|pat| env_name.contains(pat));
                if is_secret && !arg.is_hide_env_values_set() {
                    violations.push((cmd.get_name().to_string(), env_name));
                }
            }
        }

        for sub in cmd.get_subcommands() {
            violations.extend(collect_unhidden_secret_args(sub));
        }

        violations
    }

    /// Every arg whose env var name contains KEY/SECRET/TOKEN/PASSWORD/CRED/AUTH
    /// must set `hide_env_values = true` to prevent credential leakage in --help.
    #[test]
    fn secret_env_args_hide_their_values_in_help() {
        let cmd = Cli::command();
        let violations = collect_unhidden_secret_args(&cmd);
        assert!(
            violations.is_empty(),
            "Found secret-bearing env args without hide_env_values=true. \
             Add `hide_env_values = true` to each:\n{}",
            violations
                .iter()
                .map(|(cmd, env)| format!("  command={cmd:?} env={env:?}"))
                .collect::<Vec<_>>()
                .join("\n")
        );
    }

    // ── projects update mutation group ────────────────────────────────────────

    /// Multiple independent fields must be accepted in the same invocation.
    #[test]
    fn projects_update_multi_field_is_accepted() {
        assert!(
            Cli::try_parse_from([
                "buzz",
                "projects",
                "update",
                "my-slug",
                "--name",
                "X",
                "--description",
                "Y",
            ])
            .is_ok(),
            "--name and --description together must be accepted"
        );
    }

    /// A setter for one field and a clearer for a different field must be accepted.
    #[test]
    fn projects_update_setter_with_other_clearer_is_accepted() {
        assert!(
            Cli::try_parse_from([
                "buzz",
                "projects",
                "update",
                "my-slug",
                "--name",
                "X",
                "--clear-description",
            ])
            .is_ok(),
            "--name with --clear-description must be accepted"
        );
    }

    /// A setter and its own clearer are mutually exclusive — clap must reject this.
    #[test]
    fn projects_update_setter_with_own_clearer_is_rejected() {
        assert!(
            Cli::try_parse_from([
                "buzz",
                "projects",
                "update",
                "my-slug",
                "--name",
                "X",
                "--clear-name",
            ])
            .is_err(),
            "--name and --clear-name together must be rejected by clap"
        );
    }

    /// Providing no mutation options at all must be rejected by clap (required group).
    #[test]
    fn projects_update_no_mutation_is_rejected_by_clap() {
        // Without credentials, a valid parse would reach authentication and fail
        // with auth_error — but a clap-level rejection happens before any I/O.
        // We verify it's a clap error (not just any error) by checking the error
        // kind is not a runtime/auth failure — Cli::try_parse_from returns Err
        // immediately for argument violations.
        assert!(
            Cli::try_parse_from(["buzz", "projects", "update", "my-slug"]).is_err(),
            "update with no setters or clearers must be rejected at parse time"
        );
    }

    // ── pulse ────────────────────────────────────────────────────────────────

    /// A missing `--content` is a clap-level usage error, not a signed event
    /// with empty prose.
    #[test]
    fn pulse_update_requires_content() {
        assert!(
            Cli::try_parse_from(["buzz", "pulse", "update", "--kind", "plan"]).is_err(),
            "update without --content must be rejected at parse time"
        );
        assert!(
            Cli::try_parse_from([
                "buzz",
                "pulse",
                "update",
                "--kind",
                "plan",
                "--content",
                "Working in pool.rs",
            ])
            .is_ok(),
            "--content is taken verbatim, never as a file path"
        );
    }

    /// The entry type is a closed set; an unrecognised value never reaches the
    /// relay.
    #[test]
    fn pulse_update_invalid_kind_is_rejected_by_clap() {
        assert!(Cli::try_parse_from([
            "buzz",
            "pulse",
            "update",
            "--kind",
            "chartreuse",
            "--content",
            "x",
        ])
        .is_err());
        for kind in ["plan", "milestone", "note", "handoff", "blocker"] {
            assert!(
                Cli::try_parse_from(["buzz", "pulse", "update", "--kind", kind, "--content", "x"])
                    .is_ok(),
                "--kind {kind} must be accepted"
            );
        }
    }

    /// `--project` is optional at parse time on every subcommand: the value may
    /// come from `BUZZ_PULSE_PROJECT`, and its absence is a runtime usage error
    /// that names the variable.
    #[test]
    fn pulse_reads_parse_without_an_explicit_project() {
        for command in ["list", "sessions", "digest"] {
            assert!(
                Cli::try_parse_from(["buzz", "pulse", command]).is_ok(),
                "pulse {command} must parse without --project"
            );
        }
    }

    /// An unrecognised visibility token must be rejected by clap before any I/O.
    #[test]
    fn projects_create_invalid_visibility_is_rejected_by_clap() {
        assert!(
            Cli::try_parse_from([
                "buzz",
                "projects",
                "create",
                "my-slug",
                "--repo",
                "buzz",
                "--visibility",
                "chartreuse",
            ])
            .is_err(),
            "--visibility chartreuse must be rejected at parse time"
        );
    }

    /// An unrecognised visibility token on update must be rejected by clap before any I/O.
    #[test]
    fn projects_update_invalid_visibility_is_rejected_by_clap() {
        assert!(
            Cli::try_parse_from([
                "buzz",
                "projects",
                "update",
                "my-slug",
                "--visibility",
                "chartreuse",
            ])
            .is_err(),
            "--visibility chartreuse on update must be rejected at parse time"
        );
    }
}
