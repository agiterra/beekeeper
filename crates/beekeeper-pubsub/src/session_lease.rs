//! Atomic Redis register for provider-signed coding-session leases.

use beekeeper_core::coding_session_lease::{
    CodingSessionLeaseState, CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS,
    CODING_SESSION_LEASE_REPLAY_WINDOW_SECS,
};
use beekeeper_core::tenant::TenantContext;
use deadpool_redis::Pool;
use redis::Script;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::PubSubError;
use crate::topic::BEEKEEPER_PREFIX;

/// Relay-derived lease duration, measured from Redis acceptance time.
pub const SESSION_LEASE_TTL_SECS: u64 = 180;
/// Hidden Redis retention for the monotonic sequence register.
///
/// Public snapshot visibility still ends at [`SESSION_LEASE_TTL_SECS`]. The
/// value remains longer only to fence a future-dated event until it can no
/// longer pass first-acceptance replay validation.
pub const SESSION_LEASE_REGISTER_RETENTION_SECS: u64 = {
    let replay_fence =
        CODING_SESSION_LEASE_REPLAY_WINDOW_SECS + CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS + 1;
    if replay_fence > SESSION_LEASE_TTL_SECS {
        replay_fence
    } else {
        SESSION_LEASE_TTL_SECS
    }
};

const APPLY_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[1])
if current then
  local decoded = cjson.decode(current)
  local current_sequence = tonumber(decoded.sequence)
  local incoming_sequence = tonumber(ARGV[1])
  if current_sequence > incoming_sequence then
    return {'stale', current}
  end
  if current_sequence == incoming_sequence then
    if decoded.eventId == ARGV[2] then
      return {'duplicate', current}
    end
    return {'conflict', current}
  end
end
local redis_time = redis.call('TIME')
local accepted_at = tonumber(redis_time[1])
local expires_at = accepted_at + tonumber(ARGV[3])
local incoming = cjson.decode(ARGV[5])
incoming.acceptedAt = accepted_at
incoming.expiresAt = expires_at
local encoded = cjson.encode(incoming)
redis.call('SET', KEYS[1], encoded, 'EX', ARGV[4])
redis.call('ZADD', KEYS[2], expires_at, KEYS[1])
redis.call('EXPIRE', KEYS[2], tonumber(ARGV[3]) * 2)
return {'applied', encoded}
"#;

/// Immutable lifecycle proof identifiers retained with a lease register value.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLeaseProof {
    /// Lifecycle command event ID.
    pub authority_command_event_id: String,
    /// Successful lifecycle receipt event ID.
    pub authority_receipt_event_id: String,
}

/// One current Redis register value, including the original signed event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLeaseRecord {
    /// Canonical exact-generation target key.
    pub target_key: String,
    /// Channel owning the lease.
    pub channel_id: Uuid,
    /// Monotonic provider sequence.
    #[serde(with = "decimal_u64")]
    pub sequence: u64,
    /// Provider-asserted live/released state.
    pub state: CodingSessionLeaseState,
    /// Original signed event ID.
    pub event_id: String,
    /// Original full provider-signed event JSON.
    pub event_json: String,
    /// Redis server acceptance time in Unix seconds.
    pub accepted_at: u64,
    /// Redis-derived expiry in Unix seconds.
    pub expires_at: u64,
    /// Immutable lifecycle proof identifiers.
    #[serde(flatten)]
    pub proof: SessionLeaseProof,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingSessionLeaseRecord<'a> {
    target_key: &'a str,
    channel_id: Uuid,
    #[serde(with = "decimal_u64")]
    sequence: u64,
    state: CodingSessionLeaseState,
    event_id: String,
    event_json: String,
    accepted_at: u64,
    expires_at: u64,
    #[serde(flatten)]
    proof: &'a SessionLeaseProof,
}

mod decimal_u64 {
    use serde::{de::Error as _, Deserialize, Deserializer, Serializer};

    pub(super) fn serialize<S>(value: &u64, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&value.to_string())
    }

    pub(super) fn deserialize<'de, D>(deserializer: D) -> Result<u64, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        let parsed = value
            .parse::<u64>()
            .map_err(|_| D::Error::custom("session lease sequence must be decimal text"))?;
        if parsed.to_string() != value {
            return Err(D::Error::custom(
                "session lease sequence must be canonical decimal text",
            ));
        }
        Ok(parsed)
    }
}

/// Atomic register result. Only [`LeaseApplyOutcome::Applied`] should fan out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeaseApplyOutcome {
    /// A higher sequence (or first value) was stored with a fresh TTL.
    Applied(SessionLeaseRecord),
    /// Exact event replay; accepted idempotently without TTL refresh.
    Duplicate(SessionLeaseRecord),
    /// Same sequence with a different event ID.
    Conflict(SessionLeaseRecord),
    /// Lower sequence than the retained live or release tombstone.
    Stale(SessionLeaseRecord),
}

/// Community/channel/exact-target scoped Redis register key.
pub fn session_lease_key(ctx: &TenantContext, channel_id: Uuid, target_key: &str) -> String {
    format!(
        "{BEEKEEPER_PREFIX}:{}:session-lease:{channel_id}:{target_key}",
        ctx.community()
    )
}

/// Community/channel scoped expiry-index key.
pub fn session_lease_index_key(ctx: &TenantContext, channel_id: Uuid) -> String {
    format!(
        "{BEEKEEPER_PREFIX}:{}:session-lease-index:{channel_id}",
        ctx.community()
    )
}

/// Atomically apply one lease using Redis `TIME` and monotonic sequence rules.
// Keep the signed event, relay scope, monotonic register key, and authority
// proof explicit at this trust boundary; a bag struct would make it easier to
// construct a partially validated register request.
#[allow(clippy::too_many_arguments)]
pub async fn apply_session_lease(
    pool: &Pool,
    ctx: &TenantContext,
    channel_id: Uuid,
    target_key: &str,
    event: &nostr::Event,
    sequence: u64,
    state: CodingSessionLeaseState,
    proof: &SessionLeaseProof,
) -> Result<LeaseApplyOutcome, PubSubError> {
    let pending = PendingSessionLeaseRecord {
        target_key,
        channel_id,
        sequence,
        state,
        event_id: event.id.to_hex(),
        event_json: serde_json::to_string(event)?,
        accepted_at: 0,
        expires_at: 0,
        proof,
    };
    let pending_json = serde_json::to_string(&pending)?;
    let mut conn = pool.get().await?;
    let (status, record_json): (String, String) = Script::new(APPLY_SCRIPT)
        .key(session_lease_key(ctx, channel_id, target_key))
        .key(session_lease_index_key(ctx, channel_id))
        .arg(sequence)
        .arg(event.id.to_hex())
        .arg(SESSION_LEASE_TTL_SECS)
        .arg(SESSION_LEASE_REGISTER_RETENTION_SECS)
        .arg(pending_json)
        .invoke_async(&mut *conn)
        .await?;
    let record: SessionLeaseRecord = serde_json::from_str(&record_json)?;
    match status.as_str() {
        "applied" => Ok(LeaseApplyOutcome::Applied(record)),
        "duplicate" => Ok(LeaseApplyOutcome::Duplicate(record)),
        "conflict" => Ok(LeaseApplyOutcome::Conflict(record)),
        "stale" => Ok(LeaseApplyOutcome::Stale(record)),
        _ => Err(PubSubError::Redis(redis::RedisError::from((
            redis::ErrorKind::UnexpectedReturnType,
            "unknown session lease script result",
        )))),
    }
}

/// Read the unexpired current register values for one explicit channel.
pub async fn session_lease_snapshot(
    pool: &Pool,
    ctx: &TenantContext,
    channel_id: Uuid,
) -> Result<Vec<SessionLeaseRecord>, PubSubError> {
    let mut conn = pool.get().await?;
    let index = session_lease_index_key(ctx, channel_id);
    let (seconds, _micros): (u64, u64) = redis::cmd("TIME").query_async(&mut conn).await?;
    redis::cmd("ZREMRANGEBYSCORE")
        .arg(&index)
        .arg("-inf")
        .arg(seconds)
        .query_async::<()>(&mut conn)
        .await?;
    let keys: Vec<String> = redis::cmd("ZRANGE")
        .arg(&index)
        .arg(0)
        .arg(-1)
        .query_async(&mut conn)
        .await?;
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let values: Vec<Option<String>> = redis::cmd("MGET").arg(&keys).query_async(&mut conn).await?;
    let mut records = Vec::with_capacity(values.len());
    let mut missing_keys = Vec::new();
    for (key, value) in keys.into_iter().zip(values) {
        match value {
            Some(value) => records.push(serde_json::from_str(&value)?),
            None => missing_keys.push(key),
        }
    }
    if !missing_keys.is_empty() {
        redis::cmd("ZREM")
            .arg(&index)
            .arg(missing_keys)
            .query_async::<()>(&mut conn)
            .await?;
    }
    Ok(records)
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core::{CommunityId, TenantContext};
    use nostr::{EventBuilder, Keys, Kind, Timestamp};

    fn ctx(id: u128) -> TenantContext {
        TenantContext::resolved(CommunityId::from_uuid(Uuid::from_u128(id)), "relay.test")
    }

    #[test]
    fn keys_are_community_channel_and_target_scoped() {
        let channel = Uuid::new_v4();
        assert_ne!(
            session_lease_key(&ctx(1), channel, "target"),
            session_lease_key(&ctx(2), channel, "target")
        );
        assert_ne!(
            session_lease_key(&ctx(1), channel, "target-a"),
            session_lease_key(&ctx(1), channel, "target-b")
        );
        assert!(session_lease_index_key(&ctx(1), channel).contains(&channel.to_string()));
    }

    #[test]
    fn ttl_is_three_heartbeat_intervals() {
        assert_eq!(SESSION_LEASE_TTL_SECS, 180);
        assert_eq!(SESSION_LEASE_TTL_SECS, 3 * 60);
    }

    #[test]
    fn hidden_register_retention_outlives_the_last_replayable_future_timestamp() {
        let replay_fence = std::hint::black_box(CODING_SESSION_LEASE_REPLAY_WINDOW_SECS)
            + CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS
            + 1;
        let retention = std::hint::black_box(SESSION_LEASE_REGISTER_RETENTION_SECS);
        assert!(retention >= replay_fence);
        assert!(retention > SESSION_LEASE_TTL_SECS);
    }

    #[test]
    fn register_json_preserves_the_javascript_safe_sequence_ceiling_as_decimal_text() {
        let record = SessionLeaseRecord {
            target_key: "target".into(),
            channel_id: Uuid::nil(),
            sequence: beekeeper_core::coding_session_command::MAX_SAFE_GENERATION,
            state: CodingSessionLeaseState::Live,
            event_id: "11".repeat(32),
            event_json: "{}".into(),
            accepted_at: 1,
            expires_at: 181,
            proof: SessionLeaseProof {
                authority_command_event_id: "22".repeat(32),
                authority_receipt_event_id: "33".repeat(32),
            },
        };

        let encoded = serde_json::to_value(&record).expect("serialize register");
        assert_eq!(encoded["sequence"], "9007199254740991");
        assert_eq!(
            serde_json::from_value::<SessionLeaseRecord>(encoded)
                .expect("deserialize register")
                .sequence,
            beekeeper_core::coding_session_command::MAX_SAFE_GENERATION
        );
    }

    #[tokio::test]
    #[ignore = "requires Redis"]
    async fn atomic_register_enforces_sequence_tombstone_and_duplicate_no_refresh() {
        let pool = crate::test_util::make_test_pool();
        let context = ctx(Uuid::new_v4().as_u128());
        let other_context = ctx(Uuid::new_v4().as_u128());
        let channel = Uuid::new_v4();
        let other_channel = Uuid::new_v4();
        let target = "codex-acp:instance-1:session-1:1";
        let keys = Keys::generate();
        let proof = SessionLeaseProof {
            authority_command_event_id: "11".repeat(32),
            authority_receipt_event_id: "22".repeat(32),
        };
        let mut conn = pool.get().await.unwrap();
        let (redis_now, _micros): (u64, u64) =
            redis::cmd("TIME").query_async(&mut conn).await.unwrap();
        drop(conn);
        let live = EventBuilder::new(Kind::Custom(24_223), "live-1")
            .tags([])
            .custom_created_at(Timestamp::from_secs(
                redis_now + CODING_SESSION_LEASE_MAX_FUTURE_SKEW_SECS,
            ))
            .sign_with_keys(&keys)
            .unwrap();
        let live_three = EventBuilder::new(Kind::Custom(24_223), "live-3")
            .tags([])
            .sign_with_keys(&keys)
            .unwrap();
        let conflict = EventBuilder::new(Kind::Custom(24_223), "conflict")
            .tags([])
            .sign_with_keys(&keys)
            .unwrap();
        let release = EventBuilder::new(Kind::Custom(24_223), "release")
            .tags([])
            .sign_with_keys(&keys)
            .unwrap();

        let first = apply_session_lease(
            &pool,
            &context,
            channel,
            target,
            &live,
            1,
            CodingSessionLeaseState::Live,
            &proof,
        )
        .await
        .unwrap();
        let LeaseApplyOutcome::Applied(first) = first else {
            panic!("first sequence must apply")
        };
        assert_eq!(first.proof, proof);
        assert_eq!(
            serde_json::from_str::<nostr::Event>(&first.event_json).unwrap(),
            live
        );

        let duplicate = apply_session_lease(
            &pool,
            &context,
            channel,
            target,
            &live,
            1,
            CodingSessionLeaseState::Live,
            &proof,
        )
        .await
        .unwrap();
        let LeaseApplyOutcome::Duplicate(duplicate) = duplicate else {
            panic!("exact replay must be idempotent")
        };
        assert_eq!(duplicate.accepted_at, first.accepted_at);
        assert_eq!(duplicate.expires_at, first.expires_at);

        assert!(matches!(
            apply_session_lease(
                &pool,
                &context,
                channel,
                target,
                &conflict,
                1,
                CodingSessionLeaseState::Live,
                &proof,
            )
            .await
            .unwrap(),
            LeaseApplyOutcome::Conflict(_)
        ));
        let released = apply_session_lease(
            &pool,
            &context,
            channel,
            target,
            &release,
            2,
            CodingSessionLeaseState::Released,
            &proof,
        )
        .await
        .unwrap();
        let LeaseApplyOutcome::Applied(released) = released else {
            panic!("higher release must replace the live lease")
        };
        assert_eq!(
            released.expires_at - released.accepted_at,
            SESSION_LEASE_TTL_SECS
        );
        assert!(matches!(
            apply_session_lease(
                &pool,
                &context,
                channel,
                target,
                &live,
                1,
                CodingSessionLeaseState::Live,
                &proof,
            )
            .await
            .unwrap(),
            LeaseApplyOutcome::Stale(_)
        ));

        let snapshot = session_lease_snapshot(&pool, &context, channel)
            .await
            .unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].state, CodingSessionLeaseState::Released);
        assert!(session_lease_snapshot(&pool, &context, other_channel)
            .await
            .unwrap()
            .is_empty());
        assert!(session_lease_snapshot(&pool, &other_context, channel)
            .await
            .unwrap()
            .is_empty());

        let mut conn = pool.get().await.unwrap();
        let register_key = session_lease_key(&context, channel, target);
        let index_key = session_lease_index_key(&context, channel);
        let hidden_ttl: i64 = redis::cmd("TTL")
            .arg(&register_key)
            .query_async(&mut conn)
            .await
            .unwrap();
        assert!(hidden_ttl > SESSION_LEASE_TTL_SECS as i64);
        assert!(hidden_ttl >= SESSION_LEASE_REGISTER_RETENTION_SECS as i64 - 1);
        let public_expiry: Option<f64> = redis::cmd("ZSCORE")
            .arg(&index_key)
            .arg(&register_key)
            .query_async(&mut conn)
            .await
            .unwrap();
        assert_eq!(public_expiry, Some(released.expires_at as f64));

        // Advance only the public visibility boundary without sleeping three
        // minutes. The retained value models the interval after expiresAt but
        // before the last future-dated event leaves the replay window.
        redis::cmd("ZADD")
            .arg(&index_key)
            .arg(0)
            .arg(&register_key)
            .query_async::<()>(&mut conn)
            .await
            .unwrap();
        drop(conn);
        assert!(session_lease_snapshot(&pool, &context, channel)
            .await
            .unwrap()
            .is_empty());

        assert!(matches!(
            apply_session_lease(
                &pool,
                &context,
                channel,
                target,
                &live,
                1,
                CodingSessionLeaseState::Live,
                &proof,
            )
            .await
            .unwrap(),
            LeaseApplyOutcome::Stale(_)
        ));
        assert!(session_lease_snapshot(&pool, &context, channel)
            .await
            .unwrap()
            .is_empty());

        let duplicate_release = apply_session_lease(
            &pool,
            &context,
            channel,
            target,
            &release,
            2,
            CodingSessionLeaseState::Released,
            &proof,
        )
        .await
        .unwrap();
        let LeaseApplyOutcome::Duplicate(duplicate_release) = duplicate_release else {
            panic!("release replay must be idempotent")
        };
        assert_eq!(duplicate_release.accepted_at, released.accepted_at);
        assert_eq!(duplicate_release.expires_at, released.expires_at);
        assert!(session_lease_snapshot(&pool, &context, channel)
            .await
            .unwrap()
            .is_empty());

        assert!(matches!(
            apply_session_lease(
                &pool,
                &context,
                channel,
                target,
                &live_three,
                3,
                CodingSessionLeaseState::Live,
                &proof,
            )
            .await
            .unwrap(),
            LeaseApplyOutcome::Applied(_)
        ));
        let snapshot = session_lease_snapshot(&pool, &context, channel)
            .await
            .unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].sequence, 3);
        assert_eq!(snapshot[0].state, CodingSessionLeaseState::Live);

        let ceiling_target = "codex-acp:instance-1:session-ceiling:1";
        let below_ceiling = EventBuilder::new(Kind::Custom(24_223), "below-ceiling")
            .tags([])
            .sign_with_keys(&keys)
            .unwrap();
        let at_ceiling = EventBuilder::new(Kind::Custom(24_223), "at-ceiling")
            .tags([])
            .sign_with_keys(&keys)
            .unwrap();
        let max = beekeeper_core::coding_session_command::MAX_SAFE_GENERATION;
        assert!(matches!(
            apply_session_lease(
                &pool,
                &context,
                channel,
                ceiling_target,
                &below_ceiling,
                max - 1,
                CodingSessionLeaseState::Live,
                &proof,
            )
            .await
            .unwrap(),
            LeaseApplyOutcome::Applied(record) if record.sequence == max - 1
        ));
        assert!(matches!(
            apply_session_lease(
                &pool,
                &context,
                channel,
                ceiling_target,
                &at_ceiling,
                max,
                CodingSessionLeaseState::Live,
                &proof,
            )
            .await
            .unwrap(),
            LeaseApplyOutcome::Applied(record) if record.sequence == max
        ));
        assert!(matches!(
            apply_session_lease(
                &pool,
                &context,
                channel,
                ceiling_target,
                &at_ceiling,
                max,
                CodingSessionLeaseState::Live,
                &proof,
            )
            .await
            .unwrap(),
            LeaseApplyOutcome::Duplicate(record) if record.sequence == max
        ));

        let mut conn = pool.get().await.unwrap();
        redis::cmd("DEL")
            .arg(&register_key)
            .query_async::<()>(&mut conn)
            .await
            .unwrap();
        drop(conn);
        let snapshot = session_lease_snapshot(&pool, &context, channel)
            .await
            .unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].event_id, at_ceiling.id.to_hex());
        assert_eq!(snapshot[0].sequence, max);
        let mut conn = pool.get().await.unwrap();
        let stale_index_score: Option<f64> = redis::cmd("ZSCORE")
            .arg(&index_key)
            .arg(&register_key)
            .query_async(&mut conn)
            .await
            .unwrap();
        assert_eq!(stale_index_score, None);
        redis::cmd("DEL")
            .arg(&index_key)
            .query_async::<()>(&mut conn)
            .await
            .unwrap();
    }
}
