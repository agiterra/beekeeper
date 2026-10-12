//! Authorized discovery uses relay NIP-29 metadata tags, not create-request tags.
use super::*;
pub(super) async fn channel(
    app: &AppHandle,
    relay: &str,
    record: &ManagedAgentRecord,
) -> Result<Uuid, String> {
    let transport = Transport::owner(app, relay)?;
    let relay_identity = transport
        .relay_identity()
        .await
        .map_err(|e| e.to_string())?;
    let metadata = transport
        .query_all(json!({"kinds":[39000],"authors":[&relay_identity]}))
        .await
        .map_err(|e| e.to_string())?;
    let mut channels = Vec::new();
    if let Some(project) = record.project_ref.as_deref() {
        beekeeper_agent_management::authorize_project(
            &transport,
            project,
            &transport.keys().public_key().to_hex(),
        )
        .await
        .map_err(|e| e.to_string())?;
        let state = app.state::<AppState>();
        let owner = state.signing_keys()?;
        let provider = bootstrap(app, relay).await?;
        if !crate::managed_agents::project_roster::ensure_host_serving_project(
            app,
            &state,
            &app.state::<crate::agent_host::AgentHost>(),
            &owner,
            project,
            &provider.provider_pubkey,
            relay,
        )
        .await?
        {
            return Err("project host admission is unconfirmed".into());
        }
        for raw in metadata {
            let Ok(event) = serde_json::from_value::<Event>(raw) else {
                continue;
            };
            if event.verify().is_err() || event.pubkey.to_hex() != relay_identity {
                continue;
            }
            let names = event
                .tags
                .iter()
                .any(|t| t.as_slice() == ["project", project]);
            let transport = event
                .tags
                .iter()
                .any(|t| t.as_slice() == ["t", "transport"]);
            if names && transport {
                for tag in event.tags.iter() {
                    if let [name, value] = tag.as_slice() {
                        if name == "d" {
                            if let Ok(id) = Uuid::parse_str(value) {
                                channels.push(id);
                            }
                        }
                    }
                }
            }
        }
        channels.sort();
        channels.dedup();
        return match channels.as_slice(){[only]=>Ok(*only),[]=>Err("project transport is unavailable; create its session channel and retry".into()),_=>Err("project has multiple transports; select the intended responsibility before enrollment".into())};
    }
    let rows=transport.query_all(json!({"kinds":[39002],"authors":[&relay_identity],"#p":[transport.keys().public_key().to_hex()]})).await.map_err(|e|e.to_string())?;
    let mut memberships = std::collections::HashSet::new();
    for raw in rows {
        let Ok(event) = serde_json::from_value::<Event>(raw) else {
            continue;
        };
        if event.verify().is_err() || event.pubkey.to_hex() != relay_identity {
            continue;
        }
        for tag in event.tags.iter() {
            if let [name, value] = tag.as_slice() {
                if name == "d" {
                    memberships.insert(value.clone());
                }
            }
        }
    }
    for raw in metadata {
        let Ok(event) = serde_json::from_value::<Event>(raw) else {
            continue;
        };
        if event.verify().is_err() || event.pubkey.to_hex() != relay_identity {
            continue;
        }
        let stream = event.tags.iter().any(|t| t.as_slice() == ["t", "stream"]);
        if !stream {
            continue;
        }
        for tag in event.tags.iter() {
            if let [name, value] = tag.as_slice() {
                if name == "d" && memberships.contains(value) {
                    if let Ok(id) = Uuid::parse_str(value) {
                        channels.push(id);
                    }
                }
            }
        }
    }
    channels.sort();
    channels.dedup();
    channels.into_iter().next().ok_or_else(|| {
        "no authorized stream channel is available; join or create one before enrollment".into()
    })
}
