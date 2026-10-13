//! Existing-enrollment Start uses the real host client protocol without commissioning.
use super::*;
use beekeeper_host::protocol::{Request, Response, Status};
use beekeeper_host_core::{config::HostConfig, layout::Instance};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc, Mutex,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
};

fn fixture() -> (tempfile::TempDir, Context, Enrollment, HostConfig) {
    let home = tempfile::tempdir().expect("isolated host");
    let ctx = Context {
        home: home.path().into(),
        instance: Instance::Dev,
        native_home: None,
    };
    let relay = "ws://127.0.0.1:31337";
    let actor = nostr::Keys::generate().public_key().to_hex();
    let provider = nostr::Keys::generate().public_key().to_hex();
    let enrollment = Enrollment {
        operation: Uuid::new_v4(),
        actor: actor.clone(),
        relay: relay.into(),
        channel: Uuid::new_v4(),
        cwd: home.path().join("work"),
        public: serde_json::from_value(
            json!({"name":"fixture","parallelism":1,"respond_to":"owner-only"}),
        )
        .expect("public"),
        private: PrivateConfiguration {
            record: json!({"effective_command":"fixture-adapter","runtime":"fixture"}),
        },
        provider_instance: beekeeper_host_core::managed_runtime::INSTANCE_REF.into(),
        provider_authority: provider.clone(),
        activate: false,
        replay_floor_unix: Some(123),
    };
    ownership::record_host_enrollment(
        &ctx.home,
        ctx.instance,
        &actor,
        relay,
        &enrollment.operation.to_string(),
        &"aa".repeat(32),
        None,
    )
    .expect("modern configured ownership");
    let config = HostConfig {
        version: beekeeper_host_core::config::HOST_CONFIG_VERSION,
        instance: ctx.instance,
        relay_url: relay.into(),
        provider_pubkey: provider,
        session_provider_base_dir: home.path().join("provider"),
        provider_state_dir: home.path().join("provider/state"),
        runtimes: vec![beekeeper_host_core::managed_runtime::descriptor()],
        max_sessions: None,
        turn_idle_timeout_secs: None,
        turn_budget: None,
        provider_command: None,
        written_at: "2026-10-12T00:00:00Z".into(),
    };
    (home, ctx, enrollment, config)
}
fn write_config(ctx: &Context, config: &HostConfig) {
    let path = layout::host_config_path(&ctx.home, ctx.instance);
    atomic_write::create_dir_all_restricted(path.parent().expect("parent")).expect("directory");
    atomic_write::atomic_write_json_restricted(&path, &serde_json::to_vec(config).expect("config"))
        .expect("config write");
}
fn status(config: &HostConfig, live: bool) -> Status {
    Status {
        protocol_version: beekeeper_host::protocol::PROTOCOL_VERSION,
        host_version: "test".into(),
        host_pid: 99,
        host_available_at: "2026-10-12T00:00:00Z".into(),
        relay_url: config.relay_url.clone(),
        provider_pubkey: config.provider_pubkey.clone(),
        provider_state_dir: config.provider_state_dir.clone(),
        provider: if live {
            beekeeper_host::state::ProviderChildState::Live {
                pid: 4242,
                started_at: "2026-10-12T00:00:00Z".into(),
            }
        } else {
            beekeeper_host::state::ProviderChildState::NotSupervised
        },
        provider_settings_in_force: None,
        relay_connection: beekeeper_host::state::RelayConnectionState::unknowable(),
        sessions: beekeeper_host::sessions::SessionSnapshot {
            read_at: "2026-10-12T00:00:00Z".into(),
            unavailable: None,
            sessions: vec![],
        },
        turns_in_flight: 0,
        app_activity: vec![],
        app_activity_leased: false,
        warnings: vec![],
    }
}
#[tokio::test]
async fn stopped_commissioned_provider_is_started_through_control_once_and_retries_do_not_recommission(
) {
    let (_home, ctx, enrolled, config) = fixture();
    write_config(&ctx, &config);
    let path = layout::host_config_path(&ctx.home, ctx.instance);
    let bytes = std::fs::read(&path).expect("original commission");
    let socket = ctx.home.join("control.sock");
    let listener = UnixListener::bind(&socket).expect("isolated control socket");
    let requests = Arc::new(Mutex::new(Vec::new()));
    let launches = Arc::new(AtomicUsize::new(0));
    let observed_requests = requests.clone();
    let observed_launches = launches.clone();
    let served = config.clone();
    let server = tokio::spawn(async move {
        let mut live = false;
        loop {
            let (stream, _) = listener.accept().await.expect("accept");
            let (read, mut write) = stream.into_split();
            let mut reader = BufReader::new(read);
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("request");
            let request: Request = serde_json::from_str(&line).expect("actual protocol request");
            observed_requests
                .lock()
                .expect("requests")
                .push(request.clone());
            match request {
                Request::Status => {}
                Request::Start => {
                    if !live {
                        live = true;
                        observed_launches.fetch_add(1, Ordering::SeqCst);
                    }
                }
                _ => panic!("no Bind, restart or other control permitted"),
            }
            let response =
                Response::ok(serde_json::to_value(status(&served, live)).expect("status"));
            let mut answer = serde_json::to_vec(&response).expect("response");
            answer.push(b'\n');
            write.write_all(&answer).await.expect("reply");
        }
    });
    let bootstraps = AtomicUsize::new(0);
    for _ in 0..2 {
        ensure_existing_with(
            &ctx,
            &enrolled.actor,
            &enrolled.relay,
            &enrolled,
            || {
                bootstraps.fetch_add(1, Ordering::SeqCst);
                Ok(())
            },
            || async {
                beekeeper_host::client::status(&socket)
                    .await
                    .map_err(|error| error.message())
            },
            || async {
                beekeeper_host::client::call(
                    &socket,
                    &Request::Start,
                    beekeeper_host::client::LIFECYCLE_READ_TIMEOUT,
                )
                .await
                .map(|_| ())
                .map_err(|error| error.message())
            },
        )
        .await
        .expect("same commissioned worker restarted");
    }
    assert_eq!(
        launches.load(Ordering::SeqCst),
        1,
        "host owns idempotent process supervision"
    );
    assert_eq!(bootstraps.load(Ordering::SeqCst), 2);
    assert_eq!(
        requests
            .lock()
            .expect("requests")
            .iter()
            .filter(|request| **request == Request::Start)
            .count(),
        2
    );
    assert_eq!(std::fs::read(path).expect("unchanged commission"), bytes);
    assert_eq!(
        ownership::for_pair(&ctx.home, ctx.instance, &enrolled.actor, &enrolled.relay)
            .expect("ownership")
            .expect("held")
            .operation_id,
        enrolled.operation.to_string()
    );
    assert_eq!(enrolled.replay_floor_unix, Some(123));
    server.abort();
}
#[tokio::test]
async fn missing_or_foreign_disk_commission_refuses_before_any_bootstrap_or_host_control() {
    let (_home, ctx, enrolled, original) = fixture();
    let effects = AtomicUsize::new(0);
    for variant in 0..5 {
        let mut config = original.clone();
        if variant > 0 {
            match variant {
                1 => config.relay_url = "wss://other.example".into(),
                2 => config.provider_pubkey = "bb".repeat(32),
                3 => config.instance = Instance::Production,
                _ => config.runtimes.clear(),
            }
            write_config(&ctx, &config);
        }
        assert!(
            ensure_existing_with(
                &ctx,
                &enrolled.actor,
                &enrolled.relay,
                &enrolled,
                || {
                    effects.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
                || async {
                    effects.fetch_add(1, Ordering::SeqCst);
                    Ok(status(&config, false))
                },
                || async {
                    effects.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                },
            )
            .await
            .is_err(),
            "variant {variant}"
        );
        assert_eq!(effects.load(Ordering::SeqCst), 0, "refused before effects");
    }
}
#[tokio::test]
async fn cached_foreign_served_commission_is_refused_before_start_without_bind() {
    let (_home, ctx, enrolled, config) = fixture();
    write_config(&ctx, &config);
    let mut foreign = config.clone();
    foreign.provider_pubkey = "bb".repeat(32);
    let starts = AtomicUsize::new(0);
    assert!(ensure_existing_with(
        &ctx,
        &enrolled.actor,
        &enrolled.relay,
        &enrolled,
        || Ok(()),
        || async { Ok(status(&foreign, false)) },
        || async {
            starts.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .await
    .is_err());
    assert_eq!(starts.load(Ordering::SeqCst), 0);
    assert_eq!(
        HostConfig::load(&layout::host_config_path(&ctx.home, ctx.instance))
            .expect("disk commission"),
        Some(config)
    );
}

#[tokio::test]
async fn unresolved_owner_and_changed_enrollment_scope_cannot_bootstrap() {
    let (_home, ctx, mut enrolled, config) = fixture();
    write_config(&ctx, &config);
    let effects = AtomicUsize::new(0);
    let actor = enrolled.actor.clone();
    enrolled.relay = "wss://other.example".into();
    assert!(ensure_existing_with(
        &ctx,
        &actor,
        &config.relay_url,
        &enrolled,
        || {
            effects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        || async { Ok(status(&config, false)) },
        || async {
            effects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .await
    .is_err());
    enrolled.relay = config.relay_url.clone();
    let journal_path = layout::host_dir(&ctx.home, ctx.instance).join("managed-ownership.json");
    let mut journal: Value =
        serde_json::from_slice(&std::fs::read(&journal_path).expect("journal")).expect("JSON");
    journal["transfers"][0]["phase"] = json!("prepared");
    atomic_write::atomic_write_json_restricted(
        &journal_path,
        &serde_json::to_vec(&journal).expect("journal encoding"),
    )
    .expect("uncertain historical fixture");
    assert!(ensure_existing_with(
        &ctx,
        &actor,
        &config.relay_url,
        &enrolled,
        || {
            effects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        || async { Ok(status(&config, false)) },
        || async {
            effects.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
    )
    .await
    .is_err());
    assert_eq!(effects.load(Ordering::SeqCst), 0);
}
