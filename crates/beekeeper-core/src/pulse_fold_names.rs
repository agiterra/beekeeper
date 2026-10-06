//! Pulse's umbrella display name, resolved by the one shared resolver.
//!
//! Until SV-31 the Pulse fold kept the newest kind:44229 per `(h, d)` from
//! **any** signer, so a stranger's rename showed as the session's name
//! (`plans/SESSION_PARITY_SPEC_AUTOTITLE.md`, Risk 1). The rule now lives in
//! one place — [`resolve_session_display_name`] in
//! `crates/beekeeper-core/src/coding_session_title.rs`, pinned by
//! `conformance/session-display-name/` — and this module only assembles its
//! inputs from what the fold has already proven:
//!
//! - **the founder** is the signer of the 44226 genesis that the umbrella's
//!   accepted `session.create` names by event id, in the same channel and for
//!   the same `sessionRef`. Two accepted creates naming different founders
//!   leave the founder unknown, and an unknown founder makes no 44229 a
//!   person's name;
//! - **the executions** are the fold's own authority-proven generations, each
//!   with the provider key its receipt proved.
//!
//! The digest carries a person's name or a generated title, never the
//! fallback: `name: null` still means "nothing names this session", and a
//! reader picks its own placeholder.

use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::coding_session_genesis::decode_coding_session_genesis;
use crate::coding_session_title::{
    resolve_session_display_name, SessionDisplayName, SessionDisplayNameOrigin,
    SessionDisplayNameScope, SessionExecutionAuthority, SessionNameRecord,
};
use crate::kind::{
    KIND_CODING_SESSION_GENERATED_TITLE, KIND_CODING_SESSION_GENESIS, KIND_CODING_SESSION_NAME,
};

/// A relay row (signature-stripped Nostr event JSON) as the resolver reads it,
/// or `None` when a field is missing or the wrong type.
///
/// Shared with `bee sessions list|show`, so both readers hand the resolver the
/// same records from the same JSON.
pub fn session_name_record_from_json(event: &Value) -> Option<SessionNameRecord> {
    let tags = event
        .get("tags")?
        .as_array()?
        .iter()
        .map(|tag| {
            tag.as_array()?
                .iter()
                .map(|part| part.as_str().map(str::to_owned))
                .collect::<Option<Vec<String>>>()
        })
        .collect::<Option<Vec<Vec<String>>>>()?;
    Some(SessionNameRecord {
        id: event.get("id")?.as_str()?.to_owned(),
        pubkey: event.get("pubkey")?.as_str()?.to_owned(),
        created_at: event.get("created_at")?.as_u64()?,
        kind: u32::try_from(event.get("kind")?.as_u64()?).ok()?,
        tags,
        content: event.get("content")?.as_str()?.to_owned(),
    })
}

/// The event id of the record that won `resolved`, found by asking the same
/// resolver about each candidate alone.
///
/// The person tier keeps the greatest valid founder record and the generated
/// tier the smallest standing one, both by `(created_at, id)`; a record that
/// resolves to the winning tier on its own is exactly a record that tier
/// considered. No second validity rule is written here.
pub fn winning_name_event_id(
    scope: &SessionDisplayNameScope,
    records: &[SessionNameRecord],
    resolved: &SessionDisplayName,
) -> Option<String> {
    let alone = |record: &SessionNameRecord| {
        resolve_session_display_name(scope, std::iter::once(record)).origin == resolved.origin
    };
    let order = |record: &&SessionNameRecord| (record.created_at, record.id.clone());
    let candidates = records.iter().filter(|record| alone(record));
    match resolved.origin {
        SessionDisplayNameOrigin::Person => candidates.max_by_key(order),
        SessionDisplayNameOrigin::Generated => candidates.min_by_key(order),
        SessionDisplayNameOrigin::Fallback => None,
    }
    .map(|record| record.id.clone())
}

/// Name inputs gathered in one pass over the fold's events.
#[derive(Default)]
pub(super) struct NameInputs {
    /// 44229 and 44252 records by `(h, d)`.
    records: HashMap<(String, String), Vec<SessionNameRecord>>,
    /// 44226 genesis by event id: `(channel, signer, sessionRef)`.
    geneses: HashMap<String, (String, String, String)>,
}

impl NameInputs {
    /// Offer one relay row; anything not a name, title or genesis is ignored.
    pub(super) fn observe(&mut self, kind: u32, channel_id: &str, event: &Value) {
        if kind == KIND_CODING_SESSION_NAME || kind == KIND_CODING_SESSION_GENERATED_TITLE {
            let Some(session_ref) = first_tag(event, "d") else {
                return;
            };
            if let Some(record) = session_name_record_from_json(event) {
                self.records
                    .entry((channel_id.to_owned(), session_ref.to_owned()))
                    .or_default()
                    .push(record);
            }
        } else if kind == KIND_CODING_SESSION_GENESIS {
            let (Some(id), Some(pubkey), Some(content)) = (
                event.get("id").and_then(Value::as_str),
                event.get("pubkey").and_then(Value::as_str),
                event.get("content").and_then(Value::as_str),
            ) else {
                return;
            };
            if let Ok(genesis) = decode_coding_session_genesis(content) {
                self.geneses.insert(
                    id.to_owned(),
                    (
                        channel_id.to_owned(),
                        pubkey.to_ascii_lowercase(),
                        genesis.session_ref,
                    ),
                );
            }
        }
    }

    /// The founder a genesis reference proves for `(channel, sessionRef)`, or
    /// `None` when the genesis is absent, in another channel, or founds a
    /// different umbrella.
    pub(super) fn founder_of(
        &self,
        channel_id: &str,
        session_ref: &str,
        genesis_ref: &str,
    ) -> Option<String> {
        let (channel, signer, founded) = self.geneses.get(genesis_ref)?;
        (channel == channel_id && founded == session_ref).then(|| signer.clone())
    }

    /// Resolve one umbrella. `founders` is every founder its accepted creates
    /// proved; more than one is a dispute and leaves the founder unknown.
    pub(super) fn resolve(
        &self,
        channel_id: &str,
        session_ref: &str,
        founders: &HashSet<String>,
        executions: Vec<SessionExecutionAuthority>,
    ) -> Option<(SessionDisplayName, Option<String>)> {
        let founder_pubkey = match founders.len() {
            1 => founders.iter().next().cloned(),
            _ => None,
        };
        let scope = SessionDisplayNameScope {
            channel_id: channel_id.to_owned(),
            session_ref: session_ref.to_owned(),
            founder_pubkey,
            founding_execution_title: None,
            executions,
        };
        let records = self
            .records
            .get(&(channel_id.to_owned(), session_ref.to_owned()))
            .map(Vec::as_slice)
            .unwrap_or_default();
        let resolved = resolve_session_display_name(&scope, records);
        if resolved.origin == SessionDisplayNameOrigin::Fallback {
            return None;
        }
        let event_id = winning_name_event_id(&scope, records, &resolved);
        Some((resolved, event_id))
    }
}

fn first_tag<'a>(event: &'a Value, name: &str) -> Option<&'a str> {
    event
        .get("tags")?
        .as_array()?
        .iter()
        .filter_map(Value::as_array)
        .find(|parts| parts.first().and_then(Value::as_str) == Some(name))
        .and_then(|parts| parts.get(1))
        .and_then(Value::as_str)
}

#[cfg(test)]
#[path = "pulse_fold_names_tests.rs"]
mod tests;
