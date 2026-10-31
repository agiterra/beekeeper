//! CLI adapter for durable direct-owner genesis publication.
use crate::{client::BeekeeperClient, error::CliError};
use std::path::PathBuf;
use uuid::Uuid;

/// Explicit, stable inputs for an owner-founded umbrella.
#[derive(clap::Args)]
pub struct Args {
    /// Mint once; reuse with identical arguments after an uncertain response.
    #[arg(long)]
    operation_id: Uuid,
    /// Channel UUID; existing relay admission rules apply.
    #[arg(long)]
    channel: Uuid,
    /// Canonical lowercase hyphenated umbrella UUID (not an execution ID).
    #[arg(long)]
    session_ref: String,
    /// Absolute journal home. No commissioned Host or custody files required.
    #[arg(long)]
    home: PathBuf,
}

pub(super) async fn run(args: Args, client: &BeekeeperClient) -> Result<(), CliError> {
    if !args.home.is_absolute() {
        return Err(CliError::Usage(
            "--home must be an absolute journal home".into(),
        ));
    }
    let context = beekeeper_agent_management::Context {
        home: args.home,
        instance: beekeeper_host_core::layout::Instance::from_env().map_err(CliError::Usage)?,
        native_home: None,
    };
    let result = beekeeper_agent_management::session_found::found(
        &beekeeper_agent_management::session_found::FoundRequest {
            operation_id: args.operation_id,
            channel: args.channel,
            session_ref: args.session_ref,
        },
        client,
        &context,
    )
    .await?;
    println!("{result}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn found_runbook_flags_parse() {
        let cli = crate::Cli::try_parse_from([
            "bee",
            "sessions",
            "found",
            "--home",
            "/isolated/host-home",
            "--operation-id",
            "00000000-0000-4000-8000-000000000001",
            "--channel",
            "00000000-0000-4000-8000-000000000002",
            "--session-ref",
            "00000000-0000-4000-8000-000000000003",
        ])
        .unwrap();
        let crate::Cmd::Sessions(crate::SessionsCmd::Found(args)) = cli.command else {
            panic!("wrong command")
        };
        assert_eq!(args.home, PathBuf::from("/isolated/host-home"));
        assert_eq!(args.session_ref, "00000000-0000-4000-8000-000000000003");
    }

    #[tokio::test]
    async fn found_refuses_owner_secret_on_command_line_without_network() {
        assert_eq!(
            crate::run_from_args([
                "bee",
                "--private-key",
                "not-a-secret",
                "sessions",
                "found",
                "--home",
                "/isolated/host-home",
                "--operation-id",
                "00000000-0000-4000-8000-000000000001",
                "--channel",
                "00000000-0000-4000-8000-000000000002",
                "--session-ref",
                "00000000-0000-4000-8000-000000000003",
            ])
            .await,
            3
        );
    }
}

#[cfg(test)]
mod wire_tests {
    use super::*;
    use axum::{
        body::Bytes,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::post,
        Json, Router,
    };
    use base64::{engine::general_purpose::STANDARD, Engine};
    use nostr::Event;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::sync::{Arc, Mutex};

    struct Fixture {
        bodies: Mutex<Vec<Vec<u8>>>,
        journal: PathBuf,
    }
    async fn publish(
        State(f): State<Arc<Fixture>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> (StatusCode, Json<Value>) {
        let event: Event = serde_json::from_slice(&body).unwrap();
        event.verify().unwrap();
        let auth = headers["authorization"]
            .to_str()
            .unwrap()
            .strip_prefix("Nostr ")
            .unwrap();
        let auth: Event = serde_json::from_slice(&STANDARD.decode(auth).unwrap()).unwrap();
        auth.verify().unwrap();
        assert_eq!(auth.pubkey, event.pubkey);
        assert!(auth
            .tags
            .iter()
            .any(|t| t.as_slice() == ["payload", hex::encode(Sha256::digest(&body)).as_str()]));
        assert!(headers.get("x-auth-tag").is_none());
        let saved: Value = serde_json::from_slice(&std::fs::read(&f.journal).unwrap()).unwrap();
        assert_eq!(saved["event"], serde_json::to_value(&event).unwrap());
        f.bodies.lock().unwrap().push(body.to_vec());
        // Stored successfully, but every ACK is lost behind a proxy failure.
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"message":"fixture lost ACK"})),
        )
    }
    async fn query(State(f): State<Arc<Fixture>>, Json(filters): Json<Value>) -> Json<Value> {
        assert_eq!(filters[0]["kinds"], json!([44226]));
        let bodies = f.bodies.lock().unwrap();
        let rows: Vec<Value> = bodies
            .first()
            .map(|b| vec![serde_json::from_slice(b).unwrap()])
            .unwrap_or_default();
        Json(json!(rows))
    }
    #[tokio::test]
    async fn found_real_http_transport_retries_exact_bytes_and_verifies_signed_acceptance() {
        let dir = tempfile::tempdir().unwrap();
        let operation_id = Uuid::new_v4();
        let context = beekeeper_agent_management::Context {
            home: dir.path().into(),
            instance: beekeeper_host_core::layout::Instance::Production,
            native_home: None,
        };
        let journal = beekeeper_host_core::layout::host_dir(&context.home, context.instance)
            .join(format!("management/operations/{operation_id}.json"));
        let fixture = Arc::new(Fixture {
            bodies: Mutex::new(vec![]),
            journal,
        });
        let router = Router::new()
            .route("/events", post(publish))
            .route("/query", post(query))
            .with_state(fixture.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = BeekeeperClient::new(
            format!("http://{address}"),
            nostr::Keys::generate(),
            None,
            None,
        )
        .unwrap();
        let args = beekeeper_agent_management::session_found::FoundRequest {
            operation_id,
            channel: Uuid::new_v4(),
            session_ref: Uuid::new_v4().to_string(),
        };
        let first = beekeeper_agent_management::session_found::found(&args, &client, &context)
            .await
            .unwrap();
        let second = beekeeper_agent_management::session_found::found(&args, &client, &context)
            .await
            .unwrap();
        assert_eq!(first, second);
        let bodies = fixture.bodies.lock().unwrap();
        assert_eq!(bodies.len(), 3, "transport's bounded retry policy");
        assert!(bodies.windows(2).all(|pair| pair[0] == pair[1]));
        task.abort();
    }
}
