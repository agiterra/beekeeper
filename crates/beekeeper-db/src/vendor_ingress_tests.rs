// Tests for vendor_ingress. Pure ones run everywhere; the Postgres ones are
// `#[ignore = "requires Postgres"]` and run against a migrated scratch DB.

use super::*;
use crate::channel::{create_channel_with_id, ChannelType, ChannelVisibility};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use sqlx::AssertSqlSafe;

const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1
const DIGEST: [u8; 32] = [7_u8; 32];

fn installation_stub(enabled: bool) -> VendorInstallation {
    VendorInstallation {
        community_id: CommunityId::from_uuid(Uuid::new_v4()),
        id: Uuid::new_v4(),
        plugin: "github".into(),
        channel_id: Uuid::new_v4(),
        created_by: vec![1; 32],
        secret: b"super-secret-webhook-key".to_vec(),
        settings: serde_json::json!({}),
        enabled,
        created_at: Utc::now(),
        disabled_at: None,
        rejected_count: 0,
        last_rejected_at: None,
        last_rejected_reason: None,
        error_count: 0,
        last_error_at: None,
        last_error: None,
        last_accepted_at: None,
    }
}

fn app_event(channel: Uuid, created_at: u64) -> nostr::Event {
    EventBuilder::new(
        Kind::Custom(KIND_APPLICATION_EVENT as u16),
        r#"{"schema":1,"data":{"ref":"refs/heads/main"}}"#,
    )
    .tags([Tag::parse(["h", &channel.to_string()]).expect("h tag")])
    .custom_created_at(Timestamp::from(created_at))
    .sign_with_keys(&Keys::generate())
    .expect("sign")
}

fn accept<'a>(id: &'a str, event: &'a nostr::Event) -> Delivery<'a> {
    Delivery {
        delivery_id: id,
        vendor_event: "push",
        plugin_digest: &DIGEST,
        disposition: DeliveryDisposition::Accept { event },
    }
}

fn not_accept<'a>(id: &'a str, reject: bool, reason: &'a str) -> Delivery<'a> {
    Delivery {
        delivery_id: id,
        vendor_event: "push",
        plugin_digest: &DIGEST,
        disposition: if reject {
            DeliveryDisposition::Reject { reason }
        } else {
            DeliveryDisposition::Ignore { reason }
        },
    }
}

// ── pure ────────────────────────────────────────────────────────────────

#[test]
fn debug_never_prints_the_secret() {
    let rendered = format!("{:?}", installation_stub(true));
    assert!(!rendered.contains("super-secret"), "{rendered}");
    assert!(rendered.contains("<redacted>"));
}

#[test]
fn deliveries_are_validated_before_any_write() {
    let enabled = installation_stub(true);
    let event = app_event(enabled.channel_id, 1_700_000_000);
    assert!(validate_delivery(&enabled, &accept("d-1", &event)).is_ok());
    assert!(validate_delivery(&installation_stub(false), &accept("d-1", &event)).is_err());
    assert!(validate_delivery(&enabled, &accept("", &event)).is_err());
    let long_id = "x".repeat(MAX_DELIVERY_ID_LEN + 1);
    assert!(validate_delivery(&enabled, &accept(&long_id, &event)).is_err());
    let mut short_digest = accept("d-1", &event);
    short_digest.plugin_digest = &DIGEST[..31];
    assert!(validate_delivery(&enabled, &short_digest).is_err());
    let kind_nine = EventBuilder::new(Kind::Custom(9), "hi")
        .sign_with_keys(&Keys::generate())
        .expect("sign");
    assert!(validate_delivery(&enabled, &accept("d-1", &kind_nine)).is_err());
}

#[test]
fn bounded_text_cuts_on_a_char_boundary() {
    let text = "é".repeat(MAX_REASON_LEN);
    let cut = bounded(&text);
    assert!(cut.len() <= MAX_REASON_LEN);
    assert!(cut.chars().all(|c| c == 'é'));
    assert_eq!(bounded("short"), "short");
}

// ── Postgres ────────────────────────────────────────────────────────────

struct Fx {
    pool: PgPool,
    community: CommunityId,
    installation: VendorInstallation,
}

async fn fixture() -> Fx {
    let url = std::env::var("BEEKEEPER_TEST_DATABASE_URL")
        .or_else(|_| std::env::var("DATABASE_URL"))
        .unwrap_or_else(|_| TEST_DB_URL.to_owned());
    let pool = PgPool::connect(&url).await.expect("connect to test DB");
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
        .bind(id)
        .bind(format!("vendor-{}.example", id.simple()))
        .execute(&pool)
        .await
        .expect("insert community");
    let community = CommunityId::from_uuid(id);
    let creator = Keys::generate().public_key().to_bytes().to_vec();
    crate::user::ensure_user(&pool, community, &creator)
        .await
        .expect("creator");
    let channel_id = Uuid::new_v4();
    create_channel_with_id(
        &pool,
        community,
        channel_id,
        "vendor-ingress",
        ChannelType::Stream,
        ChannelVisibility::Open,
        None,
        &creator,
        None,
        None,
    )
    .await
    .expect("channel");
    let installation = create_installation(
        &pool,
        community,
        &NewInstallation {
            plugin: "github",
            channel_id,
            created_by: &creator,
            secret: b"0123456789abcdef0123456789abcdef",
            settings: &serde_json::json!({"events": ["push"]}),
        },
    )
    .await
    .expect("installation");
    Fx {
        pool,
        community,
        installation,
    }
}

impl Fx {
    async fn event_present(&self, event: &nostr::Event) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM events WHERE community_id = $1 AND id = $2",
        )
        .bind(self.community.as_uuid())
        .bind(event.id.as_bytes().as_slice())
        .fetch_one(&self.pool)
        .await
        .expect("count events")
            > 0
    }

    async fn queue_present(&self, event: &nostr::Event) -> bool {
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM workflow_dispatch_queue WHERE community_id = $1 AND event_id = $2",
        )
        .bind(self.community.as_uuid())
        .bind(event.id.as_bytes().as_slice())
        .fetch_one(&self.pool)
        .await
        .expect("count queue")
            > 0
    }

    async fn delivery(&self, id: &str) -> Option<VendorDelivery> {
        get_delivery(&self.pool, self.community, self.installation.id, id)
            .await
            .expect("read delivery")
    }

    async fn app_events_in_channel(&self) -> i64 {
        sqlx::query_scalar(
            "SELECT count(*) FROM events WHERE community_id = $1 AND channel_id = $2 AND kind = $3",
        )
        .bind(self.community.as_uuid())
        .bind(self.installation.channel_id)
        .bind(KIND_APPLICATION_EVENT as i32)
        .fetch_one(&self.pool)
        .await
        .expect("count channel events")
    }

    async fn reload(&self) -> VendorInstallation {
        get_installation(&self.pool, self.community, self.installation.id)
            .await
            .expect("load installation")
            .expect("installation exists")
    }

    fn event(&self, created_at: u64) -> nostr::Event {
        app_event(self.installation.channel_id, created_at)
    }
}

/// Accept writes the event (into the installation's channel), its dispatch
/// queue row and the delivery row in ONE transaction: none is visible to
/// another connection before commit, a rollback leaves none, a commit
/// leaves all three.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn accept_writes_event_queue_and_delivery_atomically() {
    let fx = fixture().await;
    let delivery_id = Uuid::new_v4().to_string();

    let rolled_back = fx.event(1_700_000_001);
    let mut tx = fx.pool.begin().await.expect("begin");
    let outcome = record_delivery(
        &mut tx,
        &fx.installation,
        &accept(&delivery_id, &rolled_back),
    )
    .await
    .expect("record in tx");
    assert!(matches!(outcome, DeliveryOutcome::Accepted(_)));
    assert!(
        !fx.event_present(&rolled_back).await,
        "event visible before commit"
    );
    assert!(
        !fx.queue_present(&rolled_back).await,
        "queue row visible before commit"
    );
    assert!(
        fx.delivery(&delivery_id).await.is_none(),
        "delivery visible before commit"
    );
    tx.rollback().await.expect("rollback");
    assert!(!fx.event_present(&rolled_back).await);
    assert!(!fx.queue_present(&rolled_back).await);
    assert!(fx.delivery(&delivery_id).await.is_none());

    let event = fx.event(1_700_000_002);
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &event))
            .await
            .expect("record committed");
    let DeliveryOutcome::Accepted(stored) = outcome else {
        panic!("expected Accepted, got {outcome:?}");
    };
    assert_eq!(stored.channel_id, Some(fx.installation.channel_id));
    assert!(fx.event_present(&event).await);
    assert!(fx.queue_present(&event).await);
    let row = fx.delivery(&delivery_id).await.expect("delivery row");
    assert_eq!(row.disposition, "accepted");
    assert_eq!(
        row.event_id.as_deref(),
        Some(event.id.as_bytes().as_slice())
    );
    assert_eq!(row.plugin_digest, DIGEST.to_vec());
    assert_eq!(row.vendor_event, "push");
    assert!(fx.reload().await.last_accepted_at.is_some());
}

/// A failure after the event insert (here: the delivery row insert itself
/// fails) leaves none of event, queue row and delivery row.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_failure_after_the_event_insert_leaves_none_of_the_three() {
    let fx = fixture().await;
    let delivery_id = format!("inject-{}", Uuid::new_v4().simple());
    let name = format!("inject_vendor_{}", Uuid::new_v4().simple());
    sqlx::raw_sql(AssertSqlSafe(format!(
        "CREATE FUNCTION {name}() RETURNS trigger LANGUAGE plpgsql AS $$ \
         BEGIN IF NEW.delivery_id = '{delivery_id}' THEN \
         RAISE EXCEPTION 'injected delivery insert failure'; END IF; RETURN NEW; END $$; \
         CREATE TRIGGER {name} BEFORE INSERT ON vendor_deliveries \
         FOR EACH ROW EXECUTE FUNCTION {name}();"
    )))
    .execute(&fx.pool)
    .await
    .expect("inject failure");

    let event = fx.event(1_700_000_003);
    let result =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &event)).await;

    sqlx::raw_sql(AssertSqlSafe(format!(
        "DROP TRIGGER {name} ON vendor_deliveries; DROP FUNCTION {name}();"
    )))
    .execute(&fx.pool)
    .await
    .expect("remove injected failure");

    let error = result.expect_err("the injected failure must surface");
    assert!(
        error
            .to_string()
            .contains("injected delivery insert failure"),
        "{error}"
    );
    assert!(
        !fx.event_present(&event).await,
        "event committed without its delivery row"
    );
    assert!(
        !fx.queue_present(&event).await,
        "queue row committed without its delivery row"
    );
    assert!(fx.delivery(&delivery_id).await.is_none());
    assert!(fx.reload().await.last_accepted_at.is_none());
}

/// A redelivery of an accepted (or ignored) delivery is a Duplicate and
/// writes nothing — sequentially and when two copies race.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires Postgres"]
async fn a_duplicate_authenticated_delivery_writes_no_second_event() {
    let fx = fixture().await;
    let delivery_id = Uuid::new_v4().to_string();
    let first = fx.event(1_700_000_010);
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &first))
            .await
            .expect("first");
    assert!(matches!(outcome, DeliveryOutcome::Accepted(_)));

    let second = fx.event(1_700_000_011);
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &second))
            .await
            .expect("redelivery");
    let DeliveryOutcome::Duplicate {
        disposition,
        event_id,
    } = outcome
    else {
        panic!("expected Duplicate, got {outcome:?}");
    };
    assert_eq!(disposition, "accepted");
    assert_eq!(event_id.as_deref(), Some(first.id.as_bytes().as_slice()));
    assert!(!fx.event_present(&second).await);
    assert!(!fx.queue_present(&second).await);
    assert_eq!(fx.app_events_in_channel().await, 1);

    // ignored is final too.
    let ignored_id = Uuid::new_v4().to_string();
    let outcome = record_delivery_committed(
        &fx.pool,
        &fx.installation,
        &not_accept(&ignored_id, false, "ping"),
    )
    .await
    .expect("ignore");
    assert!(matches!(outcome, DeliveryOutcome::Ignored));
    let late = fx.event(1_700_000_012);
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&ignored_id, &late))
            .await
            .expect("accept after ignore");
    assert!(
        matches!(outcome, DeliveryOutcome::Duplicate { ref disposition, .. } if disposition == "ignored")
    );
    assert!(!fx.event_present(&late).await);

    // Eight concurrent copies of a fresh delivery: exactly one event.
    let racing_id = Uuid::new_v4().to_string();
    let mut tasks = Vec::new();
    for n in 0..8_u64 {
        let pool = fx.pool.clone();
        let installation = fx.installation.clone();
        let racing_id = racing_id.clone();
        tasks.push(tokio::spawn(async move {
            let event = app_event(installation.channel_id, 1_700_000_100 + n);
            record_delivery_committed(&pool, &installation, &accept(&racing_id, &event))
                .await
                .expect("racing delivery")
        }));
    }
    let mut accepted = 0;
    let mut duplicates = 0;
    for task in tasks {
        match task.await.expect("join") {
            DeliveryOutcome::Accepted(_) => accepted += 1,
            DeliveryOutcome::Duplicate { .. } => duplicates += 1,
            other => panic!("unexpected {other:?}"),
        }
    }
    assert_eq!((accepted, duplicates), (1, 7));
    assert_eq!(fx.app_events_in_channel().await, 2);
}

/// One delivery projected twice — a retry after `Busy`, or a second
/// projection of the same `VerifiedDelivery` — reaches B1 twice with the
/// same delivery id and the same event (the host stamps the original receive
/// time, so the relay-signed id is identical). The second record is a
/// Duplicate: one event, one dispatch queue row, so one claimable job and
/// one admitted run.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_repeated_projection_of_one_delivery_publishes_and_admits_once() {
    let fx = fixture().await;
    let relay = Keys::generate();
    let delivery_id = Uuid::new_v4().to_string();
    let project = || {
        EventBuilder::new(
            Kind::Custom(KIND_APPLICATION_EVENT as u16),
            r#"{"schema":1,"data":{"ref":"refs/heads/main"}}"#,
        )
        .tags([Tag::parse(["h", &fx.installation.channel_id.to_string()]).expect("h tag")])
        .custom_created_at(Timestamp::from(1_700_000_200))
        .sign_with_keys(&relay)
        .expect("sign")
    };
    let first = project();
    let retried = project();
    assert_eq!(first.id, retried.id, "one delivery, one event id");

    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &first))
            .await
            .expect("first projection");
    assert!(
        matches!(outcome, DeliveryOutcome::Accepted(_)),
        "{outcome:?}"
    );
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &retried))
            .await
            .expect("repeated projection");
    let DeliveryOutcome::Duplicate {
        disposition,
        event_id,
    } = outcome
    else {
        panic!("expected Duplicate, got {outcome:?}");
    };
    assert_eq!(disposition, "accepted");
    assert_eq!(event_id.as_deref(), Some(first.id.as_bytes().as_slice()));
    assert_eq!(fx.app_events_in_channel().await, 1, "published once");
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_dispatch_queue WHERE community_id = $1")
            .bind(fx.community.as_uuid())
            .fetch_one(&fx.pool)
            .await
            .expect("count queue");
    assert_eq!(queued, 1, "queued once");

    // Admission: the one queue row is the one job, and it admits one run.
    let owner = Keys::generate().public_key().to_bytes();
    crate::user::ensure_user(&fx.pool, fx.community, &owner)
        .await
        .expect("workflow owner");
    let workflow = Uuid::new_v4();
    crate::workflow::upsert_workflow(
        &fx.pool,
        fx.community,
        workflow,
        None,
        &owner,
        "on push",
        r#"{"name":"on push","trigger":{"on":"manual"},"steps":[]}"#,
        &[3_u8; 32],
        None,
    )
    .await
    .expect("workflow");
    let batch = crate::workflow_dispatch::claim_dispatch_batch_in(
        &fx.pool,
        Some(fx.community),
        10,
        Utc::now() + chrono::Duration::seconds(60),
    )
    .await
    .expect("claim")
    .expect("one job due");
    assert_eq!(batch.jobs.len(), 1, "one job");
    assert_eq!(batch.jobs[0].event.event.id, first.id);
    let event_id = first.id.as_bytes().to_vec();
    crate::workflow_dispatch::freeze_candidates(
        &fx.pool,
        fx.community,
        &event_id,
        batch.claim_id,
        &[workflow],
    )
    .await
    .expect("freeze");
    let run = crate::workflow_dispatch::create_dispatch_run(
        &fx.pool,
        fx.community,
        &event_id,
        workflow,
        None,
        &[3_u8; 32],
    )
    .await
    .expect("admit");
    assert!(
        matches!(
            run,
            crate::workflow_dispatch::DispatchRunOutcome::Created(_)
        ),
        "{run:?}"
    );
    let runs: i64 =
        sqlx::query_scalar("SELECT count(*) FROM workflow_runs WHERE community_id = $1")
            .bind(fx.community.as_uuid())
            .fetch_one(&fx.pool)
            .await
            .expect("count runs");
    assert_eq!(runs, 1, "admitted once");
}

/// F3: an unauthenticated rejection is a counter on the installation, never
/// a delivery row, so the genuine delivery under that id is still accepted.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn an_unauthenticated_rejection_does_not_occupy_the_delivery_key() {
    let fx = fixture().await;
    let guessed_id = Uuid::new_v4().to_string();
    for _ in 0..3 {
        record_unauthenticated_rejection(
            &fx.pool,
            fx.community,
            fx.installation.id,
            "signature mismatch",
        )
        .await
        .expect("count rejection");
    }
    let installation = fx.reload().await;
    assert_eq!(installation.rejected_count, 3);
    assert_eq!(
        installation.last_rejected_reason.as_deref(),
        Some("signature mismatch")
    );
    assert!(installation.last_rejected_at.is_some());
    let rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM vendor_deliveries WHERE community_id = $1 AND installation_id = $2",
    )
    .bind(fx.community.as_uuid())
    .bind(fx.installation.id)
    .fetch_one(&fx.pool)
    .await
    .expect("count deliveries");
    assert_eq!(
        rows, 0,
        "an unauthenticated request must not write a delivery row"
    );

    let event = fx.event(1_700_000_020);
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&guessed_id, &event))
            .await
            .expect("genuine delivery");
    assert!(
        matches!(outcome, DeliveryOutcome::Accepted(_)),
        "{outcome:?}"
    );
    assert!(fx.event_present(&event).await);

    record_installation_error(&fx.pool, fx.community, fx.installation.id, "plugin trap")
        .await
        .expect("count error");
    let installation = fx.reload().await;
    assert_eq!(installation.error_count, 1);
    assert_eq!(installation.last_error.as_deref(), Some("plugin trap"));
    assert_eq!(
        installation.rejected_count, 3,
        "errors are counted separately"
    );
}

/// An authenticated rejection is recorded, and a later delivery under the
/// same id is re-projected over it; once accepted it is final.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_rejected_delivery_is_reprojected_by_a_redelivery() {
    let fx = fixture().await;
    let delivery_id = Uuid::new_v4().to_string();
    let outcome = record_delivery_committed(
        &fx.pool,
        &fx.installation,
        &not_accept(&delivery_id, true, "payload missing repository"),
    )
    .await
    .expect("reject");
    assert!(matches!(outcome, DeliveryOutcome::Rejected));
    let row = fx.delivery(&delivery_id).await.expect("rejected row");
    assert_eq!(row.disposition, "rejected");
    assert_eq!(row.reason.as_deref(), Some("payload missing repository"));
    assert!(row.event_id.is_none());

    // Rejected again (same plugin): still re-projectable.
    let outcome = record_delivery_committed(
        &fx.pool,
        &fx.installation,
        &not_accept(&delivery_id, true, "still malformed"),
    )
    .await
    .expect("reject again");
    assert!(matches!(outcome, DeliveryOutcome::Rejected));

    let event = fx.event(1_700_000_030);
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &event))
            .await
            .expect("re-projection");
    assert!(
        matches!(outcome, DeliveryOutcome::Accepted(_)),
        "{outcome:?}"
    );
    let row = fx.delivery(&delivery_id).await.expect("row");
    assert_eq!(row.disposition, "accepted");
    assert_eq!(row.reason, None);
    assert_eq!(
        row.event_id.as_deref(),
        Some(event.id.as_bytes().as_slice())
    );
    assert!(fx.queue_present(&event).await);

    let again = fx.event(1_700_000_031);
    let outcome =
        record_delivery_committed(&fx.pool, &fx.installation, &accept(&delivery_id, &again))
            .await
            .expect("redelivery");
    assert!(matches!(outcome, DeliveryOutcome::Duplicate { .. }));
    assert_eq!(fx.app_events_in_channel().await, 1);
}

/// A disabled installation records nothing.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn a_disabled_installation_records_nothing() {
    let fx = fixture().await;
    sqlx::query(
        "UPDATE vendor_installations SET enabled = false, disabled_at = now() \
         WHERE community_id = $1 AND id = $2",
    )
    .bind(fx.community.as_uuid())
    .bind(fx.installation.id)
    .execute(&fx.pool)
    .await
    .expect("disable");
    let disabled = fx.reload().await;
    let event = fx.event(1_700_000_040);
    let delivery_id = Uuid::new_v4().to_string();
    assert!(
        record_delivery_committed(&fx.pool, &disabled, &accept(&delivery_id, &event))
            .await
            .is_err()
    );
    assert!(!fx.event_present(&event).await);
    assert!(fx.delivery(&delivery_id).await.is_none());
}

/// Retention removes delivery rows older than 30 days and keeps newer ones.
#[tokio::test]
#[ignore = "requires Postgres"]
async fn retention_prunes_only_old_deliveries() {
    let fx = fixture().await;
    let old_id = Uuid::new_v4().to_string();
    let new_id = Uuid::new_v4().to_string();
    for id in [&old_id, &new_id] {
        record_delivery_committed(&fx.pool, &fx.installation, &not_accept(id, false, "ping"))
            .await
            .expect("ignore");
    }
    sqlx::query(
        "UPDATE vendor_deliveries SET received_at = now() - interval '31 days' \
         WHERE community_id = $1 AND installation_id = $2 AND delivery_id = $3",
    )
    .bind(fx.community.as_uuid())
    .bind(fx.installation.id)
    .bind(&old_id)
    .execute(&fx.pool)
    .await
    .expect("age row");
    prune_vendor_deliveries(&fx.pool, DELIVERY_RETENTION)
        .await
        .expect("prune");
    assert!(fx.delivery(&old_id).await.is_none());
    assert!(fx.delivery(&new_id).await.is_some());
}
