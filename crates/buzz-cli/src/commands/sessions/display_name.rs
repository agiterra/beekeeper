//! The umbrella display name `bee sessions list|show` print (SV-31).
//!
//! Nothing here decides which name wins. That is
//! [`buzz_core::coding_session_title::resolve_session_display_name`], pinned by
//! `conformance/session-display-name/`, and Pulse calls the same function
//! through the same JSON-to-record conversion
//! ([`buzz_core::pulse_fold::session_name_record_from_json`]). This module only
//! assembles the resolver's scope from what `sessions list` already resolves:
//!
//! - **the founder** is the genesis signer [`crew::build_founder_index`]
//!   proves for the umbrella's executions; executions that disagree leave it
//!   unknown, and an unknown founder makes no 44229 a person's name;
//! - **the executions** are the umbrella's confirmed rows — a lifecycle
//!   receipt from the row's signer names that exact generation — each with
//!   that signer as its provider authority;
//! - **the fallback** is the earliest-seen execution's 44223 title, then
//!   `Untitled session`.
//!
//! Every row says where its name came from (`nameOrigin`), and a generated
//! name also says which model wrote it and which provider key signed it, so a
//! model's words are never printed as though a person chose them.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{json, Map, Value};

use buzz_core::coding_session_title::{
    resolve_session_display_name, SessionDisplayName, SessionDisplayNameOrigin,
    SessionDisplayNameScope, SessionExecutionAuthority, SessionNameRecord, UNTITLED_SESSION_NAME,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_GENERATED_TITLE, KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME,
};
use buzz_core::pulse_fold::session_name_record_from_json;

use super::{crew, SessionRow};

/// Kinds `sessions list` and `sessions show` read.
///
/// 44221 and 44226 resolve the founder: a create says who asked for an
/// execution, and the genesis it names says who founded the umbrella. 44229
/// and 44252 are the two records a name can come from.
pub const SESSION_LIST_KINDS: &[u32] = &[
    KIND_CODING_SESSION_METADATA,
    KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_NAME,
    KIND_CODING_SESSION_GENERATED_TITLE,
];

/// The display name of every umbrella `rows` name, keyed by `sessionRef`.
///
/// `events` is the channel read; anything that is not a 44229 or 44252 is
/// ignored by the resolver.
pub fn umbrella_display_names(
    channel_id: &str,
    events: &[Value],
    rows: &[SessionRow],
    founders: &crew::FounderIndex,
) -> BTreeMap<String, SessionDisplayName> {
    let records: Vec<SessionNameRecord> = events
        .iter()
        .filter(|event| {
            matches!(
                event.get("kind").and_then(Value::as_u64),
                Some(kind) if kind == u64::from(KIND_CODING_SESSION_NAME)
                    || kind == u64::from(KIND_CODING_SESSION_GENERATED_TITLE)
            )
        })
        .filter_map(session_name_record_from_json)
        .collect();

    let mut umbrellas: BTreeMap<&str, Vec<&SessionRow>> = BTreeMap::new();
    for row in rows {
        if let Some(session_ref) = row.session_ref.as_deref() {
            umbrellas.entry(session_ref).or_default().push(row);
        }
    }
    umbrellas
        .into_iter()
        .map(|(session_ref, members)| {
            let scope = umbrella_scope(channel_id, session_ref, &members, founders);
            (
                session_ref.to_owned(),
                resolve_session_display_name(&scope, &records),
            )
        })
        .collect()
}

/// The resolver's scope for one umbrella, from its rows.
pub fn umbrella_scope(
    channel_id: &str,
    session_ref: &str,
    members: &[&SessionRow],
    founders: &crew::FounderIndex,
) -> SessionDisplayNameScope {
    let proven: BTreeSet<String> = members
        .iter()
        .filter_map(|row| founders.of(&row.target).founder)
        .collect();
    let founder_pubkey = match proven.len() {
        1 => proven.into_iter().next(),
        _ => None,
    };
    scope_from_rows(channel_id, session_ref, members, founder_pubkey)
}

/// [`umbrella_scope`] with the founder already decided: the founding title
/// and the standing executions, from the rows alone.
pub fn scope_from_rows(
    channel_id: &str,
    session_ref: &str,
    members: &[&SessionRow],
    founder_pubkey: Option<String>,
) -> SessionDisplayNameScope {
    // Only a confirmed row may name the umbrella: an unconfirmed 44223 is
    // anyone's claim, and a backdated one would otherwise win the
    // earliest-row tie and print a stranger's title as the session's name.
    let founding_execution_title = members
        .iter()
        .filter(|row| row.confirmed)
        .min_by(|left, right| {
            left.created_at
                .cmp(&right.created_at)
                .then(left.target_key.cmp(&right.target_key))
        })
        .and_then(|row| row.title.clone());
    let executions = members
        .iter()
        .filter(|row| row.confirmed)
        .map(|row| SessionExecutionAuthority {
            target_key: row.target_key.clone(),
            provider_authority_pubkey: row.signer.clone(),
        })
        .collect();
    SessionDisplayNameScope {
        channel_id: channel_id.to_owned(),
        session_ref: session_ref.to_owned(),
        founder_pubkey,
        founding_execution_title,
        executions,
    }
}

/// The name of a row that belongs to no umbrella: its own 44223 title, else
/// `Untitled session`. Nothing can rename it — a 44229 or 44252 is keyed by
/// `sessionRef` — so this is always the fallback tier.
pub fn solo_display_name(row: &SessionRow) -> SessionDisplayName {
    let name = row
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or(UNTITLED_SESSION_NAME)
        .to_owned();
    SessionDisplayName {
        name,
        origin: SessionDisplayNameOrigin::Fallback,
        model: None,
        signer_pubkey: None,
        diagnostics: Default::default(),
    }
}

/// The wire spelling of an origin: `person`, `generated` or `fallback`.
pub fn origin_str(origin: SessionDisplayNameOrigin) -> &'static str {
    match origin {
        SessionDisplayNameOrigin::Person => "person",
        SessionDisplayNameOrigin::Generated => "generated",
        SessionDisplayNameOrigin::Fallback => "fallback",
    }
}

/// The name keys a row carries.
///
/// `name` and `nameOrigin` always; `nameModel` and `nameSigner` only for a
/// generated name, where they are the honest attribution (the signer short in
/// compact output, full in JSON). JSON output also carries `nameDiagnostics`:
/// the 44229/44252 records the resolver set aside, so a stranger's rename is
/// counted rather than silently dropped.
pub fn name_fields(resolved: &SessionDisplayName, compact: bool) -> Map<String, Value> {
    let mut fields = Map::new();
    fields.insert("name".into(), json!(resolved.name));
    fields.insert("nameOrigin".into(), json!(origin_str(resolved.origin)));
    if resolved.origin == SessionDisplayNameOrigin::Generated {
        fields.insert("nameModel".into(), json!(resolved.model));
        let signer = resolved.signer_pubkey.as_deref().map(|signer| {
            if compact {
                crew::short_pubkey(signer)
            } else {
                signer.to_owned()
            }
        });
        fields.insert("nameSigner".into(), json!(signer));
    }
    if !compact {
        fields.insert("nameDiagnostics".into(), json!(resolved.diagnostics));
    }
    fields
}

/// What `bee sessions show` prints for one umbrella.
///
/// `members` are the umbrella's rows, newest activity first as `sessions list`
/// orders them. Pure, so the shape is tested without a relay.
pub fn show_output(
    session_ref: &str,
    resolved: &SessionDisplayName,
    founder: Option<&str>,
    members: &[&SessionRow],
    compact: bool,
) -> Value {
    let mut object = Map::new();
    object.insert("sessionRef".into(), json!(session_ref));
    object.extend(name_fields(resolved, compact));
    object.insert(
        "founder".into(),
        json!(founder.map(|founder| if compact {
            crew::short_pubkey(founder)
        } else {
            founder.to_owned()
        })),
    );
    let executions: Vec<Value> = members
        .iter()
        .map(|row| {
            if compact {
                json!({
                    "target": row.target_key,
                    "title": row.title,
                    "status": row.status,
                    "model": row.model,
                })
            } else {
                json!({
                    "target": row.target_key,
                    "signer": row.signer,
                    "title": row.title,
                    "status": row.status,
                    "model": row.model,
                    "confirmed": row.confirmed,
                    "createdAt": super::rfc3339(row.created_at),
                    "lastEventAt": super::rfc3339(row.last_event_at),
                })
            }
        })
        .collect();
    object.insert("executions".into(), Value::Array(executions));
    Value::Object(object)
}

/// `bee sessions show --channel <uuid> --session-ref <uuid>`: one umbrella,
/// its display name and where that name came from, and its executions.
pub async fn cmd_show(
    client: &crate::client::BuzzClient,
    channel_id: &str,
    session_ref: &str,
    format: &crate::OutputFormat,
) -> Result<(), crate::error::CliError> {
    crate::validate::validate_uuid(channel_id)?;
    crate::validate::validate_uuid(session_ref)?;
    let events = super::fetch_channel_events(client, channel_id, SESSION_LIST_KINDS).await?;
    let (metadata, _) = super::decode_metadata(&events);
    let (receipts, _) = super::decode_receipts(&events);
    let founders = crew::build_founder_index(&events, &receipts);
    let rows = super::resolve_sessions(&metadata, &receipts, &[]);
    let members: Vec<&SessionRow> = rows
        .iter()
        .filter(|row| row.session_ref.as_deref() == Some(session_ref))
        .collect();
    if members.is_empty() {
        return Err(crate::error::CliError::NotFound(format!(
            "no coding session with sessionRef '{session_ref}' in channel {channel_id}"
        )));
    }
    let scope = umbrella_scope(channel_id, session_ref, &members, &founders);
    let records: Vec<SessionNameRecord> = events
        .iter()
        .filter_map(session_name_record_from_json)
        .collect();
    let resolved = resolve_session_display_name(&scope, &records);
    let compact = matches!(format, crate::OutputFormat::Compact);
    println!(
        "{}",
        show_output(
            session_ref,
            &resolved,
            scope.founder_pubkey.as_deref(),
            &members,
            compact
        )
    );
    Ok(())
}

#[cfg(test)]
#[path = "display_name_tests.rs"]
mod tests;
