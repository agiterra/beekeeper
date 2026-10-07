//! Atomic storage for relay-produced CI completion events.

use beekeeper_core::kind::KIND_CI_RESULT;
use beekeeper_core::{CommunityId, StoredEvent};
use nostr::Event;
use sqlx::PgPool;

use crate::error::{DbError, Result};
use crate::event::{insert_event_with_thread_metadata_tx, row_to_stored_event};

/// Result of atomically recording a relay-produced CI completion.
#[derive(Debug, Clone)]
pub enum CiResultInsertOutcome {
    /// No result existed for the community-scoped correlation key.
    Inserted(StoredEvent),
    /// The same canonical result was already stored; no row was inserted.
    Duplicate(StoredEvent),
    /// Different canonical content already occupied this correlation key.
    Conflict(StoredEvent),
}

/// Atomically insert a kind:46008 event by community-scoped correlation key.
///
/// The transaction-level advisory lock serializes all producers for the same
/// `(community_id, correlation_id)`. Exact canonical retries return the stored
/// event; different content, tags, or signer is a conflict and cannot overwrite it.
pub async fn insert_ci_result_event(
    pool: &PgPool,
    community_id: CommunityId,
    event: &Event,
    correlation_id: &str,
) -> Result<CiResultInsertOutcome> {
    if u32::from(event.kind.as_u16()) != KIND_CI_RESULT {
        return Err(DbError::InvalidData(format!(
            "CI result insert requires kind {KIND_CI_RESULT}"
        )));
    }

    let expected_d_tag = vec!["d".to_owned(), correlation_id.to_owned()];
    if !event
        .tags
        .iter()
        .any(|tag| tag.as_slice() == expected_d_tag.as_slice())
    {
        return Err(DbError::InvalidData(
            "CI result event does not carry the requested correlation d tag".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    let lock_key = format!("buzz:ci-result:{}:{correlation_id}", community_id.as_uuid());
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&lock_key)
        .execute(&mut *tx)
        .await?;

    let tag_probe = serde_json::json!([["d", correlation_id]]);
    let existing_rows = sqlx::query(
        r#"
        SELECT id, pubkey, created_at, kind, tags, content, sig, received_at, channel_id
        FROM events
        WHERE community_id = $1
          AND kind = $2
          AND deleted_at IS NULL
          AND tags @> $3::jsonb
        ORDER BY received_at ASC, id ASC
        LIMIT 2
        "#,
    )
    .bind(community_id.as_uuid())
    .bind(KIND_CI_RESULT as i32)
    .bind(tag_probe)
    .fetch_all(&mut *tx)
    .await?;

    if !existing_rows.is_empty() {
        let multiple_existing = existing_rows.len() > 1;
        let row = existing_rows
            .into_iter()
            .next()
            .ok_or_else(|| DbError::InvalidData("CI result query returned no first row".into()))?;
        let Some(existing) = row_to_stored_event(row)? else {
            return Err(DbError::InvalidData(
                "stored CI result could not be reconstructed".into(),
            ));
        };
        let existing_tags = serde_json::to_value(&existing.event.tags)?;
        let proposed_tags = serde_json::to_value(&event.tags)?;
        tx.commit().await?;
        return if !multiple_existing
            && existing.event.pubkey == event.pubkey
            && existing.event.content == event.content
            && existing_tags == proposed_tags
        {
            Ok(CiResultInsertOutcome::Duplicate(existing))
        } else {
            Ok(CiResultInsertOutcome::Conflict(existing))
        };
    }

    let (stored, was_inserted) =
        insert_event_with_thread_metadata_tx(&mut tx, community_id, event, None, None).await?;
    if !was_inserted {
        return Err(DbError::InvalidData(
            "CI result insert lost uniqueness without a matching correlation row".into(),
        ));
    }
    tx.commit().await?;
    Ok(CiResultInsertOutcome::Inserted(stored))
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};
    use uuid::Uuid;

    const TEST_DB_URL: &str = "postgres://buzz:buzz_dev@localhost:5432/buzz"; // sadscan:disable np.postgres.1

    async fn setup_pool() -> PgPool {
        let database_url = std::env::var("BEEKEEPER_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("DATABASE_URL"))
            .unwrap_or_else(|_| TEST_DB_URL.to_owned());
        PgPool::connect(&database_url)
            .await
            .expect("connect to test DB")
    }

    async fn make_test_community(pool: &PgPool) -> CommunityId {
        let id = Uuid::new_v4();
        let host = format!("ci-result-test-{}.example", id.simple());
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(id)
            .bind(host)
            .execute(pool)
            .await
            .expect("insert test community");
        CommunityId::from_uuid(id)
    }

    fn make_event(
        keys: &Keys,
        identity: &beekeeper_core::ci_result::CiResultIdentity,
        conclusion: beekeeper_core::ci_result::CiConclusion,
    ) -> (String, Event) {
        let result = beekeeper_core::ci_result::CiResult {
            schema: beekeeper_core::ci_result::CI_RESULT_SCHEMA.to_owned(),
            identity: identity.clone(),
            conclusion,
            evidence_url: Some("https://ci.example/runs/42".into()),
            summary: Some("terminal result".into()),
        };
        let correlation = beekeeper_core::ci_result::correlation_id(identity).expect("correlation");
        let (raw_tags, content) =
            beekeeper_core::ci_result::build_ci_result(&result).expect("valid CI result");
        let tags = raw_tags
            .into_iter()
            .map(|tag| Tag::parse(tag).expect("CI result tag"))
            .collect::<Vec<_>>();
        let event = EventBuilder::new(Kind::Custom(KIND_CI_RESULT as u16), content)
            .tags(tags)
            .sign_with_keys(keys)
            .expect("sign CI result");
        (correlation, event)
    }

    fn identity() -> beekeeper_core::ci_result::CiResultIdentity {
        beekeeper_core::ci_result::CiResultIdentity {
            project: format!("30621:{}:agiterra", "a".repeat(64)),
            repository: format!("30617:{}:beekeeper", "b".repeat(64)),
            commit: "c".repeat(40),
            check: "relay-ci".into(),
            run: "42".into(),
            attempt: 1,
            workflow: Uuid::new_v4().to_string(),
            phase: beekeeper_core::ci_result::CiPhase::Build,
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires Postgres"]
    async fn concurrent_exact_retries_insert_one_fact() {
        const CALLS: usize = 6;
        let pool = setup_pool().await;
        let community = make_test_community(&pool).await;
        let (correlation, event) = make_event(
            &Keys::generate(),
            &identity(),
            beekeeper_core::ci_result::CiConclusion::Success,
        );
        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(CALLS));
        let mut calls = Vec::new();
        for _ in 0..CALLS {
            let pool = pool.clone();
            let event = event.clone();
            let correlation = correlation.clone();
            let barrier = barrier.clone();
            calls.push(tokio::spawn(async move {
                barrier.wait().await;
                insert_ci_result_event(&pool, community, &event, &correlation)
                    .await
                    .expect("atomic CI insert")
            }));
        }
        let mut inserted = 0;
        let mut duplicate = 0;
        for call in calls {
            match call.await.expect("join CI insert") {
                CiResultInsertOutcome::Inserted(_) => inserted += 1,
                CiResultInsertOutcome::Duplicate(_) => duplicate += 1,
                CiResultInsertOutcome::Conflict(_) => panic!("exact retry cannot conflict"),
            }
        }
        assert_eq!(inserted, 1);
        assert_eq!(duplicate, CALLS - 1);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "requires Postgres"]
    async fn concurrent_duplicates_and_conflicts_preserve_first_fact() {
        const EACH: usize = 3;
        let pool = setup_pool().await;
        let community = make_test_community(&pool).await;
        let keys = Keys::generate();
        let identity = identity();
        let (correlation, accepted) = make_event(
            &keys,
            &identity,
            beekeeper_core::ci_result::CiConclusion::Success,
        );
        let (_, conflicting) = make_event(
            &keys,
            &identity,
            beekeeper_core::ci_result::CiConclusion::Failure,
        );
        assert!(matches!(
            insert_ci_result_event(&pool, community, &accepted, &correlation)
                .await
                .expect("seed accepted result"),
            CiResultInsertOutcome::Inserted(_)
        ));

        let barrier = std::sync::Arc::new(tokio::sync::Barrier::new(EACH * 2));
        let mut calls = Vec::new();
        for event in std::iter::repeat_n(accepted.clone(), EACH)
            .chain(std::iter::repeat_n(conflicting.clone(), EACH))
        {
            let pool = pool.clone();
            let correlation = correlation.clone();
            let barrier = barrier.clone();
            calls.push(tokio::spawn(async move {
                barrier.wait().await;
                insert_ci_result_event(&pool, community, &event, &correlation)
                    .await
                    .expect("atomic CI retry")
            }));
        }
        let mut duplicates = 0;
        let mut conflicts = 0;
        for call in calls {
            match call.await.expect("join CI retry") {
                CiResultInsertOutcome::Duplicate(stored) => {
                    duplicates += 1;
                    assert_eq!(stored.event.content, accepted.content);
                }
                CiResultInsertOutcome::Conflict(stored) => {
                    conflicts += 1;
                    assert_eq!(stored.event.content, accepted.content);
                }
                CiResultInsertOutcome::Inserted(_) => panic!("accepted fact must remain unique"),
            }
        }
        assert_eq!(duplicates, EACH);
        assert_eq!(conflicts, EACH);
    }

    #[tokio::test]
    #[ignore = "requires Postgres"]
    async fn correlation_is_community_scoped_and_foreign_signer_conflicts() {
        let pool = setup_pool().await;
        let community_a = make_test_community(&pool).await;
        let community_b = make_test_community(&pool).await;
        let identity = identity();
        let (correlation, relay_event) = make_event(
            &Keys::generate(),
            &identity,
            beekeeper_core::ci_result::CiConclusion::Success,
        );
        assert!(matches!(
            insert_ci_result_event(&pool, community_a, &relay_event, &correlation)
                .await
                .expect("community A insert"),
            CiResultInsertOutcome::Inserted(_)
        ));
        assert!(matches!(
            insert_ci_result_event(&pool, community_b, &relay_event, &correlation)
                .await
                .expect("community B insert"),
            CiResultInsertOutcome::Inserted(_)
        ));

        let community_c = make_test_community(&pool).await;
        let (_, foreign_event) = make_event(
            &Keys::generate(),
            &identity,
            beekeeper_core::ci_result::CiConclusion::Success,
        );
        assert!(matches!(
            insert_ci_result_event(&pool, community_c, &foreign_event, &correlation)
                .await
                .expect("foreign stored result"),
            CiResultInsertOutcome::Inserted(_)
        ));
        assert!(matches!(
            insert_ci_result_event(&pool, community_c, &relay_event, &correlation)
                .await
                .expect("relay retry against foreign signer"),
            CiResultInsertOutcome::Conflict(_)
        ));
    }
}
