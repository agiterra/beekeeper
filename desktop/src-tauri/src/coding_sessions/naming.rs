//! Naming a coding session from its first message, with a model of the
//! person's choosing.
//!
//! This is the one place in the create flow that can send what someone typed
//! to a machine that is not theirs, so three things are non-negotiable:
//!
//! 1. It is **off** until configured. No default endpoint, no default key, no
//!    quiet first request. A name is a convenience; a silent egress is not.
//! 2. The **API key never reaches the webview**. It is written here, stored in
//!    the OS keyring (falling back to the `0o600` record file on builds
//!    without one), and read only when a request is being built. The settings
//!    surface learns `hasApiKey`, never the key.
//! 3. The request carries **only the first message**. No workdir, no channel,
//!    no identity, no repository — none of which would improve a four-word
//!    title, and all of which would be a fact about this machine handed to a
//!    third party.
//!
//! Two adapters cover every endpoint worth naming: Anthropic's Messages API,
//! and any OpenAI-compatible `/chat/completions` — which is what Ollama, LM
//! Studio, llama.cpp's server, and OpenAI itself all speak.

use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

use crate::app_state::keyring_service;
use crate::managed_agents::atomic_write_json_restricted;
use crate::secret_store::SecretStore;

/// Longest name worth asking for: four words, and the wire cap the session
/// name event already enforces is far above it.
const MAX_GENERATED_NAME_CHARS: usize = 64;

/// One request's wall-clock budget. A namer that has not answered in this
/// long has already lost its race with the person typing.
const NAMING_REQUEST_TIMEOUT: Duration = Duration::from_secs(20);

/// Anthropic's dated API version header. Pinned, not derived.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Ceiling on the reply. Adaptive thinking is on by default on current Claude
/// models, so this has to leave room for reasoning tokens as well as the four
/// words we actually want — a 64-token cap would return an empty text block.
const NAMING_MAX_TOKENS: u32 = 1024;

/// The instruction. Deliberately terse and output-shaped: everything about
/// "no quotes, no trailing period" exists because a title with punctuation in
/// it looks like a bug in the field it lands in.
const NAMING_SYSTEM_PROMPT: &str = "You name coding sessions. Given the first message a person sent to a coding agent, reply with a title of one to four words describing the task. Reply with the title alone — no quotes, no punctuation at the end, no explanation. Use sentence case.";

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

/// The configuration as the settings surface may see it.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodingSessionNamingSettings {
    pub provider: CodingSessionNamingProvider,
    pub base_url: String,
    pub model: String,
    /// Whether a key is stored. Never the key.
    pub has_api_key: bool,
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
    if let Some(secrets) = naming_secret_store() {
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
    CodingSessionNamingSettings {
        provider: record.provider,
        base_url: record.base_url.clone(),
        model: record.model.clone(),
        has_api_key: stored_api_key(record).is_some(),
    }
}

/// What names sessions on this computer, and whether a key is on file.
#[tauri::command]
pub async fn coding_session_naming_settings(
    app: AppHandle,
) -> Result<CodingSessionNamingSettings, String> {
    Ok(settings_from(&load_record(&app)?))
}

/// Store a new configuration.
///
/// `api_key` is three-state on purpose: `None` leaves the stored key alone
/// (so saving a model change does not require re-typing it), `Some("")`
/// deletes it, and `Some(key)` replaces it.
#[tauri::command]
pub async fn set_coding_session_naming_settings(
    app: AppHandle,
    provider: CodingSessionNamingProvider,
    base_url: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
) -> Result<CodingSessionNamingSettings, String> {
    let mut record = load_record(&app)?;
    record.provider = provider;
    if let Some(base_url) = base_url {
        record.base_url = base_url.trim().trim_end_matches('/').to_string();
    }
    if let Some(model) = model {
        record.model = model.trim().to_string();
    }
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
    if record.provider == CodingSessionNamingProvider::OpenAiCompatible {
        validate_base_url(&record.base_url)?;
    }
    save_record(&app, &record)?;
    Ok(settings_from(&record))
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

/// Reduce a model's reply to something that can be a session name.
///
/// Models answer this prompt well but not perfectly: a stray quote, a
/// trailing period, a "Title: " prefix, or an unasked-for second line all
/// show up. Trimming them here is cheaper than a longer prompt and does not
/// depend on the model obeying it.
pub fn clean_generated_name(raw: &str) -> Option<String> {
    let first_line = raw.trim().lines().find(|line| !line.trim().is_empty())?;
    let mut name = first_line.trim().to_string();
    for prefix in ["Title:", "title:", "Name:", "name:"] {
        if let Some(rest) = name.strip_prefix(prefix) {
            name = rest.trim().to_string();
        }
    }
    name = name
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '`' || c == '*')
        .trim()
        .trim_end_matches(['.', '!', ',', ':', ';'])
        .trim()
        .to_string();
    if name.is_empty() {
        return None;
    }
    if name.chars().count() > MAX_GENERATED_NAME_CHARS {
        name = name.chars().take(MAX_GENERATED_NAME_CHARS).collect();
        name = name.trim().to_string();
    }
    Some(name)
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
            "system": NAMING_SYSTEM_PROMPT,
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
    clean_generated_name(&text).ok_or_else(|| "the model answered with no name".to_string())
}

async fn name_via_openai_compatible(
    client: &reqwest::Client,
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
                { "role": "system", "content": NAMING_SYSTEM_PROMPT },
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
    clean_generated_name(text).ok_or_else(|| "the model answered with no name".to_string())
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
    if record.provider == CodingSessionNamingProvider::Off {
        return Err("no naming model is configured".to_string());
    }
    if record.model.is_empty() {
        return Err("no naming model is configured".to_string());
    }
    let api_key = stored_api_key(&record);
    let client = reqwest::Client::builder()
        .timeout(NAMING_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| format!("could not build an HTTP client: {error}"))?;
    match record.provider {
        CodingSessionNamingProvider::Off => unreachable!("checked above"),
        CodingSessionNamingProvider::Anthropic => {
            let Some(api_key) = api_key else {
                return Err("the Anthropic API needs an API key".to_string());
            };
            name_via_anthropic(&client, &record.model, &api_key, &first_message).await
        }
        CodingSessionNamingProvider::OpenAiCompatible => {
            validate_base_url(&record.base_url)?;
            name_via_openai_compatible(
                &client,
                &record.base_url,
                &record.model,
                api_key.as_deref(),
                &first_message,
            )
            .await
        }
    }
}

#[cfg(test)]
#[path = "naming_tests.rs"]
mod tests;
