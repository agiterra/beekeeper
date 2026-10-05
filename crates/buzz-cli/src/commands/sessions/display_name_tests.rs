//! `bee sessions list|show` name an umbrella through buzz-core's resolver
//! (SV-31): a person's name beats any generated title, the earliest generated
//! title wins, and a stranger's record is never the name.

use serde_json::{json, Value};

use buzz_core::coding_session_command::{coding_session_target_key, CodingSessionTarget};
use buzz_core::coding_session_genesis::CodingSessionGenesisPayload;
use buzz_core::coding_session_lifecycle_command::{
    CodingSessionLifecycleAction, CodingSessionLifecycleCommandPayload,
    CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA,
};
use buzz_core::coding_session_payload::{
    Capabilities, LifecycleReceipt, ReceiptStatus, SessionMetadata, SessionStatus,
    LIFECYCLE_RECEIPT_SCHEMA, METADATA_SCHEMA,
};
use buzz_core::coding_session_title::{
    parse_coding_session_target_key, resolve_session_display_name, SessionDisplayNameScope,
    SessionNameRecord,
};
use buzz_core::kind::{
    KIND_CODING_SESSION_GENERATED_TITLE, KIND_CODING_SESSION_GENESIS,
    KIND_CODING_SESSION_LIFECYCLE_COMMAND, KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    KIND_CODING_SESSION_METADATA, KIND_CODING_SESSION_NAME,
};
use buzz_core::pulse_fold::session_name_record_from_json;

use super::super::{crew, decode_metadata, decode_receipts, resolve_sessions, SessionRow};
use super::*;

const CHANNEL: &str = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const UMBRELLA: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const FOUNDER: &str = "f0";
const PROVIDER: &str = "a1";
const OTHER_PROVIDER: &str = "b2";
const STRANGER: &str = "c3";

fn pk(seed: &str) -> String {
    seed.repeat(32)
}

fn hex_id(n: u32) -> String {
    format!("{n:064x}")
}

fn target(session_id: &str) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude-agent-acp".into(),
        instance_id: "instance-1".into(),
        session_id: session_id.into(),
        generation: 1,
    }
}

// ── Event builders ───────────────────────────────────────────────────────────

fn genesis() -> Value {
    json!({
        "id": hex_id(0xa0), "pubkey": pk(FOUNDER), "kind": KIND_CODING_SESSION_GENESIS,
        "created_at": 500, "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["csg-v", "csg1-1"], ["csg-session", UMBRELLA]],
        "content": serde_json::to_string(&CodingSessionGenesisPayload::new(UMBRELLA))
            .unwrap_or_default(),
    })
}

/// A founder-signed create, its provider receipt, and the provider's
/// metadata naming the umbrella — one confirmed execution with a founder.
fn execution(session_id: &str, provider: &str, at: i64, title: Option<&str>) -> Vec<Value> {
    let target = target(session_id);
    let command_id = format!("create-{session_id}");
    let create = CodingSessionLifecycleCommandPayload {
        schema: CODING_SESSION_LIFECYCLE_COMMAND_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        action: CodingSessionLifecycleAction::SessionCreate {
            project_ref: None,
            repo_ref: None,
            session_ref: Some(UMBRELLA.to_owned()),
            genesis_ref: Some(hex_id(0xa0)),
            provider_instance_ref: "instance-1".try_into().unwrap_or_else(|_| unreachable!()),
            provider_authority_pubkey: pk(provider),
            model: Some("claude-opus".into()),
            title: title.map(str::to_owned),
            initial_turn: None,
            actor: None,
            role: None,
            hire_ref: None,
            routing: None,
        },
    };
    let receipt = LifecycleReceipt {
        schema: LIFECYCLE_RECEIPT_SCHEMA.to_owned(),
        command_id: command_id.clone(),
        status: ReceiptStatus::Created,
        session: Some(target.clone()),
        error: None,
        turn_id: None,
    };
    let metadata = SessionMetadata {
        schema: METADATA_SCHEMA.to_owned(),
        session: target.clone(),
        project_ref: None,
        repo_ref: None,
        title: title.map(str::to_owned),
        agent_ref: None,
        role: None,
        provider: Some(
            "claude-primary"
                .try_into()
                .unwrap_or_else(|_| unreachable!()),
        ),
        runtime: Some("claude".try_into().unwrap_or_else(|_| unreachable!())),
        model: Some("claude-opus".into()),
        status: SessionStatus::Idle,
        branch: None,
        capabilities: Capabilities::v1_claude(),
        session_ref: Some(UMBRELLA.to_owned()),
        observed_commit: None,
        dirty: None,
        relay_reachable: None,
        verified_at: None,
        turn_budget: None,
        routing: None,
        bee_stamp: None,
        pack_ref: None,
        handover: None,
        compose_ref: None,
    };
    vec![
        json!({
            "id": format!("c-{session_id}"), "pubkey": pk(FOUNDER),
            "kind": KIND_CODING_SESSION_LIFECYCLE_COMMAND, "created_at": at,
            "sig": "0".repeat(128), "tags": [["h", CHANNEL], ["csl-v", "csl1-1"]],
            "content": serde_json::to_string(&create).unwrap_or_default(),
        }),
        json!({
            "id": format!("r-{session_id}"), "pubkey": pk(provider),
            "kind": KIND_CODING_SESSION_LIFECYCLE_RECEIPT, "created_at": at + 1,
            "sig": "0".repeat(128), "tags": [["h", CHANNEL], ["cslr-v", "cslr1-1"]],
            "content": serde_json::to_string(&receipt).unwrap_or_default(),
        }),
        json!({
            "id": format!("m-{session_id}"), "pubkey": pk(provider),
            "kind": KIND_CODING_SESSION_METADATA, "created_at": at + 2,
            "sig": "0".repeat(128),
            "tags": [["h", CHANNEL], ["csm-v", "csm1-1"],
                     ["cs-target", coding_session_target_key(&target)]],
            "content": serde_json::to_string(&metadata).unwrap_or_default(),
        }),
    ]
}

fn name(n: u32, signer: &str, at: i64, content: &str) -> Value {
    json!({
        "id": hex_id(n), "pubkey": pk(signer), "kind": KIND_CODING_SESSION_NAME,
        "created_at": at, "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["d", UMBRELLA], ["csnm-v", "csnm1-1"]],
        "content": content,
    })
}

fn title(n: u32, signer: &str, session_id: &str, at: i64, text: &str) -> Value {
    json!({
        "id": hex_id(n), "pubkey": pk(signer), "kind": KIND_CODING_SESSION_GENERATED_TITLE,
        "created_at": at, "sig": "0".repeat(128),
        "tags": [["h", CHANNEL], ["d", UMBRELLA], ["cstl-v", "cstl1-1"],
                 ["cs-target", coding_session_target_key(&target(session_id))]],
        "content": json!({
            "schema": "buzz-coding-session-title/v1",
            "title": text,
            "model": "haiku",
            "basis": "first-message",
            "sourceCommand": null,
            "createEventId": hex_id(0xc0),
        }).to_string(),
    })
}

/// What `sessions list` resolves for the umbrella, read the way `cmd_list`
/// reads it.
fn resolved_for(events: &[Value]) -> buzz_core::coding_session_title::SessionDisplayName {
    let (metadata, _) = decode_metadata(events);
    let (receipts, _) = decode_receipts(events);
    let founders = crew::build_founder_index(events, &receipts);
    let rows = resolve_sessions(&metadata, &receipts, &[]);
    let mut names = umbrella_display_names(CHANNEL, events, &rows, &founders);
    names
        .remove(UMBRELLA)
        .unwrap_or_else(|| panic!("umbrella resolved: {rows:?}"))
}

fn two_executions() -> Vec<Value> {
    let mut events = vec![genesis()];
    events.extend(execution(
        "sess-a",
        PROVIDER,
        600,
        Some("Fix the login redirect"),
    ));
    events.extend(execution("sess-b", OTHER_PROVIDER, 700, None));
    events
}

// ── The rule, through the CLI read ──────────────────────────────────────────

#[test]
fn an_older_person_name_beats_a_newer_generated_title() {
    let mut events = two_executions();
    events.push(name(0x201, FOUNDER, 800, "Auth rework"));
    events.push(title(0x301, PROVIDER, "sess-a", 900, "Login redirect fix"));
    let resolved = resolved_for(&events);
    assert_eq!(resolved.name, "Auth rework");
    assert_eq!(origin_str(resolved.origin), "person");
    assert_eq!(resolved.model, None);
}

#[test]
fn the_earliest_generated_title_wins_and_says_who_wrote_it() {
    let mut events = two_executions();
    events.push(title(0x302, OTHER_PROVIDER, "sess-b", 950, "Later title"));
    events.push(title(0x301, PROVIDER, "sess-a", 900, "Login redirect fix"));
    let resolved = resolved_for(&events);
    assert_eq!(resolved.name, "Login redirect fix");
    assert_eq!(origin_str(resolved.origin), "generated");
    assert_eq!(resolved.model.as_deref(), Some("haiku"));
    assert_eq!(
        resolved.signer_pubkey.as_deref(),
        Some(pk(PROVIDER).as_str())
    );
}

#[test]
fn a_foreign_signer_is_ignored_and_counted() {
    let mut events = two_executions();
    // A stranger's rename, newer than nothing it could lose to.
    events.push(name(0x202, STRANGER, 990, "Hijacked"));
    // A title for sess-a signed by sess-b's provider, and one by a stranger.
    events.push(title(0x303, OTHER_PROVIDER, "sess-a", 850, "Wrong signer"));
    events.push(title(0x304, STRANGER, "sess-a", 840, "No execution"));
    let resolved = resolved_for(&events);
    assert_eq!(resolved.name, "Fix the login redirect");
    assert_eq!(origin_str(resolved.origin), "fallback");
    assert_eq!(resolved.diagnostics.foreign_names, 1);
    assert_eq!(resolved.diagnostics.foreign_titles, 2);
}

/// The create's signer is the founder only through the genesis it names.
#[test]
fn without_the_genesis_no_name_is_a_persons() {
    let mut events = two_executions();
    events.retain(|event| event["kind"] != KIND_CODING_SESSION_GENESIS);
    events.push(name(0x201, FOUNDER, 800, "Auth rework"));
    let resolved = resolved_for(&events);
    assert_eq!(origin_str(resolved.origin), "fallback");
    assert_eq!(resolved.diagnostics.foreign_names, 1);
}

#[test]
fn a_row_no_umbrella_claims_is_its_own_fallback() {
    let row = SessionRow {
        target_key: coding_session_target_key(&target("solo")),
        target: target("solo"),
        signer: pk(PROVIDER),
        title: Some("  ".into()),
        status: "idle".into(),
        model: None,
        created_at: 1,
        last_event_at: 1,
        transcript_items: 0,
        confirmed: true,
        metadata_conflicts: 0,
        session_ref: None,
    };
    let resolved = solo_display_name(&row);
    assert_eq!(resolved.name, "Untitled session");
    assert_eq!(origin_str(resolved.origin), "fallback");
}

/// A row for [`scope_from_rows`]: one execution of the umbrella.
fn member_row(session_id: &str, at: i64, title: Option<&str>, confirmed: bool) -> SessionRow {
    SessionRow {
        target_key: coding_session_target_key(&target(session_id)),
        target: target(session_id),
        signer: pk(PROVIDER),
        title: title.map(str::to_owned),
        status: "idle".into(),
        model: None,
        created_at: at,
        last_event_at: at,
        transcript_items: 0,
        confirmed,
        metadata_conflicts: 0,
        session_ref: Some(UMBRELLA.into()),
    }
}

/// A stranger's backdated, unconfirmed 44223 carrying the umbrella's
/// sessionRef must not name the umbrella: the founding title comes only from
/// confirmed rows.
#[test]
fn an_unconfirmed_earlier_row_does_not_name_the_umbrella() {
    let stranger = member_row("sess-x", 1, Some("Stranger's title"), false);
    let ours = member_row("sess-a", 500, Some("Founding title"), true);
    let members = vec![&stranger, &ours];
    let scope = scope_from_rows(CHANNEL, UMBRELLA, &members, None);
    assert_eq!(
        scope.founding_execution_title.as_deref(),
        Some("Founding title")
    );
    let resolved = resolve_session_display_name(&scope, &[]);
    assert_eq!(resolved.name, "Founding title");
    assert_eq!(origin_str(resolved.origin), "fallback");
}

/// With no confirmed row at all, the umbrella falls back to
/// `Untitled session` rather than any unconfirmed claimant's title.
#[test]
fn no_confirmed_row_means_untitled() {
    let stranger = member_row("sess-x", 1, Some("Stranger's title"), false);
    let members = vec![&stranger];
    let scope = scope_from_rows(CHANNEL, UMBRELLA, &members, None);
    assert_eq!(scope.founding_execution_title, None);
    assert!(scope.executions.is_empty());
    let resolved = resolve_session_display_name(&scope, &[]);
    assert_eq!(resolved.name, "Untitled session");
    assert_eq!(origin_str(resolved.origin), "fallback");
}

// ── Output shape ─────────────────────────────────────────────────────────────

/// The acceptance check: `bee --format compact sessions show` prints
/// `nameOrigin`, and a generated name its model and short signer. Printed so
/// the run is the evidence (`cargo test ... -- --nocapture`).
#[test]
fn show_prints_the_name_and_where_it_came_from() {
    let mut events = two_executions();
    events.push(title(0x301, PROVIDER, "sess-a", 900, "Login redirect fix"));
    let (metadata, _) = decode_metadata(&events);
    let (receipts, _) = decode_receipts(&events);
    let founders = crew::build_founder_index(&events, &receipts);
    let rows = resolve_sessions(&metadata, &receipts, &[]);
    let members: Vec<&SessionRow> = rows.iter().collect();
    let scope = umbrella_scope(CHANNEL, UMBRELLA, &members, &founders);
    let records: Vec<SessionNameRecord> = events
        .iter()
        .filter_map(session_name_record_from_json)
        .collect();
    let resolved = resolve_session_display_name(&scope, &records);

    let compact = show_output(
        UMBRELLA,
        &resolved,
        scope.founder_pubkey.as_deref(),
        &members,
        true,
    );
    println!("bee --format compact sessions show: {compact}");
    assert_eq!(compact["name"], "Login redirect fix");
    assert_eq!(compact["nameOrigin"], "generated");
    assert_eq!(compact["nameModel"], "haiku");
    assert_eq!(compact["nameSigner"], PROVIDER.repeat(4));
    assert_eq!(compact["founder"], FOUNDER.repeat(4));
    assert_eq!(compact["executions"].as_array().map(Vec::len), Some(2));
    assert!(compact.get("nameDiagnostics").is_none());

    let full = show_output(
        UMBRELLA,
        &resolved,
        scope.founder_pubkey.as_deref(),
        &members,
        false,
    );
    println!("bee sessions show: {full}");
    assert_eq!(full["nameSigner"], json!(pk(PROVIDER)));
    assert_eq!(full["nameDiagnostics"]["foreignTitles"], 0);
}

#[test]
fn a_persons_name_prints_no_model_or_signer() {
    let mut events = two_executions();
    events.push(name(0x201, FOUNDER, 800, "Auth rework"));
    let fields = name_fields(&resolved_for(&events), true);
    assert_eq!(fields["nameOrigin"], "person");
    assert!(!fields.contains_key("nameModel"));
    assert!(!fields.contains_key("nameSigner"));
}

#[test]
fn the_list_read_names_both_name_kinds() {
    assert!(SESSION_LIST_KINDS.contains(&KIND_CODING_SESSION_NAME));
    assert!(SESSION_LIST_KINDS.contains(&KIND_CODING_SESSION_GENERATED_TITLE));
    assert!(SESSION_LIST_KINDS.contains(&KIND_CODING_SESSION_GENESIS));
}

// ── The shared vectors ──────────────────────────────────────────────────────

/// Every `conformance/session-display-name` vector through the CLI's path:
/// its scope rebuilt from `SessionRow`s by [`scope_from_rows`], its events
/// converted by the JSON reader `list` uses, and its answer printed by
/// [`name_fields`] — forwards and reversed.
#[test]
fn shared_vectors_pass_through_the_cli_path() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../../../conformance/session-display-name/fixtures/vectors.json"
    ))
    .unwrap_or_default();
    let vectors = corpus["vectors"].as_array().cloned().unwrap_or_default();
    assert!(!vectors.is_empty());
    for vector in vectors {
        let label = vector["name"].as_str().unwrap_or("?").to_owned();
        let scope: SessionDisplayNameScope = serde_json::from_value(vector["scope"].clone())
            .unwrap_or_else(|error| panic!("{label}: scope decodes: {error}"));
        let rows: Vec<SessionRow> = scope
            .executions
            .iter()
            .enumerate()
            .map(|(position, execution)| SessionRow {
                target_key: execution.target_key.clone(),
                target: parse_coding_session_target_key(&execution.target_key)
                    .unwrap_or_else(|error| panic!("{label}: target parses: {error}")),
                signer: execution.provider_authority_pubkey.clone(),
                // The earliest row carries the founding execution's title.
                title: if position == 0 {
                    scope.founding_execution_title.clone()
                } else {
                    None
                },
                status: "idle".into(),
                model: None,
                created_at: 100 + position as i64,
                last_event_at: 100 + position as i64,
                transcript_items: 0,
                confirmed: true,
                metadata_conflicts: 0,
                session_ref: Some(scope.session_ref.clone()),
            })
            .collect();
        let members: Vec<&SessionRow> = rows.iter().collect();
        let rebuilt = scope_from_rows(
            &scope.channel_id,
            &scope.session_ref,
            &members,
            scope.founder_pubkey.clone(),
        );
        assert_eq!(rebuilt, scope, "{label}: the rows rebuild the scope");

        let mut records: Vec<SessionNameRecord> = vector["events"]
            .as_array()
            .into_iter()
            .flatten()
            .map(|event| {
                session_name_record_from_json(event)
                    .unwrap_or_else(|| panic!("{label}: event converts"))
            })
            .collect();
        let expected = &vector["expected"];
        for _ in 0..2 {
            let resolved = resolve_session_display_name(&rebuilt, &records);
            let printed = name_fields(&resolved, false);
            assert_eq!(printed["name"], expected["name"], "{label}");
            assert_eq!(printed["nameOrigin"], expected["origin"], "{label}");
            assert_eq!(
                printed.get("nameModel").cloned().unwrap_or(Value::Null),
                expected["model"],
                "{label}"
            );
            assert_eq!(
                printed.get("nameSigner").cloned().unwrap_or(Value::Null),
                expected["signerPubkey"],
                "{label}"
            );
            assert_eq!(
                printed["nameDiagnostics"], expected["diagnostics"],
                "{label}"
            );
            records.reverse();
        }
    }
}
