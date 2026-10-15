//! Atomic Redis register for provider-signed presence heartbeats (kind 24224).
//!
//! The same shape as [`crate::session_lease`]: the signed event is never
//! written to Postgres, a monotonic per-instance sequence refuses replays and
//! out-of-order heartbeats, and a cold `REQ` for kind 24224 is answered from
//! this register so a reader that was not subscribed when the heartbeat went
//! out can still tell an idle provider from a dead one (NIP-CSL § Provider
//! presence). Retention is the heartbeat's own `ttlSecs`, capped at
//! [`PROVIDER_PRESENCE_TTL_CAP_SECS`], measured from Redis acceptance time:
//! the relay never keeps a heartbeat visible longer than the provider said it
//! would count as live.
//!
//! **Instance cap (ledger 406 F4).** The relay admits 24224 from any channel
//! member, and readers verify the provider key and `instanceId` themselves,
//! so a stray heartbeat cannot make a session read `live`; what it could do
//! is fill the register. Each (channel, pubkey) may hold at most
//! [`PROVIDER_PRESENCE_INSTANCE_CAP`] live instance keys: a ninth distinct
//! `instanceId` is refused until one expires, while a heartbeat for an
//! instance already registered is never counted against the cap.

use beekeeper_core::tenant::TenantContext;
use deadpool_redis::Pool;
use redis::Script;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::PubSubError;
use crate::session_lease::decimal_u64;
use crate::topic::REDIS_KEY_PREFIX;

/// Longest the relay retains one heartbeat, whatever `ttlSecs` asked for.
///
/// A provider that heartbeats every `ttlSecs / 3` and asks for an hour would
/// otherwise keep a dead instance readable for an hour; ten minutes is the
/// relay's ceiling on how long absence can hide.
pub const PROVIDER_PRESENCE_TTL_CAP_SECS: u64 = 600;

/// Most live presence instances one pubkey may register in one channel.
pub const PROVIDER_PRESENCE_INSTANCE_CAP: u64 = 8;

const APPLY_SCRIPT: &str = r#"
local current = redis.call('GET', KEYS[1])
if current then
  local decoded = cjson.decode(current)
  local current_seq = tonumber(decoded.seq)
  local incoming_seq = tonumber(ARGV[1])
  if current_seq > incoming_seq then
    return {'stale', current}
  end
  if current_seq == incoming_seq then
    if decoded.eventId == ARGV[2] then
      return {'duplicate', current}
    end
    return {'conflict', current}
  end
end
local redis_time = redis.call('TIME')
local accepted_at = tonumber(redis_time[1])
local ttl = tonumber(ARGV[3])
local expires_at = accepted_at + ttl
if not current then
  redis.call('ZREMRANGEBYSCORE', KEYS[3], '-inf', accepted_at)
  local live = redis.call('ZCARD', KEYS[3])
  if live >= tonumber(ARGV[5]) then
    return {'capped', tostring(live)}
  end
end
local incoming = cjson.decode(ARGV[4])
incoming.acceptedAt = accepted_at
incoming.expiresAt = expires_at
local encoded = cjson.encode(incoming)
redis.call('SET', KEYS[1], encoded, 'EX', ttl)
redis.call('ZADD', KEYS[2], expires_at, KEYS[1])
redis.call('EXPIRE', KEYS[2], ttl * 2)
redis.call('ZADD', KEYS[3], expires_at, ARGV[6])
redis.call('EXPIRE', KEYS[3], ttl * 2)
return {'applied', encoded}
"#;

/// One current register value, including the original signed event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProviderPresenceRecord {
    /// Channel the heartbeat is scoped to (`h` tag).
    pub channel_id: Uuid,
    /// The provider key that signed the heartbeat, lowercase hex.
    pub provider_pubkey: String,
    /// The provider instance heartbeating (`instanceId` in content).
    pub instance_id: String,
    /// Monotonic per-process heartbeat counter.
    #[serde(with = "decimal_u64")]
    pub seq: u64,
    /// The `ttlSecs` the heartbeat claimed, before the relay's cap.
    pub ttl_secs: u64,
    /// Original signed event ID.
    pub event_id: String,
    /// Original full provider-signed event JSON.
    pub event_json: String,
    /// Redis server acceptance time in Unix seconds.
    pub accepted_at: u64,
    /// Redis-derived expiry in Unix seconds: `accepted_at` plus the capped TTL.
    pub expires_at: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct PendingProviderPresenceRecord<'a> {
    channel_id: Uuid,
    provider_pubkey: &'a str,
    instance_id: &'a str,
    #[serde(with = "decimal_u64")]
    seq: u64,
    ttl_secs: u64,
    event_id: String,
    event_json: String,
    accepted_at: u64,
    expires_at: u64,
}

/// Atomic register result. Only [`PresenceApplyOutcome::Applied`] should fan out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PresenceApplyOutcome {
    /// A higher sequence (or first value) was stored with a fresh TTL.
    Applied(ProviderPresenceRecord),
    /// Exact event replay; accepted idempotently without TTL refresh.
    Duplicate(ProviderPresenceRecord),
    /// Same sequence with a different event ID.
    Conflict(ProviderPresenceRecord),
    /// Lower sequence than the retained heartbeat.
    Stale(ProviderPresenceRecord),
    /// A new `instanceId` for a (channel, pubkey) that already holds
    /// [`PROVIDER_PRESENCE_INSTANCE_CAP`] live instances; nothing stored.
    Capped {
        /// Live instance keys the pubkey holds in the channel right now.
        live_instances: u64,
        /// The cap that refused it.
        cap: u64,
    },
}

/// The TTL the relay retains a heartbeat for: its own `ttl_secs`, capped.
pub const fn retained_ttl_secs(ttl_secs: u64) -> u64 {
    if ttl_secs > PROVIDER_PRESENCE_TTL_CAP_SECS {
        PROVIDER_PRESENCE_TTL_CAP_SECS
    } else {
        ttl_secs
    }
}

/// Community/channel/provider/instance scoped register key.
pub fn provider_presence_key(
    ctx: &TenantContext,
    channel_id: Uuid,
    provider_pubkey: &str,
    instance_id: &str,
) -> String {
    format!(
        "{REDIS_KEY_PREFIX}:{}:provider-presence:{channel_id}:{provider_pubkey}:{instance_id}",
        ctx.community()
    )
}

/// Community/channel/provider scoped set of live instance ids, scored by
/// expiry, that enforces [`PROVIDER_PRESENCE_INSTANCE_CAP`].
pub fn provider_presence_instances_key(
    ctx: &TenantContext,
    channel_id: Uuid,
    provider_pubkey: &str,
) -> String {
    format!(
        "{REDIS_KEY_PREFIX}:{}:provider-presence-instances:{channel_id}:{provider_pubkey}",
        ctx.community()
    )
}

/// Community/channel scoped expiry-index key.
pub fn provider_presence_index_key(ctx: &TenantContext, channel_id: Uuid) -> String {
    format!(
        "{REDIS_KEY_PREFIX}:{}:provider-presence-index:{channel_id}",
        ctx.community()
    )
}

/// Atomically apply one heartbeat using Redis `TIME`, monotonic `seq` rules
/// and the per-(channel, pubkey) instance cap.
// The signed event, relay scope, provider identity and the heartbeat's own
// claims stay explicit at this trust boundary, as they do for leases.
#[allow(clippy::too_many_arguments)]
pub async fn apply_provider_presence(
    pool: &Pool,
    ctx: &TenantContext,
    channel_id: Uuid,
    provider_pubkey: &str,
    instance_id: &str,
    event: &nostr::Event,
    seq: u64,
    ttl_secs: u64,
) -> Result<PresenceApplyOutcome, PubSubError> {
    let pending = PendingProviderPresenceRecord {
        channel_id,
        provider_pubkey,
        instance_id,
        seq,
        ttl_secs,
        event_id: event.id.to_hex(),
        event_json: serde_json::to_string(event)?,
        accepted_at: 0,
        expires_at: 0,
    };
    let pending_json = serde_json::to_string(&pending)?;
    let mut conn = pool.get().await?;
    let (status, detail): (String, String) = Script::new(APPLY_SCRIPT)
        .key(provider_presence_key(
            ctx,
            channel_id,
            provider_pubkey,
            instance_id,
        ))
        .key(provider_presence_index_key(ctx, channel_id))
        .key(provider_presence_instances_key(
            ctx,
            channel_id,
            provider_pubkey,
        ))
        .arg(seq)
        .arg(event.id.to_hex())
        .arg(retained_ttl_secs(ttl_secs).max(1))
        .arg(pending_json)
        .arg(PROVIDER_PRESENCE_INSTANCE_CAP)
        .arg(instance_id)
        .invoke_async(&mut *conn)
        .await?;
    if status == "capped" {
        let live_instances = detail.parse::<u64>().map_err(|_| {
            PubSubError::Redis(redis::RedisError::from((
                redis::ErrorKind::UnexpectedReturnType,
                "provider presence cap count is not a number",
            )))
        })?;
        return Ok(PresenceApplyOutcome::Capped {
            live_instances,
            cap: PROVIDER_PRESENCE_INSTANCE_CAP,
        });
    }
    let record: ProviderPresenceRecord = serde_json::from_str(&detail)?;
    match status.as_str() {
        "applied" => Ok(PresenceApplyOutcome::Applied(record)),
        "duplicate" => Ok(PresenceApplyOutcome::Duplicate(record)),
        "conflict" => Ok(PresenceApplyOutcome::Conflict(record)),
        "stale" => Ok(PresenceApplyOutcome::Stale(record)),
        _ => Err(PubSubError::Redis(redis::RedisError::from((
            redis::ErrorKind::UnexpectedReturnType,
            "unknown provider presence script result",
        )))),
    }
}

/// Read the unexpired current heartbeats for one explicit channel.
pub async fn provider_presence_snapshot(
    pool: &Pool,
    ctx: &TenantContext,
    channel_id: Uuid,
) -> Result<Vec<ProviderPresenceRecord>, PubSubError> {
    let mut conn = pool.get().await?;
    let index = provider_presence_index_key(ctx, channel_id);
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
    use nostr::{EventBuilder, Keys, Kind};

    fn ctx(id: u128) -> TenantContext {
        TenantContext::resolved(CommunityId::from_uuid(Uuid::from_u128(id)), "relay.test")
    }

    #[test]
    fn keys_are_community_channel_provider_and_instance_scoped() {
        let channel = Uuid::new_v4();
        assert_ne!(
            provider_presence_key(&ctx(1), channel, "aa", "primary"),
            provider_presence_key(&ctx(2), channel, "aa", "primary")
        );
        assert_ne!(
            provider_presence_key(&ctx(1), channel, "aa", "primary"),
            provider_presence_key(&ctx(1), channel, "bb", "primary")
        );
        assert_ne!(
            provider_presence_key(&ctx(1), channel, "aa", "primary"),
            provider_presence_key(&ctx(1), channel, "aa", "secondary")
        );
        assert!(provider_presence_index_key(&ctx(1), channel).contains(&channel.to_string()));
        assert_ne!(
            provider_presence_instances_key(&ctx(1), channel, "aa"),
            provider_presence_instances_key(&ctx(1), channel, "bb")
        );
        assert_ne!(
            provider_presence_instances_key(&ctx(1), channel, "aa"),
            provider_presence_instances_key(&ctx(1), Uuid::new_v4(), "aa")
        );
    }

    #[test]
    fn the_instance_cap_is_eight() {
        assert_eq!(PROVIDER_PRESENCE_INSTANCE_CAP, 8);
    }

    #[test]
    fn retention_is_the_heartbeat_ttl_capped_at_ten_minutes() {
        assert_eq!(retained_ttl_secs(90), 90);
        assert_eq!(retained_ttl_secs(600), 600);
        assert_eq!(retained_ttl_secs(3600), PROVIDER_PRESENCE_TTL_CAP_SECS);
        assert_eq!(PROVIDER_PRESENCE_TTL_CAP_SECS, 600);
    }

    #[tokio::test]
    #[ignore = "requires Redis"]
    async fn atomic_register_keeps_the_highest_seq_and_refuses_replays() {
        let pool = crate::test_util::make_test_pool();
        let context = ctx(Uuid::new_v4().as_u128());
        let other_context = ctx(Uuid::new_v4().as_u128());
        let channel = Uuid::new_v4();
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();
        let beat = |content: &str| {
            EventBuilder::new(Kind::Custom(24_224), content)
                .tags([])
                .sign_with_keys(&keys)
                .unwrap()
        };
        let one = beat("beat-1");
        let three = beat("beat-3");
        let conflict = beat("conflict");

        let first =
            apply_provider_presence(&pool, &context, channel, &pubkey, "primary", &one, 1, 90)
                .await
                .unwrap();
        let PresenceApplyOutcome::Applied(record) = first else {
            panic!("first heartbeat must apply: {first:?}");
        };
        assert_eq!(record.expires_at, record.accepted_at + 90);
        assert!(matches!(
            apply_provider_presence(&pool, &context, channel, &pubkey, "primary", &one, 1, 90)
                .await
                .unwrap(),
            PresenceApplyOutcome::Duplicate(_)
        ));
        assert!(matches!(
            apply_provider_presence(&pool, &context, channel, &pubkey, "primary", &conflict, 1, 90)
                .await
                .unwrap(),
            PresenceApplyOutcome::Conflict(_)
        ));
        let PresenceApplyOutcome::Applied(capped) = apply_provider_presence(
            &pool, &context, channel, &pubkey, "primary", &three, 3, 3600,
        )
        .await
        .unwrap() else {
            panic!("a higher seq must apply");
        };
        assert_eq!(
            capped.expires_at,
            capped.accepted_at + PROVIDER_PRESENCE_TTL_CAP_SECS,
            "retention is capped even when the heartbeat asked for an hour"
        );
        assert!(matches!(
            apply_provider_presence(&pool, &context, channel, &pubkey, "primary", &one, 2, 90)
                .await
                .unwrap(),
            PresenceApplyOutcome::Stale(_)
        ));
        let snapshot = provider_presence_snapshot(&pool, &context, channel)
            .await
            .unwrap();
        assert_eq!(snapshot.len(), 1);
        assert_eq!(snapshot[0].seq, 3);
        assert_eq!(snapshot[0].event_id, three.id.to_hex());
        assert!(provider_presence_snapshot(&pool, &other_context, channel)
            .await
            .unwrap()
            .is_empty());
    }

    /// F4: a ninth distinct instance for one (channel, pubkey) is refused
    /// without touching the register; refreshing a registered instance never
    /// counts against the cap; another pubkey and another channel have their
    /// own cap; an expired instance frees its slot.
    #[tokio::test]
    #[ignore = "requires Redis"]
    async fn a_pubkey_holds_at_most_eight_live_instances_per_channel() {
        let pool = crate::test_util::make_test_pool();
        let context = ctx(Uuid::new_v4().as_u128());
        let channel = Uuid::new_v4();
        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();
        let beat = |keys: &Keys, content: &str| {
            EventBuilder::new(Kind::Custom(24_224), content)
                .tags([])
                .sign_with_keys(keys)
                .unwrap()
        };
        let apply = |channel: Uuid,
                     pubkey: String,
                     instance: String,
                     content: String,
                     seq: u64,
                     ttl: u64| {
            let pool = pool.clone();
            let context = context.clone();
            let event = beat(&keys, &content);
            async move {
                apply_provider_presence(
                    &pool, &context, channel, &pubkey, &instance, &event, seq, ttl,
                )
                .await
                .unwrap()
            }
        };
        for n in 1..=PROVIDER_PRESENCE_INSTANCE_CAP {
            assert!(
                matches!(
                    apply(
                        channel,
                        pubkey.clone(),
                        format!("i{n}"),
                        format!("b{n}"),
                        1,
                        90
                    )
                    .await,
                    PresenceApplyOutcome::Applied(_)
                ),
                "instance {n} fits"
            );
        }
        assert_eq!(
            apply(channel, pubkey.clone(), "i9".into(), "b9".into(), 1, 90).await,
            PresenceApplyOutcome::Capped {
                live_instances: 8,
                cap: PROVIDER_PRESENCE_INSTANCE_CAP
            }
        );
        let snapshot = provider_presence_snapshot(&pool, &context, channel)
            .await
            .unwrap();
        assert_eq!(snapshot.len(), 8, "the refused heartbeat stored nothing");
        assert!(matches!(
            apply(
                channel,
                pubkey.clone(),
                "i1".into(),
                "b1-again".into(),
                2,
                90
            )
            .await,
            PresenceApplyOutcome::Applied(_)
        ));
        let other_key = Keys::generate().public_key().to_hex();
        assert!(matches!(
            apply(channel, other_key, "i9".into(), "other".into(), 1, 90).await,
            PresenceApplyOutcome::Applied(_)
        ));
        assert!(matches!(
            apply(
                Uuid::new_v4(),
                pubkey.clone(),
                "i9".into(),
                "elsewhere".into(),
                1,
                90
            )
            .await,
            PresenceApplyOutcome::Applied(_)
        ));
        // A slot frees when its instance expires.
        let short = Uuid::new_v4();
        for n in 1..=PROVIDER_PRESENCE_INSTANCE_CAP {
            apply(
                short,
                pubkey.clone(),
                format!("s{n}"),
                format!("s{n}"),
                1,
                1,
            )
            .await;
        }
        assert!(matches!(
            apply(short, pubkey.clone(), "s9".into(), "s9".into(), 1, 1).await,
            PresenceApplyOutcome::Capped { .. }
        ));
        tokio::time::sleep(std::time::Duration::from_millis(1_500)).await;
        assert!(matches!(
            apply(short, pubkey.clone(), "s9".into(), "s9-later".into(), 2, 1).await,
            PresenceApplyOutcome::Applied(_)
        ));
    }
}
