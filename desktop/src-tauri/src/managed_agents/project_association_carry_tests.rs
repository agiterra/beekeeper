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

fn withdrawn(mut record: ManagedAgentRecord) -> ManagedAgentRecord {
    record.project_publication_withdrawn = true;
    record
}

fn announced(digest: &str) -> Announced {
    Announced::Digest(digest.to_owned())
}

/// Every row of the projection table: `(record, expected, label)`.
#[test]
fn published_association_table() {
    let own = digest();
    let carried = "c".repeat(64);
    let rows: Vec<(ManagedAgentRecord, Announced, &str)> = vec![
        (
            with(Some(PROJECT), Some(true), None),
            announced(&own),
            "public: own digest",
        ),
        (
            with(Some(PROJECT), Some(true), Some(&carried)),
            announced(&own),
            "public: own digest over a carried one",
        ),
        (
            withdrawn(with(Some(PROJECT), Some(true), None)),
            announced(&own),
            "own verified public wins over a stale withdrawal",
        ),
        (
            with(Some(PROJECT), Some(false), None),
            Announced::Withdrawn,
            "private: marker",
        ),
        (
            with(Some(PROJECT), Some(false), Some(&carried)),
            Announced::Withdrawn,
            "private: marker, never the carried digest",
        ),
        (
            withdrawn(with(None, None, Some(&carried))),
            Announced::Withdrawn,
            "withdrawn: marker, never the carried digest",
        ),
        (
            withdrawn(with(Some(PROJECT), None, None)),
            Announced::Withdrawn,
            "withdrawn, unverified",
        ),
        (
            with(Some(PROJECT), None, None),
            Announced::Nothing,
            "unverified: nothing new",
        ),
        (
            with(Some(PROJECT), None, Some(&carried)),
            announced(&carried),
            "unverified: keeps the carried digest",
        ),
        (
            with(None, None, None),
            Announced::Nothing,
            "unassociated: none",
        ),
        (
            with(None, None, Some(&carried)),
            announced(&carried),
            "unassociated: keeps the carried digest",
        ),
        (
            with(None, Some(false), Some(&carried)),
            announced(&carried),
            "no project to be private: keeps the carried digest",
        ),
        (
            with(Some("30621:not-hex:tank-loop"), Some(true), Some(&carried)),
            announced(&carried),
            "public but malformed: falls back to the carried digest",
        ),
        (
            with(Some("30621:not-hex:tank-loop"), Some(true), None),
            Announced::Nothing,
            "public but malformed: never a digest of a guess",
        ),
        (
            with(None, None, Some("NOT-A-DIGEST")),
            Announced::Nothing,
            "a malformed carried value is never published",
        ),
    ];
    for (record, expected, label) in rows {
        assert_eq!(published_association(&record), expected, "{label}");
        let digest = match &expected {
            Announced::Digest(digest) => Some(digest.clone()),
            _ => None,
        };
        assert_eq!(published_project_digest(&record), digest, "{label}");
        assert_eq!(
            published_project_withdrawn(&record),
            expected == Announced::Withdrawn,
            "{label}"
        );
    }
}

/// Single computer, public → private → public.
#[test]
fn visibility_change_withdraws_then_republishes_on_one_computer() {
    let mut record = with(Some(PROJECT), Some(true), None);
    let content = crate::managed_agents::agent_events::agent_event_content;
    assert_eq!(content(&record).project_digest, Some(digest()));
    assert!(!content(&record).project_withdrawn);

    assert!(note_verified_visibility(&mut record, false));
    assert!(record.project_publication_withdrawn);
    assert_eq!(content(&record).project_digest, None);
    assert!(
        content(&record).project_withdrawn,
        "the marker is published"
    );
    assert!(!note_verified_visibility(&mut record, false), "idempotent");

    assert!(note_verified_visibility(&mut record, true));
    assert!(
        !record.project_publication_withdrawn,
        "own public clears it"
    );
    assert_eq!(content(&record).project_digest, Some(digest()));
    assert!(!content(&record).project_withdrawn);
}

/// `(record, inbound digest, inbound marker, changed, carried after,
/// withdrawn after, label)`.
type InboundRow<'a> = (
    ManagedAgentRecord,
    Option<&'a str>,
    bool,
    bool,
    Option<&'a str>,
    bool,
    &'a str,
);

#[test]
fn carry_inbound_project_digest_table() {
    let d = digest();
    let other = "e".repeat(64);
    let rows: Vec<InboundRow> = vec![
        (
            with(None, None, None),
            Some(&d),
            false,
            true,
            Some(&d),
            false,
            "unassociated: carries",
        ),
        (
            with(Some(PROJECT), None, None),
            Some(&d),
            false,
            true,
            Some(&d),
            false,
            "unverified: carries",
        ),
        (
            with(Some(PROJECT), Some(false), None),
            Some(&d),
            false,
            false,
            None,
            false,
            "private: ignored",
        ),
        (
            with(Some(PROJECT), Some(true), None),
            Some(&other),
            false,
            false,
            None,
            false,
            "public: own answer",
        ),
        (
            withdrawn(with(None, None, None)),
            Some(&d),
            false,
            false,
            None,
            true,
            "withdrawn: never carries",
        ),
        (
            with(None, None, Some(&d)),
            None,
            false,
            false,
            Some(&d),
            false,
            "digest-less: never clears",
        ),
        (
            with(None, None, Some(&d)),
            Some("bad"),
            false,
            false,
            Some(&d),
            false,
            "malformed: never clears",
        ),
        (
            with(None, None, Some(&d)),
            Some(&other),
            false,
            true,
            Some(&other),
            false,
            "newer digest replaces",
        ),
        (
            with(None, None, Some(&d)),
            Some(&d),
            false,
            false,
            Some(&d),
            false,
            "same digest: no change",
        ),
        (
            with(None, None, Some(&d)),
            None,
            true,
            true,
            None,
            true,
            "marker: withdraws and drops the carry",
        ),
        (
            with(None, None, None),
            Some(&d),
            true,
            true,
            None,
            true,
            "marker wins over a digest beside it",
        ),
        (
            withdrawn(with(None, None, None)),
            None,
            true,
            false,
            None,
            true,
            "marker again: no change",
        ),
        (
            withdrawn(with(None, None, Some(&d))),
            None,
            false,
            false,
            Some(&d),
            true,
            "digest-less, marker-less: never clears the flag",
        ),
    ];
    for (mut record, digest, marker, changed, carried, is_withdrawn, label) in rows {
        let (project_ref, public) = (record.project_ref.clone(), record.project_public);
        assert_eq!(
            carry_inbound_project_digest(&mut record, digest, marker),
            changed,
            "{label}"
        );
        assert_eq!(record.carried_project_digest.as_deref(), carried, "{label}");
        assert_eq!(
            record.project_publication_withdrawn, is_withdrawn,
            "{label}"
        );
        assert_eq!(record.project_ref, project_ref, "{label}");
        assert_eq!(record.project_public, public, "{label}");
        assert_eq!(record.home_role.as_deref(), Some("builder"), "{label}");
    }
}

/// Two computers of one owner, through the pure rules: A publishes a public
/// digest, B carries it, A verifies private and withdraws, B receives the
/// marker and never publishes the digest again.
#[test]
fn a_withdrawal_reaches_the_carrying_computer() {
    let content = crate::managed_agents::agent_events::agent_event_content;
    let mut host_a = with(Some(PROJECT), Some(true), None);
    let mut host_b = with(None, None, None);
    let published = content(&host_a);
    assert!(carry_inbound_project_digest(
        &mut host_b,
        published.project_digest.as_deref(),
        published.project_withdrawn
    ));
    assert_eq!(content(&host_b).project_digest, Some(digest()));

    assert!(note_verified_visibility(&mut host_a, false));
    let withdrawal = content(&host_a);
    assert_eq!(withdrawal.project_digest, None);
    assert!(withdrawal.project_withdrawn);
    assert!(carry_inbound_project_digest(
        &mut host_b,
        withdrawal.project_digest.as_deref(),
        withdrawal.project_withdrawn
    ));
    assert_eq!(
        content(&host_b).project_digest,
        None,
        "B never republishes it"
    );
    assert!(content(&host_b).project_withdrawn);
    assert!(
        !carry_inbound_project_digest(&mut host_b, Some(&digest()), false),
        "a late copy of the old digest is not carried again"
    );
}

/// `(local record, pending row, relay head, decision, label)`; `None` =
/// no local record.
type HeadRow<'a> = (
    Option<ManagedAgentRecord>,
    Announced,
    Announced,
    CarryHookOutcome,
    &'a str,
);

#[test]
fn apply_relay_head_table() {
    use CarryHookOutcome::{Corrected, Publish, Withhold};
    let d = digest();
    let other = "e".repeat(64);
    let rows: Vec<HeadRow> = vec![
        (
            Some(with(None, None, None)),
            Announced::Nothing,
            Announced::Nothing,
            Publish,
            "nothing anywhere",
        ),
        (
            None,
            Announced::Nothing,
            Announced::Nothing,
            Publish,
            "missing record, nothing to withdraw",
        ),
        (
            None,
            Announced::Nothing,
            announced(&d),
            Withhold,
            "missing record withholds over a digest",
        ),
        (
            None,
            Announced::Nothing,
            Announced::Withdrawn,
            Withhold,
            "missing record withholds over a marker",
        ),
        (
            Some(with(None, None, None)),
            Announced::Nothing,
            announced(&d),
            Corrected,
            "stale host carries (F3)",
        ),
        (
            Some(with(Some(PROJECT), None, None)),
            Announced::Nothing,
            announced(&d),
            Corrected,
            "unverified carries",
        ),
        (
            Some(with(Some(PROJECT), Some(false), None)),
            Announced::Withdrawn,
            announced(&d),
            Publish,
            "known private publishes its marker over a digest",
        ),
        (
            Some(withdrawn(with(None, None, None))),
            Announced::Withdrawn,
            announced(&d),
            Publish,
            "withdrawn publishes its marker over a digest",
        ),
        (
            Some(with(None, None, Some(&d))),
            announced(&d),
            Announced::Withdrawn,
            Corrected,
            "a carried digest is never published over a marker",
        ),
        (
            Some(with(None, None, Some(&d))),
            announced(&d),
            Announced::Nothing,
            Publish,
            "a carried digest over no head publishes as is",
        ),
        (
            Some(with(None, None, Some(&d))),
            announced(&d),
            announced(&other),
            Corrected,
            "the relay's newer digest replaces the carried one",
        ),
        (
            Some(withdrawn(with(None, None, None))),
            Announced::Nothing,
            Announced::Nothing,
            Corrected,
            "a stale row of a withdrawn record gains the marker",
        ),
        (
            Some(with(Some("30621:not-hex:tank-loop"), Some(true), None)),
            Announced::Nothing,
            announced(&d),
            Withhold,
            "a record that still projects nothing never withdraws blind",
        ),
    ];
    for (local, row, head, expected, label) in rows {
        let agent = "a".repeat(64);
        let mut agents: Vec<_> = local.into_iter().collect();
        let (decision, _) = apply_relay_head(&mut agents, &agent, &row, &head);
        assert_eq!(decision, expected, "{label}");
        if let (Some(record), Announced::Withdrawn) = (agents.first(), &head) {
            assert!(record.project_publication_withdrawn, "{label}");
            assert_eq!(record.carried_project_digest, None, "{label}");
        }
    }
}

#[test]
fn apply_relay_head_changes_only_association_state() {
    let pubkey = "a".repeat(64);
    let mut agents = vec![with(None, None, None)];
    let head = announced(&digest());
    assert_eq!(
        apply_relay_head(&mut agents, &pubkey, &Announced::Nothing, &head),
        (CarryHookOutcome::Corrected, true)
    );
    assert_eq!(agents[0].carried_project_digest, Some(digest()));
    assert_eq!(agents[0].project_ref, None);
    assert_eq!(agents[0].home_role.as_deref(), Some("builder"));
    let (_, changed) = apply_relay_head(&mut agents, &pubkey, &head, &head);
    assert!(!changed, "idempotent");
    assert_eq!(
        apply_relay_head(&mut agents, &"b".repeat(64), &Announced::Nothing, &head),
        (CarryHookOutcome::Withhold, false)
    );
}

#[test]
fn only_the_own_public_digest_is_backed() {
    let pubkey = "a".repeat(64);
    let public = vec![with(Some(PROJECT), Some(true), None)];
    assert!(backed_by_own_public(
        &public,
        &pubkey,
        &announced(&digest())
    ));
    assert!(!backed_by_own_public(
        &public,
        &pubkey,
        &announced(&"e".repeat(64))
    ));
    assert!(!backed_by_own_public(&public, &pubkey, &Announced::Nothing));
    assert!(!backed_by_own_public(
        &public,
        &"b".repeat(64),
        &announced(&digest())
    ));
    let carried = vec![with(None, None, Some(&digest()))];
    assert!(
        !backed_by_own_public(&carried, &pubkey, &announced(&digest())),
        "a carried digest is not backed"
    );
    let private = vec![with(Some(PROJECT), Some(false), None)];
    assert!(!backed_by_own_public(
        &private,
        &pubkey,
        &announced(&digest())
    ));
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
    assert_eq!(content_announced(&row.content), announced(&digest()));
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

fn marker() -> serde_json::Value {
    let mut value = content(None);
    value[buzz_core_pkg::project_agent_association::PROJECT_AGENT_WITHDRAWN_CONTENT_KEY] =
        serde_json::json!(true);
    value
}

#[test]
fn relay_head_reads_only_the_newest_signed_owner_event() {
    let owner = Keys::generate();
    let stranger = Keys::generate();
    let agent = "a".repeat(64);
    let d = digest();
    let old = agent_event(&owner, &agent, content(Some(&d)), 100);
    let newer_without = agent_event(&owner, &agent, content(None), 200);
    assert_eq!(
        relay_head(&[old.clone(), newer_without], &owner.public_key(), &agent),
        Announced::Nothing,
        "the newest head decides"
    );
    let newer_marker = agent_event(&owner, &agent, marker(), 200);
    assert_eq!(
        relay_head(&[old.clone(), newer_marker], &owner.public_key(), &agent),
        Announced::Withdrawn
    );
    let mut both = content(Some(&d));
    both["project_withdrawn"] = serde_json::json!(true);
    assert_eq!(
        content_announced(&both.to_string()),
        Announced::Withdrawn,
        "a marker wins over a digest beside it"
    );
    let foreign = agent_event(&stranger, &agent, marker(), 300);
    let other_agent = agent_event(&owner, &"b".repeat(64), marker(), 300);
    assert_eq!(
        relay_head(
            &[foreign, other_agent, old.clone()],
            &owner.public_key(),
            &agent
        ),
        announced(&d),
        "another author or address is ignored"
    );
    let mut forged: serde_json::Value = serde_json::from_str(
        &agent_event(&owner, &agent, content(Some(&"e".repeat(64))), 400).as_json(),
    )
    .expect("json");
    forged["content"] = serde_json::json!(marker().to_string());
    let forged = nostr::Event::from_json(forged.to_string()).expect("event");
    assert_eq!(
        relay_head(&[forged, old], &owner.public_key(), &agent),
        announced(&d),
        "a head whose signature does not verify is ignored"
    );
}

#[cfg(not(target_os = "windows"))]
mod flush {
    use super::*;
    use crate::app_state::build_app_state;
    use std::sync::Arc;

    /// A relay stand-in: `POST /query` answers every stored event (or HTTP 500
    /// when unreadable); `POST /events` stores and records each event.
    struct Stub {
        base: String,
        published: Arc<Mutex<Vec<serde_json::Value>>>,
    }

    async fn spawn_stub(seed: Vec<nostr::Event>, readable: bool) -> Stub {
        use axum::{http::StatusCode, routing::post, Router};
        let stored = Arc::new(Mutex::new(
            seed.iter()
                .map(|event| serde_json::from_str(&event.as_json()).expect("json"))
                .collect::<Vec<serde_json::Value>>(),
        ));
        let published = Arc::new(Mutex::new(Vec::new()));
        let (read, write, sink) = (Arc::clone(&stored), stored, Arc::clone(&published));
        let app = Router::new()
            .route(
                "/query",
                post(move || {
                    let read = Arc::clone(&read);
                    async move {
                        if !readable {
                            return (StatusCode::INTERNAL_SERVER_ERROR, String::new());
                        }
                        let events = read.lock().expect("stored").clone();
                        (StatusCode::OK, serde_json::Value::from(events).to_string())
                    }
                }),
            )
            .route(
                "/events",
                post(move |body: String| {
                    let (write, sink) = (Arc::clone(&write), Arc::clone(&sink));
                    async move {
                        let event: serde_json::Value =
                            serde_json::from_str(&body).unwrap_or_default();
                        let id = event["id"].as_str().unwrap_or("").to_string();
                        write.lock().expect("stored").push(event.clone());
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

    /// A test hook over an in-memory store, doing what `AppCarryHook` does.
    struct MemoryHook(Arc<Mutex<Vec<ManagedAgentRecord>>>);

    impl CarryHook for MemoryHook {
        fn backed(&self, agent: &str, row: &Announced) -> Result<bool, String> {
            let agents = self.0.lock().map_err(|e| e.to_string())?;
            Ok(backed_by_own_public(&agents, agent, row))
        }

        fn reconcile(
            &self,
            db_path: &Path,
            keys: &nostr::Keys,
            agent: &str,
            row: &Announced,
            head: &Announced,
        ) -> Result<CarryHookOutcome, String> {
            let mut agents = self.0.lock().map_err(|e| e.to_string())?;
            let (decision, _) = apply_relay_head(&mut agents, agent, row, head);
            if decision == CarryHookOutcome::Corrected {
                let record = agents
                    .iter()
                    .find(|record| record.pubkey == agent)
                    .ok_or("gone")?;
                retain_agent_record(&open_retention_db(db_path)?, keys, record)?;
            }
            Ok(decision)
        }
    }

    struct Rig {
        _dir: tempfile::TempDir,
        db_path: std::path::PathBuf,
        keys: Keys,
        agent: String,
        agents: Arc<Mutex<Vec<ManagedAgentRecord>>>,
    }

    /// One computer: its own retention store and record, signing as `keys`.
    fn rig_as(keys: &Keys, local: ManagedAgentRecord) -> Rig {
        let dir = tempfile::tempdir().expect("tempdir");
        let db_path = dir.path().join("retention.db");
        let agent = local.pubkey.clone();
        let conn = open_retention_db(&db_path).expect("db");
        assert!(retain_agent_record(&conn, keys, &local).expect("retain"));
        Rig {
            _dir: dir,
            db_path,
            keys: keys.clone(),
            agent,
            agents: Arc::new(Mutex::new(vec![local])),
        }
    }

    fn rig(local: ManagedAgentRecord) -> Rig {
        rig_as(&Keys::generate(), local)
    }

    /// Change the record the way a local edit does, and re-retain it.
    fn edit(rig: &Rig, change: impl FnOnce(&mut ManagedAgentRecord)) {
        let mut agents = rig.agents.lock().expect("agents");
        change(&mut agents[0]);
        let conn = open_retention_db(&rig.db_path).expect("db");
        retain_agent_record(&conn, &rig.keys, &agents[0]).expect("retain");
    }

    async fn flush(rig: &Rig, stub: &Stub) -> u32 {
        let state = build_app_state();
        let hook = MemoryHook(Arc::clone(&rig.agents));
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

    fn published(stub: &Stub) -> Vec<Announced> {
        stub.published
            .lock()
            .expect("published")
            .iter()
            .map(|event| content_announced(event["content"].as_str().expect("content")))
            .collect()
    }

    /// What every reader sees: the newest owner-signed event on the relay.
    async fn relay_now(rig: &Rig, stub: &Stub) -> Announced {
        let state = build_app_state();
        let events = crate::relay::query_relay_at_with_keys(
            &state,
            &stub.base,
            &[serde_json::json!({})],
            &rig.keys,
            None,
        )
        .await
        .expect("query");
        relay_head(&events, &rig.keys.public_key(), &rig.agent)
    }

    /// The boot race (stale host, F3): this computer's digest-less row is
    /// never published over a relay head that carries a digest; the digest is
    /// carried and the corrected row publishes in the same sweep.
    #[tokio::test]
    async fn digest_less_row_is_carried_instead_of_withdrawing() {
        let rig = rig(with(None, None, None));
        let head = agent_event(&rig.keys, &rig.agent, content(Some(&digest())), 1);
        let stub = spawn_stub(vec![head], true).await;

        assert_eq!(flush(&rig, &stub).await, 1);
        assert_eq!(published(&stub), vec![announced(&digest())]);
        let row = head_row(&rig);
        assert!(!row.pending_sync, "the corrected row is marked synced");
        assert_eq!(content_announced(&row.content), announced(&digest()));
        assert_eq!(
            rig.agents.lock().expect("agents")[0].carried_project_digest,
            Some(digest())
        );
    }

    #[tokio::test]
    async fn unreadable_relay_head_withholds_the_row() {
        let rig = rig(with(None, None, Some(&digest())));
        let stub = spawn_stub(Vec::new(), false).await;
        assert_eq!(flush(&rig, &stub).await, 0);
        assert!(
            published(&stub).is_empty(),
            "a carried digest is withheld too"
        );
        assert!(
            head_row(&rig).pending_sync,
            "stays pending for the next sweep"
        );
    }

    /// A row backed by this computer's own verified public project publishes
    /// without reading the relay, so a relay outage never holds it back.
    #[tokio::test]
    async fn own_public_digest_publishes_without_a_relay_read() {
        let rig = rig(with(Some(PROJECT), Some(true), None));
        let stub = spawn_stub(Vec::new(), false).await;
        assert_eq!(flush(&rig, &stub).await, 1);
        assert_eq!(published(&stub), vec![announced(&digest())]);
    }

    #[tokio::test]
    async fn known_private_project_withdraws_on_purpose() {
        let rig = rig(with(Some(PROJECT), Some(false), None));
        let head = agent_event(&rig.keys, &rig.agent, content(Some(&digest())), 1);
        let stub = spawn_stub(vec![head], true).await;
        assert_eq!(flush(&rig, &stub).await, 1);
        assert_eq!(published(&stub), vec![Announced::Withdrawn]);
        assert!(!head_row(&rig).pending_sync);
    }

    #[tokio::test]
    async fn relay_head_without_a_digest_publishes_as_retained() {
        let rig = rig(with(None, None, None));
        let head = agent_event(&rig.keys, &rig.agent, content(None), 1);
        let stub = spawn_stub(vec![head], true).await;
        assert_eq!(flush(&rig, &stub).await, 1);
        assert_eq!(published(&stub), vec![Announced::Nothing]);
        assert_eq!(
            rig.agents.lock().expect("agents")[0].carried_project_digest,
            None
        );
    }

    /// The review's blocker, on the wire: B holds a pending row carrying A's
    /// old digest while the relay head is A's withdrawal. B publishes the
    /// marker instead, and the relay's final head carries no digest.
    #[tokio::test]
    async fn a_carried_digest_is_never_republished_over_a_withdrawal() {
        let owner = Keys::generate();
        let host_b = rig_as(&owner, with(None, None, Some(&digest())));
        let agent = host_b.agent.clone();
        let seed = vec![
            agent_event(&owner, &agent, content(Some(&digest())), 100),
            agent_event(&owner, &agent, marker(), 200),
        ];
        let stub = spawn_stub(seed, true).await;

        assert_eq!(flush(&host_b, &stub).await, 1);
        assert_eq!(published(&stub), vec![Announced::Withdrawn]);
        {
            let agents = host_b.agents.lock().expect("agents");
            assert!(agents[0].project_publication_withdrawn);
            assert_eq!(agents[0].carried_project_digest, None);
        }
        assert!(!head_row(&host_b).pending_sync);
        assert_eq!(relay_now(&host_b, &stub).await, Announced::Withdrawn);
    }

    /// Two computers of one owner, end to end through the flush loop: A
    /// publishes its public digest, B carries it, A verifies private and
    /// withdraws, B receives the marker, and B's next local edit republishes
    /// the marker rather than the digest.
    #[tokio::test]
    async fn public_to_private_withdrawal_holds_across_computers() {
        let owner = Keys::generate();
        let host_a = rig_as(&owner, with(Some(PROJECT), Some(true), None));
        let host_b = rig_as(&owner, with(None, None, None));
        let stub = spawn_stub(Vec::new(), true).await;
        assert_eq!(flush(&host_a, &stub).await, 1, "A publishes its digest");
        let a_published = head_row(&host_a).content;
        {
            let mut agents = host_b.agents.lock().expect("agents");
            let inbound = content_announced(&a_published);
            assert_eq!(inbound, announced(&digest()));
            let Announced::Digest(inbound) = inbound else {
                unreachable!()
            };
            assert!(carry_inbound_project_digest(
                &mut agents[0],
                Some(&inbound),
                false
            ));
        }

        edit(&host_a, |record| {
            note_verified_visibility(record, false);
        });
        assert_eq!(flush(&host_a, &stub).await, 1, "A withdraws");
        assert_eq!(relay_now(&host_a, &stub).await, Announced::Withdrawn);
        {
            let mut agents = host_b.agents.lock().expect("agents");
            let withdrawal = content_announced(&head_row(&host_a).content);
            assert_eq!(withdrawal, Announced::Withdrawn);
            assert!(carry_inbound_project_digest(&mut agents[0], None, true));
            assert_eq!(published_association(&agents[0]), Announced::Withdrawn);
        }

        edit(&host_b, |record| record.parallelism += 1);
        assert_eq!(flush(&host_b, &stub).await, 1, "B's edit publishes");
        assert_eq!(
            published(&stub),
            vec![
                announced(&digest()),
                Announced::Withdrawn,
                Announced::Withdrawn
            ]
        );
        assert_eq!(relay_now(&host_b, &stub).await, Announced::Withdrawn);
    }
}
