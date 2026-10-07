//! Auto-title: name an unnamed session from the founder's first message
//! (SV-31, kind 44252, NIP-CSG § Generated title).
//!
//! The provider that runs the founder's first turn of generation 1 asks the
//! same runtime — same adapter, same account, so no new third party sees the
//! message — for a short title, and signs the answer with its **own** key as
//! a kind-44252 generated title. It is never published as a 44229: a model's
//! words are not the person's name, and readers rank any founder 44229 above
//! every generated title (`beekeeper_core::coding_session_title`).
//!
//! The shape follows T3 Code's `ThreadTitleRegenerationService.ts` (initial
//! kind): the first message only, truncated head-and-tail to 2,000
//! characters (`ThreadTitleContext.ts` `limitTitleMessage`), with its
//! attachments' metadata after it (so a pasted screenshot alone is still
//! titled, as T3 titles it), two retries
//! backing off exponentially from 2 s, a failure that is logged and leaves the
//! title as it was, and a placeholder answer that names nothing. Where T3
//! fences a rename with an in-flight `requestId`, Beekeeper has no such marker
//! on the wire, so a person's name wins twice over: a preflight skips the job
//! when a founder 44229 (or any 44252) already exists, the same check runs
//! again immediately before signing, and readers rank by tier regardless.
//!
//! This computer's session-title mode (SV-56, D9, `session_title_mode.rs`)
//! is read live twice — before a job starts and again immediately before it
//! signs — so only mode `agent` (or no file) publishes; `my-model` and `off`
//! publish nothing, an unreadable file fails closed, and a person who flips
//! to Off while the model is thinking gets no title. `BEEKEEPER_CSP_AUTO_TITLE=off`
//! wins over every mode.
//!
//! The work is a detached task. Nothing here delays the turn that triggered
//! it, the one-shot runs on its own single-slot semaphore (never against
//! `max_sessions`), and the spawn happens in a fresh directory under the
//! provider's state that a `Drop` guard removes on every path out.

#[path = "auto_title_acp.rs"]
mod acp;

use std::collections::HashSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use beekeeper_acp::relay::RestClient;
use beekeeper_core::coding_session_command::{CodingSessionTarget, TurnAttachment};
use beekeeper_core::coding_session_title::{
    clean_generated_name, CodingSessionTitleBasis, CodingSessionTitlePayload,
    CODING_SESSION_TITLE_SCHEMA, NAMING_SYSTEM_PROMPT,
};
use beekeeper_core::kind::{KIND_CODING_SESSION_GENERATED_TITLE, KIND_CODING_SESSION_NAME};
use nostr::{Alphabet, Event, Filter, Keys, Kind, PublicKey, SingleLetterTag};
use tokio::sync::{mpsc, Semaphore};
use uuid::Uuid;

use crate::execution_scope::{
    project_scope_cache_dir, RuntimeProfile, ScopeInputs, ScopePurpose, DISCOVERY_DIR,
    EXECUTIONS_DIR,
};
use crate::session_title_mode::{self, SessionTitleMode, SESSION_TITLE_MODE_FILE};

/// The host-wide switch. On unless set to a false spelling (`off`).
pub const AUTO_TITLE_ENV: &str = "BEEKEEPER_CSP_AUTO_TITLE";
/// Budget for one attempt: spawn, `initialize`, `session/new`, the model
/// switch and the prompt.
pub(crate) const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(30);
/// Attempts after the first (T3: `times: 2` for the initial kind).
pub(crate) const RETRIES: u32 = 2;
/// First backoff; each later one doubles (T3: `Schedule.exponential("2 seconds")`).
pub(crate) const BACKOFF_BASE: Duration = Duration::from_secs(2);
/// Most characters of the first message the namer sees (T3 `MAX_MESSAGE`).
pub(crate) const MAX_TITLE_MESSAGE_CHARS: usize = 2_000;
/// Most characters of the attachment-metadata section (T3
/// `threadTitlePromptSuffix`, `limitSection(…, 4_000)`).
pub(crate) const MAX_ATTACHMENT_SECTION_CHARS: usize = 4_000;
/// Most characters of one attachment's displayed name.
const MAX_ATTACHMENT_NAME_CHARS: usize = 80;
/// Marker between the kept head and tail (T3 `TRUNCATED`).
const TRUNCATED: &str = "\n[Content truncated]\n";
/// Prefix of every titling scratch directory and execution id.
const SCRATCH_PREFIX: &str = "title-";
/// Rows read back by a preflight. Existence is the whole question.
const PREFLIGHT_LIMIT: usize = 8;

/// Keep the request and its final constraints when a message is too long:
/// head and tail around a marker, `budget` characters in all (T3
/// `limitTitleMessage`).
#[must_use]
pub(crate) fn limit_title_message(text: &str, budget: usize) -> String {
    let length = text.chars().count();
    if length <= budget {
        return text.to_owned();
    }
    let marker = TRUNCATED.chars().count();
    if budget <= marker {
        return String::new();
    }
    let available = budget - marker;
    let head = available.div_ceil(2);
    let tail = available - head;
    let mut limited: String = text.chars().take(head).collect();
    limited.push_str(TRUNCATED);
    limited.extend(text.chars().skip(length - tail));
    limited
}

/// What the namer is shown: the first message, limited, and — when the turn
/// carried attachments — T3's "Attachment metadata" section after it, one
/// `- name (mime, size bytes)` line each (`TextGenerationPrompts.ts`
/// `threadTitlePromptSuffix`). Metadata only, exactly as T3 sends it: the
/// blobs themselves never reach the namer.
#[must_use]
pub(crate) fn title_message(text: &str, attachments: &[TurnAttachment]) -> String {
    let mut message = limit_title_message(text, MAX_TITLE_MESSAGE_CHARS);
    if attachments.is_empty() {
        return message;
    }
    let lines: Vec<String> = attachments
        .iter()
        .map(|attachment| {
            format!(
                "- {} ({}, {} bytes)",
                attachment_name(attachment),
                attachment.mime,
                attachment.size
            )
        })
        .collect();
    message.push_str("\n\nAttachment metadata:\n");
    message.push_str(&limit_title_message(
        &lines.join("\n"),
        MAX_ATTACHMENT_SECTION_CHARS,
    ));
    message
}

/// An attachment's display name with control characters removed, or its
/// short hash and extension when it carries none.
fn attachment_name(attachment: &TurnAttachment) -> String {
    let cleaned: String = attachment
        .filename
        .as_deref()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_ATTACHMENT_NAME_CHARS)
        .collect();
    let cleaned = cleaned.trim();
    if cleaned.is_empty() {
        let stem: String = attachment.sha256.chars().take(8).collect();
        format!("{stem}.{}", attachment.extension())
    } else {
        cleaned.to_owned()
    }
}

/// Retry and timeout budget for one job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RetryPolicy {
    /// Ceiling on one attempt.
    pub attempt_timeout: Duration,
    /// Attempts after the first.
    pub retries: u32,
    /// Wait before the first retry; doubled for each later one.
    pub backoff_base: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            attempt_timeout: ATTEMPT_TIMEOUT,
            retries: RETRIES,
            backoff_base: BACKOFF_BASE,
        }
    }
}

/// What decides whether a starting turn is the one that gets titled.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TriggerFacts<'a> {
    /// The execution's generation.
    pub generation: u64,
    /// Turns this host had already started under the umbrella before this
    /// one (`StateStore::turns_used`, read before this turn is charged).
    pub turns_used_before: u64,
    /// Who sent this turn, when the provider knows.
    pub operator: Option<&'a str>,
    /// The umbrella's founder, when recorded.
    pub founder: Option<&'a str>,
    /// The turn's own text.
    pub text: &'a str,
    /// How many attachments the turn carried.
    pub attachments: usize,
    /// Whether this process already started a job for the umbrella.
    pub already_started: bool,
}

/// Whether this turn is the founder's first turn of generation 1.
///
/// The title summarises the person's own words, so a seat assignment, a
/// granted operator's turn or a host answer never starts one — and only the
/// umbrella's very first turn does. A message with no text but an attachment
/// is titled from the attachment's metadata, as T3 titles it; only a turn
/// with neither is skipped.
#[must_use]
pub(crate) fn is_title_turn(facts: &TriggerFacts<'_>) -> bool {
    facts.generation == 1
        && facts.turns_used_before == 0
        && !facts.already_started
        && (!facts.text.trim().is_empty() || facts.attachments > 0)
        && matches!(
            (facts.operator, facts.founder),
            (Some(operator), Some(founder)) if operator.eq_ignore_ascii_case(founder)
        )
}

/// One umbrella's titling job.
#[derive(Debug, Clone)]
pub(crate) struct TitleJob {
    /// Channel the umbrella lives in.
    pub channel_id: Uuid,
    /// The umbrella (`d`).
    pub session_ref: String,
    /// The founder, whose 44229 settles the name.
    pub founder: String,
    /// The execution that ran the turn; its signer is this provider.
    pub target: CodingSessionTarget,
    /// The founder's first message, untruncated.
    pub first_message: String,
    /// The first message's attachments, whose metadata the namer is shown.
    pub attachments: Vec<TurnAttachment>,
    /// The 44220 whose text this is, or `None` for a create's initial turn.
    pub source_command: Option<String>,
    /// The 44221 create of this execution.
    pub create_event_id: String,
}

/// A title a generator produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct GeneratedTitle {
    /// Cleaned, valid title.
    pub title: String,
    /// The model that actually answered.
    pub model: String,
}

/// Produces a title for a message. `Ok(None)` is an answer that names
/// nothing (blank, or a placeholder) and is not retried; `Err` is a failed
/// attempt and is.
pub(crate) trait TitleGenerator: Send + Sync + 'static {
    /// One attempt.
    fn generate(
        &self,
        message: &str,
    ) -> impl Future<Output = Result<Option<GeneratedTitle>, String>> + Send;
}

/// Answers whether the umbrella is already named: a founder 44229 or any
/// 44252 exists for `(h, d)`.
pub(crate) trait TitlePreflight: Send + Sync + 'static {
    /// One check.
    fn already_named(&self, job: &TitleJob) -> impl Future<Output = Result<bool, String>> + Send;
}

/// How one job ended.
#[derive(Debug)]
pub(crate) enum TitleOutcome {
    /// A signed 44252, ready for the outbox.
    Ready(Box<Event>),
    /// The preflight found a name before any spawn.
    AlreadyNamed,
    /// A name landed while the title was being generated; the title is
    /// dropped unsigned.
    NamedMeanwhile,
    /// The runtime answered with nothing usable.
    NothingUsable,
    /// Every attempt failed, or a check could not be made. Nothing is
    /// published; this is the one log line.
    Failed(String),
    /// This computer's session-title mode, read just before signing, no
    /// longer lets the provider title; the title is dropped unsigned.
    ModeDeclined(SessionTitleMode),
    /// The session-title mode file could not be read just before signing; the
    /// title is dropped unsigned (fail closed).
    ModeUnreadable(String),
}

/// Run one job to its end with no session-title mode file consulted (the
/// SV-31 job alone). See [`run_title_job_in`].
#[cfg(test)]
pub(crate) async fn run_title_job<P: TitlePreflight, G: TitleGenerator>(
    job: &TitleJob,
    preflight: &P,
    generator: &G,
    policy: RetryPolicy,
    slot: &Semaphore,
    keys: &Keys,
) -> TitleOutcome {
    run_title_job_in(None, job, preflight, generator, policy, slot, keys).await
}

/// Run one job to its end. Never panics, never publishes on its own: a
/// [`TitleOutcome::Ready`] event goes back to the provider loop. With
/// `mode_dir`, the session-title mode file there is read again immediately
/// before signing, and anything but mode `agent` drops the title.
pub(crate) async fn run_title_job_in<P: TitlePreflight, G: TitleGenerator>(
    mode_dir: Option<&Path>,
    job: &TitleJob,
    preflight: &P,
    generator: &G,
    policy: RetryPolicy,
    slot: &Semaphore,
    keys: &Keys,
) -> TitleOutcome {
    match preflight.already_named(job).await {
        Ok(true) => return TitleOutcome::AlreadyNamed,
        Ok(false) => {}
        Err(error) => return TitleOutcome::Failed(format!("preflight: {error}")),
    }
    let message = title_message(&job.first_message, &job.attachments);
    let generated = {
        let Ok(_permit) = slot.acquire().await else {
            return TitleOutcome::Failed("the titling slot is closed".into());
        };
        let mut last_error = String::new();
        let mut generated = None;
        for attempt in 0..=policy.retries {
            if attempt > 0 {
                let factor = 1u32.checked_shl(attempt - 1).unwrap_or(u32::MAX);
                tokio::time::sleep(policy.backoff_base.saturating_mul(factor)).await;
            }
            match tokio::time::timeout(policy.attempt_timeout, generator.generate(&message)).await {
                Ok(Ok(Some(title))) => {
                    generated = Some(title);
                    break;
                }
                Ok(Ok(None)) => return TitleOutcome::NothingUsable,
                Ok(Err(error)) => last_error = error,
                Err(_) => {
                    last_error = format!("timed out after {:?}", policy.attempt_timeout);
                }
            }
        }
        match generated {
            Some(title) => title,
            None => {
                return TitleOutcome::Failed(format!(
                    "{} attempt(s) failed; last: {last_error}",
                    policy.retries + 1
                ))
            }
        }
    };
    // The recheck sits as close to signing as it can: a person who named the
    // session while the model was thinking has the last word.
    match preflight.already_named(job).await {
        Ok(true) => return TitleOutcome::NamedMeanwhile,
        Ok(false) => {}
        Err(error) => return TitleOutcome::Failed(format!("publish-time recheck: {error}")),
    }
    // The person's mode is read again here, after the slow part: flipping to
    // Off while the model was thinking means no title.
    if let Some(dir) = mode_dir {
        match session_title_mode::read(dir) {
            Ok(mode) if mode.provider_generates() => {}
            Ok(mode) => return TitleOutcome::ModeDeclined(mode),
            Err(reason) => return TitleOutcome::ModeUnreadable(reason),
        }
    }
    match sign_title(job, &generated, keys) {
        Ok(event) => TitleOutcome::Ready(Box::new(event)),
        Err(error) => TitleOutcome::Failed(format!("could not sign the title: {error}")),
    }
}

fn sign_title(job: &TitleJob, generated: &GeneratedTitle, keys: &Keys) -> Result<Event, String> {
    let payload = CodingSessionTitlePayload {
        schema: CODING_SESSION_TITLE_SCHEMA.to_owned(),
        title: generated.title.clone(),
        model: generated.model.clone(),
        basis: CodingSessionTitleBasis::FirstMessage,
        source_command: job.source_command.clone(),
        create_event_id: job.create_event_id.clone(),
    };
    beekeeper_sdk::builders::build_coding_session_generated_title(
        job.channel_id,
        &job.session_ref,
        &job.target,
        &payload,
    )
    .map_err(|error| error.to_string())?
    .sign_with_keys(keys)
    .map_err(|error| error.to_string())
}

/// A finished title on its way to the outbox.
#[derive(Debug)]
pub(crate) struct TitleReady {
    /// Channel it publishes into.
    pub channel_id: Uuid,
    /// The umbrella it names.
    pub session_ref: String,
    /// The provider session it was generated for.
    pub session_id: String,
    /// The signed 44252.
    pub event: Box<Event>,
}

/// The provider's titling state: the switch, the one slot, the umbrellas
/// already started this process, and the queue back to the run loop.
pub(crate) struct AutoTitler {
    enabled: bool,
    policy: RetryPolicy,
    slot: Arc<Semaphore>,
    started: HashSet<String>,
    tasks: tokio::task::JoinSet<()>,
    ready_tx: mpsc::UnboundedSender<TitleReady>,
    ready_rx: Option<mpsc::UnboundedReceiver<TitleReady>>,
}

impl AutoTitler {
    /// Titling state for a host with the switch `enabled`.
    pub(crate) fn new(enabled: bool) -> Self {
        let (ready_tx, ready_rx) = mpsc::unbounded_channel();
        Self {
            enabled,
            policy: RetryPolicy::default(),
            slot: Arc::new(Semaphore::new(1)),
            started: HashSet::new(),
            tasks: tokio::task::JoinSet::new(),
            ready_tx,
            ready_rx: Some(ready_rx),
        }
    }

    /// Whether the host switch is on.
    pub(crate) fn enabled(&self) -> bool {
        self.enabled
    }

    /// The queue of finished titles, taken once by the run loop.
    pub(crate) fn take_ready(&mut self) -> Option<mpsc::UnboundedReceiver<TitleReady>> {
        self.ready_rx.take()
    }

    /// Jobs still running (finished ones are reaped first).
    #[cfg(test)]
    pub(crate) fn running(&mut self) -> usize {
        while self.tasks.try_join_next().is_some() {}
        self.tasks.len()
    }

    /// Start one detached job with no mode file consulted, and return at
    /// once. See [`Self::start_in`].
    #[cfg(test)]
    pub(crate) fn start<P: TitlePreflight, G: TitleGenerator>(
        &mut self,
        session_id: String,
        job: TitleJob,
        preflight: P,
        generator: G,
        keys: Keys,
    ) {
        self.start_in(None, session_id, job, preflight, generator, keys);
    }

    /// Mark an umbrella decided without a job (the mode declined it), so it
    /// is considered — and logged — once per process.
    pub(crate) fn decline(&mut self, session_ref: &str) {
        self.started.insert(session_ref.to_owned());
    }

    /// Start one detached job and return at once. With `mode_dir`, the job
    /// re-reads the session-title mode there before it signs.
    pub(crate) fn start_in<P: TitlePreflight, G: TitleGenerator>(
        &mut self,
        mode_dir: Option<PathBuf>,
        session_id: String,
        job: TitleJob,
        preflight: P,
        generator: G,
        keys: Keys,
    ) {
        while self.tasks.try_join_next().is_some() {}
        self.started.insert(job.session_ref.clone());
        let slot = Arc::clone(&self.slot);
        let policy = self.policy;
        let ready = self.ready_tx.clone();
        self.tasks.spawn(async move {
            let outcome = run_title_job_in(
                mode_dir.as_deref(),
                &job,
                &preflight,
                &generator,
                policy,
                &slot,
                &keys,
            )
            .await;
            log_outcome(&job, &outcome);
            if let TitleOutcome::Ready(event) = outcome {
                let _ = ready.send(TitleReady {
                    channel_id: job.channel_id,
                    session_ref: job.session_ref,
                    session_id,
                    event,
                });
            }
        });
    }

    #[cfg(test)]
    pub(crate) fn with_policy(mut self, policy: RetryPolicy) -> Self {
        self.policy = policy;
        self
    }
}

fn log_outcome(job: &TitleJob, outcome: &TitleOutcome) {
    let session_ref = job.session_ref.as_str();
    match outcome {
        TitleOutcome::Ready(_) => {
            tracing::info!(target: "csp::auto_title", %session_ref, "generated a session title")
        }
        TitleOutcome::AlreadyNamed => tracing::debug!(
            target: "csp::auto_title",
            %session_ref,
            "session already named; no title generated"
        ),
        TitleOutcome::NamedMeanwhile => tracing::info!(
            target: "csp::auto_title",
            %session_ref,
            "session was named while its title was generated; title dropped"
        ),
        TitleOutcome::NothingUsable => tracing::info!(
            target: "csp::auto_title",
            %session_ref,
            "the runtime answered with no usable title; nothing published"
        ),
        TitleOutcome::Failed(error) => tracing::warn!(
            target: "csp::auto_title",
            %session_ref,
            "session title generation failed; nothing published: {error}"
        ),
        TitleOutcome::ModeDeclined(mode) => tracing::info!(
            target: "csp::auto_title",
            %session_ref,
            "session-title mode is now {}; generated title dropped",
            mode.as_str()
        ),
        TitleOutcome::ModeUnreadable(reason) => tracing::warn!(
            target: "csp::auto_title",
            %session_ref,
            "{SESSION_TITLE_MODE_FILE} {reason}; generated title dropped"
        ),
    }
}

/// The relay-backed preflight.
pub(crate) struct RelayTitleLedger {
    /// The provider's relay reader.
    pub rest: RestClient,
}

impl TitlePreflight for RelayTitleLedger {
    async fn already_named(&self, job: &TitleJob) -> Result<bool, String> {
        let founder = PublicKey::from_hex(&job.founder).map_err(|error| error.to_string())?;
        let h = SingleLetterTag::lowercase(Alphabet::H);
        let d = SingleLetterTag::lowercase(Alphabet::D);
        let channel = job.channel_id.to_string();
        let names = Filter::new()
            .kind(Kind::Custom(KIND_CODING_SESSION_NAME as u16))
            .author(founder)
            .custom_tags(h, [channel.as_str()])
            .custom_tags(d, [job.session_ref.as_str()])
            .limit(PREFLIGHT_LIMIT);
        let titles = Filter::new()
            .kind(Kind::Custom(KIND_CODING_SESSION_GENERATED_TITLE as u16))
            .custom_tags(h, [channel.as_str()])
            .custom_tags(d, [job.session_ref.as_str()])
            .limit(PREFLIGHT_LIMIT);
        let rows = self
            .rest
            .query(&[names, titles])
            .await
            .map_err(|error| error.to_string())?;
        let rows = rows.as_array().ok_or_else(|| {
            "the relay answered a query with something other than a list".to_owned()
        })?;
        Ok(rows.iter().any(|row| names_the_session(row, job)))
    }
}

/// Whether one relay row is a name for this umbrella: a signature-valid
/// founder 44229 or any signature-valid 44252, with this `h` and `d`.
pub(crate) fn names_the_session(row: &serde_json::Value, job: &TitleJob) -> bool {
    let Ok(event) = serde_json::from_value::<Event>(row.clone()) else {
        return false;
    };
    if event.verify().is_err() {
        return false;
    }
    let tag = |name: &str| {
        event
            .tags
            .iter()
            .map(nostr::Tag::as_slice)
            .find(|tag| tag.first().map(String::as_str) == Some(name))
            .and_then(|tag| tag.get(1).cloned())
    };
    if tag("h").as_deref() != Some(job.channel_id.to_string().as_str())
        || tag("d").as_deref() != Some(job.session_ref.as_str())
    {
        return false;
    }
    let kind = u32::from(event.kind.as_u16());
    (kind == KIND_CODING_SESSION_NAME && event.pubkey.to_hex() == job.founder)
        || kind == KIND_CODING_SESSION_GENERATED_TITLE
}

/// The runtime a job spawns, copied out of its descriptor.
#[derive(Debug, Clone)]
pub(crate) struct AcpTitleGenerator {
    /// Provider state directory: the scratch lives under it.
    pub state_dir: PathBuf,
    /// Driver slug, for the runtime profile.
    pub driver: String,
    /// Runtime slug (`claude`, `codex`, …).
    pub runtime: String,
    /// Adapter executable.
    pub agent_command: String,
    /// Adapter argv.
    pub agent_args: Vec<String>,
    /// The descriptor's `cli_env`.
    pub cli_env: Vec<(String, String)>,
    /// The title model (`RuntimeDescriptor::effective_title_model`).
    pub model: String,
    /// The test fixtures' runtime profile; `None` in production.
    pub runtime_profile_override: Option<RuntimeProfile>,
}

impl TitleGenerator for AcpTitleGenerator {
    async fn generate(&self, message: &str) -> Result<Option<GeneratedTitle>, String> {
        let scratch = TitleScratch::create(&self.state_dir)?;
        let plan = {
            let mut inputs = ScopeInputs::new(
                ScopePurpose::Discovery,
                &self.state_dir,
                &scratch.session_id,
                &scratch.cwd,
            );
            inputs.driver = &self.driver;
            inputs.runtime = self
                .runtime_profile_override
                .unwrap_or_else(|| RuntimeProfile::for_driver(&self.driver));
            inputs.agent_command = &self.agent_command;
            inputs.agent_args = &self.agent_args;
            inputs.agent_env = &self.cli_env;
            crate::execution_scope::prepare(&inputs).map_err(|failure| failure.message)?
        };
        let runtime = acp::OneShotRuntime {
            agent_command: &self.agent_command,
            agent_args: &self.agent_args,
            cli_env: &self.cli_env,
            claude: self.runtime == "claude",
            model: &self.model,
        };
        let answer =
            acp::run_one_shot(&runtime, &plan, &scratch.cwd, NAMING_SYSTEM_PROMPT, message).await?;
        drop(scratch);
        if answer.permissions_rejected > 0 {
            tracing::debug!(
                target: "csp::auto_title",
                rejected = answer.permissions_rejected,
                "the namer asked for permissions; every one was rejected"
            );
        }
        if answer.text.trim().is_empty() {
            return Err("the runtime answered with no text".into());
        }
        Ok(
            clean_generated_name(&answer.text).map(|title| GeneratedTitle {
                title,
                model: answer.model,
            }),
        )
    }
}

/// A fresh, empty working directory for one attempt, and everything the
/// boundary created for it. Removed on drop — on success, failure, timeout
/// and cancellation alike — so neither the directory nor the runtime's own
/// state (which holds the first message) outlives the attempt.
pub(crate) struct TitleScratch {
    /// The working directory, under the provider's discovery directory.
    pub cwd: PathBuf,
    /// The execution id the boundary keys its private directory with.
    pub session_id: String,
    executions: PathBuf,
    cache: Option<PathBuf>,
}

impl TitleScratch {
    /// Create the directory.
    pub(crate) fn create(state_dir: &Path) -> Result<Self, String> {
        let session_id = format!("{SCRATCH_PREFIX}{}", Uuid::new_v4().simple());
        let cwd = state_dir.join(DISCOVERY_DIR).join(&session_id);
        std::fs::create_dir_all(&cwd)
            .map_err(|error| format!("could not create the titling directory: {error}"))?;
        let canonical_state = std::fs::canonicalize(state_dir).ok();
        let cache = match (&canonical_state, std::fs::canonicalize(&cwd).ok()) {
            (Some(state), Some(tree)) => Some(project_scope_cache_dir(state, None, &tree)),
            _ => None,
        };
        Ok(Self {
            executions: canonical_state
                .unwrap_or_else(|| state_dir.to_path_buf())
                .join(EXECUTIONS_DIR),
            cwd,
            session_id,
            cache,
        })
    }
}

impl Drop for TitleScratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.cwd);
        if let Some(cache) = &self.cache {
            let _ = std::fs::remove_dir_all(cache);
        }
        let owned = format!("{}-", self.session_id);
        if let Ok(entries) = std::fs::read_dir(&self.executions) {
            for entry in entries.flatten() {
                if entry.file_name().to_string_lossy().starts_with(&owned) {
                    let _ = std::fs::remove_dir_all(entry.path());
                }
            }
        }
    }
}

impl crate::Provider {
    /// Start titling for a turn that just began, when it is the founder's
    /// first turn of generation 1 and nothing names the umbrella yet.
    ///
    /// Called from the turn-start bookkeeping **before** the turn is charged
    /// to the umbrella, so `turns_used` still reads 0 for the first one.
    /// Returns at once: the job is detached and never delays the turn.
    pub(crate) fn maybe_start_auto_title(
        &mut self,
        session_id: &str,
        command_id: &str,
        text: &str,
        attachments: &[TurnAttachment],
        operator: Option<&str>,
    ) {
        if !self.auto_title.enabled() {
            return;
        }
        let Some(record) = self.state.session(session_id) else {
            return;
        };
        let Some(session_ref) = record.session_ref.clone() else {
            return;
        };
        let facts = TriggerFacts {
            generation: record.generation,
            turns_used_before: self.state.turns_used(&session_ref),
            operator,
            founder: record.founder_pubkey.as_deref(),
            text,
            attachments: attachments.len(),
            already_started: self.auto_title.started.contains(&session_ref),
        };
        if !is_title_turn(&facts) {
            return;
        }
        if !session_title_mode::admit_job(&self.config.state_dir, &session_ref) {
            self.auto_title.decline(&session_ref);
            return;
        }
        let Some(founder) = record.founder_pubkey.clone() else {
            return;
        };
        let create_event_id = record.command_id.clone();
        let source_command = (command_id != create_event_id).then(|| command_id.to_owned());
        let instance_ref = record.provider_instance_ref.clone();
        let Some(descriptor) = self.config.runtime(&instance_ref) else {
            tracing::info!(
                target: "csp::auto_title",
                %session_ref,
                "no runtime descriptor for {instance_ref}; session not titled"
            );
            return;
        };
        let Some(model) = descriptor.effective_title_model() else {
            tracing::info!(
                target: "csp::auto_title",
                %session_ref,
                "titling is off for the {} runtime (titleModel: null)",
                descriptor.runtime
            );
            return;
        };
        let generator = AcpTitleGenerator {
            state_dir: self.config.state_dir.clone(),
            driver: descriptor.driver.clone(),
            runtime: descriptor.runtime.clone(),
            agent_command: descriptor.agent_command.clone(),
            agent_args: descriptor.agent_args.clone(),
            cli_env: descriptor
                .cli_env
                .iter()
                .map(|env| (env.name.clone(), env.value.clone()))
                .collect(),
            model,
            runtime_profile_override: self.config.runtime_profile_override,
        };
        let Some(rest) = self.rest_client.clone() else {
            tracing::info!(
                target: "csp::auto_title",
                %session_ref,
                "no relay reader yet, so the preflight cannot run; session not titled"
            );
            return;
        };
        let Some((channel_id, target)) = self.locate(session_id) else {
            return;
        };
        let job = TitleJob {
            channel_id,
            session_ref,
            founder,
            target,
            first_message: text.to_owned(),
            attachments: attachments.to_vec(),
            source_command,
            create_event_id,
        };
        let keys = self.config.keys.clone();
        self.auto_title.start_in(
            Some(self.config.state_dir.clone()),
            session_id.to_owned(),
            job,
            RelayTitleLedger { rest },
            generator,
            keys,
        );
    }

    /// Put a finished title in the outbox, through the same durable path every
    /// provider-signed fact takes.
    pub(crate) fn enqueue_generated_title(&mut self, ready: TitleReady) -> anyhow::Result<()> {
        if self.membership_known && !self.subscribed.contains(&ready.channel_id) {
            return Ok(());
        }
        if self
            .state
            .session(&ready.session_id)
            .is_some_and(crate::state::SessionRecord::is_retired)
        {
            return Ok(());
        }
        let semantic_key = format!("title:{}", ready.session_ref);
        self.outbox.enqueue(
            KIND_CODING_SESSION_GENERATED_TITLE,
            &semantic_key,
            crate::publish::Priority::High,
            *ready.event,
        )?;
        Ok(())
    }

    /// Say at startup whether this host titles sessions, and which runtimes
    /// opted out.
    pub(crate) fn log_auto_title_state(&self) {
        if !self.auto_title.enabled() {
            tracing::info!(
                target: "csp::auto_title",
                "auto-title is off ({AUTO_TITLE_ENV}=off): unnamed sessions keep their fallback name"
            );
            return;
        }
        session_title_mode::log_startup(&self.config.state_dir);
        for descriptor in &self.config.runtimes {
            match descriptor.effective_title_model() {
                Some(model) => tracing::info!(
                    target: "csp::auto_title",
                    "auto-title is on for {} with model {model}",
                    descriptor.instance_ref
                ),
                None => tracing::info!(
                    target: "csp::auto_title",
                    "auto-title is off for {} (titleModel: null)",
                    descriptor.instance_ref
                ),
            }
        }
    }
}

/// The run loop's wait on the finished-title queue; parks forever once the
/// queue is gone, like the other taken listeners.
pub(crate) async fn next_ready(
    ready: &mut Option<mpsc::UnboundedReceiver<TitleReady>>,
) -> TitleReady {
    if let Some(queue) = ready {
        if let Some(title) = queue.recv().await {
            return title;
        }
    }
    *ready = None;
    std::future::pending().await
}

#[cfg(test)]
#[path = "auto_title_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "auto_title_mode_tests.rs"]
mod mode_tests;
