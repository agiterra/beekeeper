use super::*;
use crate::managed_agents::reconcile::retain_agent_record;
use crate::managed_agents::retention::{get_pending_sync, mark_synced};
use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag};

const PROJECT: &str =
    "30621:abababababababababababababababababababababababababababababababab:tank-loop";

fn record(pubkey: &str) -> ManagedAgentRecord {
    serde_json::from_value(serde_json::json!({
        "pubkey": pubkey,
        "name": "Builder",
        "relay_url": "wss://relay.example",
        "acp_command": "buzz-acp",
        "agent_command": "goose",
        "agent_args": [],
        "mcp_command": "",
        "turn_timeout_seconds": 320,
        "home_role": "builder",
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z",
        "last_started_at": null,
        "last_stopped_at": null,
        "last_exit_code": null,
        "last_error": null
    }))
    .expect("record fixture")
}

fn digest() -> String {
    project_agent_digest(PROJECT).expect("digest")
}

fn with(
    project_ref: Option<&str>,
    project_public: Option<bool>,
    carried: Option<&str>,
) -> ManagedAgentRecord {
    let mut record = record(&"a".repeat(64));
    record.project_ref = project_ref.map(str::to_owned);
    record.project_public = project_public;
    record.carried_project_digest = carried.map(str::to_owned);
    record
}

#[test]
fn project_digest_shape_is_64_lowercase_hex() {
    assert!(is_project_digest(&digest()));
    assert!(!is_project_digest(&digest().to_ascii_uppercase()));
    assert!(!is_project_digest(&"a".repeat(63)));
    assert!(!is_project_digest(&"g".repeat(64)));
    assert!(!is_project_digest(""));
}

/// `(project_ref, project_public, carried, expected published, label)`.
type ProjectionRow<'a> = (
    Option<&'a str>,
    Option<bool>,
    Option<&'a str>,
    Option<&'a str>,
    &'a str,
);

/// `(project_ref, project_public, carried, inbound digest, changed, carried
/// after, label)`.
type InboundCarryRow<'a> = (
    Option<&'a str>,
    Option<bool>,
    Option<&'a str>,
    Option<&'a str>,
    bool,
    Option<&'a str>,
    &'a str,
);

/// Every row of the projection table.
#[test]
fn published_project_digest_table() {
    let own = digest();
    let carried = "c".repeat(64);
    let rows: &[ProjectionRow] = &[
        (
            Some(PROJECT),
            Some(true),
            None,
            Some(&own),
            "public: own digest",
        ),
        (
            Some(PROJECT),
            Some(true),
            Some(&carried),
            Some(&own),
            "public: own digest over a carried one",
        ),
        (Some(PROJECT), Some(false), None, None, "private: none"),
        (
            Some(PROJECT),
            Some(false),
            Some(&carried),
            None,
            "private: withdraws a carried digest",
        ),
        (Some(PROJECT), None, None, None, "unverified: nothing new"),
        (
            Some(PROJECT),
            None,
            Some(&carried),
            Some(&carried),
            "unverified: keeps the carried digest",
        ),
        (None, None, None, None, "unassociated: none"),
        (
            None,
            None,
            Some(&carried),
            Some(&carried),
            "unassociated: keeps the carried digest",
        ),
        (
            None,
            Some(false),
            Some(&carried),
            Some(&carried),
            "no project to be private: keeps the carried digest",
        ),
        (
            Some("30621:not-hex:tank-loop"),
            Some(true),
            Some(&carried),
            Some(&carried),
            "public but malformed: falls back to the carried digest",
        ),
        (
            Some("30621:not-hex:tank-loop"),
            Some(true),
            None,
            None,
            "public but malformed: never a digest of a guess",
        ),
        (
            None,
            None,
            Some("NOT-A-DIGEST"),
            None,
            "a malformed carried value is never published",
        ),
    ];
    for (project_ref, public, carried, expected, label) in rows {
        let record = with(*project_ref, *public, *carried);
        assert_eq!(
            published_project_digest(&record).as_deref(),
            *expected,
            "{label}"
        );
    }
}

#[test]
fn carry_inbound_project_digest_table() {
    let d = digest();
    let other = "e".repeat(64);
    let rows: &[InboundCarryRow] = &[
        (
            None,
            None,
            None,
            Some(&d),
            true,
            Some(&d),
            "unassociated: carries",
        ),
        (
            Some(PROJECT),
            None,
            None,
            Some(&d),
            true,
            Some(&d),
            "unverified: carries",
        ),
        (
            Some(PROJECT),
            Some(false),
            None,
            Some(&d),
            false,
            None,
            "private: ignored",
        ),
        (
            Some(PROJECT),
            Some(true),
            None,
            Some(&other),
            false,
            None,
            "public: own answer",
        ),
        (
            None,
            None,
            Some(&d),
            None,
            false,
            Some(&d),
            "digest-less: never clears",
        ),
        (
            None,
            None,
            Some(&d),
            Some("bad"),
            false,
            Some(&d),
            "malformed: never clears",
        ),
        (
            None,
            None,
            Some(&d),
            Some(&other),
            true,
            Some(&other),
            "newer digest replaces",
        ),
        (
            None,
            None,
            Some(&d),
            Some(&d),
            false,
            Some(&d),
            "same digest: no change",
        ),
    ];
    for (project_ref, public, carried, inbound, changed, expected, label) in rows {
        let mut record = with(*project_ref, *public, *carried);
        assert_eq!(
            carry_inbound_project_digest(&mut record, *inbound),
            *changed,
            "{label}"
        );
        assert_eq!(
            record.carried_project_digest.as_deref(),
            *expected,
            "{label}"
        );
        assert_eq!(record.project_ref.as_deref(), *project_ref, "{label}");
        assert_eq!(record.project_public, *public, "{label}");
        assert_eq!(record.home_role.as_deref(), Some("builder"), "{label}");
    }
}

#[test]
fn withheld_withdrawal_table() {
    let d = digest();
    let unassociated = with(None, None, None);
    let unverified = with(Some(PROJECT), None, None);
    let public = with(Some(PROJECT), Some(true), None);
    let private = with(Some(PROJECT), Some(false), None);
    let carry = WithdrawalGuard::Carry(d.clone());
    let rows: &[(
        Option<&ManagedAgentRecord>,
        Option<&str>,
        WithdrawalGuard,
        &str,
    )] = &[
        (
            Some(&unassociated),
            None,
            WithdrawalGuard::Publish,
            "no relay digest",
        ),
        (
            Some(&unassociated),
            Some("bad"),
            WithdrawalGuard::Publish,
            "malformed relay digest",
        ),
        (
            None,
            None,
            WithdrawalGuard::Publish,
            "missing record, nothing to withdraw",
        ),
        (
            Some(&private),
            Some(&d),
            WithdrawalGuard::Publish,
            "known private withdraws",
        ),
        (
            Some(&unassociated),
            Some(&d),
            carry.clone(),
            "unassociated carries",
        ),
        (
            Some(&unverified),
            Some(&d),
            carry.clone(),
            "unverified carries",
        ),
        (
            Some(&public),
            Some(&d),
            carry.clone(),
            "public, not known private: carries",
        ),
        (
            None,
            Some(&d),
            WithdrawalGuard::Withhold,
            "missing record withholds",
        ),
    ];
    for (local, relay, expected, label) in rows {
        assert_eq!(withheld_withdrawal(*local, *relay), *expected, "{label}");
    }
}

#[test]
fn apply_relay_digest_sets_only_the_carried_digest() {
    let pubkey = "a".repeat(64);
    let mut agents = vec![with(None, None, None)];
    let (decision, changed) = apply_relay_digest(&mut agents, &pubkey, &digest());
    assert_eq!(decision, WithdrawalGuard::Carry(digest()));
    assert!(changed);
    assert_eq!(agents[0].carried_project_digest, Some(digest()));
    assert_eq!(agents[0].project_ref, None);
    assert_eq!(agents[0].home_role.as_deref(), Some("builder"));
    let (_, changed) = apply_relay_digest(&mut agents, &pubkey, &digest());
    assert!(!changed, "idempotent");

    let mut private = vec![with(Some(PROJECT), Some(false), None)];
    assert_eq!(
        apply_relay_digest(&mut private, &pubkey, &digest()),
        (WithdrawalGuard::Publish, false)
    );
    assert_eq!(private[0].carried_project_digest, None);

    assert_eq!(
        apply_relay_digest(&mut agents, &"b".repeat(64), &digest()),
        (WithdrawalGuard::Withhold, false)
    );
}

/// The hook's record update, end to end against a retention store: a carry
/// re-retains a pending row that now carries the digest.
#[test]
fn retain_after_carry_retains_a_row_with_the_digest() {
    let dir = tempfile::tempdir().expect("tempdir");
    let conn = open_retention_db(&dir.path().join("retention.db")).expect("db");
    let keys = Keys::generate();
    let owner = keys.public_key().to_hex();
    let pubkey = "a".repeat(64);
    let mut agents = vec![with(None, None, None)];
    assert!(retain_agent_record(&conn, &keys, &agents[0]).expect("retain"));
    let before = get_retained_event(&conn, KIND_MANAGED_AGENT, &owner, &pubkey)
        .expect("read")
        .expect("row");
    assert!(!before.content.contains("project_digest"));

    apply_relay_digest(&mut agents, &pubkey, &digest());
    assert_eq!(
        retain_after_carry(&conn, &keys, &agents[0]).expect("carry"),
        CarryHookOutcome::Carried
    );
    let after = get_retained_event(&conn, KIND_MANAGED_AGENT, &owner, &pubkey)
        .expect("read")
        .expect("row");
    assert!(after.pending_sync);
    assert!(after.created_at > before.created_at);
    assert_eq!(content_digest(&after.content), Some(digest()));

    // A record that can publish no digest is withheld, not published.
    let unpublishable = with(Some(PROJECT), Some(false), Some(&digest()));
    assert_eq!(
        retain_after_carry(&conn, &keys, &unpublishable).expect("withhold"),
        CarryHookOutcome::Withhold
    );
}

/// With a carried digest persisted, reconcile writes once and then no-ops:
/// the projection is stable across boots.
#[test]
fn carried_digest_reconciles_once_then_no_ops() {
    let dir = tempfile::tempdir().expect("tempdir");
    let conn = open_retention_db(&dir.path().join("retention.db")).expect("db");
    let keys = Keys::generate();
    let record = with(None, None, Some(&digest()));
    assert!(retain_agent_record(&conn, &keys, &record).expect("first"));
    let row = get_pending_sync(&conn).expect("pending").remove(0);
    assert_eq!(content_digest(&row.content), Some(digest()));
    mark_synced(
        &conn,
        row.kind,
        &row.pubkey,
        &row.d_tag,
        row.created_at,
        &row.content,
    )
    .expect("synced");
    for _ in 0..3 {
        assert!(
            !retain_agent_record(&conn, &keys, &record).expect("again"),
            "a carried digest must not churn"
        );
    }
    assert!(get_pending_sync(&conn).expect("pending").is_empty());
}

fn agent_event(keys: &Keys, agent: &str, content: serde_json::Value, at: u64) -> nostr::Event {
    EventBuilder::new(Kind::Custom(KIND_MANAGED_AGENT as u16), content.to_string())
        .tags(vec![Tag::parse(["d", agent]).expect("d")])
        .custom_created_at(nostr::Timestamp::from(at))
        .sign_with_keys(keys)
        .expect("sign")
}

fn content(digest: Option<&str>) -> serde_json::Value {
    let mut value =
        serde_json::json!({ "name": "Builder", "parallelism": 1, "respond_to": "owner-only" });
    if let Some(digest) = digest {
        value["project_digest"] = serde_json::json!(digest);
    }
    value
}

#[test]
fn relay_head_digest_reads_only_the_newest_signed_owner_event() {
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let agent = "a".repeat(64);
    let d = digest();
    let old = agent_event(&owner, &agent, content(Some(&d)), 100);
    let newer_without = agent_event(&owner, &agent, content(None), 200);
    assert_eq!(
        relay_head_digest(&[old.clone(), newer_without], &owner.public_key(), &agent),
        None,
        "the newest head decides"
    );
    let foreign = agent_event(&stranger, &agent, content(Some(&d)), 300);
    let other_agent = agent_event(&owner, &"b".repeat(64), content(Some(&d)), 300);
    assert_eq!(
        relay_head_digest(
            &[foreign, other_agent, old.clone()],
            &owner.public_key(),
            &agent
        ),
        Some(d.clone()),
        "another author or address is ignored"
    );
    let mut forged: serde_json::Value = serde_json::from_str(
        &agent_event(&owner, &agent, content(Some(&"e".repeat(64))), 400).as_json(),
    )
    .expect("json");
    forged["content"] = serde_json::json!(content(None).to_string());
    let forged = nostr::Event::from_json(forged.to_string()).expect("event");
    assert_eq!(
        relay_head_digest(&[forged, old], &owner.public_key(), &agent),
        Some(d),
        "a head whose signature does not verify is ignored"
    );
}

#[cfg(not(target_os = "windows"))]
mod flush {
    use super::*;
    use crate::app_state::build_app_state;
    use std::sync::Arc;

    struct Stub {
        base: String,
        published: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    /// `POST /query` answers `head` (or HTTP 500 when `None`); `POST /events`
    /// accepts and records every event.
    async fn spawn_stub(head: Option<nostr::Event>) -> Stub {
        use axum::{http::StatusCode, routing::post, Router};
        let published = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&published);
        let head = head.map(|event| format!("[{}]", event.as_json()));
        let app = Router::new()
            .route(
                "/query",
                post(move || {
                    let head = head.clone();
                    async move {
                        match head {
                            Some(body) => (StatusCode::OK, body),
                            None => (StatusCode::INTERNAL_SERVER_ERROR, String::new()),
                        }
                    }
                }),
            )
            .route(
                "/events",
                post(move |body: String| {
                    let sink = Arc::clone(&sink);
                    async move {
                        let event: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        let id = event["id"].as_str().unwrap_or("").to_string();
                        sink.lock().expect("sink").push(event);
                        (
                            StatusCode::OK,
                            serde_json::json!({ "event_id": id, "accepted": true, "message": "" })
                                .to_string(),
                        )
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let addr = listener.local_addr().expect("addr");
        tokio::spawn(async move {
            axum::serve(listener, app).await.ok();
        });
        Stub {
            base: format!("http://{addr}"),
            published,
        }
    }

    /// A test hook over an in-memory store, doing what `app_carry_hook` does.
    fn memory_hook(
        agents: Arc<Mutex<Vec<ManagedAgentRecord>>>,
    ) -> impl Fn(&Path, &nostr::Keys, &str, &str) -> Result<CarryHookOutcome, String> + Send + Sync
    {
        move |db_path, keys, agent, relay_digest| {
            let mut agents = agents.lock().map_err(|e| e.to_string())?;
            let (decision, _) = apply_relay_digest(&mut agents, agent, relay_digest);
            match decision {
                WithdrawalGuard::Publish => Ok(CarryHookOutcome::Publish),
                WithdrawalGuard::Withhold => Ok(CarryHookOutcome::Withhold),
                WithdrawalGuard::Carry(_) => {
                    let record = agents
                        .iter()
                        .find(|record| record.pubkey == agent)
                        .ok_or("gone")?;
                    retain_after_carry(&open_retention_db(db_path)?, keys, record)
                }
            }
        }
    }

    struct Rig {
        _dir: tempfile::TempDir,
        db_path: std::path::PathBuf,
        keys: Keys,
        agent: String,
        agents: Arc<Mutex<Vec<ManagedAgentRecord>>>,
    }

    fn rig(local: ManagedAgentRecord) -> Rig {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("retention.db");
        let keys = Keys::generate();
        let agent = local.pubkey.clone();
        let conn = open_retention_db(&db_path).expect("db");
        assert!(retain_agent_record(&conn, &keys, &local).expect("retain"));
        Rig {
            _dir: dir,
            db_path,
            keys,
            agent,
            agents: Arc::new(Mutex::new(vec![local])),
        }
    }

    async fn flush(rig: &Rig, stub: &Stub) -> u32 {
        let state = build_app_state();
        let hook = memory_hook(Arc::clone(&rig.agents));
        crate::managed_agents::persona_events::flush_pending_events_at(
            &rig.db_path,
            &state,
            &stub.base,
            &rig.keys,
            Some(&hook),
        )
        .await
        .expect("flush")
    }

    fn head_row(rig: &Rig) -> RetainedEvent {
        let conn = open_retention_db(&rig.db_path).expect("db");
        get_retained_event(
            &conn,
            KIND_MANAGED_AGENT,
            &rig.keys.public_key().to_hex(),
            &rig.agent,
        )
        .expect("read")
        .expect("row")
    }

    /// The boot race: this computer's digest-less row is never published over
    /// a relay head that carries a digest; the digest is carried and the
    /// corrected row publishes in the same sweep.
    #[tokio::test]
    async fn digest_less_row_is_carried_instead_of_withdrawing() {
        let rig = rig(with(None, None, None));
        let head = agent_event(&rig.keys, &rig.agent, content(Some(&digest())), 1);
        let stub = spawn_stub(Some(head)).await;

        assert_eq!(flush(&rig, &stub).await, 1);
        let published = stub.published.lock().expect("published").clone();
        assert_eq!(published.len(), 1, "only the corrected row publishes");
        let published_content = published[0]["content"].as_str().expect("content");
        assert_eq!(content_digest(published_content), Some(digest()));
        let row = head_row(&rig);
        assert!(!row.pending_sync, "the corrected row is marked synced");
        assert_eq!(content_digest(&row.content), Some(digest()));
        assert_eq!(
            rig.agents.lock().expect("agents")[0].carried_project_digest,
            Some(digest())
        );
    }

    #[tokio::test]
    async fn unreadable_relay_head_withholds_the_row() {
        let rig = rig(with(None, None, None));
        let stub = spawn_stub(None).await;
        assert_eq!(flush(&rig, &stub).await, 0);
        assert!(stub.published.lock().expect("published").is_empty());
        assert!(
            head_row(&rig).pending_sync,
            "stays pending for the next sweep"
        );
        assert_eq!(
            rig.agents.lock().expect("agents")[0].carried_project_digest,
            None
        );
    }

    #[tokio::test]
    async fn known_private_project_withdraws_on_purpose() {
        let rig = rig(with(Some(PROJECT), Some(false), None));
        let head = agent_event(&rig.keys, &rig.agent, content(Some(&digest())), 1);
        let stub = spawn_stub(Some(head)).await;
        assert_eq!(flush(&rig, &stub).await, 1);
        let published = stub.published.lock().expect("published").clone();
        assert_eq!(published.len(), 1);
        let published_content = published[0]["content"].as_str().expect("content");
        assert_eq!(content_digest(published_content), None);
        assert!(!head_row(&rig).pending_sync);
    }

    #[tokio::test]
    async fn relay_head_without_a_digest_publishes_as_retained() {
        let rig = rig(with(None, None, None));
        let head = agent_event(&rig.keys, &rig.agent, content(None), 1);
        let stub = spawn_stub(Some(head)).await;
        assert_eq!(flush(&rig, &stub).await, 1);
        assert_eq!(
            rig.agents.lock().expect("agents")[0].carried_project_digest,
            None
        );
    }
}
