//! Scratch-Postgres publication tests; never mutate the configured database.

use super::*;
use buzz_core::project_pack_source::{
    build_conditional_project_pack_source, build_project_pack_source, PackPin,
};
use nostr::{EventBuilder, Keys, Kind, Tag, Timestamp};
use std::time::Duration;

struct Fixture {
    admin: PgPool,
    pool: PgPool,
    name: String,
    community: CommunityId,
    project: String,
}

impl Fixture {
    async fn new() -> Self {
        let url = std::env::var("TEST_DATABASE_URL")
            .unwrap_or_else(|_| "postgres://buzz:buzz_dev@localhost:5432/buzz".into());
        let admin = PgPool::connect(&url).await.expect("connect admin");
        let name = format!("pack_source_cas_{}", Uuid::new_v4().simple());
        sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {name}")))
            .execute(&admin)
            .await
            .expect("create scratch database");
        let prefix = url.rsplit_once('/').expect("database URL path").0;
        let pool = PgPool::connect(&format!("{prefix}/{name}"))
            .await
            .expect("connect scratch database");
        crate::migration::run_migrations(&pool)
            .await
            .expect("migrate scratch database");
        let community = Self::community(&pool).await;
        let project = format!(
            "30621:{}:CaseSensitive",
            Keys::generate().public_key().to_hex()
        );
        Self {
            admin,
            pool,
            name,
            community,
            project,
        }
    }

    async fn community(pool: &PgPool) -> CommunityId {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO communities (id, host) VALUES ($1, $2)")
            .bind(id)
            .bind(format!("pack-source-{}.example", id.simple()))
            .execute(pool)
            .await
            .expect("create community");
        CommunityId::from_uuid(id)
    }

    async fn close(self) {
        self.pool.close().await;
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "DROP DATABASE {} WITH (FORCE)",
            self.name
        )))
        .execute(&self.admin)
        .await
        .expect("drop scratch database");
        self.admin.close().await;
    }

    fn source(&self, keys: &Keys, timestamp: u64, expectation: PackSourceExpectation) -> Event {
        source(&self.project, keys, timestamp, expectation)
    }

    async fn write(&self, event: &Event) -> Result<(StoredEvent, bool)> {
        crate::Db::from_pool(self.pool.clone())
            .replace_parameterized_event(
                self.community,
                event,
                &crate::event::extract_d_tag(event).expect("d tag"),
                None,
            )
            .await
    }

    async fn head(&self) -> Option<Vec<u8>> {
        sqlx::query_scalar(
            "SELECT id FROM events WHERE community_id=$1 AND kind=30624 \
            AND d_tag=$2 AND deleted_at IS NULL ORDER BY created_at DESC,id ASC LIMIT 1",
        )
        .bind(self.community.as_uuid())
        .bind(&self.project)
        .fetch_optional(&self.pool)
        .await
        .expect("read effective head")
    }
}

fn source(project: &str, keys: &Keys, timestamp: u64, expectation: PackSourceExpectation) -> Event {
    let repo = format!("30617:{}:packs", keys.public_key().to_hex());
    let pin = PackPin::Sha("a".repeat(40));
    let draft = match expectation {
        PackSourceExpectation::Unconditional => {
            build_project_pack_source(project, &repo, &pin, None, None)
        }
        PackSourceExpectation::Expected(expected) => build_conditional_project_pack_source(
            project,
            &repo,
            &pin,
            None,
            None,
            expected.as_deref(),
        ),
    }
    .expect("source draft");
    EventBuilder::new(Kind::Custom(KIND_PROJECT_PACK_SOURCE as u16), draft.content)
        .tags(
            draft
                .tags
                .into_iter()
                .map(|tag| Tag::parse(tag).expect("source tag")),
        )
        .custom_created_at(Timestamp::from(timestamp))
        .sign_with_keys(keys)
        .expect("sign source")
}

fn expected(event: &Event) -> PackSourceExpectation {
    PackSourceExpectation::Expected(Some(event.id.to_hex()))
}

#[tokio::test]
#[ignore = "requires Postgres with CREATE DATABASE"]
async fn pack_source_cas_competing_authors_replay_and_legacy() {
    let fixture = Fixture::new().await;
    let a = Keys::generate();
    let b = Keys::generate();
    let now = Timestamp::now().as_secs();
    let initial = fixture.source(&a, now, PackSourceExpectation::Expected(None));
    assert!(fixture.write(&initial).await.expect("initial CAS").1);
    let left = fixture.source(&a, now + 1, expected(&initial));
    let right = fixture.source(&b, now + 2, expected(&initial));
    let (left_result, right_result) = tokio::join!(fixture.write(&left), fixture.write(&right));
    let winner = match (left_result, right_result) {
        (Ok((_, true)), Err(DbError::PackSourceConflict(_))) => &left,
        (Err(DbError::PackSourceConflict(_)), Ok((_, true))) => &right,
        result => panic!("exactly one cross-author CAS must win: {result:?}"),
    };
    assert_eq!(
        fixture.head().await.as_deref(),
        Some(winner.id.as_bytes().as_slice())
    );
    assert!(!fixture.write(winner).await.expect("exact winner retry").1);
    assert!(!fixture.write(&initial).await.expect("superseded retry").1);
    assert_eq!(
        fixture.head().await.as_deref(),
        Some(winner.id.as_bytes().as_slice())
    );

    // The same signed initial bytes belong to a different community there.
    let other = Fixture::community(&fixture.pool).await;
    assert!(
        crate::event::insert_event(&fixture.pool, other, &initial, None)
            .await
            .expect("isolated initial CAS")
            .1
    );
    let stale_initial = fixture.source(&b, now + 3, PackSourceExpectation::Expected(None));
    assert!(matches!(
        fixture.write(&stale_initial).await,
        Err(DbError::PackSourceConflict(_))
    ));
    let legacy = fixture.source(&b, now + 4, PackSourceExpectation::Unconditional);
    assert!(
        crate::event::insert_event_with_thread_metadata(
            &fixture.pool,
            fixture.community,
            &legacy,
            None,
            None,
        )
        .await
        .expect("legacy low-level route")
        .1
    );
    let stale_cas = fixture.source(&a, now + 5, expected(winner));
    assert!(matches!(
        fixture.write(&stale_cas).await,
        Err(DbError::PackSourceConflict(_))
    ));
    let stale_legacy = fixture.source(&b, now, PackSourceExpectation::Unconditional);
    assert!(!fixture.write(&stale_legacy).await.expect("legacy LWW").1);
    assert_eq!(
        fixture.head().await.as_deref(),
        Some(legacy.id.as_bytes().as_slice())
    );

    // Generic transaction insert cannot silently bypass source comparison.
    let mut tx = fixture.pool.begin().await.expect("begin bypass test");
    assert!(matches!(
        crate::event::insert_event_with_thread_metadata_tx(
            &mut tx,
            fixture.community,
            &stale_cas,
            None,
            None,
        )
        .await,
        Err(DbError::InvalidData(_))
    ));
    tx.rollback().await.expect("rollback bypass test");
    fixture.close().await;
}

#[tokio::test]
#[ignore = "requires Postgres with CREATE DATABASE"]
async fn pack_source_cas_ordering_rollback_and_deletion_serialization() {
    let fixture = Fixture::new().await;
    let a = Keys::generate();
    let b = Keys::generate();
    let now = Timestamp::now().as_secs();
    let initial = fixture.source(&a, now, PackSourceExpectation::Unconditional);
    fixture.write(&initial).await.expect("initial");
    let same_second = fixture.source(&b, now, expected(&initial));
    let same_result = fixture.write(&same_second).await;
    if same_second.id < initial.id {
        assert!(same_result.expect("lower ID outranks").1);
    } else {
        assert!(matches!(same_result, Err(DbError::PackSourceConflict(_))));
    }
    let head = if same_second.id < initial.id {
        &same_second
    } else {
        &initial
    };
    let older = fixture.source(&b, now - 1, expected(head));
    assert!(matches!(
        fixture.write(&older).await,
        Err(DbError::PackSourceConflict(_))
    ));

    // Fail the INSERT after predecessor retirement: the head must survive.
    sqlx::query(
        "CREATE FUNCTION reject_pack_source_test() RETURNS trigger LANGUAGE plpgsql AS $$ \
        BEGIN IF NEW.kind=30624 THEN RAISE EXCEPTION 'injected source insert failure'; END IF; \
        RETURN NEW; END $$",
    )
    .execute(&fixture.pool)
    .await
    .expect("failure function");
    sqlx::query(
        "CREATE TRIGGER reject_pack_source_test BEFORE INSERT ON events \
        FOR EACH ROW EXECUTE FUNCTION reject_pack_source_test()",
    )
    .execute(&fixture.pool)
    .await
    .expect("failure trigger");
    let successor = fixture.source(
        if head.pubkey == a.public_key() {
            &a
        } else {
            &b
        },
        now + 1,
        expected(head),
    );
    assert!(fixture
        .write(&successor)
        .await
        .expect_err("injected rollback")
        .to_string()
        .contains("injected source insert failure"));
    assert_eq!(
        fixture.head().await.as_deref(),
        Some(head.id.as_bytes().as_slice())
    );
    sqlx::query("DROP TRIGGER reject_pack_source_test ON events")
        .execute(&fixture.pool)
        .await
        .expect("remove failure trigger");

    // Each source deletion API must wait on the same project lock. Polling
    // the operation under timeout proves it is pending, then release it.
    for mode in 0..3 {
        let event = fixture.source(&a, now + 10 + mode, PackSourceExpectation::Unconditional);
        fixture.write(&event).await.expect("source to delete");
        let mut tx = fixture.pool.begin().await.expect("hold project lock");
        lock_project(&mut tx, fixture.community, &fixture.project)
            .await
            .expect("project lock");
        let deletion = async {
            match mode {
                0 => {
                    crate::event::soft_delete_event(
                        &fixture.pool,
                        fixture.community,
                        event.id.as_bytes(),
                    )
                    .await
                }
                1 => {
                    crate::event::soft_delete_event_and_update_thread(
                        &fixture.pool,
                        fixture.community,
                        event.id.as_bytes(),
                        None,
                        None,
                    )
                    .await
                }
                _ => {
                    crate::event::soft_delete_by_coordinate(
                        &fixture.pool,
                        fixture.community,
                        KIND_PROJECT_PACK_SOURCE as i32,
                        &a.public_key().to_bytes(),
                        &fixture.project,
                        (now + 12) as i64,
                    )
                    .await
                }
            }
        };
        tokio::pin!(deletion);
        assert!(
            tokio::time::timeout(Duration::from_millis(75), &mut deletion)
                .await
                .is_err()
        );
        tx.commit().await.expect("release project lock");
        assert!(deletion.await.expect("serialized deletion"));
        assert!(!fixture.write(&event).await.expect("deleted exact retry").1);
        assert_ne!(
            fixture.head().await.as_deref(),
            Some(event.id.as_bytes().as_slice())
        );
        let stale = fixture.source(&a, now + 20 + mode, expected(&event));
        assert!(matches!(
            fixture.write(&stale).await,
            Err(DbError::PackSourceConflict(_))
        ));
    }
    fixture.close().await;
}

#[tokio::test]
#[ignore = "requires Postgres with CREATE DATABASE"]
async fn pack_source_cas_legacy_alias_lock_and_community_fence() {
    let fixture = Fixture::new().await;
    let keys = Keys::generate();
    let now = Timestamp::now().as_secs();
    let canonical = fixture.source(&keys, now, PackSourceExpectation::Unconditional);
    let alias_coordinate = fixture.project.replacen(
        &fixture.project[6..70],
        &fixture.project[6..70].to_ascii_uppercase(),
        1,
    );
    let mut tags = canonical.tags.to_vec();
    tags[0] = Tag::parse(["d", &alias_coordinate]).expect("legacy alias tag");
    let alias = EventBuilder::new(canonical.kind, canonical.content.clone())
        .tags(tags)
        .custom_created_at(Timestamp::from(now))
        .sign_with_keys(&keys)
        .expect("alias");
    {
        let mut tx = fixture.pool.begin().await.expect("hold canonical lock");
        lock_project(&mut tx, fixture.community, &fixture.project)
            .await
            .expect("canonical lock");
        let write = fixture.write(&alias);
        tokio::pin!(write);
        assert!(tokio::time::timeout(Duration::from_millis(75), &mut write)
            .await
            .is_err());
        tx.commit().await.expect("release canonical lock");
        assert!(write.await.expect("legacy alias write").1);
    }
    assert!(
        fixture.head().await.is_none(),
        "legacy alias remains invisible to canonical readers"
    );
    let initial = fixture.source(&keys, now + 1, PackSourceExpectation::Expected(None));
    assert!(fixture.write(&initial).await.expect("canonical initial").1);

    // Historical pre-gate rows can lack d entirely. Seed one through raw SQL
    // to model that history; the newly guarded insert APIs correctly refuse it.
    let malformed = EventBuilder::new(
        Kind::Custom(KIND_PROJECT_PACK_SOURCE as u16),
        "old malformed source",
    )
    .custom_created_at(Timestamp::from(now))
    .sign_with_keys(&keys)
    .expect("historical source");
    sqlx::query(
        "INSERT INTO events (community_id,id,pubkey,created_at,kind,tags,content,sig,received_at) \
        VALUES ($1,$2,$3,$4,30624,$5,$6,$7,NOW())",
    )
    .bind(fixture.community.as_uuid())
    .bind(malformed.id.as_bytes().as_slice())
    .bind(malformed.pubkey.to_bytes().as_slice())
    .bind(DateTime::from_timestamp(now as i64, 0).expect("timestamp"))
    .bind(serde_json::to_value(&malformed.tags).expect("tags"))
    .bind(&malformed.content)
    .bind(malformed.sig.serialize().as_slice())
    .execute(&fixture.pool)
    .await
    .expect("seed historical malformed source");
    assert!(crate::event::soft_delete_event(
        &fixture.pool,
        fixture.community,
        malformed.id.as_bytes()
    )
    .await
    .expect("historical malformed source remains deletable"));

    let mut tx = fixture
        .pool
        .begin()
        .await
        .expect("begin lifecycle transition");
    sqlx::query("SELECT pg_advisory_xact_lock(community_deletion_lock_key($1))")
        .bind(fixture.community.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("exclusive community lock");
    sqlx::query(
        "SELECT set_config('buzz.deletion_executor_community',$1,true), \
        set_config('buzz.deletion_fence_generation','0',true)",
    )
    .bind(fixture.community.to_string())
    .execute(&mut *tx)
    .await
    .expect("executor authorization");
    sqlx::query("UPDATE communities SET deletion_state='quiescing' WHERE id=$1")
        .bind(fixture.community.as_uuid())
        .execute(&mut *tx)
        .await
        .expect("fence community");
    tx.commit().await.expect("commit lifecycle fence");
    let next = fixture.source(&keys, now + 2, expected(&initial));
    assert!(
        fixture.write(&next).await.is_err(),
        "CAS cannot cross lifecycle fence"
    );
    assert_eq!(
        fixture.head().await.as_deref(),
        Some(initial.id.as_bytes().as_slice())
    );
    fixture.close().await;
}
