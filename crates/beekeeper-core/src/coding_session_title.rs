//! NIP-CSG § Generated title: provider-signed session titles (kind 44252) and
//! the one display-name resolver every reader shares.
//!
//! A generated title is a model's words, so it is never a person's name and is
//! never published as one: kind 44229 stays human-authored by definition
//! (`docs/nips/NIP-CSG.md`). The provider instance that ran the founder's
//! first turn signs the title with its own key, and readers accept it only
//! from a signer that is the provider authority of the execution its
//! `cs-target` names, inside the same umbrella. The relay checks structure
//! alone, exactly as it does for 44229; standing is the reader's fold.
//!
//! [`resolve_session_display_name`] ranks three tiers and never compares them
//! by time:
//!
//! 1. **person** — the latest valid founder-signed 44229;
//! 2. **generated** — the **earliest** valid 44252 from a standing signer, so
//!    a title never flips once shown;
//! 3. **fallback** — the founding execution's title, then
//!    [`UNTITLED_SESSION_NAME`].
//!
//! The rule in executable form, shared with the TypeScript and Dart readers,
//! is `conformance/session-display-name/` (`CONTRACT.md` and its vectors).

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::coding_session_command::{
    coding_session_target_key, CodingSessionTarget, MAX_IDENTIFIER_BYTES, MAX_SAFE_GENERATION,
};
use crate::coding_session_name::{
    validate_coding_session_name_content, validate_coding_session_name_parts,
    validate_coding_session_name_session_ref,
};
use crate::kind::{KIND_CODING_SESSION_GENERATED_TITLE, KIND_CODING_SESSION_NAME};

/// Exact version carried by the `cstl-v` tag.
pub const CODING_SESSION_TITLE_TAG_VERSION: &str = "cstl1-1";
/// Exact `schema` of a generated-title payload.
pub const CODING_SESSION_TITLE_SCHEMA: &str = "buzz-coding-session-title/v1";
/// Maximum UTF-8 byte length of a generated-title event's content.
///
/// The title itself is bounded at 256 bytes by the name rule; the rest is a
/// model id and two event ids, so 2 KiB is generous without being a channel
/// for anything else.
pub const MAX_CODING_SESSION_TITLE_CONTENT_BYTES: usize = 2048;
/// Maximum UTF-8 byte length of the `model` field.
pub const MAX_CODING_SESSION_TITLE_MODEL_BYTES: usize = 128;
/// The last fallback a reader shows when nothing names the session.
pub const UNTITLED_SESSION_NAME: &str = "Untitled session";
/// Longest generated name kept after cleaning, in characters.
pub const MAX_GENERATED_NAME_CHARS: usize = 64;

/// The title instruction a namer sends with the first message.
///
/// Deliberately terse and output-shaped: everything about "no quotes, no
/// trailing period" exists because a title with punctuation in it looks like
/// a bug in the field it lands in. Moved here from the desktop namer
/// (`desktop/src-tauri/src/coding_sessions/naming.rs`) so the host producer
/// and the desktop share one text.
pub const NAMING_SYSTEM_PROMPT: &str = "You name coding sessions. Given the first message a person sent to a coding agent, reply with a title of one to four words describing the task. Reply with the title alone — no quotes, no punctuation at the end, no explanation. Use sentence case.";

/// Answers a namer can give that name nothing: the placeholders readers
/// already show for an unnamed session. T3 Code discards its own placeholder
/// the same way (`ThreadTitleRegenerationService.ts`, "New thread").
const PLACEHOLDER_NAMES: [&str; 4] = [
    UNTITLED_SESSION_NAME,
    "Coding session",
    "New session",
    "New thread",
];

/// What the title was generated from. Closed: v1 knows only the first message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CodingSessionTitleBasis {
    /// The founder's first message, and nothing else.
    #[serde(rename = "first-message")]
    FirstMessage,
}

/// The strict JSON content of a kind 44252 event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodingSessionTitlePayload {
    /// Must equal [`CODING_SESSION_TITLE_SCHEMA`].
    pub schema: String,
    /// The generated title; the 44229 content rule applies.
    pub title: String,
    /// The model id the provider used.
    pub model: String,
    /// What the title summarises.
    pub basis: CodingSessionTitleBasis,
    /// The 44220 turn command whose text was summarised, or `null` for a
    /// create's initial turn. The key is required; only its value may be null.
    #[serde(deserialize_with = "deserialize_required_nullable")]
    pub source_command: Option<String>,
    /// The 44221 create event of the execution that ran the turn.
    pub create_event_id: String,
}

fn deserialize_required_nullable<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    Option::<String>::deserialize(deserializer)
}

impl CodingSessionTitlePayload {
    /// Validate every field after decoding.
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != CODING_SESSION_TITLE_SCHEMA {
            return Err("unsupported coding-session title schema".into());
        }
        validate_coding_session_name_content(&self.title)
            .map_err(|error| format!("coding-session title: {error}"))?;
        if self.model.trim().is_empty() {
            return Err("coding-session title model must not be empty".into());
        }
        if self.model.len() > MAX_CODING_SESSION_TITLE_MODEL_BYTES {
            return Err(format!(
                "coding-session title model exceeds {MAX_CODING_SESSION_TITLE_MODEL_BYTES} bytes"
            ));
        }
        if self.model.chars().any(char::is_control) {
            return Err("coding-session title model must not contain control characters".into());
        }
        if let Some(source) = &self.source_command {
            validate_event_id("sourceCommand", source)?;
        }
        validate_event_id("createEventId", &self.create_event_id)
    }
}

fn validate_event_id(field: &str, value: &str) -> Result<(), String> {
    let well_formed = value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if well_formed {
        Ok(())
    } else {
        Err(format!(
            "coding-session title {field} must be a 64-character lowercase hex event id"
        ))
    }
}

/// Decode and validate a generated-title content string.
pub fn parse_coding_session_title_content(
    content: &str,
) -> Result<CodingSessionTitlePayload, String> {
    if content.len() > MAX_CODING_SESSION_TITLE_CONTENT_BYTES {
        return Err(format!(
            "coding-session title content exceeds {MAX_CODING_SESSION_TITLE_CONTENT_BYTES} bytes"
        ));
    }
    let payload: CodingSessionTitlePayload = serde_json::from_str(content)
        .map_err(|error| format!("coding-session title content is not v1 JSON: {error}"))?;
    payload.validate()?;
    Ok(payload)
}

/// Decode a `cs-target` key written by [`coding_session_target_key`].
///
/// Strict: the result must re-encode to exactly the input, and every field
/// obeys the bounds a 44220 command's target does.
pub fn parse_coding_session_target_key(key: &str) -> Result<CodingSessionTarget, String> {
    const PREFIX: &str = "coding-session/v1|";
    let malformed = || "cs-target is not a coding-session/v1 target key".to_owned();
    let mut rest = key.strip_prefix(PREFIX).ok_or_else(malformed)?;
    let mut fields: Vec<&str> = Vec::with_capacity(4);
    while !rest.is_empty() {
        let (length, tail) = rest.split_once(':').ok_or_else(malformed)?;
        if length.is_empty() || !length.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(malformed());
        }
        let length: usize = length.parse().map_err(|_| malformed())?;
        let field = tail.get(..length).ok_or_else(malformed)?;
        fields.push(field);
        rest = tail.get(length..).ok_or_else(malformed)?;
    }
    let [driver, instance_id, session_id, generation] = fields.as_slice() else {
        return Err(malformed());
    };
    for (name, value) in [
        ("driver", driver),
        ("instanceId", instance_id),
        ("sessionId", session_id),
    ] {
        if value.trim().is_empty()
            || value.len() > MAX_IDENTIFIER_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(format!("cs-target {name} is empty, oversized or not text"));
        }
    }
    let generation: u64 = generation.parse().map_err(|_| malformed())?;
    if generation == 0 || generation > MAX_SAFE_GENERATION {
        return Err("cs-target generation must be a positive safe integer".into());
    }
    let target = CodingSessionTarget {
        driver: (*driver).to_owned(),
        instance_id: (*instance_id).to_owned(),
        session_id: (*session_id).to_owned(),
        generation,
    };
    if coding_session_target_key(&target) != key {
        return Err(malformed());
    }
    Ok(target)
}

/// A structurally valid kind 44252 event, decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CodingSessionTitleEnvelope {
    /// The `h` channel.
    pub channel_id: String,
    /// The `d` umbrella sessionRef.
    pub session_ref: String,
    /// The `cs-target` key of the execution whose provider signed the title.
    pub target_key: String,
    /// The decoded content.
    pub payload: CodingSessionTitlePayload,
}

/// Validate the exact ordered envelope of a generated title over an event's
/// parts: `h`, `d`, `cstl-v`, `cs-target`, each with exactly two fields, and
/// strict v1 JSON content.
pub fn validate_coding_session_title_parts(
    tags: &[&[String]],
    content: &str,
) -> Result<CodingSessionTitleEnvelope, String> {
    if tags.len() != 4 || tags.iter().any(|parts| parts.len() != 2) {
        return Err("coding-session title requires exactly four two-field tags".into());
    }
    if tags[0][0] != "h" || Uuid::parse_str(&tags[0][1]).is_err() {
        return Err("coding-session title first tag must be a channel UUID h tag".into());
    }
    if tags[1][0] != "d" {
        return Err("coding-session title second tag must be d=sessionRef".into());
    }
    validate_coding_session_name_session_ref(&tags[1][1])
        .map_err(|_| "coding-session title d tag must be a lowercase canonical UUID".to_owned())?;
    if tags[2][0] != "cstl-v" || tags[2][1] != CODING_SESSION_TITLE_TAG_VERSION {
        return Err("unsupported coding-session title tag version".into());
    }
    if tags[3][0] != "cs-target" {
        return Err("coding-session title fourth tag must be cs-target".into());
    }
    parse_coding_session_target_key(&tags[3][1])?;
    let payload = parse_coding_session_title_content(content)?;
    Ok(CodingSessionTitleEnvelope {
        channel_id: tags[0][1].clone(),
        session_ref: tags[1][1].clone(),
        target_key: tags[3][1].clone(),
        payload,
    })
}

/// [`validate_coding_session_title_parts`] over a signed event. The relay
/// calls this at ingest; it checks no signer standing.
pub fn validate_coding_session_title_envelope(
    event: &nostr::Event,
) -> Result<CodingSessionTitleEnvelope, String> {
    let tags: Vec<&[String]> = event.tags.iter().map(|tag| tag.as_slice()).collect();
    validate_coding_session_title_parts(&tags, &event.content)
}

/// Reduce a model's reply to something that can be a session name, or `None`
/// when nothing usable is left.
///
/// Models answer [`NAMING_SYSTEM_PROMPT`] well but not perfectly: a stray
/// quote, a trailing period, a "Title: " prefix, or an unasked-for second line
/// all show up. Trimming them here is cheaper than a longer prompt and does
/// not depend on the model obeying it. A reply that is one of the readers'
/// own placeholders names nothing and is discarded, and so is anything that
/// would not pass the 44229/44252 title rule.
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
    if PLACEHOLDER_NAMES
        .iter()
        .any(|placeholder| placeholder.eq_ignore_ascii_case(&name))
    {
        return None;
    }
    validate_coding_session_name_content(&name).ok()?;
    Some(name)
}

/// One event as the resolver reads it: a Nostr event without its signature.
///
/// Readers verify signatures before folding; the resolver judges shape and
/// standing only. This is also the shape of an event in the conformance
/// vectors, whose ids and signers are synthetic labels.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionNameRecord {
    /// Lowercase hex event id; the ordering tie-breaker.
    pub id: String,
    /// Lowercase hex signer pubkey.
    pub pubkey: String,
    /// Unix seconds.
    pub created_at: u64,
    /// Event kind.
    pub kind: u32,
    /// Tags, in order.
    pub tags: Vec<Vec<String>>,
    /// Content.
    pub content: String,
}

impl From<&nostr::Event> for SessionNameRecord {
    fn from(event: &nostr::Event) -> Self {
        Self {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            created_at: event.created_at.as_secs(),
            kind: u32::from(event.kind.as_u16()),
            tags: event
                .tags
                .iter()
                .map(|tag| tag.as_slice().to_vec())
                .collect(),
            content: event.content.clone(),
        }
    }
}

/// One execution of the umbrella, as the reader already knows it from the
/// provider-signed 44223 metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionExecutionAuthority {
    /// The exact `cs-target` key, generation included. A reader lists every
    /// generation it holds, so a title signed for generation 1 still stands
    /// after a restart moves the execution to generation 2.
    pub target_key: String,
    /// The provider pubkey that signs that generation's facts.
    pub provider_authority_pubkey: String,
}

/// What the resolver needs to know about the umbrella besides its events.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDisplayNameScope {
    /// The `h` channel UUID.
    pub channel_id: String,
    /// The umbrella sessionRef (`d`).
    pub session_ref: String,
    /// The genesis founder, when known. Without one no 44229 is a person's
    /// name: the resolver cannot say whose it is.
    pub founder_pubkey: Option<String>,
    /// The founding execution's 44223 title, when it has one.
    pub founding_execution_title: Option<String>,
    /// Every execution of the umbrella the reader holds.
    pub executions: Vec<SessionExecutionAuthority>,
}

/// Which tier named the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionDisplayNameOrigin {
    /// A founder-signed 44229.
    Person,
    /// A provider-signed 44252.
    Generated,
    /// The founding execution's title, or [`UNTITLED_SESSION_NAME`].
    Fallback,
}

/// Events the resolver set aside, so a reader can say so instead of hiding it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDisplayNameDiagnostics {
    /// Valid 44229 revisions for this session not signed by the founder.
    pub foreign_names: u32,
    /// Valid 44252 titles for this session whose signer is not the provider
    /// authority of the umbrella execution their `cs-target` names —
    /// including a signer with no execution in the umbrella at all.
    pub foreign_titles: u32,
    /// 44229/44252 events carrying this session's `h` and `d` that fail
    /// envelope validation.
    pub malformed: u32,
}

/// The resolved display name of one umbrella session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionDisplayName {
    /// The text to show.
    pub name: String,
    /// The tier that supplied it.
    pub origin: SessionDisplayNameOrigin,
    /// The model that generated it; `generated` only.
    pub model: Option<String>,
    /// The provider pubkey that signed it; `generated` only.
    pub signer_pubkey: Option<String>,
    /// What was set aside.
    pub diagnostics: SessionDisplayNameDiagnostics,
}

fn carries_tag(record: &SessionNameRecord, name: &str, value: &str) -> bool {
    record
        .tags
        .iter()
        .any(|tag| tag.len() >= 2 && tag[0] == name && tag[1] == value)
}

/// `(created_at, id)` — the deterministic order every 44229/44252 fold uses.
fn order_key(record: &SessionNameRecord) -> (u64, &str) {
    (record.created_at, record.id.as_str())
}

/// Resolve one umbrella's display name from its 44229 and 44252 events.
///
/// Events of other kinds, and events not carrying this session's `h` and `d`,
/// are ignored. Tiers are ranked, never compared by time: any valid founder
/// name beats any generated title, however old the name and new the title.
/// Within the generated tier the earliest valid title wins.
pub fn resolve_session_display_name<'a>(
    scope: &SessionDisplayNameScope,
    records: impl IntoIterator<Item = &'a SessionNameRecord>,
) -> SessionDisplayName {
    let founder = scope.founder_pubkey.as_deref().map(str::to_ascii_lowercase);
    let mut diagnostics = SessionDisplayNameDiagnostics::default();
    let mut person: Option<&SessionNameRecord> = None;
    let mut generated: Option<(&SessionNameRecord, CodingSessionTitlePayload)> = None;

    for record in records {
        if record.kind != KIND_CODING_SESSION_NAME
            && record.kind != KIND_CODING_SESSION_GENERATED_TITLE
        {
            continue;
        }
        if !carries_tag(record, "h", &scope.channel_id)
            || !carries_tag(record, "d", &scope.session_ref)
        {
            continue;
        }
        let tags: Vec<&[String]> = record.tags.iter().map(Vec::as_slice).collect();
        let signer = record.pubkey.to_ascii_lowercase();
        if record.kind == KIND_CODING_SESSION_NAME {
            if validate_coding_session_name_parts(&tags, &record.content).is_err() {
                diagnostics.malformed += 1;
                continue;
            }
            if founder.as_deref() != Some(signer.as_str()) {
                diagnostics.foreign_names += 1;
                continue;
            }
            if person.is_none_or(|best| order_key(record) > order_key(best)) {
                person = Some(record);
            }
            continue;
        }
        let envelope = match validate_coding_session_title_parts(&tags, &record.content) {
            Ok(envelope) => envelope,
            Err(_) => {
                diagnostics.malformed += 1;
                continue;
            }
        };
        let standing = scope.executions.iter().any(|execution| {
            execution.target_key == envelope.target_key
                && execution
                    .provider_authority_pubkey
                    .eq_ignore_ascii_case(&signer)
        });
        if !standing {
            diagnostics.foreign_titles += 1;
            continue;
        }
        if generated
            .as_ref()
            .is_none_or(|(best, _)| order_key(record) < order_key(best))
        {
            generated = Some((record, envelope.payload));
        }
    }

    if let Some(record) = person {
        return SessionDisplayName {
            name: record.content.clone(),
            origin: SessionDisplayNameOrigin::Person,
            model: None,
            signer_pubkey: None,
            diagnostics,
        };
    }
    if let Some((record, payload)) = generated {
        return SessionDisplayName {
            name: payload.title,
            origin: SessionDisplayNameOrigin::Generated,
            model: Some(payload.model),
            signer_pubkey: Some(record.pubkey.to_ascii_lowercase()),
            diagnostics,
        };
    }
    let name = scope
        .founding_execution_title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(UNTITLED_SESSION_NAME)
        .to_owned();
    SessionDisplayName {
        name,
        origin: SessionDisplayNameOrigin::Fallback,
        model: None,
        signer_pubkey: None,
        diagnostics,
    }
}

#[cfg(test)]
#[path = "coding_session_title_tests.rs"]
mod tests;
