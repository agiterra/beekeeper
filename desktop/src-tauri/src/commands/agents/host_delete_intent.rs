//! Protected client deletion retry receipt and cleanup custody.
use super::*;
use beekeeper_host_core::custody_backup::{self, BackupFile, BackupScope};
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeleteIntent {
    pub operation_id: Uuid,
    pub revision: String,
    source_hash: String,
}
/// Preserve the original revision through a accepted tombstone/lost reply.
/// This is a client retry receipt, never a second deletion implementation.
async fn deletion_intent_inner(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
) -> Result<DeleteIntent, String> {
    use sha2::{Digest, Sha256};
    let source_hash = hex::encode(Sha256::digest(
        serde_json::to_vec(record).map_err(|_| "source configuration invalid")?,
    ));
    let path = pair_directory(&record.pubkey, relay)?.join("delete-operation.json");
    if path.exists() {
        let held: DeleteIntent = serde_json::from_slice(&private_bytes(&path)?)
            .map_err(|_| "delete retry receipt invalid")?;
        if held.source_hash != source_hash {
            return Err(
                "pending deletion belongs to a different source configuration; custody retained"
                    .into(),
            );
        }
        return Ok(held);
    }
    let current = head(app, relay, &record.pubkey).await?;
    let public: beekeeper_core_pkg::managed_agent::ManagedAgentEventContent =
        serde_json::from_str(&current.content).map_err(|_| "accepted definition malformed")?;
    if serde_json::to_value(public.public_projection())
        .map_err(|_| "shared configuration invalid")?
        != serde_json::to_value(crate::managed_agents::agent_events::agent_event_content(
            record,
        ))
        .map_err(|_| "source configuration invalid")?
    {
        return Err(
            "intended configuration differs from current shared revision; deletion refused".into(),
        );
    }
    let ctx = context()?;
    let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
    if path.exists() {
        let held: DeleteIntent = serde_json::from_slice(&private_bytes(&path)?)
            .map_err(|_| "delete retry receipt invalid")?;
        if held.source_hash != source_hash {
            return Err("concurrent deletion differs; custody retained".into());
        }
        return Ok(held);
    }
    let held = DeleteIntent {
        operation_id: Uuid::new_v4(),
        revision: current.id.to_hex(),
        source_hash,
    };
    save(&path, &held)?;
    Ok(held)
}

pub(crate) async fn deletion_intent(
    app: &AppHandle,
    record: &ManagedAgentRecord,
    relay: &str,
) -> Result<DeleteIntent, String> {
    let held = deletion_intent_inner(app, record, relay).await?;
    let original = serde_json::to_vec(record).map_err(|_| "delete source cannot be captured")?;
    let auth = record
        .auth_tag
        .as_deref()
        .ok_or("delete source attestation unavailable; custody retained")?;
    let ctx = context()?;
    let _lock = ownership::management_lock(&ctx.home, ctx.instance)?;
    custody_backup::capture(
        &pair_directory(&record.pubkey, relay)?.join("backup-delete"),
        BackupScope {
            actor_pubkey: record.pubkey.clone(),
            relay_url: relay.into(),
            operation_id: held.operation_id.to_string(),
        },
        &[
            BackupFile {
                name: "record.json",
                bytes: &original,
            },
            BackupFile {
                name: "identity-key",
                bytes: record.private_key_nsec.as_bytes(),
            },
            BackupFile {
                name: "auth.json",
                bytes: auth.as_bytes(),
            },
        ],
    )?;
    Ok(held)
}
/// Recover only client cleanup inputs after verified retirement removed its cache.
/// Shared delete is retried before any Keychain cleanup; this grants no execution.
pub(crate) fn deletion_source(actor: &str, relay: &str) -> Result<ManagedAgentRecord, String> {
    let directory = pair_directory(actor, relay)?.join("backup-delete");
    let manifest = custody_backup::verify(&directory)?;
    if manifest.scope.actor_pubkey != actor
        || beekeeper_host_core::record::canonical_relay_key(&manifest.scope.relay_url)
            != beekeeper_host_core::record::canonical_relay_key(relay)
    {
        return Err("delete recovery scope conflicts".into());
    }
    serde_json::from_slice(
        &std::fs::read(directory.join("record.json"))
            .map_err(|_| "delete recovery source unavailable")?,
    )
    .map_err(|_| "delete recovery source invalid".into())
}
