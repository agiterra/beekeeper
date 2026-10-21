//! `bee plugins` — any plugin the relay runs, through the same route the
//! desktop uses, and the draft executor every plugin write goes through.
//!
//! A plugin write returns unsigned drafts. This client signs each with the
//! caller's own key and publishes it through ordinary ingest, in order, under
//! the plugin's stated refusal policy (`docs/plugins.md` § Drafts):
//!
//! - `abort` — stop. Nothing before it saved: the relay's refusal is the
//!   error. Something did: the write is partial, and the error says what
//!   landed (the draft's `partialSummary`, or a generic line).
//! - `skip-group` — record the group as failed, skip the rest of that group,
//!   carry on with the next.
//! - a submit with no answer (`DeliveryUnknown`, a timeout) stops the run as
//!   unconfirmed, whatever the policy: the event may still land.
//!
//! Nobody reads a draft before it is signed, so this client is the last
//! check, not a person. Before signing anything it seals every draft itself
//! ([`seal_draft`]), whatever the relay already did: a kind it signs for
//! plugins, only that kind's data tags, its own `a` tag written from the
//! project this call named (a proposed one may only echo it), the tag caps
//! before and after, and the kind's own validator over the result.

use serde_json::{json, Value};

use crate::client::BeekeeperClient;
use crate::error::CliError;

/// The kinds this client will sign on a plugin's behalf.
const SIGNABLE_KINDS: &[u64] = &[beekeeper_core::kind::KIND_PROJECT_TODO_OP as u64];

/// `GET /plugins`.
pub async fn list(client: &BeekeeperClient) -> Result<Value, CliError> {
    let raw = client.get_authed("/plugins").await?;
    serde_json::from_str(&raw).map_err(|e| CliError::Other(format!("bad /plugins reply: {e}")))
}

/// The plugin that implements `api`: the one named, or the only one. Two
/// with no name is an error that lists them — never a silent pick.
pub async fn provider_for(
    client: &BeekeeperClient,
    api: &str,
    named: Option<&str>,
) -> Result<String, CliError> {
    let listing = list(client).await?;
    let providers: Vec<String> = listing["plugins"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|p| {
            p["api"]
                .as_array()
                .is_some_and(|apis| apis.iter().any(|a| a == api))
        })
        .filter_map(|p| p["name"].as_str().map(str::to_owned))
        .collect();
    match (named, providers.as_slice()) {
        (Some(name), _) if providers.iter().any(|p| p == name) => Ok(name.to_owned()),
        (Some(name), _) => Err(CliError::Usage(format!(
            "this relay has no plugin {name:?} implementing {api}; it has: {}",
            if providers.is_empty() {
                "none".to_owned()
            } else {
                providers.join(", ")
            }
        ))),
        (None, [only]) => Ok(only.clone()),
        (None, []) => Err(CliError::Other(format!(
            "this relay runs no plugin implementing {api}"
        ))),
        (None, many) => Err(CliError::Usage(format!(
            "this relay has {} providers of {api} ({}); name one with --provider",
            many.len(),
            many.join(", ")
        ))),
    }
}

/// `POST /plugins/{name}/invoke`.
pub async fn invoke(
    client: &BeekeeperClient,
    plugin: &str,
    operation: &str,
    project: &str,
    input: Value,
) -> Result<Value, CliError> {
    let raw = client
        .post_authed(
            &format!("/plugins/{plugin}/invoke"),
            &json!({ "operation": operation, "scope": { "project": project }, "input": input }),
        )
        .await?;
    serde_json::from_str(&raw).map_err(|e| CliError::Other(format!("bad invoke reply: {e}")))
}

fn str_tags(draft: &Value) -> Result<Vec<Vec<String>>, CliError> {
    draft["tags"]
        .as_array()
        .ok_or_else(|| CliError::Other("draft has no tags".into()))?
        .iter()
        .map(|tag| {
            tag.as_array()
                .ok_or_else(|| CliError::Other("draft tag is not an array".into()))?
                .iter()
                .map(|v| {
                    v.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| CliError::Other("draft tag value is not a string".into()))
                })
                .collect()
        })
        .collect()
}

/// The same caps the relay's plugin host holds a draft's tags to
/// (`beekeeper_plugin_host::drafts`), checked again here on what came back
/// and on what this client built.
const MAX_DRAFT_TAGS: usize = 64;
const MAX_TAG_VALUES: usize = 8;
const MAX_TAG_VALUE_BYTES: usize = 1_024;
const MAX_DRAFT_TAG_BYTES: usize = 16 * 1_024;

fn check_tag_caps(stage: &str, tags: &[Vec<String>]) -> Result<(), CliError> {
    let refuse = |why: String| {
        CliError::Other(format!(
            "the plugin returned a draft whose {stage} tags {why}; nothing was signed"
        ))
    };
    if tags.len() > MAX_DRAFT_TAGS {
        return Err(refuse(format!("number {}", tags.len())));
    }
    let mut total = 0;
    for tag in tags {
        if tag.is_empty() || tag.len() > MAX_TAG_VALUES {
            return Err(refuse(format!("include one of {} values", tag.len())));
        }
        if tag.iter().any(|v| v.len() > MAX_TAG_VALUE_BYTES) {
            return Err(refuse("include a value over 1 KiB".into()));
        }
        total += tag.iter().map(String::len).sum::<usize>();
    }
    if total > MAX_DRAFT_TAG_BYTES {
        return Err(refuse(format!("are {total} bytes")));
    }
    Ok(())
}

/// A draft as this client will sign it.
#[derive(Debug)]
struct SealedDraft {
    kind: u16,
    created_at: u64,
    tags: Vec<Vec<String>>,
    content: String,
}

/// Rebuild a draft for signing, or refuse it: a kind this client signs for
/// plugins, a `createdAt` that is present, an integer and inside the window
/// ingest will still accept (`beekeeper_core::project_todo::
/// check_draft_created_at` at `now`; never re-stamped), the kind's data tags
/// only, the `a` tag written here from `project` (a proposed one must echo
/// it exactly, once), caps before and after, and the kind's validator over
/// the sealed event.
fn seal_draft(draft: &Value, project: &str, now: u64) -> Result<SealedDraft, CliError> {
    let kind = draft["kind"].as_u64().unwrap_or(0);
    if !SIGNABLE_KINDS.contains(&kind) {
        return Err(CliError::Other(format!(
            "the plugin returned a kind {kind} draft; this client signs only kinds {SIGNABLE_KINDS:?} for plugins"
        )));
    }
    let created_at = draft["createdAt"].as_u64().ok_or_else(|| {
        CliError::Other(
            "the plugin returned a draft without a whole-second createdAt; nothing was signed"
                .into(),
        )
    })?;
    beekeeper_core::project_todo::check_draft_created_at(created_at, now).map_err(|e| {
        CliError::Other(format!(
            "the plugin returned a draft this client will not sign ({e}); nothing was signed"
        ))
    })?;
    let proposed = str_tags(draft)?;
    check_tag_caps("proposed", &proposed)?;
    let mut tags = vec![vec!["a".to_owned(), project.to_owned()]];
    let mut echoed = false;
    for tag in proposed {
        if tag[0] == "a" {
            if echoed || tag.len() != 2 || tag[1] != project {
                return Err(CliError::Other(
                    "the plugin returned a draft scoped outside the project this call named; nothing was signed"
                        .into(),
                ));
            }
            echoed = true;
        } else if beekeeper_core::project_todo::PROJECT_TODO_DATA_TAG_KEYS
            .contains(&tag[0].as_str())
        {
            tags.push(tag);
        } else {
            return Err(CliError::Other(format!(
                "the plugin returned a draft with a {:?} tag, which this client never signs for a plugin; nothing was signed",
                tag[0]
            )));
        }
    }
    check_tag_caps("constructed", &tags)?;
    let content = draft["content"].as_str().unwrap_or("").to_owned();
    beekeeper_core::project_todo::validate_project_todo_parts(kind as u32, &tags, &content)
        .map_err(|e| {
            CliError::Other(format!(
                "the plugin returned a draft this client will not sign ({e}); nothing was signed"
            ))
        })?;
    Ok(SealedDraft {
        kind: kind as u16,
        created_at,
        tags,
        content,
    })
}

/// Sign and publish `drafts` in order as the caller. Returns what was
/// published and which groups failed; an `abort` refusal is an error.
pub async fn execute_drafts(
    client: &BeekeeperClient,
    project: &str,
    drafts: &Value,
) -> Result<Value, CliError> {
    execute_drafts_at(client, project, drafts, nostr::Timestamp::now().as_secs()).await
}

/// [`execute_drafts`] against the clock `now`. Every draft is sealed before
/// the first is signed, so one this client refuses — anywhere in the list —
/// means nothing is published. That avoids a preventable partial write; it
/// is not atomicity: a relay refusal or a lost answer after the first
/// publish is still reported as partial or unconfirmed.
async fn execute_drafts_at(
    client: &BeekeeperClient,
    project: &str,
    drafts: &Value,
    now: u64,
) -> Result<Value, CliError> {
    let Some(drafts) = drafts.as_array().cloned() else {
        return Err(CliError::Other(
            "the plugin's reply carried no drafts list; nothing was signed".into(),
        ));
    };
    let sealed = drafts
        .iter()
        .map(|draft| seal_draft(draft, project, now))
        .collect::<Result<Vec<_>, _>>()?;
    // Sign every sealed draft before publishing the first, so nothing this
    // client can refuse on its own is left to fail after a write landed.
    let signed = sealed
        .iter()
        .map(|seal| {
            let tags = seal
                .tags
                .iter()
                .cloned()
                .map(|t| {
                    nostr::Tag::parse(t).map_err(|e| CliError::Other(format!("draft tag: {e}")))
                })
                .collect::<Result<Vec<_>, _>>()?;
            let builder =
                nostr::EventBuilder::new(nostr::Kind::Custom(seal.kind), seal.content.as_str())
                    .tags(tags)
                    .custom_created_at(nostr::Timestamp::from(seal.created_at));
            // Signed verbatim (see `todos::publish`): the draft's tag set is
            // closed, so the auth tag `sign_event` injects would be refused.
            client.sign_event_unchecked(builder)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let mut published = Vec::new();
    let mut failed: Vec<Value> = Vec::new();
    let mut saved_groups = std::collections::BTreeSet::new();
    let mut skipped = std::collections::BTreeSet::new();
    for (index, draft) in drafts.iter().enumerate() {
        let group = draft["group"].as_str().unwrap_or("").to_owned();
        if skipped.contains(&group) {
            continue;
        }
        let event = signed[index].clone();
        let event_id = event.id.to_hex();
        let outcome = match client.submit_event(event).await {
            Ok(raw) => super::parse_write_response(&raw, "plugin draft was superseded"),
            Err(error) => Err(error),
        };
        match outcome {
            Ok(_) => {
                published.push(json!({ "event_id": event_id, "group": group }));
                saved_groups.insert(group);
            }
            Err(error) => {
                let reason = error.to_string();
                // No answer: the event may still land. Stop and say so —
                // reporting it as refused would invite a retry that could
                // mint a duplicate.
                if matches!(error, CliError::DeliveryUnknown(_))
                    || matches!(&error, CliError::Network(e) if e.is_timeout())
                {
                    // The same accounting as the desktop's executor: what
                    // landed, what was refused earlier, and which later
                    // groups were never tried.
                    let mut not_attempted: Vec<String> = Vec::new();
                    for later in &drafts[index + 1..] {
                        let g = later["group"].as_str().unwrap_or("").to_owned();
                        if g != group
                            && !skipped.contains(&g)
                            && !saved_groups.contains(&g)
                            && !not_attempted.contains(&g)
                        {
                            not_attempted.push(g);
                        }
                    }
                    return Err(CliError::DeliveryUnknown(format!(
                        "the relay did not confirm the {group} change ({reason}); it may still \
                         land — read before retrying. published: {}; failed: {}; not attempted: {}",
                        Value::Array(published),
                        Value::Array(failed),
                        json!(not_attempted)
                    )));
                }
                let summary = draft["partialSummary"].as_str();
                if draft["ifRefused"] == "skip-group" {
                    let message = match summary {
                        Some(summary) if saved_groups.contains(&group) => {
                            format!("{summary} ({reason})")
                        }
                        _ => reason,
                    };
                    failed.push(json!({ "group": group, "message": message }));
                    skipped.insert(group);
                    continue;
                }
                if published.is_empty() {
                    return Err(error);
                }
                let summary = summary.map(str::to_owned).unwrap_or_else(|| {
                    format!(
                        "Only part of the change was confirmed: {} of {} signed changes landed",
                        published.len(),
                        drafts.len()
                    )
                });
                return Err(CliError::Other(format!(
                    "{summary} ({reason}); published: {}",
                    Value::Array(published)
                )));
            }
        }
    }
    Ok(json!({ "published": published, "failed": failed }))
}

/// An unconfirmed write still names what the plugin said it was creating —
/// a create's issue key above all — so the caller can read before retrying
/// instead of minting a duplicate.
pub fn with_result(error: CliError, result: &Value) -> CliError {
    match error {
        CliError::DeliveryUnknown(message) => {
            CliError::DeliveryUnknown(format!("{message}; result: {result}"))
        }
        other => other,
    }
}

/// Dispatch `bee plugins`.
pub async fn dispatch(cmd: crate::PluginsCmd, client: &BeekeeperClient) -> Result<(), CliError> {
    match cmd {
        crate::PluginsCmd::List => {
            println!("{}", list(client).await?);
            Ok(())
        }
        crate::PluginsCmd::Invoke {
            plugin,
            operation,
            project,
            input,
            publish,
        } => {
            let project = super::pulse::resolve_project(client, project.as_deref()).await?;
            let input: Value = match input {
                Some(text) => serde_json::from_str(&text)
                    .map_err(|e| CliError::Usage(format!("--input is not JSON: {e}")))?,
                None => json!({}),
            };
            let mut reply = invoke(client, &plugin, &operation, &project, input).await?;
            if publish {
                let ran = execute_drafts(client, &project, &reply["drafts"])
                    .await
                    .map_err(|error| with_result(error, &reply["result"]))?;
                reply["published"] = ran["published"].clone();
                reply["failed"] = ran["failed"].clone();
            }
            println!("{reply}");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROJECT: &str =
        "30621:1111111111111111111111111111111111111111111111111111111111111111:demo";

    #[test]
    fn an_unconfirmed_write_keeps_the_result() {
        let error = with_result(
            CliError::DeliveryUnknown("no answer".into()),
            &json!({ "key": "aa/bb" }),
        );
        assert!(error.to_string().contains("\"key\":\"aa/bb\""), "{error}");
        let other = with_result(CliError::Usage("x".into()), &json!({}));
        assert!(matches!(other, CliError::Usage(_)));
    }

    /// The fixed clock every test seals against.
    const NOW: u64 = 1_800_000_000;
    const LIST: &str = "abcdefabcdefabcdefabcdefabcdefab";
    const ITEM: &str = "1234567890abcdef1234567890abcdef";

    /// An `item.assignee` draft as a relay returns it, with `extra` tags.
    fn assign(assignee: &str, extra: Value) -> Value {
        let mut tags = json!([
            ["a", PROJECT],
            ["td-v", "td1-1"],
            ["td-op", "item.assignee"],
            ["td-list", LIST],
            ["td-vis", "project"],
            ["td-item", ITEM]
        ]);
        for tag in extra.as_array().cloned().unwrap_or_default() {
            tags.as_array_mut().unwrap().push(tag);
        }
        json!({
            "kind": 44248, "createdAt": NOW, "group": "assignee", "ifRefused": "abort",
            "tags": tags,
            "content": json!({ "schema": "buzz-project-todo/v1", "op": "item.assignee",
                "listId": LIST, "itemId": ITEM, "assignee": assignee }).to_string(),
        })
    }

    #[test]
    fn a_legitimate_assignment_is_sealed_with_this_clients_own_scope() {
        let member = "b".repeat(64);
        let sealed = seal_draft(&assign(&member, json!([])), PROJECT, NOW).unwrap();
        assert_eq!(sealed.tags[0], vec!["a".to_owned(), PROJECT.to_owned()]);
        assert_eq!(sealed.tags.iter().filter(|t| t[0] == "a").count(), 1);
        assert_eq!(sealed.kind, 44248);
        // A draft that leaves the scope to the signer seals identically.
        let mut bare = assign(&member, json!([]));
        bare["tags"].as_array_mut().unwrap().remove(0);
        assert_eq!(seal_draft(&bare, PROJECT, NOW).unwrap().tags, sealed.tags);
    }

    #[test]
    fn anything_but_the_kinds_data_in_the_named_project_is_refused() {
        let member = "b".repeat(64);
        let mut other_kind = assign(&member, json!([]));
        other_kind["kind"] = json!(1);
        assert!(seal_draft(&other_kind, PROJECT, NOW).is_err());
        let elsewhere = format!("30621:{}:elsewhere", "2".repeat(64));
        for extra in [
            json!([["a", elsewhere]]),
            json!([["a", PROJECT]]),
            json!([["a", PROJECT, "wss://elsewhere"]]),
            json!([["p", member]]),
            json!([["e", "e".repeat(64)]]),
            json!([["h", "channel"]]),
            json!([["unknown", "x"]]),
        ] {
            assert!(
                seal_draft(&assign(&member, extra.clone()), PROJECT, NOW).is_err(),
                "{extra}"
            );
        }
        // Content that is not this kind's: a malformed assignee, a list id
        // the tag does not carry.
        assert!(seal_draft(&assign("not-hex", json!([])), PROJECT, NOW).is_err());
        let mut forged = assign(&member, json!([]));
        forged["tags"][3][1] = json!("f".repeat(32));
        assert!(seal_draft(&forged, PROJECT, NOW).is_err());
    }

    #[test]
    fn caps_hold_before_and_after_the_scope_is_added() {
        let member = "b".repeat(64);
        // 64 proposed tags with no scope: 65 once this client adds it.
        let mut full = assign(&member, json!([]));
        let filler: Vec<Value> = (0..64).map(|_| json!(["td-v", "td1-1"])).collect();
        full["tags"] = json!(filler);
        let error = seal_draft(&full, PROJECT, NOW).unwrap_err().to_string();
        assert!(error.contains("constructed tags"), "{error}");
        let mut long = assign(&member, json!([]));
        long["tags"][1][1] = json!("x".repeat(2_000));
        let error = seal_draft(&long, PROJECT, NOW).unwrap_err().to_string();
        assert!(error.contains("proposed tags"), "{error}");
    }

    #[test]
    fn a_stamp_is_signed_only_inside_the_window_ingest_still_accepts() {
        use beekeeper_core::project_todo::{DRAFT_FUTURE_SECS, DRAFT_PAST_SECS};
        let member = "b".repeat(64);
        let at = |stamp: Value| {
            let mut draft = assign(&member, json!([]));
            draft["createdAt"] = stamp;
            seal_draft(&draft, PROJECT, NOW)
        };
        for edge in [NOW - DRAFT_PAST_SECS, NOW, NOW + DRAFT_FUTURE_SECS] {
            assert_eq!(
                at(json!(edge)).unwrap().created_at,
                edge,
                "never re-stamped"
            );
        }
        for stamp in [
            json!(NOW - DRAFT_PAST_SECS - 1),
            json!(NOW - 1_800),
            json!(NOW + DRAFT_FUTURE_SECS + 1),
            json!(NOW + 86_400),
            json!(0),
        ] {
            let error = at(stamp.clone()).unwrap_err().to_string();
            assert!(error.contains("created_at"), "{stamp}: {error}");
        }
        // Missing or malformed is refused, never read as zero or as now.
        let mut missing = assign(&member, json!([]));
        missing.as_object_mut().unwrap().remove("createdAt");
        let mut malformed = vec![missing];
        for stamp in [json!("1800000000"), json!(-1), json!(1.8e9), Value::Null] {
            let mut draft = assign(&member, json!([]));
            draft["createdAt"] = stamp;
            malformed.push(draft);
        }
        for draft in malformed {
            let error = seal_draft(&draft, PROJECT, NOW).unwrap_err().to_string();
            assert!(error.contains("createdAt"), "{draft}: {error}");
        }
    }

    /// A relay stand-in that counts every request and accepts each event.
    async fn counting_relay() -> (String, std::sync::Arc<std::sync::atomic::AtomicUsize>) {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let hits = std::sync::Arc::new(AtomicUsize::new(0));
        let seen = hits.clone();
        let app = axum::Router::new().fallback(move || {
            let seen = seen.clone();
            async move {
                seen.fetch_add(1, Ordering::SeqCst);
                axum::Json(json!({ "event_id": "0".repeat(64), "accepted": true, "message": "" }))
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (format!("http://{addr}"), hits)
    }

    #[tokio::test]
    async fn one_unsignable_draft_anywhere_means_nothing_is_published() {
        use std::sync::atomic::Ordering;
        let member = "b".repeat(64);
        let (url, hits) = counting_relay().await;
        let client =
            BeekeeperClient::new(url, nostr::Keys::generate(), None, None).expect("client");
        let mut stale = assign(&member, json!([]));
        stale["createdAt"] = json!(NOW - 1_800);
        let drafts = json!([assign(&member, json!([])), stale]);
        let error = execute_drafts_at(&client, PROJECT, &drafts, NOW)
            .await
            .unwrap_err()
            .to_string();
        assert!(error.contains("nothing was signed"), "{error}");
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "not even the valid first draft"
        );

        // The same stand-in does see a run that seals: the zero above is
        // the executor refusing, not the relay unreachable.
        let good = json!([assign(&member, json!([])), assign(&member, json!([]))]);
        let ran = execute_drafts_at(&client, PROJECT, &good, NOW)
            .await
            .expect("published");
        assert_eq!(ran["published"].as_array().map(Vec::len), Some(2));
        assert_eq!(hits.load(Ordering::SeqCst), 2);
    }
}
