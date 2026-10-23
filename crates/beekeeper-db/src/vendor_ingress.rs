//! Vendor webhook ingress storage (migration 0048; plan § 1.2 B1, § 4, F3).
//!
//! A webhook delivery that passed signature verification is recorded by
//! [`record_delivery`] inside the caller's transaction:
//!
//! - **accept**: the relay-signed kind:50000 event (whose insert fires the
//!   `events` trigger that writes its `workflow_dispatch_queue` row) and the
//!   `accepted` delivery row, in that order, in one transaction. Either all
//!   of event, queue row and delivery row commit, or none does. This path
//!   does not pass through ordinary ingest, so the event is checked here
//!   before any write: the application-event envelope, the relay's own
//!   signature (pubkey and a verifying id/sig), its `h` tag naming the
//!   installation's channel, and its [`SOURCE_TAG`] naming the
//!   installation's plugin and id.
//! - **ignore** / **reject** (authenticated): the delivery row only.
//!
//! The delivery key `(community, installation, delivery_id)` is taken only by
//! authenticated deliveries (F3). A request whose signature did not verify
//! never writes a delivery row; it bumps the bounded counters on its
//! installation through [`record_unauthenticated_rejection`].
//!
//! A redelivery of an `accepted` or `ignored` delivery is a
//! [`DeliveryOutcome::Duplicate`] and writes nothing. A `rejected` row is
//! re-projected: a later delivery under the same id overwrites it.

use std::time::Duration;

use beekeeper_core::application_event::validate_application_event;
use beekeeper_core::kind::KIND_APPLICATION_EVENT;
use beekeeper_core::{CommunityId, StoredEvent};
use chrono::{DateTime, Utc};
use sqlx::{PgPool, Postgres, Row, Transaction};
use uuid::Uuid;

use crate::error::{DbError, Result};
use crate::event::insert_event_with_thread_metadata_tx;

/// The provenance tag the plugin host seals on every application event:
/// `["beekeeper:source", <plugin>, <installation uuid>]`. The same shape the
/// workflow engine's `application_event` trigger reads.
pub const SOURCE_TAG: &str = "beekeeper:source";

/// Longest vendor delivery id stored (GitHub's is a 36-character GUID).
pub const MAX_DELIVERY_ID_LEN: usize = 200;
/// Longest vendor event name stored.
pub const MAX_VENDOR_EVENT_LEN: usize = 200;
/// Longest reason or error text stored; longer text is truncated on a char
/// boundary.
pub const MAX_REASON_LEN: usize = 500;
/// Length of a plugin digest (sha256).
pub const PLUGIN_DIGEST_LEN: usize = 32;
/// Delivery rows are kept this long after receipt (plan § 4).
pub const DELIVERY_RETENTION: Duration = Duration::from_secs(30 * 24 * 3600);
/// Delivery rows the retention sweep removes per call.
pub const PRUNE_BATCH: i64 = 5_000;

/// One vendor webhook installation.
#[derive(Clone)]
pub struct VendorInstallation {
    /// Owning community.
    pub community_id: CommunityId,
    /// Installation id (the webhook URL path segment).
    pub id: Uuid,
    /// Registered plugin name.
    pub plugin: String,
    /// Channel the installation's events are published into.
    pub channel_id: Uuid,
    /// Pubkey of the member who created it.
    pub created_by: Vec<u8>,
    /// Webhook secret (plaintext at rest, ASK-201). Never logged: `Debug`
    /// redacts it.
    pub secret: Vec<u8>,
    /// Plugin settings object.
    pub settings: serde_json::Value,
    /// Disabled installations accept nothing.
    pub enabled: bool,
    /// Creation time.
    pub created_at: DateTime<Utc>,
    /// When it was disabled.
    pub disabled_at: Option<DateTime<Utc>>,
    /// Unauthenticated (pre-signature) rejections so far.
    pub rejected_count: i64,
    /// Time of the last unauthenticated rejection.
    pub last_rejected_at: Option<DateTime<Utc>>,
    /// Reason of the last unauthenticated rejection.
    pub last_rejected_reason: Option<String>,
    /// Host or plugin failures so far.
    pub error_count: i64,
    /// Time of the last failure.
    pub last_error_at: Option<DateTime<Utc>>,
    /// Last failure text.
    pub last_error: Option<String>,
    /// Time of the last accepted delivery.
    pub last_accepted_at: Option<DateTime<Utc>>,
}

impl std::fmt::Debug for VendorInstallation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VendorInstallation")
            .field("community_id", &self.community_id)
            .field("id", &self.id)
            .field("plugin", &self.plugin)
            .field("channel_id", &self.channel_id)
            .field("created_by", &hex::encode(&self.created_by))
            .field("secret", &"<redacted>")
            .field("settings", &self.settings)
            .field("enabled", &self.enabled)
            .field("rejected_count", &self.rejected_count)
            .field("error_count", &self.error_count)
            .finish_non_exhaustive()
    }
}

/// Fields for [`create_installation`]. Authorisation (channel admin, archived
/// channel, plugin registered, settings valid) is the caller's.
pub struct NewInstallation<'a> {
    /// Registered plugin name.
    pub plugin: &'a str,
    /// Target channel.
    pub channel_id: Uuid,
    /// Creating member's pubkey (must be a community user).
    pub created_by: &'a [u8],
    /// Webhook secret, at least 16 bytes.
    pub secret: &'a [u8],
    /// Plugin settings object.
    pub settings: &'a serde_json::Value,
}

/// An authenticated delivery to record.
pub struct Delivery<'a> {
    /// The vendor's delivery id (dedup key within the installation).
    pub delivery_id: &'a str,
    /// The vendor's event name (e.g. GitHub `X-GitHub-Event`).
    pub vendor_event: &'a str,
    /// sha256 of the plugin that handled it.
    pub plugin_digest: &'a [u8],
    /// What the plugin decided.
    pub disposition: DeliveryDisposition<'a>,
}

/// The plugin's decision for an authenticated delivery.
pub enum DeliveryDisposition<'a> {
    /// Publish this relay-signed kind:50000 event into the installation's
    /// channel.
    Accept {
        /// The signed event.
        event: &'a nostr::Event,
    },
    /// Authenticated, deliberately not published (filter miss, ping).
    Ignore {
        /// Why.
        reason: &'a str,
    },
    /// Authenticated but malformed.
    Reject {
        /// Why.
        reason: &'a str,
    },
}

impl DeliveryDisposition<'_> {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Accept { .. } => "accepted",
            Self::Ignore { .. } => "ignored",
            Self::Reject { .. } => "rejected",
        }
    }
}

/// Result of [`record_delivery`].
#[derive(Debug, Clone)]
pub enum DeliveryOutcome {
    /// Event, queue row and delivery row written.
    Accepted(Box<StoredEvent>),
    /// Delivery row written as `ignored`.
    Ignored,
    /// Delivery row written as `rejected`.
    Rejected,
    /// The delivery id is already `accepted` or `ignored`; nothing written.
    Duplicate {
        /// The stored disposition.
        disposition: String,
        /// The stored event id, when accepted.
        event_id: Option<Vec<u8>>,
    },
}

/// A stored delivery row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VendorDelivery {
    /// Installation.
    pub installation_id: Uuid,
    /// Vendor delivery id.
    pub delivery_id: String,
    /// `accepted`, `ignored` or `rejected`.
    pub disposition: String,
    /// Vendor event name.
    pub vendor_event: String,
    /// Reason for ignore / reject.
    pub reason: Option<String>,
    /// Event id, iff accepted.
    pub event_id: Option<Vec<u8>>,
    /// Plugin digest that handled it.
    pub plugin_digest: Vec<u8>,
    /// Receipt time.
    pub received_at: DateTime<Utc>,
}

fn bounded(text: &str) -> &str {
    if text.len() <= MAX_REASON_LEN {
        return text;
    }
    let mut end = MAX_REASON_LEN;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}

fn row_to_installation(row: sqlx::postgres::PgRow) -> Result<VendorInstallation> {
    Ok(VendorInstallation {
        community_id: CommunityId::from_uuid(row.try_get("community_id")?),
        id: row.try_get("id")?,
        plugin: row.try_get("plugin")?,
        channel_id: row.try_get("channel_id")?,
        created_by: row.try_get("created_by")?,
        secret: row.try_get("secret")?,
        settings: row.try_get("settings")?,
        enabled: row.try_get("enabled")?,
        created_at: row.try_get("created_at")?,
        disabled_at: row.try_get("disabled_at")?,
        rejected_count: row.try_get("rejected_count")?,
        last_rejected_at: row.try_get("last_rejected_at")?,
        last_rejected_reason: row.try_get("last_rejected_reason")?,
        error_count: row.try_get("error_count")?,
        last_error_at: row.try_get("last_error_at")?,
        last_error: row.try_get("last_error")?,
        last_accepted_at: row.try_get("last_accepted_at")?,
    })
}

const INSTALLATION_COLUMNS: &str = "community_id, id, plugin, channel_id, created_by, secret, \
     settings, enabled, created_at, disabled_at, rejected_count, last_rejected_at, \
     last_rejected_reason, error_count, last_error_at, last_error, last_accepted_at";

/// Insert a new, enabled installation with a fresh id.
pub async fn create_installation(
    pool: &PgPool,
    community_id: CommunityId,
    new: &NewInstallation<'_>,
) -> Result<VendorInstallation> {
    let row = sqlx::query(sqlx::AssertSqlSafe(format!(
        "INSERT INTO vendor_installations \
             (community_id, plugin, channel_id, created_by, secret, settings) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING {INSTALLATION_COLUMNS}"
    )))
    .bind(community_id.as_uuid())
    .bind(new.plugin)
    .bind(new.channel_id)
    .bind(new.created_by)
    .bind(new.secret)
    .bind(new.settings)
    .fetch_one(pool)
    .await?;
    row_to_installation(row)
}

/// Load an installation of this community, enabled or not.
pub async fn get_installation(
    pool: &PgPool,
    community_id: CommunityId,
    id: Uuid,
) -> Result<Option<VendorInstallation>> {
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "SELECT {INSTALLATION_COLUMNS} FROM vendor_installations \
         WHERE community_id = $1 AND id = $2"
    )))
    .bind(community_id.as_uuid())
    .bind(id)
    .fetch_optional(pool)
    .await?
    .map(row_to_installation)
    .transpose()
}

/// Count a request whose signature (or pre-auth envelope) did not verify.
/// One bounded UPDATE on the installation row; never a delivery row (F3).
pub async fn record_unauthenticated_rejection(
    pool: &PgPool,
    community_id: CommunityId,
    installation_id: Uuid,
    reason: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE vendor_installations \
         SET rejected_count = rejected_count + 1, last_rejected_at = now(), \
             last_rejected_reason = $3 \
         WHERE community_id = $1 AND id = $2",
    )
    .bind(community_id.as_uuid())
    .bind(installation_id)
    .bind(bounded(reason))
    .execute(pool)
    .await?;
    Ok(())
}

/// Count a host or plugin failure (the request got a 5xx and recorded no
/// delivery). Best effort: the caller logs a failure of this write too.
pub async fn record_installation_error(
    pool: &PgPool,
    community_id: CommunityId,
    installation_id: Uuid,
    error: &str,
) -> Result<()> {
    sqlx::query(
        "UPDATE vendor_installations \
         SET error_count = error_count + 1, last_error_at = now(), last_error = $3 \
         WHERE community_id = $1 AND id = $2",
    )
    .bind(community_id.as_uuid())
    .bind(installation_id)
    .bind(bounded(error))
    .execute(pool)
    .await?;
    Ok(())
}

fn validate_delivery(
    installation: &VendorInstallation,
    relay_pubkey: &nostr::PublicKey,
    delivery: &Delivery<'_>,
) -> Result<()> {
    if !installation.enabled {
        return Err(DbError::InvalidData(
            "vendor installation is disabled".into(),
        ));
    }
    if delivery.delivery_id.is_empty() || delivery.delivery_id.len() > MAX_DELIVERY_ID_LEN {
        return Err(DbError::InvalidData(format!(
            "vendor delivery id must be 1..={MAX_DELIVERY_ID_LEN} bytes"
        )));
    }
    if delivery.vendor_event.len() > MAX_VENDOR_EVENT_LEN {
        return Err(DbError::InvalidData(format!(
            "vendor event name must be at most {MAX_VENDOR_EVENT_LEN} bytes"
        )));
    }
    if delivery.plugin_digest.len() != PLUGIN_DIGEST_LEN {
        return Err(DbError::InvalidData(format!(
            "plugin digest must be {PLUGIN_DIGEST_LEN} bytes"
        )));
    }
    if let DeliveryDisposition::Accept { event } = &delivery.disposition {
        validate_accepted_event(installation, relay_pubkey, event)?;
    }
    Ok(())
}

/// The event of an accepted delivery: a well-formed application event,
/// signed by the relay, scoped to the installation's channel and sourced
/// from the installation. Nothing about it is taken on trust from the
/// caller, because nothing downstream re-checks it before it is stored.
fn validate_accepted_event(
    installation: &VendorInstallation,
    relay_pubkey: &nostr::PublicKey,
    event: &nostr::Event,
) -> Result<()> {
    let invalid = |reason: String| Err(DbError::InvalidData(reason));
    if u32::from(event.kind.as_u16()) != KIND_APPLICATION_EVENT {
        return invalid(format!(
            "an accepted vendor delivery must be a kind {KIND_APPLICATION_EVENT} event"
        ));
    }
    if let Err(error) = validate_application_event(event) {
        return invalid(format!(
            "accepted vendor delivery is not a valid application event: {error}"
        ));
    }
    if &event.pubkey != relay_pubkey {
        return invalid("accepted vendor delivery is not signed by the relay".into());
    }
    if let Err(error) = event.verify() {
        return invalid(format!(
            "accepted vendor delivery has an invalid id or signature: {error}"
        ));
    }
    // validate_application_event guarantees exactly one well-formed h tag.
    let channel = event
        .tags
        .iter()
        .map(|tag| tag.as_slice())
        .find(|parts| parts.first().map(String::as_str) == Some("h"))
        .and_then(|parts| Uuid::parse_str(&parts[1]).ok());
    if channel != Some(installation.channel_id) {
        return invalid(
            "accepted vendor delivery is not scoped to the installation's channel".into(),
        );
    }
    let mut source = None;
    for parts in event.tags.iter().map(|tag| tag.as_slice()) {
        if parts.first().map(String::as_str) != Some(SOURCE_TAG) {
            continue;
        }
        if source.is_some() {
            return invalid("accepted vendor delivery carries more than one source tag".into());
        }
        if parts.len() != 3 || parts[1].is_empty() {
            return invalid("accepted vendor delivery source tag is malformed".into());
        }
        let Ok(id) = Uuid::parse_str(&parts[2]) else {
            return invalid("accepted vendor delivery source tag is malformed".into());
        };
        source = Some((parts[1].as_str(), id));
    }
    match source {
        None => invalid("accepted vendor delivery has no source tag".into()),
        Some((plugin, id)) if plugin == installation.plugin && id == installation.id => Ok(()),
        Some(_) => {
            invalid("accepted vendor delivery source tag does not name this installation".into())
        }
    }
}

/// Record an AUTHENTICATED delivery in the caller's transaction (B1).
///
/// The caller commits, and answers the vendor 2xx only after that commit.
/// On [`DeliveryOutcome::Duplicate`] nothing was written, so commit or
/// rollback are equivalent.
///
/// A transaction-scoped advisory lock on the delivery key serialises
/// concurrent copies of one delivery: the second waits for the first to
/// commit and then sees its row, so a racing redelivery is a duplicate, not
/// a second event.
///
/// `relay_pubkey` is the relay's signing key: an accepted event must be
/// signed by it (and pass the other checks in the module doc), or nothing is
/// written.
///
/// For an accept the event is inserted first (its insert writes the
/// dispatch queue row through the `events` trigger) and the delivery row
/// second; an error from either aborts the transaction, so neither the event
/// nor its queue row can commit without the delivery row.
pub async fn record_delivery(
    tx: &mut Transaction<'_, Postgres>,
    installation: &VendorInstallation,
    relay_pubkey: &nostr::PublicKey,
    delivery: &Delivery<'_>,
) -> Result<DeliveryOutcome> {
    validate_delivery(installation, relay_pubkey, delivery)?;
    let community_id = installation.community_id;
    let lock_key = format!(
        "beekeeper:vendor-delivery:{}:{}:{}",
        community_id.as_uuid(),
        installation.id,
        delivery.delivery_id
    );
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(&lock_key)
        .execute(&mut **tx)
        .await?;

    let existing = sqlx::query(
        "SELECT disposition, event_id FROM vendor_deliveries \
         WHERE community_id = $1 AND installation_id = $2 AND delivery_id = $3",
    )
    .bind(community_id.as_uuid())
    .bind(installation.id)
    .bind(delivery.delivery_id)
    .fetch_optional(&mut **tx)
    .await?;
    if let Some(row) = existing {
        let disposition: String = row.try_get("disposition")?;
        if disposition != "rejected" {
            return Ok(DeliveryOutcome::Duplicate {
                disposition,
                event_id: row.try_get("event_id")?,
            });
        }
    }

    let (stored, reason) = match &delivery.disposition {
        DeliveryDisposition::Accept { event } => {
            let (stored, inserted) = insert_event_with_thread_metadata_tx(
                tx,
                community_id,
                event,
                Some(installation.channel_id),
                None,
            )
            .await?;
            if !inserted {
                return Err(DbError::InvalidData(
                    "vendor delivery event id already stored".into(),
                ));
            }
            (Some(stored), None)
        }
        DeliveryDisposition::Ignore { reason } | DeliveryDisposition::Reject { reason } => {
            (None, Some(bounded(reason)))
        }
    };

    let event_id = stored.as_ref().map(|s| s.event.id.as_bytes().to_vec());
    let written = sqlx::query(
        "INSERT INTO vendor_deliveries \
             (community_id, installation_id, delivery_id, disposition, vendor_event, \
              reason, event_id, plugin_digest) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         ON CONFLICT (community_id, installation_id, delivery_id) DO UPDATE \
         SET disposition = EXCLUDED.disposition, vendor_event = EXCLUDED.vendor_event, \
             reason = EXCLUDED.reason, event_id = EXCLUDED.event_id, \
             plugin_digest = EXCLUDED.plugin_digest, received_at = now() \
         WHERE vendor_deliveries.disposition = 'rejected'",
    )
    .bind(community_id.as_uuid())
    .bind(installation.id)
    .bind(delivery.delivery_id)
    .bind(delivery.disposition.as_str())
    .bind(delivery.vendor_event)
    .bind(reason)
    .bind(event_id.as_deref())
    .bind(delivery.plugin_digest)
    .execute(&mut **tx)
    .await?
    .rows_affected();
    if written != 1 {
        // The advisory lock makes the pre-check authoritative; reaching this
        // means the key was taken outside that lock. Abort rather than
        // commit an event without its delivery row.
        return Err(DbError::InvalidData(
            "vendor delivery key taken concurrently".into(),
        ));
    }

    Ok(match stored {
        Some(stored) => {
            sqlx::query(
                "UPDATE vendor_installations SET last_accepted_at = now() \
                 WHERE community_id = $1 AND id = $2",
            )
            .bind(community_id.as_uuid())
            .bind(installation.id)
            .execute(&mut **tx)
            .await?;
            DeliveryOutcome::Accepted(Box::new(stored))
        }
        None => match delivery.disposition {
            DeliveryDisposition::Ignore { .. } => DeliveryOutcome::Ignored,
            _ => DeliveryOutcome::Rejected,
        },
    })
}

/// [`record_delivery`] in its own transaction, committed before returning.
/// When this returns `Ok`, the outcome is durable.
pub async fn record_delivery_committed(
    pool: &PgPool,
    installation: &VendorInstallation,
    relay_pubkey: &nostr::PublicKey,
    delivery: &Delivery<'_>,
) -> Result<DeliveryOutcome> {
    let mut tx = pool.begin().await?;
    let outcome = record_delivery(&mut tx, installation, relay_pubkey, delivery).await?;
    tx.commit().await?;
    Ok(outcome)
}

/// Load one delivery row.
pub async fn get_delivery(
    pool: &PgPool,
    community_id: CommunityId,
    installation_id: Uuid,
    delivery_id: &str,
) -> Result<Option<VendorDelivery>> {
    let Some(row) = sqlx::query(
        "SELECT installation_id, delivery_id, disposition, vendor_event, reason, event_id, \
                plugin_digest, received_at \
         FROM vendor_deliveries \
         WHERE community_id = $1 AND installation_id = $2 AND delivery_id = $3",
    )
    .bind(community_id.as_uuid())
    .bind(installation_id)
    .bind(delivery_id)
    .fetch_optional(pool)
    .await?
    else {
        return Ok(None);
    };
    Ok(Some(VendorDelivery {
        installation_id: row.try_get("installation_id")?,
        delivery_id: row.try_get("delivery_id")?,
        disposition: row.try_get("disposition")?,
        vendor_event: row.try_get("vendor_event")?,
        reason: row.try_get("reason")?,
        event_id: row.try_get("event_id")?,
        plugin_digest: row.try_get("plugin_digest")?,
        received_at: row.try_get("received_at")?,
    }))
}

/// Retention sweep: delete up to [`PRUNE_BATCH`] delivery rows received
/// before `retention` ago, only for communities whose write fence is open.
/// Returns the number deleted. Not yet scheduled by the relay (the webhook
/// host, step 8, owns that).
pub async fn prune_vendor_deliveries(pool: &PgPool, retention: Duration) -> Result<u64> {
    Ok(sqlx::query(
        "DELETE FROM vendor_deliveries d USING ( \
             SELECT community_id, installation_id, delivery_id FROM vendor_deliveries \
             WHERE received_at < now() - make_interval(secs => $1) \
               AND community_write_allowed(community_id) \
             LIMIT $2) old \
         WHERE d.community_id = old.community_id \
           AND d.installation_id = old.installation_id \
           AND d.delivery_id = old.delivery_id",
    )
    .bind(retention.as_secs_f64())
    .bind(PRUNE_BATCH)
    .execute(pool)
    .await?
    .rows_affected())
}

#[cfg(test)]
#[path = "vendor_ingress_tests.rs"]
mod tests;
