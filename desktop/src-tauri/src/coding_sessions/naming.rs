//! The draft-time Name-field suggestion and the Solo goal summary, asked of a
//! model of the person's choosing while the founded page is being filled in.
//!
//! Naming a session **after Start** moved to the host (NIP-CSG § Generated
//! title): the provider that runs the founder's first turn titles it through
//! the session's own runtime and signs a kind 44252 with its own key. The
//! desktop no longer publishes a model's words as the founder's 44229. What
//! stays here runs before Start, so a person sees it in the field and chooses
//! it; the instruction and the cleaner are buzz-core's
//! (`buzz_core_pkg::coding_session_title`), the same text the host uses.
//!
//! This is the one place in the create flow that can send what someone typed
//! to a machine that is not theirs, so three things are non-negotiable:
//!
//! 1. It is **off** until configured. No default endpoint, no default key, no
//!    quiet first request. A suggestion is a convenience; a silent egress is
//!    not.
//! 2. The **API key never reaches the webview**. It is written here, stored in
//!    the OS keyring (falling back to the `0o600` record file on builds
//!    without one), and read only when a request is being built. The settings
//!    surface learns `hasApiKey`, never the key.
//! 3. The request carries **only the first message**. No workdir, no channel,
//!    no identity, no repository — none of which would improve a four-word
//!    title, and all of which would be a fact about this machine handed to a
//!    third party.
//!
//! Whether any of this runs is the person's session-title mode (D9, SV-56,
//! [`super::naming_mode`]): the generate commands refuse unless it is
//! "Use my naming model".
//!
//! Two adapters cover every endpoint worth naming: Anthropic's Messages API,
//! and any OpenAI-compatible `/chat/completions` — which is what Ollama, LM
//! Studio, llama.cpp's server, and OpenAI itself all speak.

use std::time::Duration;

use buzz_core_pkg::coding_session_title::{clean_generated_name, NAMING_SYSTEM_PROMPT};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use super::naming_mode::{
    effective_title_mode, fail_closed_in_dir, host_mode_mismatch, publish_title_mode,
    require_naming_model, validate_mode_with_provider, CodingSessionTitleMode,
};
use crate::app_state::keyring_service;
use crate::managed_agents::atomic_write_json_restricted;
use crate::secret_store::SecretStore;

/// One request's wall-clock budget. A namer that has not answered in this
/// long has already lost its race with the person typing.
const NAMING_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Anthropic's dated API version header. Pinned, not derived.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Ceiling on the reply. Adaptive thinking is on by default on current Claude
/// models, so this has to leave room for reasoning tokens as well as the four
/// words we actually want — a 64-token cap would return an empty text block.
const NAMING_MAX_TOKENS: u32 = 1024;

/// Longest goal worth showing on one line above a transcript.
const MAX_GENERATED_GOAL_CHARS: usize = 200;

/// The goal instruction: one sentence, in more detail than the name, so the
/// line above the transcript reminds the person what the session is for
/// without repeating the whole first message the transcript already shows.
const GOAL_SYSTEM_PROMPT: &str = "You summarize coding sessions. Given the first message a person sent to a coding agent, reply with one sentence of at most twenty words stating what the session is for. Reply with the sentence alone — no quotes, no heading, no explanation.";

/// Which of the two short answers a request is for. Same providers, same
/// keys, same transport; only the instruction and the cleaner differ.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NamingTask {
    Name,
    Goal,
}

impl NamingTask {
    fn system_prompt(self) -> &'static str {
        match self {
            NamingTask::Name => NAMING_SYSTEM_PROMPT,
            NamingTask::Goal => GOAL_SYSTEM_PROMPT,
        }
    }

    fn clean(self, raw: &str) -> Option<String> {
        match self {
            NamingTask::Name => clean_generated_name(raw),
            NamingTask::Goal => clean_generated_goal(raw),
        }
    }

    fn empty_answer(self) -> &'static str {
        match self {
            NamingTask::Name => "the model answered with no name",
            NamingTask::Goal => "the model answered with no goal",
        }
    }
}

fn keyring_name() -> &'static str {
    "coding-session-naming"
}

fn naming_secret_store() -> Option<&'static SecretStore> {
    if cfg!(feature = "system-keyring") {
        Some(SecretStore::shared(keyring_service()))
    } else {
        None
    }
}

/// Which endpoint, if any, names sessions on this computer.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum CodingSessionNamingProvider {
    /// Nothing is sent anywhere and the name field stays the person's own.
    #[default]
    Off,
    /// Anthropic's Messages API.
    Anthropic,
    /// Any `/chat/completions` endpoint — Ollama, LM Studio, llama.cpp,
    /// OpenAI, or a mesh ingress on this machine.
    #[serde(rename = "openai-compatible")]
    OpenAiCompatible,
}

/// The stored configuration, as it lives on disk.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct CodingSessionNamingRecord {
    /// Who titles a new session on this computer. Absent on a record saved
    /// before modes existed; [`effective_title_mode`] reads that case.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    title_mode: Option<CodingSessionTitleMode>,
    #[serde(default)]
    provider: CodingSessionNamingProvider,
    /// Endpoint root for the OpenAI-compatible adapter, e.g.
    /// `http://127.0.0.1:11434/v1`. Unused by the Anthropic adapter.
    #[serde(default)]
    base_url: String,
    #[serde(default)]
    model: String,
    /// Inline key. Empty (and omitted) whenever the keyring holds it — the
    /// same mechanism the provider record and managed agents use.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    api_key: String,
}

impl CodingSessionNamingRecord {
    /// The mode in force for this record.
    fn title_mode(&self) -> CodingSessionTitleMode {
        effective_title_mode(self.title_mode, self.provider)
    }

    /// Apply a save's mode and endpoint, and check the result can be stored.
    ///
    /// The mode is fixed *before* the endpoint changes, so a save that omits
    /// it keeps the mode in force rather than letting a new endpoint flip a
    /// legacy record into "my model". The endpoint is validated only when
    /// it will be used.
    fn apply_mode(
        &mut self,
        provider: CodingSessionNamingProvider,
        title_mode: Option<CodingSessionTitleMode>,
    ) -> Result<(), String> {
        let mode = title_mode.unwrap_or_else(|| self.title_mode());
        self.provider = provider;
        self.title_mode = Some(mode);
        validate_mode_with_provider(mode, provider)?;
        if mode == CodingSessionTitleMode::MyModel
            && provider == CodingSessionNamingProvider::OpenAiCompatible
        {
            validate_base_url(&self.base_url)?;
        }
        Ok(())
    }
}

/// The configuration as the settings surface may see it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionNamingSettings {
    /// The mode in force, never absent: a legacy record is read through
    /// [`effective_title_mode`].
    pub title_mode: CodingSessionTitleMode,
    pub provider: CodingSessionNamingProvider,
    pub base_url: String,
    pub model: String,
    /// Whether a key is stored. Never the key.
    pub has_api_key: bool,
    /// Set when this computer's agent host files do not say `title_mode`: a
    /// save or a provisioning could not write them, or one cannot be read.
    /// Read from the files on every settings read, so the card keeps saying
    /// it until a later write lands — never only for the life of one save.
    pub host_mode_mismatch: Option<String>,
}

fn naming_record_path(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_config_dir()
        .map_err(|error| format!("failed to resolve app config dir: {error}"))?;
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("failed to create app config dir: {error}"))?;
    Ok(dir.join("coding-session-naming.json"))
}

fn load_record(app: &AppHandle) -> Result<CodingSessionNamingRecord, String> {
    let path = naming_record_path(app)?;
    if !path.exists() {
        return Ok(CodingSessionNamingRecord::default());
    }
    let content = std::fs::read_to_string(&path)
        .map_err(|error| format!("failed to read coding-session naming settings: {error}"))?;
    serde_json::from_str(&content)
        .map_err(|error| format!("failed to parse coding-session naming settings: {error}"))
}

fn save_record(app: &AppHandle, record: &CodingSessionNamingRecord) -> Result<(), String> {
    let payload = serde_json::to_vec_pretty(record)
        .map_err(|error| format!("failed to serialize coding-session naming settings: {error}"))?;
    atomic_write_json_restricted(&naming_record_path(app)?, &payload)
}

/// The stored key, from the keyring when there is one and the record file
/// otherwise. `None` means no key has been set.
fn stored_api_key(record: &CodingSessionNamingRecord) -> Option<String> {
    stored_api_key_in(record, naming_secret_store())
}

/// [`stored_api_key`] against an explicit store.
///
/// The store is a parameter so the mapping can be exercised with `None` — a
/// unit test must never reach the OS keychain, which blocks on an access
/// prompt nobody is there to answer and hangs the run.
fn stored_api_key_in(
    record: &CodingSessionNamingRecord,
    secrets: Option<&SecretStore>,
) -> Option<String> {
    if let Some(secrets) = secrets {
        if let Ok(Some(key)) = secrets.load(keyring_name()) {
            if !key.is_empty() {
                return Some(key);
            }
        }
    }
    if record.api_key.is_empty() {
        None
    } else {
        Some(record.api_key.clone())
    }
}

fn settings_from(record: &CodingSessionNamingRecord) -> CodingSessionNamingSettings {
    settings_from_in(record, naming_secret_store())
}

/// [`settings_from`] against an explicit store — see [`stored_api_key_in`].
fn settings_from_in(
    record: &CodingSessionNamingRecord,
    secrets: Option<&SecretStore>,
) -> CodingSessionNamingSettings {
    CodingSessionNamingSettings {
        title_mode: record.title_mode(),
        provider: record.provider,
        base_url: record.base_url.clone(),
        model: record.model.clone(),
        has_api_key: stored_api_key_in(record, secrets).is_some(),
        host_mode_mismatch: None,
    }
}

/// What names sessions on this computer, and whether a key is on file.
#[tauri::command]
pub async fn coding_session_naming_settings(
    app: AppHandle,
) -> Result<CodingSessionNamingSettings, String> {
    let record = load_record(&app)?;
    let mut settings = settings_from(&record);
    settings.host_mode_mismatch = host_mode_mismatch(&app, settings.title_mode);
    Ok(settings)
}

/// Store a new configuration.
///
/// `api_key` is three-state on purpose: `None` leaves the stored key alone
/// (so saving a model change does not require re-typing it), `Some("")`
/// deletes it, and `Some(key)` replaces it. `title_mode` `None` leaves the
/// mode in force unchanged.
///
/// After the record is stored, the mode is written to every agent-host
/// identity on this computer. A failure there is not an `Err`: the record
/// (and the key) already changed, so the result is the stored settings with
/// `host_mode_mismatch` saying the host was not told. The card then shows
/// what is stored and the warning together, and the settings read keeps
/// showing it until a later write lands. An `Err` means the record was not
/// saved — though a key change may already have reached the keychain, so
/// the card re-reads after any error.
#[tauri::command]
pub async fn set_coding_session_naming_settings(
    app: AppHandle,
    provider: CodingSessionNamingProvider,
    title_mode: Option<CodingSessionTitleMode>,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
) -> Result<CodingSessionNamingSettings, String> {
    let mut record = load_record(&app)?;
    if let Some(base_url) = base_url {
        record.base_url = base_url.trim().trim_end_matches('/').to_string();
    }
    if let Some(model) = model {
        record.model = model.trim().to_string();
    }
    // Validated before the key is touched, so a refused save changes nothing.
    record.apply_mode(provider, title_mode)?;
    if let Some(api_key) = api_key {
        let api_key = api_key.trim().to_string();
        match naming_secret_store() {
            Some(secrets) if api_key.is_empty() => {
                secrets.delete(keyring_name())?;
                record.api_key.clear();
            }
            Some(secrets) => {
                secrets.store(keyring_name(), &api_key)?;
                // The keyring is now authoritative; do not leave a copy in a
                // file that a backup or a sync client might carry off.
                record.api_key.clear();
            }
            None => record.api_key = api_key,
        }
    }
    save_record(&app, &record)?;
    let mut settings = settings_from(&record);
    settings.host_mode_mismatch = match publish_title_mode(&app, settings.title_mode) {
        Ok(()) => host_mode_mismatch(&app, settings.title_mode),
        Err(error) => Some(error),
    };
    Ok(settings)
}

/// Write this computer's stored session-title mode to every agent-host
/// identity here, at provisioning — before the host starts, so a host never
/// runs on a mode the person did not choose — failing closed for the
/// identity being provisioned.
///
/// Provisioning stays non-fatal, but a host whose file was never written
/// reads the default and titles sessions — so on any failure the new
/// identity's file is set to `off` unless it already holds the stored mode.
/// The returned error says both what failed and what the host was left on;
/// the Session titles card reads the same files and shows the mismatch.
///
/// # Errors
/// The stored mode did not reach every identity; the sentence says which
/// mode the new identity's host was left on.
pub(crate) fn publish_stored_title_mode_at_provisioning(
    app: &AppHandle,
    provider_pubkey: &str,
) -> Result<(), String> {
    let (wanted, error) = match load_record(app) {
        Ok(record) => {
            let mode = record.title_mode();
            match publish_title_mode(app, mode) {
                Ok(()) => return Ok(()),
                Err(error) => (Some(mode), error),
            }
        }
        Err(error) => (None, error),
    };
    let fallback = crate::session_provider::provider_state_dir(app, provider_pubkey)
        .and_then(|dir| fail_closed_in_dir(&dir, wanted));
    Err(match fallback {
        Ok(left_on) => format!(
            "{error}; the new identity's host is on {} until the mode is saved again",
            left_on.as_str()
        ),
        Err(fallback) => format!(
            "{error}; and its host could not be set to off either ({fallback}), so it may title sessions"
        ),
    })
}

/// Reject a base URL that is not an `http(s)` origin.
///
/// The person types this field, and a typo becomes an outbound request to
/// wherever the typo points — so it is checked before it is stored, not at
/// the first name that fails.
fn validate_base_url(base_url: &str) -> Result<(), String> {
    if base_url.is_empty() {
        return Err(
            "an OpenAI-compatible namer needs an API URL, e.g. http://127.0.0.1:11434/v1"
                .to_string(),
        );
    }
    let parsed = url::Url::parse(base_url)
        .map_err(|error| format!("that API URL could not be read: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("an API URL must start with http:// or https://".to_string());
    }
    if parsed.host_str().is_none() {
        return Err("that API URL names no host".to_string());
    }
    Ok(())
}

/// Reduce a model's reply to one line that can be a session goal.
///
/// Like buzz-core's [`clean_generated_name`] but a sentence keeps its full
/// stop: the goal is prose, and a period is how a line reads as finished
/// rather than cut.
pub fn clean_generated_goal(raw: &str) -> Option<String> {
    let first_line = raw.trim().lines().find(|line| !line.trim().is_empty())?;
    let mut goal = first_line.trim().to_string();
    for prefix in ["Goal:", "goal:", "Summary:", "summary:"] {
        if let Some(rest) = goal.strip_prefix(prefix) {
            goal = rest.trim().to_string();
        }
    }
    goal = goal
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == '*')
        .trim()
        .to_string();
    if goal.is_empty() {
        return None;
    }
    if goal.chars().count() > MAX_GENERATED_GOAL_CHARS {
        goal = goal.chars().take(MAX_GENERATED_GOAL_CHARS).collect();
        goal = goal.trim().to_string();
    }
    Some(goal)
}

#[derive(Deserialize)]
struct AnthropicResponse {
    #[serde(default)]
    content: Vec<AnthropicBlock>,
}

#[derive(Deserialize)]
struct AnthropicBlock {
    #[serde(default)]
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: String,
}

#[derive(Deserialize)]
struct OpenAiResponse {
    #[serde(default)]
    choices: Vec<OpenAiChoice>,
}

#[derive(Deserialize)]
struct OpenAiChoice {
    #[serde(default)]
    message: OpenAiMessage,
}

#[derive(Default, Deserialize)]
struct OpenAiMessage {
    #[serde(default)]
    content: String,
}

/// Turn a non-2xx response into the sentence a person can act on.
async fn http_failure(label: &str, response: reqwest::Response) -> String {
    let status = response.status();
    let body = response.text().await.unwrap_or_default();
    let detail = body.trim();
    let detail: String = detail.chars().take(300).collect();
    if detail.is_empty() {
        format!("{label} answered {status}")
    } else {
        format!("{label} answered {status}: {detail}")
    }
}

async fn name_via_anthropic(
    client: &reqwest::Client,
    task: NamingTask,
    model: &str,
    api_key: &str,
    first_message: &str,
) -> Result<String, String> {
    let response = client
        .post("https://api.anthropic.com/v1/messages")
        .header("x-api-key", api_key)
        .header("anthropic-version", ANTHROPIC_VERSION)
        .json(&serde_json::json!({
            "model": model,
            "max_tokens": NAMING_MAX_TOKENS,
            // Low effort rather than disabled thinking: on current Claude
            // models thinking is on by default, and turning it off is the
            // documented way to get stray tags in the visible text.
            "output_config": { "effort": "low" },
            "system": task.system_prompt(),
            "messages": [{ "role": "user", "content": first_message }],
        }))
        .send()
        .await
        .map_err(|error| format!("could not reach the Anthropic API: {error}"))?;
    if !response.status().is_success() {
        return Err(http_failure("The Anthropic API", response).await);
    }
    let parsed: AnthropicResponse = response
        .json()
        .await
        .map_err(|error| format!("could not read the Anthropic API's answer: {error}"))?;
    let text = parsed
        .content
        .iter()
        .filter(|block| block.block_type == "text")
        .map(|block| block.text.as_str())
        .collect::<Vec<_>>()
        .join("");
    task.clean(&text)
        .ok_or_else(|| task.empty_answer().to_string())
}

async fn name_via_openai_compatible(
    client: &reqwest::Client,
    task: NamingTask,
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    first_message: &str,
) -> Result<String, String> {
    let mut request = client.post(format!("{base_url}/chat/completions"));
    // A local Ollama or llama.cpp server wants no key at all; sending an
    // empty bearer token is how you get a 401 from something that would
    // otherwise have answered.
    if let Some(api_key) = api_key {
        request = request.bearer_auth(api_key);
    }
    let response = request
        .json(&serde_json::json!({
            "model": model,
            "stream": false,
            "messages": [
                { "role": "system", "content": task.system_prompt() },
                { "role": "user", "content": first_message },
            ],
        }))
        .send()
        .await
        .map_err(|error| format!("could not reach {base_url}: {error}"))?;
    if !response.status().is_success() {
        return Err(http_failure("That API URL", response).await);
    }
    let parsed: OpenAiResponse = response
        .json()
        .await
        .map_err(|error| format!("could not read that API's answer: {error}"))?;
    let text = parsed
        .choices
        .first()
        .map(|choice| choice.message.content.as_str())
        .unwrap_or_default();
    task.clean(text)
        .ok_or_else(|| task.empty_answer().to_string())
}

/// Run one naming request against an explicit configuration.
///
/// Separate from the stored record so the settings surface can try what is
/// *in its fields* rather than what was last saved — the two differ exactly
/// when someone is fixing a URL, which is when a test is worth having.
async fn name_with(
    provider: CodingSessionNamingProvider,
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    first_message: &str,
) -> Result<String, String> {
    answer_with(
        NamingTask::Name,
        provider,
        base_url,
        model,
        api_key,
        first_message,
    )
    .await
}

/// One request for either short answer against an explicit configuration.
async fn answer_with(
    task: NamingTask,
    provider: CodingSessionNamingProvider,
    base_url: &str,
    model: &str,
    api_key: Option<&str>,
    first_message: &str,
) -> Result<String, String> {
    if provider == CodingSessionNamingProvider::Off || model.is_empty() {
        return Err("no naming model is configured".to_string());
    }
    let client = reqwest::Client::builder()
        .timeout(NAMING_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("could not build an HTTP client: {error}"))?;
    match provider {
        CodingSessionNamingProvider::Off => unreachable!("checked above"),
        CodingSessionNamingProvider::Anthropic => {
            let Some(api_key) = api_key else {
                return Err("the Anthropic API needs an API key".to_string());
            };
            name_via_anthropic(&client, task, model, api_key, first_message).await
        }
        CodingSessionNamingProvider::OpenAiCompatible => {
            validate_base_url(base_url)?;
            name_via_openai_compatible(&client, task, base_url, model, api_key, first_message).await
        }
    }
}

/// Ask the configured model for a short name for this first message.
///
/// Errors are returned rather than swallowed so the dialog can say *why* a
/// name never appeared — a namer that fails silently is indistinguishable
/// from one that is off, and the person configured it precisely because they
/// wanted the difference to be visible.
#[tauri::command]
pub async fn generate_coding_session_name(
    app: AppHandle,
    first_message: String,
) -> Result<String, String> {
    let first_message = first_message.trim().to_string();
    if first_message.is_empty() {
        return Err("there is no first message to name".to_string());
    }
    let record = load_record(&app)?;
    // Before anything is built: in any mode but "my model" the person was
    // told nothing is sent, and that has to be true here, not in the webview.
    require_naming_model(record.title_mode())?;
    name_with(
        record.provider,
        &record.base_url,
        &record.model,
        stored_api_key(&record).as_deref(),
        &first_message,
    )
    .await
}

/// The message a test sends.
///
/// A fixed, obviously-synthetic sentence, and never anything the person has
/// written: pressing Test to find out whether an endpoint works should not be
/// the moment a real first message leaves the machine. It is also long enough
/// and specific enough that a working model returns a recognisably *derived*
/// title rather than a generic one, which is what makes the result readable
/// as a yes.
pub const NAMING_TEST_MESSAGE: &str =
    "The relay rejects a git push that takes longer than sixty seconds, because the \
     credential helper's token has already expired by the time the push finishes.";

/// What one test run produced.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionNamingTest {
    /// The sample that was sent, so the surface can show what left the machine.
    pub sent: String,
    /// The name that came back, cleaned exactly as a real one would be.
    pub name: String,
    /// Round trip in milliseconds.
    ///
    /// Reported because this call repeats every few seconds while someone
    /// writes: a namer that takes eight seconds technically works and is
    /// still the wrong choice, and no other surface would ever show that.
    pub elapsed_ms: u64,
}

/// Try a naming configuration without storing it.
///
/// `api_key` is the same three-state value [`set_coding_session_naming_settings`]
/// takes: `None` means "use the stored key", so a test of an unchanged
/// configuration does not require re-typing it.
/// Ask the configured model for a one-line goal for this first message.
///
/// The same model, key and transport as the namer — a person who set up one
/// summarizer has set up both. A Solo session's goal used to be the whole
/// first message, which the transcript already shows in full; this is the
/// one line that stands above it (Andy, 2026-09-14).
#[tauri::command]
pub async fn generate_coding_session_goal(
    app: AppHandle,
    first_message: String,
) -> Result<String, String> {
    let first_message = first_message.trim().to_string();
    if first_message.is_empty() {
        return Err("there is no first message to summarize".to_string());
    }
    let record = load_record(&app)?;
    require_naming_model(record.title_mode())?;
    answer_with(
        NamingTask::Goal,
        record.provider,
        &record.base_url,
        &record.model,
        stored_api_key(&record).as_deref(),
        &first_message,
    )
    .await
}

#[tauri::command]
pub async fn test_coding_session_naming(
    app: AppHandle,
    provider: CodingSessionNamingProvider,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
) -> Result<CodingSessionNamingTest, String> {
    let record = load_record(&app)?;
    let base_url = base_url
        .map(|value| value.trim().trim_end_matches('/').to_string())
        .unwrap_or(record.base_url.clone());
    let model = model
        .map(|value| value.trim().to_string())
        .unwrap_or(record.model.clone());
    let api_key = match api_key {
        Some(key) if !key.trim().is_empty() => Some(key.trim().to_string()),
        // An explicitly emptied field means "no key", which is a valid
        // configuration for a local model — not a reason to fall back to a
        // stored one the person is in the middle of clearing.
        Some(_) => None,
        None => stored_api_key(&record),
    };
    let started = std::time::Instant::now();
    let name = name_with(
        provider,
        &base_url,
        &model,
        api_key.as_deref(),
        NAMING_TEST_MESSAGE,
    )
    .await?;
    Ok(CodingSessionNamingTest {
        sent: NAMING_TEST_MESSAGE.to_string(),
        name,
        elapsed_ms: started.elapsed().as_millis().min(u128::from(u64::MAX)) as u64,
    })
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
