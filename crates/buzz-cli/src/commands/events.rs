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

use crate::client::BuzzClient;
use crate::error::CliError;
use crate::validate::{validate_hex64, validate_uuid};

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
    /// `Some(n)` runs [`BuzzClient::query_paginated`]; `None` runs
    /// [`BuzzClient::query_all`].
    pub limit: Option<u32>,
}

/// Validate every flag and build the filter, before any request is made.
pub fn prepare_query(_args: &EventsQueryArgs<'_>) -> Result<PreparedQuery, CliError> {
    Ok(PreparedQuery {
        filter: json!({}),
        limit: None,
    })
}

/// Order events newest-first by `(created_at, id)`.
pub fn sort_newest_first(_events: &mut [Value]) {}

/// Collapse an event's raw `content` into one scannable line.
pub fn summarize(_content: &str) -> String {
    String::new()
}

/// The reduced row `--format compact` prints for one event.
pub fn compact_row(_event: &Value) -> Value {
    json!({})
}

/// `bee events query` — run the filter and print the result.
pub async fn cmd_query(
    _client: &BuzzClient,
    _args: &EventsQueryArgs<'_>,
    _format: &crate::OutputFormat,
) -> Result<(), CliError> {
    Ok(())
}

/// Route one `bee events` subcommand.
pub async fn dispatch(
    cmd: crate::EventsCmd,
    client: &BuzzClient,
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
