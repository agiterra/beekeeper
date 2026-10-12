//! Thin local evidence input for a protected, existing migration.
use super::*;
use beekeeper_host_core::ownership;
use nostr::Event;
use serde_json::json;

/// Supply a retained original owner-signed definition without changing ownership.
#[derive(Args, Debug)]
pub struct SupplyMigrationDefinitionArgs {
    /// Existing managed actor whose protected migration needs the original revision.
    #[arg(long)]
    pub agent_pubkey: String,
    /// Original signed kind30177 event JSON, not a reconstructed definition.
    #[arg(long)]
    pub event_file: PathBuf,
    /// Commissioned local service home containing the protected migration.
    #[arg(long)]
    pub home: Option<PathBuf>,
}

/// Retain immutable evidence; no execution, revision replacement or uncertainty clearing.
pub fn supply_migration_definition(
    args: &SupplyMigrationDefinitionArgs,
    client: &BeekeeperClient,
) -> Result<(), CliError> {
    crate::validate::validate_hex64(&args.agent_pubkey)?;
    let ctx = context(&args.home)?;
    let event: Event = read_json(Some(&args.event_file), false)?
        .ok_or_else(|| CliError::Usage("original signed event is required".into()))?;
    let mut expected = ownership::for_pair(
        &ctx.home,
        ctx.instance,
        &args.agent_pubkey,
        &client.ws_url(),
    )
    .map_err(CliError::Other)?
    .ok_or_else(|| CliError::Usage("existing protected migration is required".into()))?;
    // After an idempotent revision repair, immutable evidence still describes
    // the original revision. The shared reader verifies the retained scope.
    expected.definition_event_id = event.id.to_hex();
    beekeeper_agent_management::supply_import_definition_evidence(
        &ctx,
        &expected,
        &client.keys().public_key(),
        &event,
    )?;
    println!(
        "{}",
        json!({"agent_pubkey":args.agent_pubkey,"event_id":event.id.to_hex(),"evidence_retained":true,"execution_changed":false})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::{EventBuilder, Keys, Kind, Tag};

    #[test]
    fn http_relay_supplies_evidence_to_existing_websocket_scoped_import() {
        let home = tempfile::tempdir().expect("home");
        let owner = Keys::generate();
        let actor = "ab".repeat(32);
        let event = EventBuilder::new(
            Kind::Custom(30177),
            "{\"name\":\"Source\",\"system_prompt\":\"Preserve this\"}",
        )
        .tags([Tag::parse(["d", actor.as_str()]).expect("scope")])
        .sign_with_keys(&owner)
        .expect("signed source");
        let instance = beekeeper_host_core::layout::Instance::from_env().expect("instance");
        let transfer = ownership::Transfer {
            operation_id: Uuid::new_v4().to_string(),
            actor_pubkey: actor.clone(),
            relay_url: "ws://127.0.0.1:3037".into(),
            legacy_nonce: "verified-source-generation".into(),
            definition_event_id: event.id.to_hex(),
            target: None,
            phase: ownership::Phase::Prepared,
        };
        ownership::advance(home.path(), instance, transfer.clone()).expect("protected import");
        let input = home.path().join("original-event.json");
        beekeeper_host_core::atomic_write::atomic_write_json_restricted(
            &input,
            &serde_json::to_vec(&event).expect("event bytes"),
        )
        .expect("source input");
        let client =
            BeekeeperClient::new("http://127.0.0.1:3037".into(), owner.clone(), None, None)
                .expect("HTTP client");
        let args = SupplyMigrationDefinitionArgs {
            agent_pubkey: actor,
            event_file: input,
            home: Some(home.path().to_owned()),
        };
        supply_migration_definition(&args, &client).expect("same community import");
        let ctx = context(&args.home).expect("explicit scope");
        assert_eq!(
            beekeeper_agent_management::read_import_definition_evidence(
                &ctx,
                &transfer,
                &owner.public_key()
            )
            .expect("retained source")
            .expect("source")
            .id,
            event.id
        );
        assert_eq!(
            ownership::for_pair(
                home.path(),
                instance,
                &transfer.actor_pubkey,
                &transfer.relay_url
            )
            .expect("read"),
            Some(transfer)
        );
    }
}
