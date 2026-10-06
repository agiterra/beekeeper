//! `bee pulse` — Project Pulse: the explicit coordination entries a project's
//! workers publish (kind 44240) folded together with the coding-session facts
//! their providers assert through lifecycle, metadata, lease, and umbrella facts.
//!
//! Three rules shape this module, and each of them is a rule about honesty
//! rather than convenience:
//!
//! 1. **A read failure is never a quiet project.** Every failed source query
//!    lands in the digest's `errors[]`, flips `complete` to false, and exits
//!    non-zero. A partial fold must never print as a complete digest with an
//!    empty session list.
//! 2. **The absence of a closure is not evidence of life.** Reachability needs
//!    a current, authority-bound, unexpired lease for the exact generation.
//! 3. **A peer can never quietly retract your claim.** Supersession is honored
//!    only within one author, so nobody can push another author's `blocker` out
//!    of the set that drives the `wait | consult | proceed` advisory.
//!
//! The fold's single source of truth is `conformance/project-pulse-fold/`;
//! [`fold_pulse_digest`] is bound to those vectors by a test at the bottom of
//! this file, and the Desktop and (Slice 2) relay folds bind to the same ones.

use std::collections::HashSet;
use std::str::FromStr;

use beekeeper_core::coding_session_command::coding_session_target_key;
use beekeeper_core::coding_session_payload::{
    decode_coding_session_metadata, TranscriptEnvelope, TurnUsageReport,
};
use beekeeper_core::kind::{
    normalize_project_coordinate, KIND_CODING_SESSION_CLOSURE, KIND_CODING_SESSION_GENERATED_TITLE,
    KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_GOAL, KIND_CODING_SESSION_LEASE,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME, KIND_CODING_SESSION_TRANSCRIPT,
    KIND_NIP29_GROUP_METADATA, KIND_PROJECT, KIND_PROJECT_MEMBERS, KIND_PULSE_ENTRY,
};
use beekeeper_core::pulse::{
    PulseCost, PulseCostSeat, PulseEntry, PulseEntryType, MAX_PULSE_TEXT_BYTES, PULSE_ENTRY_SCHEMA,
};
use beekeeper_core::pulse_fold::{
    fold_pulse_digest, PulseDigest, PulseDigestEntry, PulseDigestError, PulseDigestSession,
};
use serde_json::{json, Value};

#[cfg(test)]
use beekeeper_core::pulse_fold::{
    commit_confirmation, PulseGenerationReachability, PULSE_DIGEST_SCHEMA,
};
#[cfg(test)]
use serde::Deserialize;

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// The reserved `--branch` value selecting rows that carry no branch at all.
const NO_BRANCH: &str = "-";

/// Durable session facts fetched as paginated history per project channel.
///
/// The genesis (44226) is here for the name alone: a 44229 is a person's name
/// only when the founder signed it, and the founder is whoever signed the
/// genesis the umbrella's create names. A generated title (44252) is the
/// provider's, and the digest says so (`nameOrigin`).
const DURABLE_SESSION_FACT_KINDS: [u32; 8] = [
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_GOAL,
    KIND_CODING_SESSION_NAME,
    KIND_CODING_SESSION_CLOSURE,
    KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_GENERATED_TITLE,
];

/// Upper bound on the channel-metadata scan used to resolve a project's
/// channel set, matching `channels list`'s own default page budget.
const CHANNEL_SCAN_LIMIT: u32 = 500;

/// Upper bound on the project heads a bare-dtag resolution will consider.
const PROJECT_SCAN_LIMIT: u32 = 500;

/// `--branch` semantics, identical for `list`, `digest`, and Desktop's chips:
/// a named filter matches byte-exactly and never returns null-branch rows, the
/// reserved `-` returns only null-branch rows, and no filter returns
/// everything.
fn branch_matches(filter: Option<&str>, branch: Option<&str>) -> bool {
    match filter {
        None => true,
        Some(NO_BRANCH) => branch.is_none(),
        Some(name) => branch == Some(name),
    }
}

fn json_str<'a>(event: &'a Value, field: &str) -> Option<&'a str> {
    event.get(field).and_then(Value::as_str)
}

fn json_kind(event: &Value) -> Option<u32> {
    event
        .get("kind")
        .and_then(Value::as_u64)
        .and_then(|kind| u32::try_from(kind).ok())
}

fn json_tag_value<'a>(event: &'a Value, name: &str) -> Option<&'a str> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .find(|parts| parts.first().and_then(Value::as_str) == Some(name))
        .and_then(|parts| parts.get(1))
        .and_then(Value::as_str)
}

fn json_tag_values<'a>(event: &'a Value, name: &str) -> Vec<&'a str> {
    event
        .get("tags")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .filter(|parts| parts.first().and_then(Value::as_str) == Some(name))
        .filter_map(|parts| parts.get(1).and_then(Value::as_str))
        .collect()
}

// ── Relay reads ──────────────────────────────────────────────────────────────

/// The Pulse-entry filter: kind 44240 alone, scoped to exactly one project
/// coordinate.
///
/// 44240 is always queried in its own filter. Mixing it with the session kinds
/// "for efficiency" satisfies the bridge's `#a` rule and still returns only
/// 44240 events — 44223 carries no `a` tag — so the caller gets an empty
/// session list with no error.
fn entries_filter(coordinate: &str, since: Option<u64>) -> Value {
    let mut filter = json!({ "kinds": [KIND_PULSE_ENTRY], "#a": [coordinate] });
    if let Some(since) = since {
        filter["since"] = json!(since);
    }
    filter
}

/// The per-channel session-fact filter. `kinds` is never omitted: an
/// open-ended filter trips the relay's p-gate and comes back 403.
fn durable_session_facts_filter(channel_id: &str) -> Value {
    json!({ "kinds": DURABLE_SESSION_FACT_KINDS, "#h": [channel_id] })
}

/// One cold snapshot of the relay's ephemeral per-generation lease keys.
///
/// This deliberately has no history limit and is never passed to
/// `query_all`: a second page would treat an ephemeral Redis snapshot like a
/// durable event log and can join lease states from different instants.
fn session_lease_filter(channel_id: &str) -> Value {
    json!({ "kinds": [KIND_CODING_SESSION_LEASE], "#h": [channel_id] })
}

/// Fetch a project's Pulse entries, reporting whether `--limit` truncated them.
async fn fetch_entry_events(
    client: &BeekeeperClient,
    coordinate: &str,
    since: Option<u64>,
    limit: Option<u32>,
) -> Result<(Vec<Value>, bool), CliError> {
    let filter = entries_filter(coordinate, since);
    match limit {
        Some(limit) => {
            let events = client.query_paginated(filter, limit).await?;
            let truncated = events.len() as u32 >= limit;
            Ok((events, truncated))
        }
        None => Ok((client.query_all(filter).await?, false)),
    }
}

/// Resolve the channels a project's coding sessions can be reached through:
/// the channels whose kind:39000 metadata carries this project's coordinate,
/// plus the `channel` references on the project head itself.
/// Resolve the project's channel set, plus whether the metadata scan hit
/// [`CHANNEL_SCAN_LIMIT`].
///
/// The truncation flag is not cosmetic: `query_paginated` stops as soon as it
/// has `limit` events, so in a community with 500+ kind:39000 events the scan
/// silently returns one page and every project channel outside it is never
/// queried for session facts. A digest that then printed `"complete": true`
/// with an empty `sessions` list would tell an agent the project is quiet
/// while a live session runs in a channel it never looked at.
async fn project_channel_ids(
    client: &BeekeeperClient,
    coordinate: &str,
) -> Result<(Vec<String>, bool), CliError> {
    let mut channels: HashSet<String> = HashSet::new();

    if let Some((owner, dtag)) = split_project_coordinate(coordinate) {
        let head = client
            .query(&json!({
                "kinds": [KIND_PROJECT],
                "authors": [owner],
                "#d": [dtag],
                "limit": 1,
            }))
            .await?;
        let heads: Vec<Value> = serde_json::from_str(&head)
            .map_err(|e| CliError::Other(format!("failed to parse relay response: {e}")))?;
        for head in &heads {
            for channel in json_tag_values(head, "channel") {
                channels.insert(channel.to_owned());
            }
            if let Some(channel) = json_tag_value(head, "buzz-channel") {
                channels.insert(channel.to_owned());
            }
        }
    }

    let metadata = client
        .query_paginated(
            json!({ "kinds": [KIND_NIP29_GROUP_METADATA] }),
            CHANNEL_SCAN_LIMIT,
        )
        .await?;
    for event in &metadata {
        // Normalize before comparing: the relay's `validate_project_ref_tag`
        // accepts any `is_ascii_hexdigit()` pubkey, so `channels.project_ref` —
        // and the 39000 `project` tag projected from it — can hold an
        // upper-case-hex coordinate. A byte-exact match would drop that
        // channel from the set, never query its session facts, and still print
        // `"complete": true` — the same quiet-project lie the truncation guard
        // above exists to prevent. Desktop already normalizes here.
        if json_tag_value(event, "project")
            .and_then(normalize_project_coordinate)
            .as_deref()
            == Some(coordinate)
        {
            if let Some(channel) = json_tag_value(event, "d") {
                channels.insert(channel.to_owned());
            }
        }
    }

    let truncated = metadata.len() as u32 >= CHANNEL_SCAN_LIMIT;

    let mut channels: Vec<String> = channels.into_iter().collect();
    channels.sort();
    Ok((channels, truncated))
}

/// The events and failures of one project's session scan.
struct SessionScan {
    events: Vec<Value>,
    errors: Vec<PulseDigestError>,
    /// The first real transport failure, kept so the caller can exit on the
    /// relay's own error rather than a manufactured one.
    failure: Option<CliError>,
}

/// Fetch every coding-session fact reachable through the project's channels.
///
/// A channel that fails is recorded and the scan continues: a partial read is
/// reported as partial, never silently narrowed to the channels that answered.
async fn scan_project_sessions(client: &BeekeeperClient, coordinate: &str) -> SessionScan {
    let mut scan = SessionScan {
        events: Vec::new(),
        errors: Vec::new(),
        failure: None,
    };
    let channels = match project_channel_ids(client, coordinate).await {
        Ok((channels, truncated)) => {
            if truncated {
                // A limit condition, not a relay failure: recorded in
                // `errors[]` (so `complete` goes false) while `scan.failure`
                // stays `None`, exactly as the entries read does.
                scan.errors.push(PulseDigestError {
                    scope: "channels".to_owned(),
                    message: format!(
                        "channel scan truncated at {CHANNEL_SCAN_LIMIT} events; some project channels were not queried"
                    ),
                });
            }
            channels
        }
        Err(error) => {
            scan.errors.push(PulseDigestError {
                scope: "channels".to_owned(),
                message: error.to_string(),
            });
            scan.failure = Some(error);
            return scan;
        }
    };
    for channel in channels {
        match client
            .query_all(durable_session_facts_filter(&channel))
            .await
        {
            Ok(events) => scan.events.extend(events),
            Err(error) => {
                scan.errors.push(PulseDigestError {
                    scope: format!("sessions:{channel}"),
                    message: error.to_string(),
                });
                if scan.failure.is_none() {
                    scan.failure = Some(error);
                }
            }
        }
        match client.query(&session_lease_filter(&channel)).await {
            Ok(body) => match serde_json::from_str::<Vec<Value>>(&body) {
                Ok(events) => scan.events.extend(events),
                Err(error) => {
                    scan.errors.push(PulseDigestError {
                        scope: format!("leases:{channel}"),
                        message: format!("failed to parse lease snapshot: {error}"),
                    });
                    if scan.failure.is_none() {
                        scan.failure = Some(CliError::Other(format!(
                            "failed to parse lease snapshot: {error}"
                        )));
                    }
                }
            },
            Err(error) => {
                scan.errors.push(PulseDigestError {
                    scope: format!("leases:{channel}"),
                    message: error.to_string(),
                });
                if scan.failure.is_none() {
                    scan.failure = Some(error);
                }
            }
        }
    }
    scan
}

/// Compose a full digest from the relay: entries by `#a`, sessions by `#h`.
///
/// Returns the digest plus the first transport failure, if any, so the caller
/// can print the (partial) digest *and* exit on the relay's own error.
async fn compose_digest(
    client: &BeekeeperClient,
    coordinate: &str,
    limit: Option<u32>,
) -> (PulseDigest, Option<CliError>) {
    let mut events: Vec<Value> = Vec::new();
    let mut errors: Vec<PulseDigestError> = Vec::new();
    let mut failure: Option<CliError> = None;

    match fetch_entry_events(client, coordinate, None, limit).await {
        Ok((entries, truncated)) => {
            events.extend(entries);
            if truncated {
                errors.push(PulseDigestError {
                    scope: "entries".to_owned(),
                    message: format!(
                        "entry read truncated at --limit {}; more entries may exist",
                        limit.unwrap_or_default()
                    ),
                });
            }
        }
        Err(error) => {
            errors.push(PulseDigestError {
                scope: "entries".to_owned(),
                message: error.to_string(),
            });
            failure = Some(error);
        }
    }

    let scan = scan_project_sessions(client, coordinate).await;
    events.extend(scan.events);
    errors.extend(scan.errors);
    if failure.is_none() {
        failure = scan.failure;
    }

    // `asOf` is the wall clock read after the last source query returned.
    let now = chrono::Utc::now().timestamp();
    (fold_pulse_digest(coordinate, now, errors, &events), failure)
}

/// The error a partial digest exits with.
///
/// A transport failure exits on the relay's own error (2, or 3 for an auth
/// status). A read the caller's own `--limit` truncated has no underlying
/// error, and is reported as the usage condition it is — raise `--limit` — so
/// the CLI never manufactures a relay status the relay never returned.
fn partial_digest_error(failure: Option<CliError>) -> CliError {
    failure.unwrap_or_else(|| {
        CliError::Usage(
            "digest is incomplete: the entry read was truncated by --limit; \
             raise --limit for a complete digest"
                .to_owned(),
        )
    })
}

// ── Project resolution ───────────────────────────────────────────────────────

/// Split a canonical coordinate into `(owner-hex, dtag)`.
fn split_project_coordinate(coordinate: &str) -> Option<(&str, &str)> {
    let mut parts = coordinate.splitn(3, ':');
    let kind = parts.next()?;
    let owner = parts.next()?;
    let dtag = parts.next()?;
    (kind == "30621").then_some((owner, dtag))
}

/// Resolve `--project` / `BUZZ_PULSE_PROJECT` into a canonical coordinate.
///
/// Precedence is the explicit flag, then the environment variable (clap
/// applies it), then an actionable usage error. A value containing `:` must
/// already be a full `30621:<owner-hex>:<dtag>` coordinate; a bare dtag
/// resolves only when exactly one **visible** project matches, because
/// guessing an owner would silently write another person's project.
pub(crate) async fn resolve_project(
    client: &BeekeeperClient,
    project: Option<&str>,
) -> Result<String, CliError> {
    if let Some(coordinate) = direct_project_coordinate(project)? {
        return Ok(coordinate);
    }
    // `direct_project_coordinate` returns `None` only for a bare dtag, and only
    // after proving the argument was supplied.
    let dtag = project.unwrap_or_default();
    let candidates = visible_project_coordinates(client, dtag).await?;
    resolve_bare_dtag(dtag, candidates)
}

/// The half of project resolution that needs no relay: a missing argument is an
/// actionable usage error, a value containing `:` must already be a canonical
/// coordinate, and a bare dtag returns `None` for the caller to look up.
fn direct_project_coordinate(project: Option<&str>) -> Result<Option<String>, CliError> {
    let Some(project) = project else {
        return Err(CliError::Usage(
            "--project is required: pass a `30621:<owner-hex>:<dtag>` coordinate or a project \
             dtag, or set BUZZ_PULSE_PROJECT"
                .to_owned(),
        ));
    };
    if !project.contains(':') {
        return Ok(None);
    }
    normalize_project_coordinate(project)
        .map(Some)
        .ok_or_else(|| {
            CliError::Usage(format!(
                "--project must be a `30621:<owner-hex>:<dtag>` coordinate or a project dtag \
                 (got {project:?})"
            ))
        })
}

/// Every project coordinate visible to the caller whose dtag equals `dtag`.
///
/// The three queries are the whole visible set: projects the caller owns, the
/// roster projections naming the caller, and the heads those projections point
/// at. `projects list` cannot answer this — it always queries one explicit
/// author.
async fn visible_project_coordinates(
    client: &BeekeeperClient,
    dtag: &str,
) -> Result<Vec<String>, CliError> {
    let self_hex = client.keys().public_key().to_hex();
    let mut heads = client
        .query_paginated(
            json!({ "kinds": [KIND_PROJECT], "authors": [self_hex] }),
            PROJECT_SCAN_LIMIT,
        )
        .await?;

    let rosters = client
        .query_paginated(
            json!({ "kinds": [KIND_PROJECT_MEMBERS], "#p": [self_hex] }),
            PROJECT_SCAN_LIMIT,
        )
        .await?;
    let mut owners: Vec<String> = rosters
        .iter()
        .filter_map(|event| json_tag_value(event, "d"))
        .filter_map(normalize_project_coordinate)
        .filter_map(|coordinate| {
            split_project_coordinate(&coordinate).map(|(owner, _)| owner.to_owned())
        })
        .collect();
    owners.sort();
    owners.dedup();
    owners.retain(|owner| *owner != self_hex);
    if !owners.is_empty() {
        heads.extend(
            client
                .query_paginated(
                    json!({ "kinds": [KIND_PROJECT], "authors": owners }),
                    PROJECT_SCAN_LIMIT,
                )
                .await?,
        );
    }

    let mut coordinates: Vec<String> = heads
        .iter()
        .filter(|event| json_tag_value(event, "d") == Some(dtag))
        .filter_map(|event| {
            let author = json_str(event, "pubkey")?;
            normalize_project_coordinate(&format!("30621:{author}:{dtag}"))
        })
        .collect();
    coordinates.sort();
    coordinates.dedup();
    Ok(coordinates)
}

/// Pick the one visible project a bare dtag names, or say why it cannot.
fn resolve_bare_dtag(dtag: &str, candidates: Vec<String>) -> Result<String, CliError> {
    match candidates.len() {
        1 => Ok(candidates.into_iter().next().unwrap_or_default()),
        0 => Err(CliError::Usage(format!("no visible project named {dtag}"))),
        _ => Err(CliError::Usage(format!(
            "project dtag {dtag} is ambiguous — pass one of these coordinates to --project: {}",
            candidates.join(", ")
        ))),
    }
}

// ── Cost ─────────────────────────────────────────────────────────────────────

/// The two kinds `--cost-from` reads.
///
/// 44223 names the seat (`agentRef`, `role`, `model`); 44225 carries the usage
/// blocks every number comes from. `kinds` is never omitted: an open-ended
/// filter trips the relay's p-gate and comes back 403.
const COST_FACT_KINDS: [u32; 2] = [KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_TRANSCRIPT];

/// `true` for the lowercase hyphenated UUID form and nothing else.
///
/// The same rule `beekeeper_core::pulse` applies to a 44240 `h` tag, and for the
/// same reason: `Uuid::parse_str` also accepts uppercase and the 32-hex simple
/// form, neither of which any `#h` relay filter can match — the SQL
/// containment probe compares exact bytes.
fn is_canonical_uuid(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok_and(|parsed| parsed.to_string() == value)
}

/// A parsed `--cost-from <channel>[:<sessionRef>]`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CostSource {
    /// Channel UUID whose coding-session facts are read.
    channel: String,
    /// Umbrella `sessionRef`, when the caller narrowed to one session.
    session_ref: Option<String>,
}

/// Parse `--cost-from`.
///
/// Both halves are canonical lowercase UUIDs. Uppercase is refused rather than
/// lowercased: `#h` is matched by the relay's SQL containment probe on exact
/// bytes, so a case-variant channel would query nothing and the entry would
/// carry a cost of "nothing measured" that looks like a quiet session.
fn parse_cost_from(value: &str) -> Result<CostSource, CliError> {
    let mut parts = value.split(':');
    let channel = parts.next().unwrap_or_default();
    let session_ref = parts.next();
    if parts.next().is_some() {
        return Err(CliError::Usage(
            "--cost-from takes <channel-uuid> or <channel-uuid>:<session-ref-uuid>".to_owned(),
        ));
    }
    if !is_canonical_uuid(channel) {
        return Err(CliError::Usage(format!(
            "--cost-from channel {channel:?} is not a lowercase canonical UUID"
        )));
    }
    if let Some(session_ref) = session_ref {
        if !is_canonical_uuid(session_ref) {
            return Err(CliError::Usage(format!(
                "--cost-from session reference {session_ref:?} is not a lowercase canonical UUID"
            )));
        }
    }
    Ok(CostSource {
        channel: channel.to_owned(),
        session_ref: session_ref.map(str::to_owned),
    })
}

/// Parse `--cost-seat`: a lowercase hex prefix of one seat's pubkey.
///
/// Four characters is the floor. Shorter prefixes collide often enough that
/// the ambiguity check below would be the only thing standing between a lane's
/// milestone and another lane's numbers.
fn parse_cost_seat(value: &str) -> Result<String, CliError> {
    let ok = (4..=64).contains(&value.len())
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte));
    if !ok {
        return Err(CliError::Usage(format!(
            "--cost-seat {value:?} must be 4-64 lowercase hex characters of a seat pubkey"
        )));
    }
    Ok(value.to_owned())
}

/// What one execution's newest metadata says about the seat behind it.
struct CostTarget {
    actor: Option<String>,
    role: Option<String>,
    model: Option<String>,
    session_ref: Option<String>,
    created_at: i64,
}

/// One seat's running totals while the fold walks the transcript.
#[derive(Default)]
struct CostTally {
    actor: Option<String>,
    role: Option<String>,
    model: Option<String>,
    input_tokens: Option<u64>,
    output_tokens: Option<u64>,
    cache_read_tokens: Option<u64>,
    cache_write_tokens: Option<u64>,
    tool_calls: Option<u64>,
    turns: u64,
}

/// Add one turn's reported count into a running total.
///
/// An unreported count leaves the total exactly as it was — including still
/// `None` — so a seat whose provider never reported `toolCalls` publishes no
/// `toolCalls`, rather than a zero it never measured.
fn accumulate(total: &mut Option<u64>, part: Option<u64>) {
    if let Some(part) = part {
        *total = Some(total.unwrap_or(0).saturating_add(part));
    }
}

/// What one `--cost-from` read produced.
#[derive(Debug)]
struct CostFold {
    /// The cost to attach, or `None` when nothing on the wire measured it.
    cost: Option<PulseCost>,
    /// Turns whose usage could not be attributed to any seat, because the
    /// execution that signed them published no metadata in this read.
    ///
    /// Reported rather than folded in: a number attributed to nobody is not a
    /// seat's cost, and dropping it silently would understate the session
    /// without saying so.
    unattributed_turns: u64,
}

/// Fold the usage blocks a channel's providers signed into a per-seat cost.
///
/// The numbers come from exactly one place: the `usage` object on a terminal
/// `result` transcript item (kind 44225), which is
/// [`beekeeper_core::coding_session_payload::TurnUsageReport`] — the same block
/// `bee sessions status` reads for its context column
/// (`crates/beekeeper-cli/src/commands/sessions/crew_cmds.rs:963`). That command
/// keeps the *newest* turn's block, because occupancy is a now-fact; a lane's
/// cost is the opposite question, so this sums every turn's block instead.
///
/// Seats are keyed by `agentRef`, not by generation: a seat that was resumed
/// runs under a new `cs-target` generation but is the same lane, and two rows
/// for one actor would double-count it (and be refused by
/// [`beekeeper_core::pulse`]'s duplicate-seat rule).
///
/// A seat that reported no usage at all is left out entirely. Publishing it
/// with zeros would claim its lane cost nothing.
fn fold_session_cost(
    events: &[Value],
    session_ref: Option<&str>,
    seat: Option<&str>,
) -> Result<CostFold, CliError> {
    let mut targets: std::collections::HashMap<String, CostTarget> =
        std::collections::HashMap::new();
    for event in events {
        if json_kind(event) != Some(KIND_CODING_SESSION_METADATA) {
            continue;
        }
        let Some(content) = json_str(event, "content") else {
            continue;
        };
        let Ok(metadata) = decode_coding_session_metadata(content) else {
            continue;
        };
        let created_at = event.get("created_at").and_then(Value::as_i64).unwrap_or(0);
        let key = coding_session_target_key(&metadata.session);
        let entry = targets.entry(key).or_insert(CostTarget {
            actor: None,
            role: None,
            model: None,
            session_ref: None,
            created_at: i64::MIN,
        });
        if created_at >= entry.created_at {
            entry.actor = metadata.agent_ref.clone();
            entry.role = metadata.role.clone();
            entry.model = metadata.model.clone();
            entry.session_ref = metadata.session_ref.clone();
            entry.created_at = created_at;
        }
    }

    // Resolved against the *same* rows the fold will walk, `--cost-from`'s
    // session narrowing included. A prefix that names a seat elsewhere in the
    // channel but not in this umbrella is refused rather than folded to
    // nothing: an entry whose cost silently vanished reads exactly like a lane
    // nobody measured.
    if let Some(seat) = seat {
        let mut matches: HashSet<&str> = HashSet::new();
        for target in targets.values() {
            if session_ref.is_some() && target.session_ref.as_deref() != session_ref {
                continue;
            }
            if let Some(actor) = target.actor.as_deref() {
                if actor.starts_with(seat) {
                    matches.insert(actor);
                }
            }
        }
        match matches.len() {
            0 => {
                return Err(CliError::Usage(format!(
                    "--cost-seat {seat:?} matches no seat in that read; the entry would have carried a cost measured for nobody"
                )))
            }
            1 => {}
            count => {
                return Err(CliError::Usage(format!(
                    "--cost-seat {seat:?} matches {count} seats; name more of the pubkey"
                )))
            }
        }
    }

    let mut tallies: std::collections::BTreeMap<String, CostTally> =
        std::collections::BTreeMap::new();
    let mut unattributed_turns = 0u64;
    for event in events {
        if json_kind(event) != Some(KIND_CODING_SESSION_TRANSCRIPT) {
            continue;
        }
        let Some(content) = json_str(event, "content") else {
            continue;
        };
        let Ok(envelope) = serde_json::from_str::<TranscriptEnvelope>(content) else {
            continue;
        };
        let item = &envelope.item;
        if item.get("kind").and_then(Value::as_str) != Some("result") {
            continue;
        }
        let Some(usage) = item.get("usage") else {
            continue;
        };
        let Ok(usage) = serde_json::from_value::<TurnUsageReport>(usage.clone()) else {
            continue;
        };
        if usage.is_empty() {
            continue;
        }
        let key = coding_session_target_key(&envelope.session);
        let Some(target) = targets.get(&key) else {
            unattributed_turns = unattributed_turns.saturating_add(1);
            continue;
        };
        if let Some(session_ref) = session_ref {
            if target.session_ref.as_deref() != Some(session_ref) {
                continue;
            }
        }
        if let Some(seat) = seat {
            if !target
                .actor
                .as_deref()
                .is_some_and(|actor| actor.starts_with(seat))
            {
                continue;
            }
        }
        // One row per seat, and per *execution* only when there is no seat to
        // name — an unseated execution is somebody's terminal, not a lane, and
        // merging two of them under one anonymous row would invent a seat.
        let tally = tallies
            .entry(match target.actor.as_deref() {
                Some(actor) => format!("actor:{actor}"),
                None => format!("target:{key}"),
            })
            .or_default();
        tally.actor.clone_from(&target.actor);
        tally.role.clone_from(&target.role);
        tally.model.clone_from(&target.model);
        accumulate(&mut tally.input_tokens, usage.input_tokens);
        accumulate(&mut tally.output_tokens, usage.output_tokens);
        accumulate(&mut tally.cache_read_tokens, usage.cache_read_tokens);
        accumulate(&mut tally.cache_write_tokens, usage.cache_write_tokens);
        accumulate(&mut tally.tool_calls, usage.tool_calls);
        tally.turns = tally.turns.saturating_add(1);
    }

    let seats: Vec<PulseCostSeat> = tallies
        .into_values()
        .map(|tally| PulseCostSeat {
            actor: tally.actor,
            role: tally.role,
            model: tally.model,
            input_tokens: tally.input_tokens,
            output_tokens: tally.output_tokens,
            cache_read_tokens: tally.cache_read_tokens,
            cache_write_tokens: tally.cache_write_tokens,
            tool_calls: tally.tool_calls,
            turns: Some(tally.turns),
        })
        .collect();
    if seats.is_empty() {
        return Ok(CostFold {
            cost: None,
            unattributed_turns,
        });
    }
    let mut cost = PulseCost {
        seats,
        total_tokens: None,
    };
    cost.total_tokens = cost.seat_token_sum();
    Ok(CostFold {
        cost: Some(cost),
        unattributed_turns,
    })
}

/// Read one channel's coding-session facts and fold them into a cost.
///
/// A failed read is an error, never an omitted cost: an entry that silently
/// dropped its cost because the relay was unreachable would be indistinguishable
/// from one whose lane genuinely published no usage.
async fn resolve_cost(
    client: &BeekeeperClient,
    source: &CostSource,
    seat: Option<&str>,
) -> Result<CostFold, CliError> {
    let events = client
        .query_all(json!({ "kinds": COST_FACT_KINDS, "#h": [source.channel] }))
        .await?;
    fold_session_cost(&events, source.session_ref.as_deref(), seat)
}

// ── Commands ─────────────────────────────────────────────────────────────────

/// Build the entry payload from the command-line arguments.
///
/// Every rejection here happens **before** signing: an invalid code area, an
/// over-cap text, or a malformed `supersedes` is a usage error the caller can
/// fix, never a signed event the relay refuses.
fn build_entry(
    kind: crate::PulseKindArg,
    text: String,
    areas: Option<&str>,
    branch: Option<String>,
    supersedes: Option<String>,
    cost: Option<PulseCost>,
) -> Result<PulseEntry, CliError> {
    if text.len() > MAX_PULSE_TEXT_BYTES {
        return Err(CliError::Usage(format!(
            "--content is {} bytes; the maximum is {MAX_PULSE_TEXT_BYTES}",
            text.len()
        )));
    }
    let entry_type = PulseEntryType::from_str(kind.as_str()).map_err(CliError::Usage)?;
    let code_areas: Vec<String> = match areas {
        // An empty `--areas` claims nothing; a stray comma yields an empty
        // path, which buzz-core rejects rather than silently dropping.
        Some(areas) if areas.trim().is_empty() => Vec::new(),
        Some(areas) => areas
            .split(',')
            .map(|area| area.trim().to_owned())
            .collect(),
        None => Vec::new(),
    };
    Ok(PulseEntry {
        schema: PULSE_ENTRY_SCHEMA.to_owned(),
        entry_type,
        text,
        code_areas,
        branch,
        supersedes,
        cost,
    })
}

/// `bee pulse update` — publish one entry.
///
/// `--cost-from` is read *before* the entry is built, so a relay that cannot
/// answer stops the publish rather than producing a costless entry the reader
/// would take for a lane that measured nothing.
#[allow(clippy::too_many_arguments)]
async fn cmd_update(
    client: &BeekeeperClient,
    project: Option<&str>,
    kind: crate::PulseKindArg,
    areas: Option<&str>,
    branch: Option<String>,
    session: Option<&str>,
    supersedes: Option<String>,
    cost_from: Option<&str>,
    cost_seat: Option<&str>,
    content: &str,
) -> Result<(), CliError> {
    let seat = cost_seat.map(parse_cost_seat).transpose()?;
    let source =
        match (cost_from, seat.as_deref()) {
            (Some(value), _) => Some(parse_cost_from(value)?),
            (None, Some(_)) => return Err(CliError::Usage(
                "--cost-seat needs --cost-from <channel>[:<session-ref>] to read the usage from"
                    .to_owned(),
            )),
            (None, None) => None,
        };
    let coordinate = resolve_project(client, project).await?;
    let fold = match source.as_ref() {
        Some(source) => Some(resolve_cost(client, source, seat.as_deref()).await?),
        None => None,
    };
    let cost = fold.as_ref().and_then(|fold| fold.cost.clone());
    let text = crate::validate::read_or_stdin(content)?;
    let entry = build_entry(kind, text, areas, branch, supersedes, cost)?;
    let builder = beekeeper_sdk::builders::build_pulse_entry(&coordinate, &entry, None, session)
        .map_err(crate::validate::sdk_err)?;

    // Signed verbatim, not through `sign_event`: 44240's tag grammar is a
    // closed key set, so the NIP-OA `auth` tag that method injects would be
    // rejected at ingest. Membership delegation still travels with the request
    // — `submit_event` attaches the same tag as the `x-auth-tag` header.
    let event = client.sign_event_unchecked(builder)?;
    let event_id = event.id.to_hex();
    let created_at = event.created_at.as_secs();
    let raw = client.submit_event(event).await?;
    let response: Value = serde_json::from_str(&raw)
        .map_err(|e| CliError::Other(format!("relay response is not JSON: {e} ({raw})")))?;
    let accepted = response
        .get("accepted")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if !accepted {
        let message = response
            .get("message")
            .and_then(Value::as_str)
            .unwrap_or_default();
        return Err(CliError::Other(format!(
            "relay rejected pulse entry: {message}"
        )));
    }
    let mut printed = json!({
        "event_id": response.get("event_id").and_then(Value::as_str).unwrap_or(&event_id),
        "accepted": accepted,
        "project": coordinate,
        "kind": kind.as_str(),
        "created_at": created_at,
    });
    // The cost keys appear only when a cost was asked for, and `cost: null`
    // with its reason is a first-class answer: "nothing on the wire measured
    // this lane" is a fact the caller needs, and is not the same as the entry
    // having carried numbers.
    if let (Some(object), Some(source), Some(fold)) =
        (printed.as_object_mut(), source.as_ref(), fold.as_ref())
    {
        object.insert(
            "costSource".into(),
            json!({
                "channel": source.channel,
                "sessionRef": source.session_ref,
                "seat": seat,
            }),
        );
        object.insert(
            "cost".into(),
            serde_json::to_value(&fold.cost).unwrap_or(Value::Null),
        );
        if fold.cost.is_none() {
            object.insert(
                "costNote".into(),
                json!("no signed turn usage on the wire for that read; cost omitted rather than published as zero"),
            );
        }
        if fold.unattributed_turns > 0 {
            object.insert(
                "costUnattributedTurns".into(),
                json!(fold.unattributed_turns),
            );
        }
    }
    println!("{printed}");
    Ok(())
}

/// One row of `bee pulse list` — an unfolded entry with its signature
/// stripped.
fn list_row(entry: &PulseDigestEntry, format: &crate::OutputFormat) -> Value {
    match format {
        crate::OutputFormat::Compact => json!({
            "eventId": entry.event_id,
            "createdAt": entry.created_at,
            "type": entry.entry_type,
            "text": entry.text,
            "branch": entry.branch,
        }),
        crate::OutputFormat::Json => json!({
            "eventId": entry.event_id,
            "pubkey": entry.pubkey,
            "createdAt": entry.created_at,
            "type": entry.entry_type,
            "text": entry.text,
            "claimedAreas": entry.claimed_areas,
            "branch": entry.branch,
            "sessionRef": entry.session_ref,
            "supersedes": entry.supersedes,
        }),
    }
}

/// `bee pulse list` — unfolded entries, newest first.
async fn cmd_list(
    client: &BeekeeperClient,
    project: Option<&str>,
    since: Option<u64>,
    kind: Option<crate::PulseKindArg>,
    branch: Option<&str>,
    limit: Option<u32>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let coordinate = resolve_project(client, project).await?;
    let (events, _) = fetch_entry_events(client, &coordinate, since, limit).await?;
    let now = chrono::Utc::now().timestamp();
    let digest = fold_pulse_digest(&coordinate, now, Vec::new(), &events);
    let rows: Vec<Value> = digest
        .entries
        .iter()
        .filter(|entry| branch_matches(branch, entry.branch.as_deref()))
        .filter(|entry| kind.is_none_or(|kind| entry.entry_type == kind.as_str()))
        .map(|entry| list_row(entry, format))
        .collect();
    println!("{}", Value::Array(rows));
    Ok(())
}

/// The name keys every session row carries: `name` and `nameOrigin` always
/// (`nameOrigin` null exactly when nothing names the session), plus
/// `nameModel` and `nameSigner` when the name is a provider's generated title,
/// so a model's words are never printed as though a person chose them.
///
/// `PulseDigestSession` does not serialize the origin (its bytes are pinned by
/// the shared pulse vectors), so every printed row adds these keys itself.
fn compact_name_fields(session: &PulseDigestSession) -> serde_json::Map<String, Value> {
    let mut fields = serde_json::Map::new();
    fields.insert("name".into(), json!(session.name));
    fields.insert("nameOrigin".into(), json!(session.name_origin));
    if session.name_origin.as_deref() == Some("generated") {
        fields.insert("nameModel".into(), json!(session.name_model));
        fields.insert("nameSigner".into(), json!(session.name_signer));
    }
    fields
}

/// `row` with the name keys inserted after `sessionKey`.
fn with_name_fields(session: &PulseDigestSession, row: Value) -> Value {
    let Value::Object(rest) = row else {
        return row;
    };
    let mut object = serde_json::Map::new();
    object.insert("sessionKey".into(), json!(session.session_key));
    object.extend(compact_name_fields(session));
    object.extend(rest);
    Value::Object(object)
}

/// One row of `bee pulse sessions`.
fn session_row(session: &PulseDigestSession, format: &crate::OutputFormat) -> Value {
    match format {
        crate::OutputFormat::Compact => with_name_fields(
            session,
            json!({
                "lifecycle": session.lifecycle,
                "coordinationState": session.coordination_state,
                "generations": session.generations.iter().map(|generation| json!({
                    "targetKey": generation.target_key,
                    "current": generation.current,
                    "reachability": generation.reachability,
                    "status": generation.status,
                    "branch": generation.branch,
                })).collect::<Vec<Value>>(),
                "observedAgeSeconds": session.observed_age_seconds,
            }),
        ),
        crate::OutputFormat::Json => {
            let mut row = serde_json::to_value(session).unwrap_or(Value::Null);
            if let Value::Object(object) = &mut row {
                object.extend(compact_name_fields(session));
            }
            row
        }
    }
}

/// Where each named session's `name` came from, keyed by `sessionKey`:
/// `{origin, model, signerPubkey}`, the shape Desktop's fold returns as
/// `nameOriginsBySession`. An unnamed session has no entry.
///
/// Printed beside the `--format json` digest rather than inside its sessions,
/// whose bytes the shared pulse vectors pin.
fn name_origins(digest: &PulseDigest) -> Value {
    let origins: serde_json::Map<String, Value> = digest
        .sessions
        .iter()
        .filter_map(|session| {
            let origin = session.name_origin.as_deref()?;
            Some((
                session.session_key.clone(),
                json!({
                    "origin": origin,
                    "model": session.name_model,
                    "signerPubkey": session.name_signer,
                }),
            ))
        })
        .collect();
    Value::Object(origins)
}

/// `bee pulse sessions` — the coding sessions observed in the project's
/// channels.
///
/// The list is printed even when a channel failed, and the command still exits
/// non-zero in that case: the scope of a Pulse session list is "project
/// channels", and a channel that could not be read is disclosed rather than
/// quietly narrowing the answer.
async fn cmd_sessions(
    client: &BeekeeperClient,
    project: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let coordinate = resolve_project(client, project).await?;
    let scan = scan_project_sessions(client, &coordinate).await;
    let now = chrono::Utc::now().timestamp();
    let digest = fold_pulse_digest(&coordinate, now, Vec::new(), &scan.events);
    let rows: Vec<Value> = digest
        .sessions
        .iter()
        .map(|session| session_row(session, format))
        .collect();
    println!("{}", Value::Array(rows));
    if !scan.errors.is_empty() {
        return Err(partial_digest_error(scan.failure));
    }
    Ok(())
}

/// `bee pulse digest` — the §6 envelope, printed verbatim.
async fn cmd_digest(
    client: &BeekeeperClient,
    project: Option<&str>,
    branch: Option<&str>,
    limit: Option<u32>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let coordinate = resolve_project(client, project).await?;
    let (mut digest, failure) = compose_digest(client, &coordinate, limit).await;
    digest
        .entries
        .retain(|entry| branch_matches(branch, entry.branch.as_deref()));
    digest.sessions.retain(|session| {
        session
            .generations
            .iter()
            .any(|generation| branch_matches(branch, generation.branch.as_deref()))
    });
    digest.provider_reachable_sessions = digest
        .sessions
        .iter()
        .filter(|session| session.coordination_state == "provider_reachable")
        .map(|session| session.session_key.clone())
        .collect();
    digest.open_unverified_sessions = digest
        .sessions
        .iter()
        .filter(|session| session.coordination_state == "open_unverified")
        .map(|session| session.session_key.clone())
        .collect();
    digest.closed_sessions = digest
        .sessions
        .iter()
        .filter(|session| session.coordination_state == "closed")
        .map(|session| session.session_key.clone())
        .collect();

    // The eight mission keys ride **beside** the digest, never inside it: the
    // 44240 body and `PulseDigest`'s own serialization are byte-unchanged, so
    // an older reader ignores eight unknown keys (L9.11). They are attached
    // here as well as on `bee pulse missions` because the spec says they are
    // added "to what `bee pulse digest` prints and Desktop holds", and shipping
    // them on only one of the two would have been a silent deviation.
    let open_sessions: std::collections::BTreeMap<String, (Option<String>, Option<i64>)> = digest
        .sessions
        .iter()
        .filter(|session| session.lifecycle == "open")
        .filter_map(|session| {
            // A mission row carries a bare `name` with no origin beside it,
            // so it gets a person's name only: a generated title there would
            // read as a name somebody chose.
            let person_name = session
                .name
                .clone()
                .filter(|_| session.name_origin.as_deref() == Some("person"));
            Some((
                session.session_ref.clone()?,
                (person_name, session.latest_observation_at),
            ))
        })
        .collect();
    let open_session_count = open_sessions.len();
    let mut mission_errors: Vec<beekeeper_core::pulse_mission::PulseMissionError> = Vec::new();
    let channels = match project_channel_ids(client, &coordinate).await {
        Ok((channels, _truncated)) => channels,
        Err(error) => {
            mission_errors.push(beekeeper_core::pulse_mission::PulseMissionError {
                scope: "channels".to_owned(),
                message: error.to_string(),
            });
            Vec::new()
        }
    };
    let targets = crate::commands::pulse_mission::discover_mission_sessions(
        client,
        &channels,
        &open_sessions,
        &mut mission_errors,
    )
    .await;
    let missions = crate::commands::pulse_mission::compose_mission_rows(
        client,
        &targets,
        open_session_count,
        None,
        mission_errors,
    )
    .await;

    let mut rendered = match format {
        // Both formats print `source`; compact drops the two constants a
        // reader already knows and the per-row detail, never a fact.
        crate::OutputFormat::Compact => compact_digest(&digest),
        crate::OutputFormat::Json => {
            let mut rendered = serde_json::to_value(&digest).unwrap_or(Value::Null);
            if let Value::Object(object) = &mut rendered {
                object.insert("nameOrigins".into(), name_origins(&digest));
            }
            rendered
        }
    };
    attach_mission_rows(&mut rendered, &missions);
    println!("{rendered}");

    if digest.complete {
        Ok(())
    } else {
        Err(partial_digest_error(failure))
    }
}

/// Attach the eight mission keys beside a rendered digest.
///
/// Additive by construction: it inserts and never replaces, so a key the digest
/// already prints is left exactly as `PulseDigest` serialized it. A non-object
/// rendering (there is none today) is returned untouched rather than reshaped.
fn attach_mission_rows(
    rendered: &mut Value,
    missions: &beekeeper_core::pulse_mission::PulseMissionRows,
) {
    let Some(object) = rendered.as_object_mut() else {
        return;
    };
    let Some(sibling) = crate::commands::pulse_mission::mission_rows_sibling_keys(missions)
        .as_object()
        .cloned()
    else {
        return;
    };
    for (key, value) in sibling {
        object.entry(key).or_insert(value);
    }
}

/// The `--format compact` digest: the same facts, fewer per-row fields.
fn compact_digest(digest: &PulseDigest) -> Value {
    json!({
        "source": digest.source,
        "project": digest.project,
        "asOf": digest.as_of,
        "complete": digest.complete,
        "sessionsScope": digest.sessions_scope,
        "providerReachableSessions": digest.provider_reachable_sessions,
        "openUnverifiedSessions": digest.open_unverified_sessions,
        "closedSessions": digest.closed_sessions,
        "sessions": digest.sessions.iter().map(|session| with_name_fields(session, json!({
            "lifecycle": session.lifecycle,
            "coordinationState": session.coordination_state,
            "generations": session.generations.iter().map(|generation| json!({
                "targetKey": generation.target_key,
                "current": generation.current,
                "reachability": generation.reachability,
                "status": generation.status,
                "branch": generation.branch,
                "commitConfirmation": generation.commit_confirmation,
            })).collect::<Vec<Value>>(),
            "observedAgeSeconds": session.observed_age_seconds,
        }))).collect::<Vec<Value>>(),
        "entries": digest.entries.iter().map(|entry| json!({
            "eventId": entry.event_id,
            "pubkey": entry.pubkey,
            "createdAt": entry.created_at,
            "type": entry.entry_type,
            "text": entry.text,
            "branch": entry.branch,
            "active": entry.active,
        })).collect::<Vec<Value>>(),
        "errors": digest.errors,
    })
}

/// Route a `bee pulse` subcommand.
pub async fn dispatch(
    cmd: crate::PulseCmd,
    client: &BeekeeperClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    use crate::PulseCmd;
    match cmd {
        PulseCmd::Update {
            project,
            kind,
            areas,
            branch,
            session,
            supersedes,
            cost_from,
            cost_seat,
            content,
        } => {
            cmd_update(
                client,
                project.as_deref(),
                kind,
                areas.as_deref(),
                branch,
                session.as_deref(),
                supersedes,
                cost_from.as_deref(),
                cost_seat.as_deref(),
                &content,
            )
            .await
        }
        PulseCmd::List {
            project,
            since,
            kind,
            branch,
            limit,
        } => {
            cmd_list(
                client,
                project.as_deref(),
                since,
                kind,
                branch.as_deref(),
                limit,
                format,
            )
            .await
        }
        PulseCmd::Sessions { project } => cmd_sessions(client, project.as_deref(), format).await,
        PulseCmd::Digest {
            project,
            branch,
            limit,
        } => cmd_digest(client, project.as_deref(), branch.as_deref(), limit, format).await,
        PulseCmd::Missions {
            channel,
            session_ref,
            genesis,
            repo,
        } => {
            crate::commands::pulse_mission::cmd_missions(
                client,
                &channel,
                &session_ref,
                &genesis,
                repo.as_deref(),
                format,
            )
            .await
        }
        PulseCmd::PruneWip { repo, merged } => {
            crate::commands::wip_refs::cmd_prune_wip(client, &repo, merged.as_deref()).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::exit_code;

    /// The conformance corpus, loaded from the tree so the Rust fold and the
    /// Desktop fold cannot drift apart silently.
    const FOLD_VECTORS: &str =
        include_str!("../../../../conformance/project-pulse-fold/fixtures/fold-vectors.json");

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FoldVectorFile {
        digest_schema: String,
        entry_schema: String,
        vectors: Vec<FoldVector>,
    }

    #[derive(Deserialize)]
    struct FoldVector {
        name: String,
        input: FoldVectorInput,
        expected: Value,
    }

    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct FoldVectorInput {
        project: String,
        now: i64,
        source_errors: Vec<PulseDigestError>,
        events: Vec<Value>,
    }

    fn vectors() -> FoldVectorFile {
        serde_json::from_str(FOLD_VECTORS).expect("fold vectors parse")
    }

    // ---- Conformance binding ----

    /// The corpus pins the constants; the constants live in code.
    #[test]
    fn corpus_pins_the_schemas() {
        let file = vectors();
        assert_eq!(file.digest_schema, PULSE_DIGEST_SCHEMA);
        assert_eq!(file.entry_schema, PULSE_ENTRY_SCHEMA);
    }

    /// Every vector, folded by this implementation, must serialize to the same
    /// bytes as the corpus's expected digest.
    #[test]
    fn fold_matches_every_conformance_vector() {
        let file = vectors();
        assert!(!file.vectors.is_empty(), "corpus must not be empty");
        for vector in &file.vectors {
            let actual = fold_pulse_digest(
                &vector.input.project,
                vector.input.now,
                vector.input.source_errors.clone(),
                &vector.input.events,
            );
            let actual_value = serde_json::to_value(&actual).expect("serialize digest");
            assert_eq!(
                actual_value, vector.expected,
                "vector {} produced a different digest",
                vector.name
            );

            // Byte identity, not just semantic equality: the expected object
            // round-trips through the same strict struct, so a missing or
            // extra key fails here as well.
            let expected: PulseDigest =
                serde_json::from_value(vector.expected.clone()).expect("expected digest decodes");
            assert_eq!(
                serde_json::to_string(&actual).expect("serialize actual"),
                serde_json::to_string(&expected).expect("serialize expected"),
                "vector {} is not byte-identical",
                vector.name
            );
        }
    }

    #[test]
    fn reordered_lifecycle_and_lease_envelopes_are_rejected() {
        for kind in [
            KIND_CODING_SESSION_LIFECYCLE_COMMAND,
            KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
            KIND_CODING_SESSION_LEASE,
        ] {
            let mut vector = vectors()
                .vectors
                .into_iter()
                .find(|vector| vector.name == "idle-hours-old-with-live-authorized-lease")
                .expect("live lease vector present");
            let mut changed = 0;
            for event in vector
                .input
                .events
                .iter_mut()
                .filter(|event| json_kind(event) == Some(kind))
            {
                event["tags"].as_array_mut().expect("tags array").swap(0, 1);
                changed += 1;
            }
            assert!(changed > 0, "kind {kind} present");
            let digest = fold_pulse_digest(
                &vector.input.project,
                vector.input.now,
                vector.input.source_errors,
                &vector.input.events,
            );
            if kind == KIND_CODING_SESSION_LEASE {
                assert!(digest.provider_reachable_sessions.is_empty());
                assert_eq!(
                    digest.open_unverified_sessions,
                    vec!["5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10"]
                );
            } else {
                assert!(digest.sessions.is_empty(), "kind {kind} was admitted");
            }
        }
    }

    /// The envelope's key order is part of the contract both languages bind to.
    #[test]
    fn digest_key_order_is_the_envelope_order() {
        let digest = fold_pulse_digest("30621:aa:demo", 1, Vec::new(), &[]);
        let rendered = serde_json::to_string(&digest).expect("serialize");
        let keys: Vec<&str> = rendered
            .trim_start_matches('{')
            .split(",\"")
            .filter_map(|chunk| chunk.trim_start_matches('"').split('"').next())
            .collect();
        assert_eq!(
            keys,
            vec![
                "schema",
                "source",
                "project",
                "asOf",
                "complete",
                "sessionsScope",
                "sessions",
                "providerReachableSessions",
                "openUnverifiedSessions",
                "closedSessions",
                "entries",
                "errors",
            ]
        );
    }

    // ---- Confirmed empty vs partial read ----

    fn source_error() -> PulseDigestError {
        PulseDigestError {
            scope: "sessions:05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2".to_owned(),
            message: "relay unavailable".to_owned(),
        }
    }

    #[test]
    fn confirmed_empty_and_partial_read_are_different_outcomes() {
        let empty = fold_pulse_digest("30621:aa:demo", 100, Vec::new(), &[]);
        assert!(empty.complete);
        assert!(empty.errors.is_empty());
        assert!(empty.sessions.is_empty() && empty.entries.is_empty());

        let partial = fold_pulse_digest("30621:aa:demo", 100, vec![source_error()], &[]);
        assert!(!partial.complete);
        assert_eq!(partial.errors.len(), 1);
    }

    /// A partial read exits 2 on the relay's own error; a `--limit` truncation
    /// exits on the usage condition it actually is, rather than a manufactured
    /// relay status.
    #[test]
    fn partial_digest_exit_codes_follow_the_error_table() {
        let relay_failure = CliError::Relay {
            status: 503,
            body: String::new(),
        };
        assert_eq!(exit_code(&partial_digest_error(Some(relay_failure))), 2);
        assert_eq!(exit_code(&partial_digest_error(None)), 1);
    }

    // ---- Query shapes ----

    /// The digest issues two distinct filters: 44240 by `#a`, session kinds by
    /// `#h`. Combining them would return only the 44240 rows, because 44223
    /// carries no `a` tag — an empty session list with no error.
    #[test]
    fn digest_issues_two_distinct_filters() {
        let entries = entries_filter("30621:aa:demo", Some(42));
        assert_eq!(entries["kinds"], json!([44240]));
        assert_eq!(entries["#a"], json!(["30621:aa:demo"]));
        assert_eq!(entries["since"], json!(42));
        assert!(entries.get("#h").is_none());

        let sessions = durable_session_facts_filter("05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2");
        assert_eq!(
            sessions["kinds"],
            json!([44221, 44223, 44224, 44227, 44229, 44230, 44226, 44252])
        );
        assert_eq!(
            sessions["#h"],
            json!(["05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2"])
        );
        assert!(sessions.get("#a").is_none());

        let leases = session_lease_filter("05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2");
        assert_eq!(leases["kinds"], json!([24223]));
        assert_eq!(
            leases["#h"],
            json!(["05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2"])
        );
        assert!(leases.get("limit").is_none());
    }

    // ---- Project resolution ----

    #[test]
    fn bare_dtag_resolves_only_when_exactly_one_project_matches() {
        let one = vec![format!("30621:{}:demo", "a".repeat(64))];
        assert_eq!(
            resolve_bare_dtag("demo", one.clone()).expect("resolves"),
            one[0]
        );

        let none = resolve_bare_dtag("demo", Vec::new()).expect_err("must not resolve");
        assert_eq!(exit_code(&none), 1);
        assert!(none.to_string().contains("no visible project named demo"));

        let two = vec![
            format!("30621:{}:demo", "a".repeat(64)),
            format!("30621:{}:demo", "b".repeat(64)),
        ];
        let ambiguous = resolve_bare_dtag("demo", two.clone()).expect_err("must not resolve");
        assert_eq!(exit_code(&ambiguous), 1);
        for candidate in &two {
            assert!(
                ambiguous.to_string().contains(candidate),
                "the error must print every candidate coordinate"
            );
        }
    }

    /// The precedence, end to end: an explicit coordinate is normalized, a
    /// bare dtag defers to the visible-project lookup, a malformed value is a
    /// usage error, and no value at all names the environment variable that
    /// would have supplied it.
    #[test]
    fn project_precedence_resolves_without_a_relay_where_it_can() {
        let canonical = format!("30621:{}:demo", "a".repeat(64));
        // A case-variant owner normalizes; the dtag is byte-exact and is not
        // touched.
        assert_eq!(
            direct_project_coordinate(Some(&format!("30621:{}:demo", "A".repeat(64))))
                .expect("normalizes"),
            Some(canonical.clone())
        );
        assert_eq!(
            direct_project_coordinate(Some(&canonical)).expect("normalizes"),
            Some(canonical)
        );
        assert_eq!(direct_project_coordinate(Some("demo")).expect("bare"), None);

        let malformed = direct_project_coordinate(Some("30617:abc:repo")).expect_err("rejects");
        assert_eq!(exit_code(&malformed), 1);

        let missing = direct_project_coordinate(None).expect_err("rejects");
        assert_eq!(exit_code(&missing), 1);
        assert!(
            missing.to_string().contains("BUZZ_PULSE_PROJECT"),
            "the error must name the environment variable that supplies it"
        );
    }

    #[test]
    fn coordinate_splitting_tolerates_a_dtag_with_colons() {
        let coordinate = format!("30621:{}:team:demo", "a".repeat(64));
        let (owner, dtag) = split_project_coordinate(&coordinate).expect("splits");
        assert_eq!(owner, "a".repeat(64));
        assert_eq!(dtag, "team:demo");
        assert!(split_project_coordinate("30617:abc:repo").is_none());
    }

    // ---- Pre-sign validation ----

    #[test]
    fn invalid_areas_fail_before_signing() {
        for area in [
            "/etc/passwd",
            "crates/../etc",
            "~/notes",
            "crates\\acp\\pool.rs",
            "crates//pool.rs",
            "crates/",
            // A stray comma yields an empty path rather than a silent drop.
            "crates/beekeeper-cli/src/lib.rs,,",
        ] {
            let entry = build_entry(
                crate::PulseKindArg::Plan,
                "Working here.".to_owned(),
                Some(area),
                None,
                None,
                None,
            )
            .expect("payload assembles");
            // The payload itself is only assembled here; buzz-core rejects it
            // through the builder, before any signing happens.
            let built = beekeeper_sdk::builders::build_pulse_entry(
                &format!("30621:{}:demo", "a".repeat(64)),
                &entry,
                None,
                None,
            );
            let error = crate::validate::sdk_err(
                built
                    .err()
                    .unwrap_or_else(|| panic!("code area {area:?} must be rejected")),
            );
            assert_eq!(exit_code(&error), 1, "code area {area:?} is a usage error");
        }
    }

    /// The one row of the exit-code table the read path can never produce: a
    /// write refused by the project gate is exit 3, not 1. Reads never 403 by
    /// design — an inadmissible private project returns an empty 200 — so
    /// `list`, `sessions`, and `digest` cannot reach this row.
    ///
    /// The relay end of this row is the assertion in
    /// `crates/beekeeper-test-client/tests/e2e_pulse.rs` that a refused
    /// `POST /events` answers **403** — this unit test only pins the mapping
    /// once that status arrives, and would pass vacuously on its own.
    #[test]
    fn a_refused_write_exits_three() {
        assert_eq!(
            exit_code(&CliError::Relay {
                status: 403,
                body: "restricted: project write access required".to_owned(),
            }),
            3
        );
    }

    #[test]
    fn over_cap_content_is_a_usage_error_before_signing() {
        let error = build_entry(
            crate::PulseKindArg::Plan,
            "a".repeat(MAX_PULSE_TEXT_BYTES + 1),
            None,
            None,
            None,
            None,
        )
        .expect_err("must reject");
        assert_eq!(exit_code(&error), 1);
        assert!(build_entry(
            crate::PulseKindArg::Plan,
            "a".repeat(MAX_PULSE_TEXT_BYTES),
            None,
            None,
            None,
            None,
        )
        .is_ok());
    }

    #[test]
    fn areas_are_split_on_commas_and_trimmed() {
        let entry = build_entry(
            crate::PulseKindArg::Blocker,
            "Do not touch these.".to_owned(),
            Some("crates/beekeeper-acp/src/pool.rs, crates/beekeeper-cli/src/lib.rs"),
            None,
            None,
            None,
        )
        .expect("payload assembles");
        assert_eq!(
            entry.code_areas,
            vec![
                "crates/beekeeper-acp/src/pool.rs".to_owned(),
                "crates/beekeeper-cli/src/lib.rs".to_owned()
            ]
        );
        assert_eq!(entry.entry_type.as_str(), "blocker");

        let empty = build_entry(
            crate::PulseKindArg::Note,
            "Nothing claimed.".to_owned(),
            Some("  "),
            None,
            None,
            None,
        )
        .expect("payload assembles");
        assert!(empty.code_areas.is_empty());
    }

    // ---- Output shapes ----

    fn demo_digest() -> PulseDigest {
        let file = vectors();
        let vector = file
            .vectors
            .iter()
            .find(|vector| vector.name == "idle-hours-old-with-live-authorized-lease")
            .expect("vector present");
        fold_pulse_digest(
            &vector.input.project,
            vector.input.now,
            vector.input.source_errors.clone(),
            &vector.input.events,
        )
    }

    #[test]
    fn both_formats_carry_source_and_preserve_nullable_facts() {
        let digest = demo_digest();
        let standard = serde_json::to_value(&digest).expect("serialize");
        let compact = compact_digest(&digest);
        assert_eq!(standard["source"], json!("client-composed"));
        assert_eq!(compact["source"], json!("client-composed"));

        let session = &standard["sessions"][0];
        let generation = &session["generations"][0];
        for field in ["observedCommit", "dirty", "relayReachable", "verifiedAt"] {
            assert_eq!(
                generation[field],
                Value::Null,
                "{field} must stay null — unknown is not false"
            );
        }
        assert_eq!(
            generation["commitConfirmation"],
            json!("Commit not checked")
        );
        // The age comes from the 44223 `created_at`, never from `verifiedAt`.
        assert_eq!(session["observedAgeSeconds"], json!(10_000));
        assert_eq!(
            compact["sessions"][0]["generations"][0]["commitConfirmation"],
            json!("Commit not checked")
        );
    }

    #[test]
    fn list_rows_are_stable_in_both_formats() {
        let file = vectors();
        let vector = file
            .vectors
            .iter()
            .find(|vector| vector.name == "no-branch-rows-preserved")
            .expect("vector present");
        let digest = fold_pulse_digest(
            &vector.input.project,
            vector.input.now,
            Vec::new(),
            &vector.input.events,
        );
        let entry = &digest.entries[1];
        assert_eq!(
            list_row(entry, &crate::OutputFormat::Compact),
            json!({
                "eventId": entry.event_id,
                "createdAt": entry.created_at,
                "type": "plan",
                "text": "Branch-scoped claim.",
                "branch": "wip/project-pulse",
            })
        );
        assert_eq!(
            list_row(entry, &crate::OutputFormat::Json),
            json!({
                "eventId": entry.event_id,
                "pubkey": entry.pubkey,
                "createdAt": entry.created_at,
                "type": "plan",
                "text": "Branch-scoped claim.",
                "claimedAreas": [],
                "branch": "wip/project-pulse",
                "sessionRef": Value::Null,
                "supersedes": Value::Null,
            })
        );
    }

    // ---- Session names (SV-31) ----

    /// A generated title is printed with its origin, model and signer; a
    /// person's name with its origin alone; no name as two nulls.
    #[test]
    fn compact_session_rows_say_where_the_name_came_from() {
        let digest = folded_vector("idle-hours-old-with-live-authorized-lease");
        let mut session = digest.sessions[0].clone();

        let unnamed = session_row(&session, &crate::OutputFormat::Compact);
        assert_eq!(unnamed["name"], Value::Null);
        assert_eq!(unnamed["nameOrigin"], Value::Null);
        assert!(unnamed.get("nameModel").is_none());

        session.name = Some("Fix login redirect".into());
        session.name_origin = Some("generated".into());
        session.name_model = Some("haiku".into());
        session.name_signer = Some("d4".repeat(32));
        let generated = session_row(&session, &crate::OutputFormat::Compact);
        assert_eq!(generated["name"], "Fix login redirect");
        assert_eq!(generated["nameOrigin"], "generated");
        assert_eq!(generated["nameModel"], "haiku");
        assert_eq!(generated["nameSigner"], json!("d4".repeat(32)));
        let mut whole = digest.clone();
        whole.sessions = vec![session.clone()];
        assert_eq!(
            compact_digest(&whole)["sessions"][0]["nameOrigin"],
            "generated"
        );

        session.name = Some("Auth rework".into());
        session.name_origin = Some("person".into());
        session.name_model = None;
        session.name_signer = None;
        let person = session_row(&session, &crate::OutputFormat::Compact);
        assert_eq!(person["nameOrigin"], "person");
        assert!(person.get("nameModel").is_none());
        assert!(person.get("nameSigner").is_none());
    }

    /// The JSON row and the digest-level map carry the origin the session
    /// object itself does not serialize.
    #[test]
    fn json_output_carries_the_name_origin_beside_the_pinned_session() {
        let mut digest = folded_vector("idle-hours-old-with-live-authorized-lease");
        assert_eq!(name_origins(&digest), json!({}));
        let session = &mut digest.sessions[0];
        session.name = Some("Fix login redirect".into());
        session.name_origin = Some("generated".into());
        session.name_model = Some("haiku".into());
        session.name_signer = Some("d4".repeat(32));
        let row = session_row(session, &crate::OutputFormat::Json);
        assert_eq!(row["nameOrigin"], "generated");
        assert_eq!(row["nameModel"], "haiku");
        let key = session.session_key.clone();
        assert_eq!(
            name_origins(&digest)[key.as_str()],
            json!({ "origin": "generated", "model": "haiku", "signerPubkey": "d4".repeat(32) })
        );
    }

    // ---- Supersession and attribution ----

    fn folded_vector(name: &str) -> PulseDigest {
        let file = vectors();
        let vector = file
            .vectors
            .iter()
            .find(|vector| vector.name == name)
            .unwrap_or_else(|| panic!("vector {name} present"));
        fold_pulse_digest(
            &vector.input.project,
            vector.input.now,
            vector.input.source_errors.clone(),
            &vector.input.events,
        )
    }

    #[test]
    fn non_live_lease_outcomes_fail_closed_without_erasing_evidence() {
        let digest = folded_vector("unverified-lease-outcomes");
        assert!(digest.provider_reachable_sessions.is_empty());
        assert_eq!(digest.open_unverified_sessions.len(), 1);
        let generations = &digest.sessions[0].generations;
        assert_eq!(generations.len(), 6);
        let named = |name: &str| {
            generations
                .iter()
                .find(|generation| generation.execution_key.ends_with(name))
                .unwrap_or_else(|| panic!("{name} generation present"))
        };
        assert_eq!(named("none").lease_state, None);
        assert_eq!(named("released").lease_state.as_deref(), Some("released"));
        assert_eq!(named("expired").lease_expires_at, Some(1_785_599_970));
        assert_eq!(
            named("expired").reachability,
            PulseGenerationReachability::Unverified
        );
        assert_eq!(named("conflict").lease_sequence, Some(7));
        assert_eq!(named("conflict").lease_source_event_id, None);
        assert_eq!(named("wrong").lease_sequence, None);
        assert_eq!(
            named("terminal").reachability,
            PulseGenerationReachability::Terminal
        );
    }

    #[test]
    fn resume_is_generation_isolated_and_requires_exact_continuity() {
        let digest = folded_vector("resume-generation-isolation-and-continuity");
        assert_eq!(digest.sessions.len(), 1);
        let generations = &digest.sessions[0].generations;
        assert_eq!(generations.len(), 2, "skipped generation must be rejected");
        assert!(generations[0].current);
        assert!(generations[0].target_key.ends_with("1:2"));
        assert_eq!(
            generations[0].reachability,
            PulseGenerationReachability::Unverified
        );
        assert!(!generations[1].current);
        assert_eq!(generations[1].lease_state.as_deref(), Some("live"));
        assert_eq!(
            generations[1].reachability,
            PulseGenerationReachability::Unverified
        );
    }

    #[test]
    fn closure_outranks_live_generation_evidence() {
        let digest = folded_vector("closure-outranks-live-generation");
        assert_eq!(
            digest.sessions[0].generations[0].reachability,
            PulseGenerationReachability::ProviderReachable
        );
        assert_eq!(digest.sessions[0].coordination_state, "closed");
        assert!(digest.provider_reachable_sessions.is_empty());
        assert_eq!(
            digest.closed_sessions,
            vec!["8d3f6b41-2c07-4e6a-9f52-31ab7c9e0d64"]
        );
    }

    /// A peer cannot quietly retract another author's claim: the target stays
    /// active and the claim is rendered as unhonored on the claimant.
    #[test]
    fn cross_author_supersession_leaves_the_original_active() {
        let digest = folded_vector("cross-author-supersession-not-honored");
        assert!(
            digest.entries.iter().all(|entry| entry.active),
            "no entry may lose its active flag to a cross-author claim"
        );
        let claimant = &digest.entries[0];
        assert_eq!(claimant.superseded_by.len(), 1);
        assert!(!claimant.superseded_by[0].honored);
        assert_eq!(
            claimant.superseded_by[0].reason.as_deref(),
            Some("cross-author")
        );
        // The claim is a fold observation, not a read failure.
        assert!(digest.complete);
    }

    /// Entry-to-session attribution is a consumer law with no representation in
    /// the v2 envelope: the CLI echoes `pu-session` verbatim and never nests an
    /// entry inside a session row, so it cannot attribute one author's entry to
    /// another team's card.
    #[test]
    fn entries_are_never_nested_inside_a_session_row() {
        let digest = folded_vector("idle-hours-old-with-live-authorized-lease");
        let session = serde_json::to_value(&digest.sessions[0]).expect("serialize");
        let object = session.as_object().expect("object");
        assert!(!object.contains_key("entries"));
        assert!(!object.contains_key("founder"));
    }

    // ---- Branch semantics ----

    #[test]
    fn branch_filter_never_mixes_named_and_null_rows() {
        assert!(branch_matches(None, Some("main")));
        assert!(branch_matches(None, None));
        assert!(branch_matches(Some("main"), Some("main")));
        assert!(!branch_matches(Some("main"), Some("Main")));
        assert!(!branch_matches(Some("main"), None));
        assert!(branch_matches(Some("-"), None));
        assert!(!branch_matches(Some("-"), Some("main")));
    }

    #[test]
    fn commit_confirmation_is_a_fixed_tri_state() {
        assert_eq!(commit_confirmation(Some(true)), "Commit confirmed on relay");
        assert_eq!(
            commit_confirmation(Some(false)),
            "Commit not found on relay"
        );
        assert_eq!(commit_confirmation(None), "Commit not checked");
    }

    // ---- Cost (`--cost-from` / `--cost-seat`) ----

    const COST_CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
    const COST_SESSION: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
    const OTHER_SESSION: &str = "9c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
    const BUILDER: &str = "cc00000000000000000000000000000000000000000000000000000000000022";
    const REFUTER: &str = "dd00000000000000000000000000000000000000000000000000000000000033";

    fn target(session_id: &str, generation: u64) -> Value {
        json!({
            "driver": "claude-acp",
            "instanceId": "instance-1",
            "sessionId": session_id,
            "generation": generation,
        })
    }

    /// A signed-shape 44223 metadata event, as `POST /query` hands it back.
    fn metadata_event(
        session_id: &str,
        generation: u64,
        actor: Option<&str>,
        role: Option<&str>,
        model: &str,
        session_ref: Option<&str>,
        created_at: i64,
    ) -> Value {
        let mut content = json!({
            "schema": "buzz-coding-session-metadata/v1",
            "session": target(session_id, generation),
            "projectRef": null,
            "repoRef": null,
            "title": null,
            "agentRef": actor,
            "provider": null,
            "runtime": "claude",
            "model": model,
            "status": "running",
            "branch": null,
            "capabilities": {
                "threadTurnStart": true,
                "threadTurnInterrupt": true,
                "threadSteer": false,
                "context": false,
                "diff": false,
                "plan": true,
            },
        });
        if let (Some(object), Some(role)) = (content.as_object_mut(), role) {
            object.insert("role".into(), json!(role));
        }
        if let (Some(object), Some(session_ref)) = (content.as_object_mut(), session_ref) {
            object.insert("sessionRef".into(), json!(session_ref));
        }
        json!({
            "id": "a".repeat(64),
            "pubkey": "b".repeat(64),
            "created_at": created_at,
            "kind": KIND_CODING_SESSION_METADATA,
            "tags": [["h", COST_CHANNEL]],
            "content": content.to_string(),
        })
    }

    /// A 44225 transcript event carrying one terminal `result` item.
    fn result_event(session_id: &str, generation: u64, seq: u64, usage: Option<Value>) -> Value {
        let mut item = json!({
            "kind": "result",
            "subtype": "success",
            "isError": false,
            "durationMs": 1_000,
            "result": "done",
        });
        if let (Some(object), Some(usage)) = (item.as_object_mut(), usage) {
            object.insert("usage".into(), usage);
        }
        let content = json!({
            "schema": "buzz-coding-session-transcript/v1",
            "session": target(session_id, generation),
            "eventSeq": seq,
            "timestamp": 1_700_000_000_000i64,
            "turnId": format!("turn-{seq}"),
            "item": item,
        });
        json!({
            "id": "c".repeat(64),
            "pubkey": "b".repeat(64),
            "created_at": 1_700_000_000i64 + seq as i64,
            "kind": KIND_CODING_SESSION_TRANSCRIPT,
            "tags": [["h", COST_CHANNEL]],
            "content": content.to_string(),
        })
    }

    fn usage(input: u64, output: u64, read: u64, write: u64, tools: u64) -> Value {
        json!({
            "inputTokens": input,
            "outputTokens": output,
            "cacheReadTokens": read,
            "cacheWriteTokens": write,
            "toolCalls": tools,
        })
    }

    fn two_seat_channel() -> Vec<Value> {
        vec![
            metadata_event(
                "s-builder",
                1,
                Some(BUILDER),
                Some("builder"),
                "opus-5[1m]",
                Some(COST_SESSION),
                10,
            ),
            metadata_event(
                "s-refuter",
                1,
                Some(REFUTER),
                Some("refuter"),
                "sonnet-5",
                Some(COST_SESSION),
                11,
            ),
            result_event("s-builder", 1, 1, Some(usage(10, 5, 100, 20, 4))),
            result_event("s-builder", 1, 2, Some(usage(1, 2, 3, 4, 5))),
            result_event("s-refuter", 1, 1, Some(usage(7, 3, 0, 0, 1))),
        ]
    }

    #[test]
    fn cost_from_parses_a_bare_channel_and_a_session_suffix() {
        assert_eq!(
            parse_cost_from(COST_CHANNEL).expect("bare channel parses"),
            CostSource {
                channel: COST_CHANNEL.to_owned(),
                session_ref: None
            }
        );
        assert_eq!(
            parse_cost_from(&format!("{COST_CHANNEL}:{COST_SESSION}")).expect("suffix parses"),
            CostSource {
                channel: COST_CHANNEL.to_owned(),
                session_ref: Some(COST_SESSION.to_owned())
            }
        );
        for bad in [
            "not-a-uuid",
            &format!("{COST_CHANNEL}:not-a-uuid"),
            &format!("{COST_CHANNEL}:{COST_SESSION}:extra"),
            &COST_CHANNEL.to_uppercase(),
        ] {
            let error = parse_cost_from(bad).expect_err("must reject");
            assert_eq!(exit_code(&error), 1, "{bad:?} is a usage error");
        }
    }

    #[test]
    fn cost_seat_takes_a_short_lowercase_hex_prefix() {
        assert_eq!(
            parse_cost_seat("cc000000").as_deref().ok(),
            Some("cc000000")
        );
        assert_eq!(parse_cost_seat(BUILDER).as_deref().ok(), Some(BUILDER));
        for bad in ["cc", "CC000000", "zzzzzzzz", &"c".repeat(65)] {
            assert_eq!(
                exit_code(&parse_cost_seat(bad).expect_err("must reject")),
                1,
                "{bad:?} is a usage error"
            );
        }
    }

    #[test]
    fn folds_every_seats_usage_and_totals_it() {
        let fold = fold_session_cost(&two_seat_channel(), None, None).expect("fold succeeds");
        let cost = fold.cost.expect("two seats reported usage");
        assert_eq!(cost.seats.len(), 2);

        let builder = cost
            .seats
            .iter()
            .find(|seat| seat.actor.as_deref() == Some(BUILDER))
            .expect("builder seat");
        assert_eq!(builder.role.as_deref(), Some("builder"));
        assert_eq!(builder.model.as_deref(), Some("opus-5[1m]"));
        assert_eq!(builder.input_tokens, Some(11));
        assert_eq!(builder.output_tokens, Some(7));
        assert_eq!(builder.cache_read_tokens, Some(103));
        assert_eq!(builder.cache_write_tokens, Some(24));
        assert_eq!(builder.tool_calls, Some(9));
        assert_eq!(builder.turns, Some(2));

        assert_eq!(cost.total_tokens, Some(11 + 7 + 103 + 24 + 7 + 3));
        assert_eq!(cost.total_tokens, cost.seat_token_sum());
        assert_eq!(fold.unattributed_turns, 0);
    }

    #[test]
    fn cost_seat_restricts_the_fold_to_one_lane() {
        let fold =
            fold_session_cost(&two_seat_channel(), None, Some("cc000000")).expect("fold succeeds");
        let cost = fold.cost.expect("the named seat reported usage");
        assert_eq!(cost.seats.len(), 1);
        assert_eq!(cost.seats[0].actor.as_deref(), Some(BUILDER));
        assert_eq!(cost.total_tokens, Some(145));
    }

    #[test]
    fn a_seat_prefix_nothing_matches_is_a_usage_error_not_a_silent_omission() {
        let error = fold_session_cost(&two_seat_channel(), None, Some("ffffffff"))
            .expect_err("an unmatched seat must be reported");
        assert_eq!(exit_code(&error), 1);
        assert!(error.to_string().contains("ffffffff"));
    }

    /// A seat that exists in the channel but not in the umbrella the caller
    /// narrowed to must fail loudly. Folding it to `None` would publish a
    /// milestone with no cost that reads exactly like a lane nothing measured.
    #[test]
    fn a_seat_outside_the_named_session_is_a_usage_error_not_an_empty_cost() {
        let error = fold_session_cost(&two_seat_channel(), Some(OTHER_SESSION), Some("cc000000"))
            .expect_err("a seat outside the umbrella must be reported");
        assert_eq!(exit_code(&error), 1);
    }

    #[test]
    fn a_session_ref_restricts_the_fold_to_that_umbrella() {
        let mut events = two_seat_channel();
        events.push(metadata_event(
            "s-other",
            1,
            Some(REFUTER),
            Some("refuter"),
            "sonnet-5",
            Some(OTHER_SESSION),
            12,
        ));
        events.push(result_event(
            "s-other",
            1,
            1,
            Some(usage(999, 999, 0, 0, 9)),
        ));

        let fold = fold_session_cost(&events, Some(COST_SESSION), None).expect("fold succeeds");
        let cost = fold.cost.expect("the umbrella reported usage");
        assert_eq!(cost.total_tokens, Some(155));
    }

    #[test]
    fn a_channel_with_no_usage_on_the_wire_yields_no_cost_never_zero() {
        let events = vec![
            metadata_event(
                "s-builder",
                1,
                Some(BUILDER),
                Some("builder"),
                "opus-5[1m]",
                Some(COST_SESSION),
                10,
            ),
            result_event("s-builder", 1, 1, None),
        ];
        let fold = fold_session_cost(&events, None, None).expect("fold succeeds");
        assert!(fold.cost.is_none(), "no usage means no cost, not a zero");
    }

    #[test]
    fn usage_from_an_execution_with_no_metadata_is_counted_but_never_attributed() {
        let events = vec![result_event("s-ghost", 1, 1, Some(usage(10, 10, 0, 0, 1)))];
        let fold = fold_session_cost(&events, None, None).expect("fold succeeds");
        assert!(fold.cost.is_none());
        assert_eq!(fold.unattributed_turns, 1);
    }

    #[test]
    fn a_folded_cost_survives_the_builder_that_signs_the_entry() {
        let fold = fold_session_cost(&two_seat_channel(), None, None).expect("fold succeeds");
        let entry = build_entry(
            crate::PulseKindArg::Milestone,
            "Lane B landed.".to_owned(),
            None,
            None,
            None,
            fold.cost,
        )
        .expect("payload assembles");
        beekeeper_sdk::builders::build_pulse_entry(
            &format!("30621:{}:demo", "a".repeat(64)),
            &entry,
            None,
            Some(COST_SESSION),
        )
        .expect("a cost-bearing entry builds and validates");
    }
}
