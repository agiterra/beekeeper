//! `bee events query` — one raw authenticated REQ against the relay.
//!
//! Everything else in `bee` decodes a contract: `sessions` folds coding-session
//! events into executions, `messages` renders NIP-29 chat. This command decodes
//! nothing. It builds a Nostr filter from flags, runs it through the relay's
//! `/query` bridge with the same NIP-98 auth every other read uses, and prints
//! what came back — signature included, unparsed.
//!
//! Two rules make that honest rather than merely raw:
//!
//! 1. **`--kinds` is refused locally when absent.** An open-ended filter trips
//!    the relay's p-gate and comes back 403, so a CLI that forwarded it would
//!    turn a knowable input error into a network round-trip and an auth-shaped
//!    failure. The refusal names the reason.
//! 2. **`--format compact` never parses `content`.** The reduced row carries a
//!    summary of the raw content string, so an event whose payload is malformed
//!    — precisely the event a debugging query is usually looking for — still
//!    prints a row instead of vanishing.

use serde_json::{json, Value};

use crate::client::BeekeeperClient;
use crate::error::CliError;
use crate::validate::{validate_lower_hex64, validate_uuid};

/// Refusal printed when a filter would name no kinds.
///
/// Verbatim product copy: the relay's p-gate answers 403 to a kindless filter,
/// so the CLI says that rather than letting the caller discover it as an auth
/// error.
pub const KINDS_REQUIRED: &str =
    "--kinds is required: a filter with no kinds is refused by the relay with 403.";

/// Longest `summary` a compact row carries, in characters.
pub const SUMMARY_CHARS: usize = 120;

/// The flags `bee events query` was invoked with, before validation.
#[derive(Debug, Clone, Default)]
pub struct EventsQueryArgs<'a> {
    /// Comma-separated kind integers. Empty or absent is refused.
    pub kinds: Option<&'a str>,
    /// Channel UUID; written to the filter's `#h` key.
    pub channel: Option<&'a str>,
    /// Raw `#h` tag value, for an h-scope that is not a channel UUID.
    pub h: Option<&'a str>,
    /// Comma-separated author pubkeys, 64-char lowercase hex.
    pub authors: Option<&'a str>,
    /// Comma-separated event ids, 64-char lowercase hex.
    pub ids: Option<&'a str>,
    /// RFC 3339 timestamp or Unix seconds.
    pub since: Option<&'a str>,
    /// RFC 3339 timestamp or Unix seconds.
    pub until: Option<&'a str>,
    /// Stop after this many events; absent pages the whole result.
    pub limit: Option<u32>,
}

/// A validated filter and the page budget it runs under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreparedQuery {
    /// The Nostr filter, exactly as it goes to `POST /query`.
    pub filter: Value,
    /// `Some(n)` runs [`BeekeeperClient::query_paginated`]; `None` runs
    /// [`BeekeeperClient::query_all`].
    pub limit: Option<u32>,
}

/// Split a comma-separated flag value into non-empty trimmed items.
///
/// An empty item is kept as an empty string rather than dropped, so
/// `--authors a,,b` is refused by the validator below instead of silently
/// becoming a two-author filter.
fn split_list(raw: &str) -> Vec<&str> {
    raw.split(',').map(str::trim).collect()
}

/// Parse `--since`/`--until`: RFC 3339 first, then Unix seconds.
fn parse_time(flag: &str, raw: &str) -> Result<i64, CliError> {
    if let Ok(unix) = raw.trim().parse::<i64>() {
        return Ok(unix);
    }
    chrono::DateTime::parse_from_rfc3339(raw.trim())
        .map(|time| time.timestamp())
        .map_err(|error| {
            CliError::Usage(format!(
                "{flag} must be an RFC 3339 timestamp or Unix seconds: {raw} ({error})"
            ))
        })
}

/// Validate a comma-separated list of 64-character lowercase hex values.
fn hex64_list(flag: &str, raw: &str) -> Result<Vec<String>, CliError> {
    let items = split_list(raw);
    for item in &items {
        validate_lower_hex64(flag, item)?;
    }
    Ok(items.into_iter().map(str::to_owned).collect())
}

/// Validate every flag and build the filter, before any request is made.
///
/// Nothing here touches the network: a caller who mistypes a pubkey learns it
/// from the CLI rather than from a 200 with zero rows, which is
/// indistinguishable from "nothing matched".
pub fn prepare_query(args: &EventsQueryArgs<'_>) -> Result<PreparedQuery, CliError> {
    let kinds_raw = args.kinds.unwrap_or("");
    let kind_items: Vec<&str> = split_list(kinds_raw)
        .into_iter()
        .filter(|item| !item.is_empty())
        .collect();
    if kind_items.is_empty() {
        return Err(CliError::Usage(KINDS_REQUIRED.to_owned()));
    }
    let mut kinds = Vec::with_capacity(kind_items.len());
    for item in kind_items {
        kinds.push(item.parse::<u32>().map_err(|error| {
            CliError::Usage(format!("--kinds must be integers: {item} ({error})"))
        })?);
    }

    let mut filter = json!({ "kinds": kinds });

    // `--channel` and `--h` write the same key; clap already refuses both at
    // once, and this repeats the refusal so the pure function cannot be
    // handed a contradiction by a future caller.
    match (args.channel, args.h) {
        (Some(_), Some(_)) => {
            return Err(CliError::Usage(
                "--channel and --h both write the filter's `#h` key — pass one".to_owned(),
            ))
        }
        (Some(channel), None) => {
            validate_uuid(channel)?;
            filter["#h"] = json!([channel]);
        }
        (None, Some(h)) => {
            if h.trim().is_empty() {
                return Err(CliError::Usage("--h must not be empty".to_owned()));
            }
            filter["#h"] = json!([h]);
        }
        (None, None) => {}
    }

    if let Some(authors) = args.authors {
        filter["authors"] = json!(hex64_list("--authors", authors)?);
    }
    if let Some(ids) = args.ids {
        filter["ids"] = json!(hex64_list("--ids", ids)?);
    }
    if let Some(since) = args.since {
        filter["since"] = json!(parse_time("--since", since)?);
    }
    if let Some(until) = args.until {
        filter["until"] = json!(parse_time("--until", until)?);
    }

    Ok(PreparedQuery {
        filter,
        limit: args.limit,
    })
}

/// Order events newest-first by `(created_at, id)`.
///
/// `created_at` alone is second-granularity, and a provider legitimately signs
/// several events inside one second; breaking the tie on id makes the printed
/// order stable across runs instead of however the relay's pages happened to
/// arrive.
pub fn sort_newest_first(events: &mut [Value]) {
    events.sort_by(|left, right| {
        let key = |event: &Value| {
            (
                event.get("created_at").and_then(Value::as_i64).unwrap_or(0),
                event
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            )
        };
        key(right).cmp(&key(left))
    });
}

/// Collapse an event's raw `content` into one scannable line.
///
/// Every run of whitespace — newlines included — becomes a single space, and
/// the result is cut at [`SUMMARY_CHARS`] *characters* so a multi-byte
/// character cannot be split in half. The content is never parsed: this is a
/// preview of the bytes the signer signed, not of a payload the CLI believes
/// in.
pub fn summarize(content: &str) -> String {
    let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(SUMMARY_CHARS).collect()
}

/// The first `h` tag value on an event, or `None`.
fn h_tag(event: &Value) -> Option<&str> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .find(|tag| tag.first().and_then(Value::as_str) == Some("h"))
        .and_then(|tag| tag.get(1))
        .and_then(Value::as_str)
}

/// The reduced row `--format compact` prints for one event.
///
/// Built from event fields only — id, kind, pubkey, `created_at`, the `h` tag,
/// and a summary of the raw content string. A malformed payload therefore
/// still produces a row, which is the whole point: the events worth querying
/// raw are usually the ones that did not decode.
pub fn compact_row(event: &Value) -> Value {
    json!({
        "id": event.get("id").cloned().unwrap_or(Value::Null),
        "kind": event.get("kind").cloned().unwrap_or(Value::Null),
        "pubkey": event.get("pubkey").cloned().unwrap_or(Value::Null),
        "createdAt": event
            .get("created_at")
            .and_then(Value::as_i64)
            .and_then(|seconds| chrono::DateTime::from_timestamp(seconds, 0))
            .map(|time| Value::String(time.to_rfc3339()))
            .unwrap_or(Value::Null),
        "h": h_tag(event).map(Value::from).unwrap_or(Value::Null),
        "summary": event
            .get("content")
            .and_then(Value::as_str)
            .map(|content| Value::String(summarize(content)))
            .unwrap_or(Value::Null),
    })
}

/// `bee events query` — run the filter and print the result.
///
/// An empty result prints `[]` and exits 0: "nothing matched" is an answer,
/// not a failure.
pub async fn cmd_query(
    client: &BeekeeperClient,
    args: &EventsQueryArgs<'_>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let prepared = prepare_query(args)?;
    let mut events = match prepared.limit {
        Some(limit) => {
            client
                .query_paginated(prepared.filter.clone(), limit)
                .await?
        }
        None => client.query_all(prepared.filter.clone()).await?,
    };
    sort_newest_first(&mut events);
    let output: Vec<Value> = match format {
        crate::OutputFormat::Compact => events.iter().map(compact_row).collect(),
        crate::OutputFormat::Json => events,
    };
    println!("{}", Value::Array(output));
    Ok(())
}

/// Route one `bee events` subcommand.
pub async fn dispatch(
    cmd: crate::EventsCmd,
    client: &BeekeeperClient,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    match cmd {
        crate::EventsCmd::Query {
            kinds,
            channel,
            h,
            authors,
            ids,
            since,
            until,
            limit,
        } => {
            cmd_query(
                client,
                &EventsQueryArgs {
                    kinds: kinds.as_deref(),
                    channel: channel.as_deref(),
                    h: h.as_deref(),
                    authors: authors.as_deref(),
                    ids: ids.as_deref(),
                    since: since.as_deref(),
                    until: until.as_deref(),
                    limit,
                },
                format,
            )
            .await
        }
    }
}

#[cfg(test)]
mod tests;
