//! The start census: which stored lifecycle commands could a start of this
//! provider act on, and is that list complete?
//!
//! A one-shot read, run as `beekeeper-session-provider census` by
//! `beekeeper-host census`, branched in `main` before anything a provider start
//! does. It opens no websocket, takes no state lock, spawns no adapter, and
//! writes no file. Its only relay calls are two constant `POST /query` shapes.
//!
//! It is a reading at a moment, not a transaction, and not a promise: a command
//! stored after it is not on its list. It can over-report and it never reports
//! clean on a guess. See `plans/HOST_START_CENSUS_CONTRACT.md`.

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use beekeeper_core::coding_session_lifecycle_command::{
    decode_coding_session_lifecycle_command, CodingSessionLifecycleAction,
};
use nostr::Event;
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::config::Config;
use crate::state::{self, lifecycle_fingerprint, CensusState, LifecycleStartPhase};

/// Complete, and no row would act.
pub const EXIT_CLEAN: i32 = 0;
/// Complete, and some row may act: read the list.
pub const EXIT_ROWS: i32 = 10;
/// Incomplete: unknown channels, an unstable snapshot. Stops a cutover.
pub const EXIT_INCOMPLETE: i32 = 20;
/// Refused before any query.
pub const EXIT_REFUSED: i32 = 30;

/// What this reads, and what a start also does that it does not.
const SCOPE: &str = "kind 44221 lifecycle commands only";
const NOT_COVERED: [&str; 5] = [
    "kind 44220 turn commands stored since the floor, which a start runs and which can spend a model turn",
    "routed kind 9 mentions and conversation backfill, which can spend model turns",
    "host notices, host-step requests and stored answers",
    "CI continuations, steers and stops recovered from state",
    "anything stored after this read",
];

/// The caller's environment must not choose what counts as expired: a start under
/// the Host does not see the caller's shell. Both spellings, as `main` maps the old one.
const HORIZON_VARS: [&str; 2] = [
    "BEEKEEPER_CSP_COMMAND_HORIZON_SECS",
    "BUZZ_CSP_COMMAND_HORIZON_SECS",
];

pub(crate) fn horizon_overridden(lookup: impl Fn(&str) -> Option<std::ffi::OsString>) -> bool {
    HORIZON_VARS.iter().any(|name| lookup(name).is_some())
}

/// 64 lowercase hex: an event id, a pubkey.
fn hex64(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn canonical_uuid(value: &str) -> bool {
    Uuid::parse_str(value).is_ok_and(|id| id.to_string() == value)
}

/// A UUID, or short lowercase words. No word long enough to hold a key in any encoding.
fn safe_command_id(value: &str) -> bool {
    canonical_uuid(value)
        || (value.len() <= 128
            && value.split('-').all(|word| {
                !word.is_empty()
                    && word.len() <= 32
                    && word
                        .bytes()
                        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            }))
}

/// A catalog id with its bracketed modifiers: `sonnet[low]`, `opus[1m][high]`.
fn safe_model(value: &str) -> bool {
    let (base, modifiers) = value.split_once('[').map_or((value, ""), |(b, m)| (b, m));
    let word = |w: &str| {
        w.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-' || b == b'_')
    };
    !base.is_empty()
        && base.len() <= 40
        && word(base)
        && (modifiers.is_empty()
            || (modifiers.len() <= 40
                && modifiers.strip_suffix(']').is_some_and(|inner| {
                    inner.split("][").all(|m| {
                        !m.is_empty()
                            && m.bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
                    })
                })))
}

/// Keep a field only if it is what its name says. Otherwise name it, and print nothing of it.
fn checked(
    value: Option<&str>,
    field: &'static str,
    valid: fn(&str) -> bool,
    withheld: &mut Vec<&'static str>,
) -> Option<String> {
    let value = value?;
    if valid(value) {
        Some(value.to_owned())
    } else {
        withheld.push(field);
        None
    }
}

const LIFECYCLE_KIND: u32 = beekeeper_core::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND;
const MEMBERS_KIND: u32 = beekeeper_core::kind::KIND_NIP29_GROUP_MEMBERS;
const PAGE_SIZE: u32 = 500;
const PAGE_LIMIT: usize = 100;
const SNAPSHOT_TRIES: usize = 3;

/// What one read of the relay can answer. Two constant shapes, not a query
/// surface: the census asks for nothing else.
pub(crate) trait CensusReader {
    async fn page(&self, filter: Value, cursor: Option<(u64, String)>) -> Result<Vec<Value>, ()>;
}

struct RestReader(beekeeper_acp::relay::RestClient);

impl CensusReader for RestReader {
    async fn page(&self, filter: Value, cursor: Option<(u64, String)>) -> Result<Vec<Value>, ()> {
        self.0.query_page(filter, cursor).await.map_err(|_| ())
    }
}

fn lifecycle_filter(channel: Uuid, since: u64) -> Value {
    json!({"kinds":[LIFECYCLE_KIND],"#h":[channel.to_string()],"since":since,"limit":PAGE_SIZE})
}

fn members_filter(provider: &str) -> Value {
    json!({"kinds":[MEMBERS_KIND],"#p":[provider],"limit":PAGE_SIZE})
}

/// Read a filter to its end, by the relay's `(created_at, id)` keyset.
///
/// Ends only on an empty page. A short page is not the end: the relay may clamp
/// below what was asked. A page with no new id means the cursor is not moving,
/// which is never read as complete.
pub(crate) async fn read_all(
    reader: &impl CensusReader,
    filter: Value,
) -> Result<Vec<Value>, &'static str> {
    let mut seen = HashSet::new();
    let mut rows = Vec::new();
    let mut cursor = None;
    for _ in 0..PAGE_LIMIT {
        let page = reader
            .page(filter.clone(), cursor.clone())
            .await
            .map_err(|()| "QUERY_FAILED")?;
        if page.is_empty() {
            return Ok(rows);
        }
        let last = page.last().ok_or("QUERY_FAILED")?;
        let next = last["created_at"]
            .as_u64()
            .zip(last["id"].as_str().map(str::to_owned))
            .ok_or("MALFORMED_PAGE")?;
        let mut added = 0;
        for row in page {
            let id = row["id"].as_str().ok_or("MALFORMED_PAGE")?.to_owned();
            if seen.insert(id) {
                added += 1;
                rows.push(row);
            }
        }
        if added == 0 {
            return Err("TIMESTAMP_SATURATED");
        }
        cursor = Some(next);
    }
    Err("PAGE_LIMIT")
}

/// Every local file classification reads, with absence as a value.
#[derive(Default)]
pub(crate) struct LocalInputs {
    state: Option<Vec<u8>>,
    commands: Option<Vec<u8>>,
    refusals: Option<Vec<u8>>,
    routes: Option<Vec<u8>>,
    digests: BTreeMap<&'static str, Option<[u8; 32]>>,
}

fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, &'static str> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err("LOCAL_INPUT_UNREADABLE"),
    }
}

impl LocalInputs {
    /// The lock file's pid is here because a different owner is a different moment.
    /// Projects and seats are here because a start reads them, though no class does.
    pub(crate) fn read(
        dir: &Path,
        projects: Option<&PathBuf>,
        seats: Option<&PathBuf>,
    ) -> Result<Self, &'static str> {
        let mut inputs = Self {
            state: read_optional(&dir.join("state.json"))?,
            commands: read_optional(&dir.join("commands.jsonl"))?,
            refusals: read_optional(&dir.join("refusals.jsonl"))?,
            routes: read_optional(&dir.join("conversation-routes.json"))?,
            digests: BTreeMap::new(),
        };
        let digest = |bytes: &Option<Vec<u8>>| bytes.as_ref().map(|b| Sha256::digest(b).into());
        let lock = read_optional(&dir.join(state::LOCK_FILE))?;
        let projects = projects.map_or(Ok(None), |p| read_optional(p))?;
        let seats = seats.map_or(Ok(None), |p| read_optional(p))?;
        inputs.digests = BTreeMap::from([
            ("state", digest(&inputs.state)),
            ("commands", digest(&inputs.commands)),
            ("refusals", digest(&inputs.refusals)),
            ("routes", digest(&inputs.routes)),
            ("lock", digest(&lock)),
            ("projects", digest(&projects)),
            ("seats", digest(&seats)),
        ]);
        Ok(inputs)
    }
}

/// What this census saw a row to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Class {
    Invalid,
    Malformed,
    OtherProvider,
    Consumed,
    Refused,
    Expired,
    /// Reserved, not started: a start retries it.
    IntentBeforeEffects,
    /// Past that phase: the outcome is the provider's to say.
    IntentUncertain,
    /// The stored intent is for a different request under this id.
    IntentConflict,
    /// Nothing here rules it out. What it would do needs state only a start has.
    Candidate,
}

impl Class {
    pub(crate) fn may_act(self) -> bool {
        matches!(
            self,
            Self::IntentBeforeEffects
                | Self::IntentUncertain
                | Self::IntentConflict
                | Self::Candidate
        )
    }
}

/// One row. Field by field, from typed values: never a copy of the payload.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Row {
    channel: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    event_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    created_at: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    author: Option<String>,
    class: Class,
    span: &'static str,
    /// The fields this row has and will not print, as they are not what they should be.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    withheld: Vec<&'static str>,
    /// The id as stored, for matching only. Never printed.
    #[serde(skip)]
    stored_command_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    action: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    actor: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    target_generation: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    initial_turn: bool,
}

/// What classification needs, fixed for one pass.
pub(crate) struct Facts<'a> {
    pub provider: &'a str,
    pub now: u64,
    pub horizon: u64,
    pub state: &'a CensusState,
    pub consumed: &'a HashSet<String>,
    pub refused: &'a HashSet<String>,
}

/// Which part of the read a row is in.
fn span(created_at: u64, floor: u64, now: u64) -> &'static str {
    if created_at > now {
        "futureDated"
    } else if created_at < floor {
        "behindFloor"
    } else {
        "sinceFloor"
    }
}

/// Classify one stored row, in the order `decide_lifecycle` applies these.
pub(crate) fn classify(raw: &Value, channel: Uuid, floor: u64, facts: &Facts<'_>) -> Row {
    let created_at = raw["created_at"].as_u64().unwrap_or(0);
    let mut withheld = Vec::new();
    let event_id = checked(raw["id"].as_str(), "eventId", hex64, &mut withheld);
    let author = checked(raw["pubkey"].as_str(), "author", hex64, &mut withheld);
    let mut row = Row {
        channel,
        event_id,
        created_at: raw["created_at"].as_u64(),
        author,
        class: Class::Invalid,
        span: span(created_at, floor, facts.now),
        withheld,
        stored_command_id: None,
        command_id: None,
        action: None,
        actor: None,
        target_session: None,
        target_generation: None,
        model: None,
        initial_turn: false,
    };
    let Ok(event) = serde_json::from_value::<Event>(raw.clone()) else {
        return row;
    };
    let in_channel = event.tags.iter().any(|tag| {
        let tag = tag.as_slice();
        tag.first().is_some_and(|name| name == "h")
            && tag
                .get(1)
                .is_some_and(|value| *value == channel.to_string())
    });
    if beekeeper_core::verify_event(&event).is_err() || !in_channel {
        return row;
    }
    let Ok(payload) = decode_coding_session_lifecycle_command(&event.content) else {
        row.class = Class::Malformed;
        return row;
    };
    row.stored_command_id = Some(payload.command_id.clone());
    row.command_id = checked(
        Some(&payload.command_id),
        "commandId",
        safe_command_id,
        &mut row.withheld,
    );
    use CodingSessionLifecycleAction as A;
    let (action, authority, session) = match &payload.action {
        A::SessionCreate {
            provider_authority_pubkey,
            model,
            initial_turn,
            actor,
            ..
        } => {
            row.model = checked(model.as_deref(), "model", safe_model, &mut row.withheld);
            row.actor = checked(actor.as_deref(), "actor", hex64, &mut row.withheld);
            row.initial_turn = initial_turn.is_some();
            ("session.create", Some(provider_authority_pubkey), None)
        }
        A::SessionResume {
            provider_authority_pubkey,
            session,
            ..
        } => (
            "session.resume",
            Some(provider_authority_pubkey),
            Some(session),
        ),
        A::SessionRestart {
            provider_authority_pubkey,
            session,
            ..
        } => (
            "session.restart",
            Some(provider_authority_pubkey),
            Some(session),
        ),
        A::SessionRewind {
            provider_authority_pubkey,
            session,
            ..
        } => (
            "session.rewind",
            Some(provider_authority_pubkey),
            Some(session),
        ),
        A::SessionStop {
            provider_authority_pubkey,
            session,
            ..
        } => (
            "session.stop",
            Some(provider_authority_pubkey),
            Some(session),
        ),
        A::SessionHire { .. } => ("session.hire", None, None),
    };
    row.action = Some(action);
    if let Some(session) = session {
        row.target_session = checked(
            Some(&session.session_id),
            "targetSession",
            canonical_uuid,
            &mut row.withheld,
        );
        row.target_generation = Some(session.generation);
    }
    if !authority.is_some_and(|key| key.eq_ignore_ascii_case(facts.provider)) {
        row.class = Class::OtherProvider;
        return row;
    }
    let held = facts.state.dispositions.get(&payload.command_id);
    row.class = match held {
        Some(Some(held)) => {
            let this = lifecycle_fingerprint(channel, &event.pubkey.to_hex(), &event.content);
            if held.fingerprint != this {
                Class::IntentConflict
            } else if held.phase == LifecycleStartPhase::BeforeEffects {
                Class::IntentBeforeEffects
            } else {
                Class::IntentUncertain
            }
        }
        Some(None) => Class::Refused,
        None if facts.consumed.contains(&payload.command_id) => Class::Consumed,
        None if facts.refused.contains(&payload.command_id) => Class::Refused,
        None if created_at + facts.horizon < facts.now => Class::Expired,
        None => Class::Candidate,
    };
    row
}

/// A start reservation no row in the read answers for. A start acts on it from state,
/// so it counts whatever the relay shows. Its stored request is not re-read here.
fn held_row(command: &str, held: &state::HeldStart) -> Row {
    let mut withheld = Vec::new();
    let command_id = checked(Some(command), "commandId", safe_command_id, &mut withheld);
    let actor = checked(held.actor.as_deref(), "actor", hex64, &mut withheld);
    Row {
        channel: held.channel,
        event_id: None,
        created_at: None,
        author: None,
        class: if held.phase == LifecycleStartPhase::BeforeEffects {
            Class::IntentBeforeEffects
        } else {
            Class::IntentUncertain
        },
        span: "heldInState",
        withheld,
        stored_command_id: Some(command.to_owned()),
        command_id,
        action: Some("session.create"),
        actor,
        target_session: None,
        target_generation: None,
        model: None,
        initial_turn: held.initial_turn,
    }
}

/// One channel's standing in the document.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChannelRow {
    channel: Uuid,
    result: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    floor: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    floor_source: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    read_since: Option<u64>,
    rows: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Document {
    schema: &'static str,
    scope: &'static str,
    not_covered: [&'static str; 5],
    ordinary_turn_replay: &'static str,
    provider_pubkey: String,
    commit: String,
    read_at: u64,
    point_in_time: bool,
    snapshot_stable: bool,
    horizon_secs: u64,
    lookback_secs: u64,
    exit: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<&'static str>,
    channels: Vec<ChannelRow>,
    commands: Vec<Row>,
    counts: BTreeMap<Class, usize>,
}

/// The `since` of the lifecycle read a start would send for this floor.
pub(crate) fn lookback_since(floor: u64) -> u64 {
    floor
        .saturating_sub(beekeeper_acp::relay::SINCE_SKEW_SECS)
        .saturating_sub(crate::LIFECYCLE_REPLAY_OVERLAP_SECS)
}

/// One pass over the inputs it was given.
pub(crate) async fn survey(
    reader: &impl CensusReader,
    provider: &str,
    horizon: u64,
    now: u64,
    inputs: &LocalInputs,
) -> Result<(Vec<ChannelRow>, Vec<Row>), &'static str> {
    let census =
        state::census_state(inputs.state.as_deref()).map_err(|_| "LOCAL_INPUT_UNREADABLE")?;
    let consumed = state::census_ledger(inputs.commands.as_deref(), horizon, now);
    let refused = state::census_ledger(inputs.refusals.as_deref(), horizon, now);
    let routes: crate::conversation::ConversationRoutes = match inputs.routes.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes).map_err(|_| "LOCAL_INPUT_UNREADABLE")?,
        None => Default::default(),
    };
    let facts = Facts {
        provider,
        now,
        horizon,
        state: &census,
        consumed: &consumed,
        refused: &refused,
    };

    let members = read_all(reader, members_filter(provider)).await?;
    let mut member_channels = BTreeSet::new();
    for event in &members {
        for tag in event["tags"].as_array().into_iter().flatten() {
            if tag[0] == "d" {
                if let Some(channel) = tag[1].as_str().and_then(|v| v.parse::<Uuid>().ok()) {
                    member_channels.insert(channel);
                }
            }
        }
    }

    let mut known: BTreeSet<Uuid> = census.channels.clone();
    known.extend(routes.routes.iter().map(|route| route.channel_id));
    known.extend(member_channels.iter().copied());

    let mut channels = Vec::new();
    let mut commands = Vec::new();
    for channel in known {
        let watermark = census.watermarks.get(&channel).copied();
        let route_floor = crate::conversation::conversation_floor(&routes, channel);
        let floor = crate::replay_floor(watermark, Some(now), route_floor).unwrap_or(now);
        let source = if route_floor == Some(floor) && watermark != Some(floor) {
            "conversationRoute"
        } else if watermark.is_some() {
            "watermark"
        } else {
            "startupGrace"
        };
        let since = lookback_since(floor);
        let mut row = ChannelRow {
            channel,
            result: "complete",
            reason: None,
            floor: Some(floor),
            floor_source: Some(source),
            read_since: Some(since),
            rows: 0,
        };
        if !member_channels.contains(&channel) {
            // A start does not subscribe it. Unknown only if it could still matter.
            row.result = "notMember";
            let recent = watermark
                .into_iter()
                .chain(route_floor)
                .any(|mark| mark + horizon >= now);
            if recent || census.open_channels.contains(&channel) {
                row.result = "unknown";
                row.reason = Some("NOT_MEMBER_WITH_RECENT_FLOOR");
            }
            channels.push(row);
            continue;
        }
        match read_all(reader, lifecycle_filter(channel, since)).await {
            Ok(rows) => {
                row.rows = rows.len();
                commands.extend(rows.iter().map(|raw| classify(raw, channel, floor, &facts)));
            }
            Err(reason) => {
                row.result = "unknown";
                row.reason = Some(reason);
            }
        }
        channels.push(row);
    }
    let answered: HashSet<String> = commands
        .iter()
        .filter(|row| row.class.may_act())
        .filter_map(|row| row.stored_command_id.clone())
        .collect();
    for (command, held) in &census.dispositions {
        if let Some(held) = held.as_ref().filter(|_| !answered.contains(command)) {
            commands.push(held_row(command, held));
        }
    }
    Ok((channels, commands))
}

fn verdict(channels: &[ChannelRow], commands: &[Row]) -> (i32, Option<&'static str>) {
    if channels.iter().any(|row| row.result == "unknown") {
        (EXIT_INCOMPLETE, Some("UNKNOWN_CHANNELS"))
    } else if commands.iter().any(|row| row.class == Class::Invalid) {
        // A start does not re-verify what it is delivered, so this is not "cannot act".
        (EXIT_INCOMPLETE, Some("INVALID_ROW"))
    } else if commands.iter().any(|row| row.class.may_act()) {
        (EXIT_ROWS, None)
    } else {
        (EXIT_CLEAN, None)
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// Repeat until two reads of every local input around a pass agree.
pub(crate) async fn stable_survey(
    reader: &impl CensusReader,
    provider: &str,
    horizon: u64,
    read: impl Fn() -> Result<LocalInputs, &'static str>,
) -> Result<(u64, Vec<ChannelRow>, Vec<Row>), &'static str> {
    for attempt in 0..SNAPSHOT_TRIES {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }
        let now = now_secs();
        let before = read()?;
        let (channels, commands) = survey(reader, provider, horizon, now, &before).await?;
        if read()?.digests == before.digests {
            return Ok((now, channels, commands));
        }
    }
    Err("SNAPSHOT_UNSTABLE")
}

/// `beekeeper-session-provider census`: print one document, return the exit code.
pub async fn run() -> i32 {
    let finish = |document: Document| {
        println!(
            "{}",
            serde_json::to_string(&document).unwrap_or_else(|_| "{}".into())
        );
        document.exit
    };
    let empty = |provider: String, reason| Document {
        schema: "host-start-census/v1",
        scope: SCOPE,
        not_covered: NOT_COVERED,
        ordinary_turn_replay: "notRead",
        provider_pubkey: provider,
        commit: beekeeper_core::build_info::build_info().commit_display(),
        read_at: now_secs(),
        point_in_time: true,
        snapshot_stable: false,
        horizon_secs: 0,
        lookback_secs: 0,
        exit: EXIT_INCOMPLETE,
        reason: Some(reason),
        channels: Vec::new(),
        commands: Vec::new(),
        counts: BTreeMap::new(),
    };
    if horizon_overridden(|name| std::env::var_os(name)) {
        return finish(Document {
            exit: EXIT_REFUSED,
            ..empty(String::new(), "CENSUS_HORIZON_OVERRIDE")
        });
    }
    let Ok(config) = Config::from_env() else {
        return finish(empty(String::new(), "CONFIG_UNREADABLE"));
    };
    let provider = config.pubkey_hex();
    let auth_tag = config
        .auth_tag
        .as_ref()
        .and_then(|tag| serde_json::to_string(tag.as_slice()).ok());
    let Ok(rest) = beekeeper_acp::relay::RestClient::for_read(
        &config.relay_url,
        config.keys.clone(),
        auth_tag,
    ) else {
        return finish(empty(provider, "CONFIG_UNREADABLE"));
    };
    let horizon = config.command_horizon.as_secs();
    let read = || {
        LocalInputs::read(
            &config.state_dir,
            config.projects_file.as_ref(),
            config.actor_seats_file.as_ref(),
        )
    };
    match stable_survey(&RestReader(rest), &provider, horizon, read).await {
        Ok((now, channels, commands)) => {
            let (exit, reason) = verdict(&channels, &commands);
            let mut counts = BTreeMap::new();
            for row in &commands {
                *counts.entry(row.class).or_insert(0) += 1;
            }
            finish(Document {
                read_at: now,
                snapshot_stable: true,
                horizon_secs: horizon,
                lookback_secs: beekeeper_acp::relay::SINCE_SKEW_SECS
                    + crate::LIFECYCLE_REPLAY_OVERLAP_SECS,
                exit,
                reason,
                channels,
                commands,
                counts,
                ..empty(provider, "")
            })
        }
        Err(reason) => finish(empty(provider, reason)),
    }
}

#[cfg(test)]
#[path = "census_tests.rs"]
mod tests;
