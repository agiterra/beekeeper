//! Durable workflow dispatch queue (migration 0047).
//!
//! Every committed kind:50000 application event and kind:46008 CI result gets
//! one `workflow_dispatch_queue` row, written by the `events` AFTER INSERT
//! trigger inside the transaction that makes the event durable. A worker
//! claims rows, freezes the candidate workflows into
//! `workflow_dispatch_outcomes`, and admits each candidate at most once
//! through the partial unique index `workflow_runs_dispatch_once`.
//!
//! Rows are never deleted when attempts run out or the source event is gone:
//! they end in state `failed` with a reason, so a dispatch that did not
//! happen stays visible.

use beekeeper_core::kind::{KIND_APPLICATION_EVENT, KIND_CI_RESULT};

/// Event kinds the `enqueue_workflow_dispatch()` trigger enqueues.
///
/// Must stay identical to the `IN (...)` list in migration 0047 and in
/// `schema/schema.sql`; a unit test pins all three.
pub const DISPATCH_KINDS: [u32; 2] = [KIND_APPLICATION_EVENT, KIND_CI_RESULT];

#[cfg(test)]
mod tests {
    use super::*;

    /// The kind list lives in SQL (the trigger) and in Rust (this module);
    /// a drift in either would silently stop dispatch for one kind.
    #[test]
    fn trigger_kind_list_matches_dispatch_kinds() {
        let expected = format!(
            "IF NEW.kind IN ({}) THEN",
            DISPATCH_KINDS
                .iter()
                .map(u32::to_string)
                .collect::<Vec<_>>()
                .join(", ")
        );
        let migration = include_str!("../../../migrations/0047_workflow_dispatch_queue.sql");
        let schema = include_str!("../../../schema/schema.sql");
        assert!(migration.contains(&expected), "migration 0047: {expected}");
        let function_start = schema
            .find("CREATE FUNCTION enqueue_workflow_dispatch()")
            .expect("schema.sql defines enqueue_workflow_dispatch");
        assert!(
            schema[function_start..].contains(&expected),
            "schema.sql: {expected}"
        );
    }
}

#[cfg(test)]
mod db_tests {
    use super::*;
    use crate::event::{insert_event, insert_event_with_thread_metadata_tx};
    use beekeeper_core::CommunityId;
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use sqlx::{AssertSqlSafe, PgPool};
    use uuid::Uuid;

    const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

    fn test_db_url() -> String {
        std::env::var("BEEKEEPER_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| TEST_DB_URL.to_owned())
    }

    async fn setup_pool() -> PgPool {
        PgPool::connect(&test_db_url())
            .await
            .expect("connect to test DB")
    }

    async fn make_community(pool: &PgPool) -> CommunityId {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(id)
            .bind(format!("dispatch-{}.example", id.simple()))
            .execute(pool)
            .await
            .expect("insert test community");
        CommunityId::from_uuid(id)
    }

    fn event_of_kind(kind: u32) -> nostr::Event {
        EventBuilder::new(Kind::Custom(kind as u16), format!("kind {kind}"))
            .tags([Tag::parse(["h", &Uuid::new_v4().to_string()]).expect("h tag")])
            .sign_with_keys(&Keys::generate())
            .expect("sign event")
    }

    async fn queue_rows(pool: &PgPool, community: CommunityId, event_id: &[u8]) -> Vec<i32> {
        sqlx::query_scalar(
            "SELECT kind FROM workflow_dispatch_queue WHERE community_id = $1 AND event_id = $2",
        )
        .bind(community.as_uuid())
        .bind(event_id)
        .fetch_all(pool)
        .await
        .expect("read queue")
    }

    async fn make_workflow(pool: &PgPool, community: CommunityId) -> Uuid {
        let id = Uuid::new_v4();
        let owner = Keys::generate().public_key().to_bytes();
        crate::user::ensure_user(pool, community, &owner)
            .await
            .expect("insert workflow owner");
        crate::workflow::upsert_workflow(
            pool,
            community,
            id,
            None,
            &owner,
            "dispatch test",
            r#"{"name":"dispatch test","trigger":{"on":"manual"},"steps":[]}"#,
            &[3_u8; 32],
            None,
        )
        .await
        .expect("insert workflow");
        id
    }

    /// (a) Both dispatch kinds are enqueued by the insert's own transaction:
    /// visible inside it, invisible to another connection until commit,
    /// present after.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn dispatch_kinds_enqueue_in_the_inserting_transaction() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        for kind in DISPATCH_KINDS {
            let event = event_of_kind(kind);
            let id = event.id.as_bytes().to_vec();
            let mut tx = pool.begin().await.expect("begin");
            let (_, inserted) =
                insert_event_with_thread_metadata_tx(&mut tx, community, &event, None, None)
                    .await
                    .expect("insert event");
            assert!(inserted);
            let inside: Vec<i32> = sqlx::query_scalar(
                "SELECT kind FROM workflow_dispatch_queue \
                 WHERE community_id = $1 AND event_id = $2 AND state = 'pending'",
            )
            .bind(community.as_uuid())
            .bind(&id)
            .fetch_all(&mut *tx)
            .await
            .expect("read queue inside tx");
            assert_eq!(inside, vec![kind as i32], "kind {kind} enqueued in-tx");
            assert!(
                queue_rows(&pool, community, &id).await.is_empty(),
                "kind {kind}: the queue row is not visible before commit"
            );
            tx.commit().await.expect("commit");
            assert_eq!(
                queue_rows(&pool, community, &id).await,
                vec![kind as i32],
                "kind {kind}: exactly one queue row after commit"
            );
        }
    }

    /// (b) A rolled-back insert leaves neither the event nor a queue row.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn rolled_back_insert_leaves_no_queue_row() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        for kind in DISPATCH_KINDS {
            let event = event_of_kind(kind);
            let id = event.id.as_bytes().to_vec();
            let mut tx = pool.begin().await.expect("begin");
            insert_event_with_thread_metadata_tx(&mut tx, community, &event, None, None)
                .await
                .expect("insert event");
            tx.rollback().await.expect("rollback");
            assert!(queue_rows(&pool, community, &id).await.is_empty());
            let events: i64 = sqlx::query_scalar(
                "SELECT count(*) FROM events WHERE community_id = $1 AND id = $2",
            )
            .bind(community.as_uuid())
            .bind(&id)
            .fetch_one(&pool)
            .await
            .expect("count events");
            assert_eq!(events, 0);
        }
    }

    /// (c) Non-dispatch kinds are not enqueued (kind 9 is the busiest).
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn other_kinds_are_not_enqueued() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        // 46001 is a neighbouring workflow-execution kind; 9 a chat message.
        for kind in [9_u32, 46_001] {
            let event = event_of_kind(kind);
            let (_, inserted) = insert_event(&pool, community, &event, None)
                .await
                .expect("insert event");
            assert!(inserted);
            assert!(
                queue_rows(&pool, community, event.id.as_bytes())
                    .await
                    .is_empty(),
                "kind {kind} must not be enqueued"
            );
        }
    }

    /// (d) F2 regression: `ref_updated` creates one run per changed ref, all
    /// with the same trigger_event_id and no dispatch_event_id. The new
    /// exactly-once index must not constrain that.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn runs_sharing_a_trigger_event_id_still_insert() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let workflow = make_workflow(&pool, community).await;
        let trigger = [42_u8; 32];
        for _ in 0..2 {
            let outcome = crate::workflow::create_workflow_run(
                &pool,
                community,
                workflow,
                Some(&trigger),
                None,
                &[3_u8; 32],
            )
            .await
            .expect("ref_updated-style run inserts");
            assert!(matches!(
                outcome,
                crate::workflow::CreateRunOutcome::Created(_)
            ));
        }
        let runs: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM workflow_runs \
             WHERE community_id = $1 AND workflow_id = $2 AND trigger_event_id = $3",
        )
        .bind(community.as_uuid())
        .bind(workflow)
        .bind(&trigger[..])
        .fetch_one(&pool)
        .await
        .expect("count runs");
        assert_eq!(runs, 2);
    }

    /// (e) A second run for the same (workflow, dispatch_event_id) is a
    /// unique violation; another workflow, or another event, is not.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn duplicate_dispatch_event_id_conflicts() {
        let pool = setup_pool().await;
        let community = make_community(&pool).await;
        let workflow = make_workflow(&pool, community).await;
        let other_workflow = make_workflow(&pool, community).await;
        let insert = |workflow: Uuid, dispatch: [u8; 32]| {
            let pool = pool.clone();
            async move {
                sqlx::query(
                    "INSERT INTO workflow_runs (community_id, id, workflow_id, dispatch_event_id) \
                     VALUES ($1, $2, $3, $4)",
                )
                .bind(community.as_uuid())
                .bind(Uuid::new_v4())
                .bind(workflow)
                .bind(&dispatch[..])
                .execute(&pool)
                .await
            }
        };
        insert(workflow, [7; 32]).await.expect("first dispatch run");
        let duplicate = insert(workflow, [7; 32])
            .await
            .expect_err("second run for the same dispatch event must conflict");
        let code = duplicate
            .as_database_error()
            .and_then(|e| e.code())
            .map(|c| c.into_owned());
        assert_eq!(code.as_deref(), Some("23505"), "{duplicate}");
        insert(other_workflow, [7; 32])
            .await
            .expect("another workflow may admit the same event");
        insert(workflow, [8; 32])
            .await
            .expect("the same workflow may admit another event");
    }

    /// (f) The pgschema bootstrap path: pgschema emits partition children as
    /// standalone tables carrying copies of the parent's triggers, and
    /// `scripts/reconcile-schema-after-pgschema.sql` must drop those copies
    /// before re-attaching (PostgreSQL rejects an ATTACH whose child already
    /// has a same-named trigger). Simulated on a throwaway database built
    /// from schema.sql: detach the partition current events land in, give it
    /// standalone copies of every events trigger, run the reconcile script,
    /// and prove the child is re-attached with the dispatch trigger inherited
    /// and firing exactly once.
    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn pgschema_reconcile_leaves_the_dispatch_trigger_on_partition_children() {
        let base_url = test_db_url();
        let admin = PgPool::connect(&base_url).await.expect("connect admin");
        let probe_db = format!("beekeeper_dispatch_reconcile_{}", Uuid::new_v4().simple());
        sqlx::query(AssertSqlSafe(format!("CREATE DATABASE {probe_db}")))
            .execute(&admin)
            .await
            .expect("create probe database");
        let (prefix, _) = base_url.rsplit_once('/').expect("database url has a path");
        let probe = PgPool::connect(&format!("{prefix}/{probe_db}"))
            .await
            .expect("connect probe database");

        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let schema =
            std::fs::read_to_string(root.join("schema/schema.sql")).expect("read schema.sql");
        sqlx::raw_sql(AssertSqlSafe(schema))
            .execute(&probe)
            .await
            .expect("apply desired-state schema");

        // The partition an event created now lands in.
        let child: String = sqlx::query_scalar(
            "SELECT c.relname::text FROM pg_inherits i \
             JOIN pg_class c ON c.oid = i.inhrelid \
             WHERE i.inhparent = 'events'::regclass \
               AND pg_get_expr(c.relpartbound, c.oid) LIKE '%MAXVALUE)'",
        )
        .fetch_one(&probe)
        .await
        .expect("find the open-ended events partition");
        sqlx::raw_sql(AssertSqlSafe(format!(
            "ALTER TABLE events DETACH PARTITION {child}"
        )))
        .execute(&probe)
        .await
        .expect("detach the child as pgschema would leave it");
        // pgschema's standalone child carries its own copy of each parent
        // row trigger. Recreate any the detach did not leave behind.
        let copies = [
            (
                "events_enqueue_push_match",
                "AFTER INSERT",
                "enqueue_push_match_job",
            ),
            (
                "events_enqueue_workflow_dispatch",
                "AFTER INSERT",
                "enqueue_workflow_dispatch",
            ),
        ];
        for (name, timing, function) in copies {
            let present: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_trigger \
                 WHERE tgrelid = $1::regclass AND tgname = $2)",
            )
            .bind(&child)
            .bind(name)
            .fetch_one(&probe)
            .await
            .expect("inspect child triggers");
            if !present {
                sqlx::raw_sql(AssertSqlSafe(format!(
                    "CREATE TRIGGER {name} {timing} ON {child} \
                     FOR EACH ROW EXECUTE FUNCTION {function}()"
                )))
                .execute(&probe)
                .await
                .expect("create standalone trigger copy");
            }
        }
        let standalone_copy: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_trigger WHERE tgrelid = $1::regclass \
             AND tgname = 'events_enqueue_workflow_dispatch' AND tgparentid = 0)",
        )
        .bind(&child)
        .fetch_one(&probe)
        .await
        .expect("inspect standalone copy");
        assert!(
            standalone_copy,
            "precondition: the detached child carries a standalone dispatch trigger"
        );

        let reconcile =
            std::fs::read_to_string(root.join("scripts/reconcile-schema-after-pgschema.sql"))
                .expect("read reconcile script");
        let reconciled = sqlx::raw_sql(AssertSqlSafe(reconcile))
            .execute(&probe)
            .await;

        let attached: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM pg_inherits \
             WHERE inhparent = 'events'::regclass AND inhrelid = $1::regclass)",
        )
        .bind(&child)
        .fetch_one(&probe)
        .await
        .expect("inspect attachment");
        let inherited: Vec<i64> = sqlx::query_scalar(
            "SELECT count(*) FROM pg_trigger WHERE tgrelid = $1::regclass \
             AND tgname = 'events_enqueue_workflow_dispatch' AND tgparentid <> 0",
        )
        .bind(&child)
        .fetch_all(&probe)
        .await
        .expect("inspect inherited trigger");

        let community = make_community(&probe).await;
        let event = event_of_kind(KIND_APPLICATION_EVENT);
        let inserted = insert_event(&probe, community, &event, None).await;
        let rows = queue_rows(&probe, community, event.id.as_bytes()).await;

        probe.close().await;
        sqlx::query(AssertSqlSafe(format!(
            "DROP DATABASE {probe_db} WITH (FORCE)"
        )))
        .execute(&admin)
        .await
        .expect("drop probe database");

        reconciled.expect("the reconcile script re-attaches the child");
        assert!(attached, "{child} is attached to events again");
        assert_eq!(inherited, vec![1], "{child} inherits the dispatch trigger");
        inserted.expect("insert into the re-attached partition");
        assert_eq!(rows, vec![KIND_APPLICATION_EVENT as i32]);
    }
}
