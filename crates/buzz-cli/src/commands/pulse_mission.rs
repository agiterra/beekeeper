//! `bee pulse missions` — the same rows Desktop shows, printed as text.
//!
//! **Both consumers call `render_pulse_mission_lines`.** Desktop renders the
//! strings this command prints into elements with testids and re-words nothing,
//! so the CLI and the app cannot say different things about the same mission.
//! Every sentence is composed in `buzz-core`; nothing here writes English.
//!
//! Nothing here asks anyone to report either: the rows come from the signed
//! 44244/44245 records, the hooks' 44246 observations, and the relay's own
//! 30618 ref state.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Value};

use buzz_core::pulse_mission::{
    fold_pulse_mission_row, open_rulings, pulse_mission_cap_disclosure, render_pulse_mission_lines,
    rulings_waiting_on_viewer, PulseMissionError, PulseMissionFacts, PulseMissionNames,
    PulseMissionRows, PulseMissionSources, MAX_PULSE_MISSION_ROWS, PULSE_MISSION_ROWS_SCHEMA,
    PULSE_MISSION_SCOPE,
};
use buzz_core::pulse_overlap::{fold_pulse_overlaps, render_pulse_overlap_rows, PulseOverlapSide};

use crate::client::BuzzClient;
use crate::commands::sessions::operations_reads::{fetch_session_authority, fetch_transactions};
use crate::commands::wip_refs::fetch_ref_state;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_uuid};

/// Relay filter for one umbrella's kind 44245 policy records.
fn policy_filter(channel: &str, session_ref: &str) -> Value {
    json!({
        "kinds": [buzz_core::kind::KIND_CODING_SESSION_POLICY],
        "#h": [channel],
        "#d": [session_ref],
    })
}

/// Relay filter for one umbrella's kind 44246 observations.
fn observation_filter(channel: &str, session_ref: &str) -> Value {
    json!({
        "kinds": [buzz_core::kind::KIND_CODING_SESSION_OBSERVATION],
        "#h": [channel],
        "#d": [session_ref],
    })
}

async fn fetch_signed(
    client: &BuzzClient,
    filter: Value,
    scope: &str,
    errors: &mut Vec<PulseMissionError>,
) -> Vec<nostr::Event> {
    match client.query_all(filter).await {
        Ok(rows) => rows
            .iter()
            .filter_map(|row| serde_json::from_value::<nostr::Event>(row.clone()).ok())
            .collect(),
        Err(error) => {
            // A failed read never renders as a quiet session.
            errors.push(PulseMissionError {
                scope: scope.to_owned(),
                message: error.to_string(),
            });
            Vec::new()
        }
    }
}

/// Display names for a set of pubkeys, read from their own kind-0 profiles.
///
/// The same field order every other reader in this repo uses — `display_name`,
/// else `name` — so a `{Who}` here is the `{Who}` Desktop renders for the same
/// key. Without this the CLI rendered every author as 8 hex while Desktop
/// rendered "Bob", which is two surfaces disagreeing about who did something
/// (REVIEW-L9 F4.3).
///
/// A failed profile read is **not** an error on the digest: a name nobody could
/// resolve falls back to the 8 hex the fold already carries, which is a weaker
/// rendering of the same fact rather than a wrong one.
pub async fn fetch_display_names(
    client: &BuzzClient,
    pubkeys: &BTreeSet<String>,
) -> BTreeMap<String, String> {
    let mut names = BTreeMap::new();
    if pubkeys.is_empty() {
        return names;
    }
    let authors: Vec<&str> = pubkeys.iter().map(String::as_str).collect();
    let filter = json!({
        "kinds": [0],
        "authors": authors,
        "limit": authors.len(),
    });
    let Ok(body) = client.query(&filter).await else {
        return names;
    };
    let events: Vec<Value> = serde_json::from_str(&body).unwrap_or_default();
    for event in &events {
        let Some(pubkey) = event.get("pubkey").and_then(Value::as_str) else {
            continue;
        };
        let Some(content) = event.get("content").and_then(Value::as_str) else {
            continue;
        };
        let Ok(profile) = serde_json::from_str::<Value>(content) else {
            continue;
        };
        let name = profile
            .get("display_name")
            .or_else(|| profile.get("name"))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|name| !name.is_empty());
        if let Some(name) = name {
            names.insert(pubkey.to_owned(), name.to_owned());
        }
    }
    names
}

/// Every pubkey one set of mission facts will render a `{Who}` for.
fn mission_pubkeys(facts: &[PulseMissionFacts]) -> BTreeSet<String> {
    let mut keys = BTreeSet::new();
    for row in facts {
        if let Some(waiting) = &row.waiting {
            keys.insert(waiting.asked_by.clone());
            if waiting.held_on != "founder" {
                keys.insert(waiting.held_on.clone());
            }
        }
        if let Some(verdict) = &row.verdict {
            keys.insert(verdict.author.clone());
        }
        if let Some(author) = &row.policy.author {
            keys.insert(author.clone());
        }
        for seat in &row.seats {
            keys.insert(seat.pubkey.clone());
        }
        for moved in &row.moved {
            keys.insert(moved.author_pubkey.clone());
        }
    }
    keys
}

// ── The eight sibling keys, for both consumers ───────────────────────────────

/// One umbrella a mission-row read will fold.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MissionSessionTarget {
    /// Umbrella key, as the Pulse digest keys sessions.
    pub session_key: String,
    /// Channel the records live in.
    pub channel: String,
    /// Canonical umbrella UUID.
    pub session_ref: String,
    /// Immutable session-genesis event id.
    pub genesis: String,
    /// The session's name, when the digest knows one.
    pub name: Option<String>,
    /// Newest durable observation time — display only.
    pub latest_observation_at: Option<i64>,
}

/// Find the genesis of every named session ref, one query per channel.
///
/// A session whose genesis is not in its channel is **left out with an error**,
/// never folded from a guessed genesis: the genesis id is what the whole
/// authority chain hangs off, and inventing one would attribute another
/// umbrella's seats to this row.
pub async fn discover_mission_sessions(
    client: &BuzzClient,
    channels: &[String],
    wanted: &BTreeMap<String, (Option<String>, Option<i64>)>,
    errors: &mut Vec<PulseMissionError>,
) -> Vec<MissionSessionTarget> {
    let mut targets: Vec<MissionSessionTarget> = Vec::new();
    for channel in channels {
        let filter = json!({
            "kinds": [buzz_core::kind::KIND_CODING_SESSION_GENESIS],
            "#h": [channel],
        });
        let events = match client.query_all(filter).await {
            Ok(events) => events,
            Err(error) => {
                errors.push(PulseMissionError {
                    scope: format!("genesis:{channel}"),
                    message: error.to_string(),
                });
                continue;
            }
        };
        for event in &events {
            let Some(id) = event.get("id").and_then(Value::as_str) else {
                continue;
            };
            let Some(session_ref) = genesis_session_ref(event) else {
                continue;
            };
            let Some((name, latest)) = wanted.get(&session_ref) else {
                continue;
            };
            if targets
                .iter()
                .any(|target| target.session_ref == session_ref)
            {
                continue;
            }
            targets.push(MissionSessionTarget {
                session_key: session_ref.clone(),
                channel: channel.clone(),
                session_ref,
                genesis: id.to_owned(),
                name: name.clone(),
                latest_observation_at: *latest,
            });
        }
    }
    // Newest observation first, ties on the key so two hosts agree.
    targets.sort_by(|left, right| {
        right
            .latest_observation_at
            .cmp(&left.latest_observation_at)
            .then_with(|| left.session_key.cmp(&right.session_key))
    });
    targets
}

/// The `csg-session` tag of a signature-stripped genesis event.
fn genesis_session_ref(event: &Value) -> Option<String> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(|tag| tag.as_array())
        .find(|tag| tag.first().and_then(Value::as_str) == Some("csg-session"))
        .and_then(|tag| tag.get(1))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

/// Fold every supplied umbrella into the eight sibling keys.
///
/// One reader for both consumers: `bee pulse missions` calls it with a single
/// target and `bee pulse digest` with the project's open sessions, so the two
/// commands cannot print different sentences about the same wire.
pub async fn compose_mission_rows(
    client: &BuzzClient,
    targets: &[MissionSessionTarget],
    open_session_count: usize,
    repo: Option<&str>,
    mut errors: Vec<PulseMissionError>,
) -> PulseMissionRows {
    let now_unix = chrono::Utc::now().timestamp();
    let viewer = Some(client.keys().public_key().to_hex());
    let ref_state = match repo {
        Some(repo) => fetch_ref_state(client, repo).await.unwrap_or_else(|error| {
            errors.push(PulseMissionError {
                scope: format!("refState:{repo}"),
                message: error.to_string(),
            });
            Vec::new()
        }),
        None => Vec::new(),
    };

    let mut facts: Vec<PulseMissionFacts> = Vec::new();
    let mut founders: BTreeMap<String, String> = BTreeMap::new();
    let mut sides: Vec<PulseOverlapSide> = Vec::new();

    for target in targets.iter().take(MAX_PULSE_MISSION_ROWS) {
        let authority = match fetch_session_authority(
            client,
            &target.channel,
            &target.session_ref,
            &target.genesis,
        )
        .await
        {
            Ok(authority) => authority,
            Err(error) => {
                // One session's authority read failing is one row's failure and
                // is named; the rest of the digest is untouched.
                errors.push(PulseMissionError {
                    scope: format!("authority:{}", target.session_key),
                    message: error.to_string(),
                });
                continue;
            }
        };
        founders.insert(
            target.session_key.clone(),
            authority.context.founder_pubkey.clone(),
        );
        let team_events = fetch_transactions(
            client,
            &target.channel,
            &target.session_ref,
            &target.genesis,
        )
        .await
        .unwrap_or_else(|error| {
            errors.push(PulseMissionError {
                scope: format!("missions:{}", target.channel),
                message: error.to_string(),
            });
            Vec::new()
        });
        let policy_events = fetch_signed(
            client,
            policy_filter(&target.channel, &target.session_ref),
            &format!("policy:{}", target.channel),
            &mut errors,
        )
        .await;
        // `PulseMissionSources::observation_events` is ascending — the fold's
        // newest-wins is last in the slice — and `query_all` hands pages over
        // in the relay's own order, newest first. Reverse here, once, with the
        // reason attached: fed as read, the digest showed a seat's *first* row
        // per gate as its current one (finding 79).
        let mut observation_events = fetch_signed(
            client,
            observation_filter(&target.channel, &target.session_ref),
            &format!("observations:{}", target.channel),
            &mut errors,
        )
        .await;
        observation_events.reverse();

        // `checkpoint.files` is Lane L5's key, so no checkpoint names a path
        // and no overlap side is built. The pairing runs anyway, over an empty
        // set, so the day the key lands the digest computes rows with no change
        // here.
        for event in &observation_events {
            if let Some(files) = buzz_core::pulse_overlap::pulse_checkpoint_files(event) {
                if !files.is_empty() {
                    sides.push(PulseOverlapSide {
                        session_key: target.session_key.clone(),
                        author_pubkey: event.pubkey.to_hex(),
                        sha: event.id.to_hex(),
                        as_of: Some(event.created_at.as_secs() as i64),
                        files,
                    });
                }
            }
        }

        let sources = PulseMissionSources {
            session_key: &target.session_key,
            channel_id: &target.channel,
            name: target.name.as_deref(),
            latest_observation_at: target.latest_observation_at,
            context: &authority.context,
            policy_grants: &authority.policy_grants,
            team_events: &team_events,
            policy_events: &policy_events,
            observation_events: &observation_events,
            ref_state: &ref_state,
            claimed_seats: &[],
            gate_source: None,
        };
        facts.push(fold_pulse_mission_row(&sources, now_unix));
    }

    if let Some(disclosure) = pulse_mission_cap_disclosure(open_session_count) {
        errors.push(disclosure);
    }

    let rendering = PulseMissionNames {
        names: fetch_display_names(client, &mission_pubkeys(&facts)).await,
        viewer: viewer.clone(),
    };
    let open = open_rulings(&facts);
    let founder_of = |session_key: &str| founders.get(session_key).cloned();
    let waiting = rulings_waiting_on_viewer(&open, viewer.as_deref(), &founder_of);

    PulseMissionRows {
        missions_schema: PULSE_MISSION_ROWS_SCHEMA.to_owned(),
        mission_scope: PULSE_MISSION_SCOPE.to_owned(),
        missions: facts
            .iter()
            .map(|row| render_pulse_mission_lines(row, &rendering, now_unix))
            .collect(),
        mission_errors: errors,
        open_rulings: open,
        rulings_waiting_on_viewer: waiting,
        overlaps: render_pulse_overlap_rows(&fold_pulse_overlaps(&sides), &rendering, now_unix),
        viewer_pubkey: viewer,
    }
}

/// The eight sibling keys as an object, for attaching beside a digest.
pub fn mission_rows_sibling_keys(rows: &PulseMissionRows) -> Value {
    serde_json::to_value(rows).unwrap_or(Value::Null)
}

/// `bee pulse missions --channel <uuid> --session-ref <uuid> --genesis <hex64>`.
pub async fn cmd_missions(
    client: &BuzzClient,
    channel: &str,
    session_ref: &str,
    genesis: &str,
    repo: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    validate_uuid(channel)?;
    validate_uuid(session_ref)?;
    validate_lower_hex64("--genesis", genesis)?;

    // One umbrella through the same reader `bee pulse digest` uses, so the two
    // commands cannot print different sentences about the same wire.
    let targets = vec![MissionSessionTarget {
        session_key: session_ref.to_owned(),
        channel: channel.to_owned(),
        session_ref: session_ref.to_owned(),
        genesis: genesis.to_owned(),
        name: None,
        latest_observation_at: None,
    }];
    let rows = compose_mission_rows(client, &targets, 1, repo, Vec::new()).await;

    match format {
        crate::OutputFormat::Compact => print_mission_text(&rows),
        crate::OutputFormat::Json => println!(
            "{}",
            serde_json::to_string_pretty(&rows)
                .map_err(|error| CliError::Other(error.to_string()))?
        ),
    }
    Ok(())
}

/// Print exactly the sentences Desktop renders, one per line, in row order.
///
/// No prose is added here and none is re-worded: this is the `text` of each
/// line and nothing else, which is what makes the golden test between the two
/// surfaces meaningful.
pub fn print_mission_text(rows: &PulseMissionRows) {
    for text in mission_text_lines(rows) {
        println!("{text}");
    }
}

/// The lines [`print_mission_text`] prints, as values a test can assert on.
pub fn mission_text_lines(rows: &PulseMissionRows) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for row in &rows.missions {
        for line in &row.lines {
            out.push(line.text.clone());
        }
        for seat in &row.seats {
            for line in &seat.lines {
                out.push(line.text.clone());
            }
        }
        for moved in &row.moved {
            for line in &moved.lines {
                out.push(line.text.clone());
            }
        }
        for line in &row.timing {
            out.push(line.text.clone());
        }
    }
    for overlap in &rows.overlaps {
        for line in &overlap.lines {
            out.push(line.text.clone());
        }
    }
    // The no-identity sentence is a `not-read` line the model composes, printed
    // above with every other mission line. It used to be appended here and
    // nowhere else, so the CLI said it and Desktop did not (REVIEW-L9 F4.1).
    //
    // An error's `scope` is a machine field for the JSON form; printing
    // `{scope}: {message}` here made the two consumers differ on the one
    // surface that is supposed to prove they cannot (REVIEW-L9 F4.2), so the
    // text form prints the message Desktop renders and nothing else.
    for error in &rows.mission_errors {
        out.push(error.message.clone());
    }
    out
}

#[cfg(test)]
#[path = "pulse_mission_tests.rs"]
mod tests;
