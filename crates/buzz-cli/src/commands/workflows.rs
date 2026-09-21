use crate::client::{
    extract_d_tag, extract_relay_response_field, normalize_write_response, print_create_response,
    BuzzClient,
};
use crate::error::CliError;
use crate::validate::{parse_uuid, read_or_stdin, sdk_err, validate_hex64, validate_uuid};

// TODO(phase-4): Replace raw nostr::EventBuilder usage with buzz-sdk builder functions

/// List workflows in a channel — query kind:30620 workflow definition events.
pub async fn cmd_list_workflows(client: &BuzzClient, channel_id: &str) -> Result<(), CliError> {
    validate_uuid(channel_id)?;
    let filter = serde_json::json!({
        "kinds": [30620],
        "#h": [channel_id]
    });
    let resp = client.query(&filter).await?;
    let events: Vec<serde_json::Value> = serde_json::from_str(&resp).unwrap_or_default();
    let workflows: Vec<serde_json::Value> = events
        .iter()
        .map(|e| {
            serde_json::json!({
                "workflow_id": extract_d_tag(e),
                "content": e.get("content").and_then(|v| v.as_str()).unwrap_or(""),
                "created_at": e.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0),
                "pubkey": e.get("pubkey").and_then(|v| v.as_str()).unwrap_or(""),
            })
        })
        .collect();
    let output = serde_json::to_string(&workflows).unwrap_or_default();
    println!("{output}");
    Ok(())
}

/// Get a single workflow definition.
pub async fn cmd_get_workflow(client: &BuzzClient, workflow_id: &str) -> Result<(), CliError> {
    validate_uuid(workflow_id)?;
    let filter = serde_json::json!({
        "kinds": [30620],
        "#d": [workflow_id]
    });
    let resp = client.query(&filter).await?;
    let events: Vec<serde_json::Value> = serde_json::from_str(&resp).unwrap_or_default();
    if let Some(e) = events.first() {
        let normalized = serde_json::json!({
            "workflow_id": extract_d_tag(e),
            "content": e.get("content").and_then(|v| v.as_str()).unwrap_or(""),
            "created_at": e.get("created_at").and_then(|v| v.as_u64()).unwrap_or(0),
            "pubkey": e.get("pubkey").and_then(|v| v.as_str()).unwrap_or(""),
        });
        println!("{normalized}");
    } else {
        println!("null");
    }
    Ok(())
}

/// Get workflow run history — `GET /workflows/{workflow_id}/runs`.
///
/// Runs, approvals and host steps are relay-owned database rows, never
/// Nostr events: kinds 46001-46008 are declared in `buzz-core::kind` but
/// nothing publishes them (`buzz-workflow` and `buzz-relay` only ever write
/// the 46010-46032 range plus the `workflow_runs`/`workflow_host_steps`/
/// `workflow_approvals` tables — see `crates/buzz-relay/src/api/workflows.rs`).
/// Querying those dead kinds, as this command used to, returns `[]` for
/// every run regardless of its real state (ledger 178(k)). Each row already
/// carries the run's actual lifecycle state — `pending` | `running` |
/// `waiting_approval` | `waiting_host` | `completed` | `failed` |
/// `cancelled` — read straight off the relay's own status column, plus the
/// triggering event id and author when the trigger context recorded one.
pub async fn cmd_get_workflow_runs(
    client: &BuzzClient,
    workflow_id: &str,
    limit: Option<u32>,
) -> Result<(), CliError> {
    validate_uuid(workflow_id)?;
    let limit = limit.unwrap_or(20).min(100);
    let resp = client
        .get_authed(&format!("/workflows/{workflow_id}/runs?limit={limit}"))
        .await?;
    println!("{}", disclose_run_checkouts(&resp));
    Ok(())
}

/// Mark each run in a relay page with whether the relay reported the commit
/// the run is bound to.
///
/// The relay has named `checkout` on both run reads since ledger 206 B, so a
/// current relay always carries the key — `null` when the run names no
/// commit, a 40-hex sha when it does. An **older** relay omits the key
/// entirely, and an absent key and a `null` mean opposite things: "this relay
/// cannot say" versus "this run tests the working tree as found". Adding
/// `checkout: null` to an older relay's page would turn the first into the
/// second, so instead each run gets `checkout_reported`, and a run the relay
/// said nothing about keeps no `checkout` key at all. Everything else in the
/// page is passed through verbatim; a body that is not the expected shape is
/// printed exactly as the relay sent it.
fn disclose_run_checkouts(body: &str) -> String {
    let Ok(mut page) = serde_json::from_str::<serde_json::Value>(body) else {
        return body.to_owned();
    };
    let runs = match page.get_mut("runs").and_then(|runs| runs.as_array_mut()) {
        Some(runs) => runs,
        // `run-status` returns one run, not a page of them.
        None => std::slice::from_mut(&mut page),
    };
    for run in runs {
        let Some(run) = run.as_object_mut() else {
            continue;
        };
        let reported = run.contains_key("checkout");
        run.insert("checkout_reported".to_owned(), serde_json::json!(reported));
    }
    page.to_string()
}

/// Get one run's full state by run id alone — `GET /workflow-runs/{run_id}`.
///
/// Every other run read is nested under `/workflows/{workflow_id}/...`, but
/// a run id is what actually surfaces first: a kind:46010 approval request's
/// `runId`, a kind:46013 host-step request's `d` tag (`<runId>:<stepId>`), a
/// kind:46023 host result. This is the one-shot read for "what happened to
/// this run": its status, the triggering event, and every host step —
/// exit code, `headSha`, `dirty`, duration, and the result event id — plus
/// every approval, so a caller never has to first resolve the workflow id
/// just to ask about a run it already has by hand.
pub async fn cmd_get_run_status(client: &BuzzClient, run_id: &str) -> Result<(), CliError> {
    validate_uuid(run_id)?;
    let resp = client
        .get_authed(&format!("/workflow-runs/{run_id}"))
        .await?;
    println!("{}", disclose_run_checkouts(&resp));
    Ok(())
}

/// Create a workflow — sign and submit a kind:30620 event.
pub async fn cmd_create_workflow(
    client: &BuzzClient,
    channel_id: &str,
    yaml: &str,
) -> Result<(), CliError> {
    let channel_uuid = parse_uuid(channel_id)?;
    let yaml_definition = read_or_stdin(yaml)?;

    let workflow_id = uuid::Uuid::new_v4();
    let builder = buzz_sdk::build_workflow_def(channel_uuid, workflow_id, &yaml_definition)
        .map_err(sdk_err)?;
    let event = client.sign_event(builder)?;

    let resp = client.submit_event(event).await?;
    let final_workflow_id = extract_relay_response_field(&resp, "workflow_id")
        .unwrap_or_else(|| workflow_id.to_string());
    print_create_response(&resp, "workflow_id", &final_workflow_id);
    Ok(())
}

/// Update a workflow — sign and submit an updated kind:30620 event with same d-tag.
pub async fn cmd_update_workflow(
    client: &BuzzClient,
    channel_id: &str,
    workflow_id: &str,
    yaml: &str,
) -> Result<(), CliError> {
    let channel_uuid = parse_uuid(channel_id)?;
    let wf_uuid = parse_uuid(workflow_id)?;
    let yaml_definition = read_or_stdin(yaml)?;

    let filter = serde_json::json!({
        "kinds": [30620],
        "#d": [workflow_id]
    });
    let resp = client.query(&filter).await?;
    let events: Vec<serde_json::Value> = serde_json::from_str(&resp).unwrap_or_default();
    let expected_revision = events
        .first()
        .and_then(|event| event.get("id"))
        .and_then(|id| id.as_str())
        .ok_or_else(|| CliError::NotFound(format!("workflow {workflow_id} not found")))?;

    let builder =
        buzz_sdk::build_workflow_update(channel_uuid, wf_uuid, &yaml_definition, expected_revision)
            .map_err(sdk_err)?;
    let event = client.sign_event(builder)?;

    let resp = client.submit_event(event).await?;
    println!("{}", normalize_write_response(&resp));
    Ok(())
}

/// Delete a workflow — sign and submit a kind:5 deletion event.
pub async fn cmd_delete_workflow(client: &BuzzClient, workflow_id: &str) -> Result<(), CliError> {
    let wf_uuid = parse_uuid(workflow_id)?;
    let keys = client.keys();

    let builder =
        buzz_sdk::build_workflow_delete(&keys.public_key().to_hex(), wf_uuid).map_err(sdk_err)?;
    let event = client.sign_event(builder)?;

    let resp = client.submit_event(event).await?;
    println!("{}", normalize_write_response(&resp));
    Ok(())
}

/// Normalize a `--checkout` value: a full 40-hex commit, lowercased.
///
/// The relay applies the same rule, so a typo is refused here rather than
/// becoming a run whose record names a commit nothing checked out.
pub fn normalize_checkout_sha(value: &str) -> Result<String, CliError> {
    let trimmed = value.trim();
    if trimmed.len() == 40 && trimmed.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        Ok(trimmed.to_ascii_lowercase())
    } else {
        Err(CliError::Usage(
            "--checkout must be a full 40-hex commit sha".into(),
        ))
    }
}

/// The kind:46020 content a trigger carries: the `--inputs` object, plus the
/// `checkout` commit when `--checkout` bound one.
///
/// Returns `None` when neither was given, so a plain trigger keeps using the
/// SDK builder and its empty content exactly as before.
pub fn trigger_content(
    inputs: Option<&str>,
    checkout: Option<&str>,
) -> Result<Option<serde_json::Value>, CliError> {
    let mut map = match inputs {
        None => serde_json::Map::new(),
        Some(raw) => {
            let parsed: serde_json::Value = serde_json::from_str(raw)
                .map_err(|e| CliError::Usage(format!("--inputs is not valid JSON: {e}")))?;
            match parsed {
                serde_json::Value::Object(map) => map,
                _ => return Err(CliError::Usage("--inputs must be a JSON object".into())),
            }
        }
    };
    let checkout = match checkout {
        None => None,
        Some(value) => Some(normalize_checkout_sha(value)?),
    };
    if let Some(sha) = checkout {
        if let Some(existing) = map.get("checkout").and_then(serde_json::Value::as_str) {
            if existing.trim().to_ascii_lowercase() != sha {
                return Err(CliError::Usage(
                    "--checkout and the 'checkout' field of --inputs name different commits".into(),
                ));
            }
        }
        map.insert("checkout".into(), serde_json::Value::String(sha));
    }
    if map.is_empty() && inputs.is_none() {
        return Ok(None);
    }
    Ok(Some(serde_json::Value::Object(map)))
}

/// Trigger a workflow — sign and submit a kind:46020 event.
///
/// When `inputs` is provided, it is parsed as a JSON object and used as the
/// event content (MCP parity). `checkout` binds the run to a commit: the
/// host runs the action's `run_on_host` steps in a fresh detached worktree at
/// that commit, and a step declaring `checkout: required` refuses a run
/// without one. When neither is given, the event content is `{}`.
pub async fn cmd_trigger_workflow(
    client: &BuzzClient,
    workflow_id: &str,
    inputs: Option<&str>,
    checkout: Option<&str>,
) -> Result<(), CliError> {
    let wf_uuid = parse_uuid(workflow_id)?;

    if let Some(content_value) = trigger_content(inputs, checkout)? {
        // Build the event manually so the inputs ride as the event content.
        let content = serde_json::to_string(&content_value).unwrap_or_default();
        use nostr::{EventBuilder, Kind, Tag};
        let tags = vec![Tag::parse(["d", &wf_uuid.to_string()])
            .map_err(|e| CliError::Other(format!("tag error: {e}")))?];
        let builder = EventBuilder::new(
            Kind::Custom(buzz_sdk::kind::KIND_WORKFLOW_TRIGGER as u16),
            &content,
        )
        .tags(tags);
        let event = client.sign_event(builder)?;
        let resp = client.submit_event(event).await?;
        println!("{}", normalize_write_response(&resp));
    } else {
        let builder = buzz_sdk::build_workflow_trigger(wf_uuid).map_err(sdk_err)?;
        let event = client.sign_event(builder)?;
        let resp = client.submit_event(event).await?;
        println!("{}", normalize_write_response(&resp));
    }
    Ok(())
}

/// Normalize `--token`: a full 64-hex approval ref, lowercased.
///
/// The relay stores and looks up approvals by this exact hash
/// (`get_approval_by_stored_hash`); it is never a UUID and is never hashed
/// again here — see [`cmd_approve_step`].
pub fn normalize_approval_ref(value: &str) -> Result<String, CliError> {
    validate_hex64(value)?;
    Ok(value.to_ascii_lowercase())
}

/// Approve or deny a workflow step — sign and submit a kind:46030 (grant) or 46031 (deny) event.
///
/// `--token` is the approval ref: the 64-hex `hex(SHA256(raw token))` the
/// relay names in a kind:46010 approval request's `d` tag and in
/// `GET /workflows/{workflow_id}/runs/{run_id}/approvals`. The relay never
/// discloses the raw token it hashed to produce that ref — only the ref
/// leaves the process that generated it (`buzz-workflow`'s
/// `suspend::persist_and_publish` hashes and discards the raw UUID in the
/// same statement) — so this command used to hash an already-hashed ref a
/// second time, producing a `d` tag the relay had never stored and refusing
/// every host-step approval by hand (ledger 171(c)). It now signs the ref
/// through unchanged, exactly as `buzz_sdk::build_workflow_approval` and the
/// relay's own lookup (`get_approval_by_stored_hash`) expect.
pub async fn cmd_approve_step(
    client: &BuzzClient,
    approval_ref: &str,
    approved: bool,
    note: Option<&str>,
    allow_future_runs: bool,
) -> Result<(), CliError> {
    let approval_ref = normalize_approval_ref(approval_ref)?;

    // Spec § 5.4: a grant's content is `{note, scope}`; `scope: action` also
    // records an autorun grant bound to the definition as stored now. A deny
    // carries the plain note.
    let content_owned = if approved {
        buzz_core::workflow_autorun::encode_approval_grant_content(
            note,
            if allow_future_runs {
                buzz_core::workflow_autorun::ApprovalScope::Action
            } else {
                buzz_core::workflow_autorun::ApprovalScope::Run
            },
        )
    } else {
        note.unwrap_or("").to_owned()
    };
    let content = content_owned.as_str();

    let builder =
        buzz_sdk::build_workflow_approval(&approval_ref, approved, content).map_err(sdk_err)?;
    let event = client.sign_event(builder)?;

    let resp = client.submit_event(event).await?;
    println!("{}", normalize_write_response(&resp));
    Ok(())
}

pub async fn dispatch(cmd: crate::WorkflowsCmd, client: &BuzzClient) -> Result<(), CliError> {
    use crate::WorkflowsCmd;
    match cmd {
        WorkflowsCmd::List { channel } => cmd_list_workflows(client, &channel).await,
        WorkflowsCmd::Get { workflow } => cmd_get_workflow(client, &workflow).await,
        WorkflowsCmd::Create { channel, yaml } => {
            cmd_create_workflow(client, &channel, &yaml).await
        }
        WorkflowsCmd::Update {
            channel,
            workflow,
            yaml,
        } => cmd_update_workflow(client, &channel, &workflow, &yaml).await,
        WorkflowsCmd::Delete { workflow } => cmd_delete_workflow(client, &workflow).await,
        WorkflowsCmd::Trigger {
            workflow,
            inputs,
            checkout,
        } => cmd_trigger_workflow(client, &workflow, inputs.as_deref(), checkout.as_deref()).await,
        WorkflowsCmd::Runs { workflow, limit } => {
            cmd_get_workflow_runs(client, &workflow, limit).await
        }
        WorkflowsCmd::RunStatus { run } => cmd_get_run_status(client, &run).await,
        WorkflowsCmd::Approve {
            token,
            approved,
            note,
            allow_future_runs,
        } => {
            // approved is already a bool — no parse_bool_flag needed
            cmd_approve_step(client, &token, approved, note.as_deref(), allow_future_runs).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_trigger_still_carries_no_content() {
        assert!(trigger_content(None, None).expect("no content").is_none());
    }

    #[test]
    fn checkout_rides_the_content_as_a_lowercase_sha() {
        let content = trigger_content(None, Some(&"AB".repeat(20)))
            .expect("valid")
            .expect("content");
        assert_eq!(content["checkout"], "ab".repeat(20));
        let merged = trigger_content(Some(r#"{"reason":"verify"}"#), Some(&"cd".repeat(20)))
            .expect("valid")
            .expect("content");
        assert_eq!(merged["reason"], "verify");
        assert_eq!(merged["checkout"], "cd".repeat(20));
    }

    #[test]
    fn a_short_or_non_hex_checkout_is_a_usage_error() {
        for value in ["fa927fd", "main", &"z".repeat(40)] {
            let error = trigger_content(None, Some(value)).expect_err("refused");
            assert!(
                matches!(error, CliError::Usage(ref message) if message.contains("40-hex")),
                "{value} should be refused by name, got {error:?}"
            );
        }
    }

    #[test]
    fn two_different_commits_in_one_trigger_are_refused() {
        let inputs = format!("{{\"checkout\":\"{}\"}}", "ab".repeat(20));
        let error = trigger_content(Some(&inputs), Some(&"cd".repeat(20)))
            .expect_err("a run cannot name two commits");
        assert!(matches!(error, CliError::Usage(_)), "{error:?}");
        // The same commit twice is fine.
        trigger_content(Some(&inputs), Some(&"AB".repeat(20))).expect("agreeing values");
    }

    /// Ledger 206 B: the relay names the commit a run is bound to on both run
    /// reads. The page is passed through verbatim apart from
    /// `checkout_reported`, which keeps "this relay does not say" from reading
    /// as "this run tests the tree as found".
    #[test]
    fn a_runs_page_keeps_the_relays_checkout_and_says_whether_it_was_reported() {
        let sha = "fa927fd".repeat(6);
        let page = format!(
            r#"{{"runs":[{{"id":"a","checkout":"{sha}"}},{{"id":"b","checkout":null}}],"next":null}}"#
        );
        let out: serde_json::Value =
            serde_json::from_str(&disclose_run_checkouts(&page)).expect("json");
        assert_eq!(out["runs"][0]["checkout"], sha);
        assert_eq!(out["runs"][0]["checkout_reported"], true);
        assert!(out["runs"][1]["checkout"].is_null());
        assert_eq!(out["runs"][1]["checkout_reported"], true);
        assert!(out["next"].is_null(), "the rest of the page is untouched");
    }

    /// A relay older than that change omits the key. It must not be invented:
    /// the run keeps no `checkout`, and `checkout_reported` is false.
    #[test]
    fn an_older_relays_page_is_not_given_a_checkout_it_never_reported() {
        let out: serde_json::Value = serde_json::from_str(&disclose_run_checkouts(
            r#"{"runs":[{"id":"a","status":"waiting_approval"}]}"#,
        ))
        .expect("json");
        assert!(out["runs"][0].get("checkout").is_none());
        assert_eq!(out["runs"][0]["checkout_reported"], false);
        assert_eq!(out["runs"][0]["status"], "waiting_approval");
    }

    /// `run-status` returns one run, not a page of them, and gets the same
    /// treatment. A body that is not JSON at all is printed exactly as the
    /// relay sent it.
    #[test]
    fn a_single_run_status_body_is_disclosed_and_a_non_json_body_is_untouched() {
        let out: serde_json::Value = serde_json::from_str(&disclose_run_checkouts(
            r#"{"id":"a","checkout":null,"host_steps":[]}"#,
        ))
        .expect("json");
        assert_eq!(out["checkout_reported"], true);
        assert!(out["checkout"].is_null());
        assert_eq!(disclose_run_checkouts("not json at all"), "not json at all");
    }

    #[test]
    fn approval_ref_is_lowercased_not_rehashed() {
        // A 64-hex ref, as named in a kind:46010 request's `d` tag or the
        // approvals read, passes through unchanged apart from case.
        let ref_hex = "AB".repeat(32);
        let normalized = normalize_approval_ref(&ref_hex).expect("valid ref");
        assert_eq!(normalized, "ab".repeat(32));
    }

    #[test]
    fn a_uuid_is_refused_as_an_approval_ref() {
        // ledger 171(c): the old `--token <UUID>` contract cannot be
        // satisfied by any value the relay actually hands out, so a UUID is
        // refused by name rather than silently hashed into a `d` tag the
        // relay never stored.
        let error = normalize_approval_ref("00000000-0000-0000-0000-000000000000")
            .expect_err("a UUID is not a stored approval ref");
        assert!(matches!(error, CliError::Usage(_)), "{error:?}");
    }

    #[test]
    fn a_short_approval_ref_is_a_usage_error() {
        let error = normalize_approval_ref("ab").expect_err("too short");
        assert!(matches!(error, CliError::Usage(_)), "{error:?}");
    }
}
