//! The honesty properties of the provider status, as tests.

use super::*;
use beekeeper_host::state::{LockOwnerKind, ProviderChildState, RelayConnectionState};
use beekeeper_host_core::record::CodingSessionProviderRecord;

fn record(pubkey: &str) -> CodingSessionProviderRecord {
    CodingSessionProviderRecord {
        provider_pubkey: pubkey.to_string(),
        instance_id: pubkey[..16].to_string(),
        auth_tag: None,
        created_at: "2026-09-30T00:00:00Z".to_string(),
        relay_url: "wss://hive.example.org".to_string(),
        private_key_nsec: String::new(),
    }
}

fn host_status(pubkey: &str, provider: ProviderChildState) -> beekeeper_host::protocol::Status {
    beekeeper_host::protocol::Status {
        protocol_version: beekeeper_host::protocol::PROTOCOL_VERSION,
        host_version: "0.1.0".to_string(),
        host_pid: 1234,
        host_available_at: "2026-09-30T00:00:00Z".to_string(),
        relay_url: "wss://hive.example.org".to_string(),
        provider_pubkey: pubkey.to_string(),
        provider_state_dir: std::path::PathBuf::from("/data/session-provider/aaaa"),
        provider,
        provider_settings_in_force: None,
        relay_connection: RelayConnectionState::unknowable(),
        // No rows in this fixture, so no turns. The host derives this from the
        // rows when it answers; a fixture where the two disagree describes a
        // status the host cannot produce.
        turns_in_flight: 0,
        app_activity: Vec::new(),
        app_activity_leased: false,
        sessions: beekeeper_host::sessions::SessionSnapshot {
            read_at: "2026-09-30T00:00:00Z".to_string(),
            unavailable: None,
            sessions: Vec::new(),
        },
        warnings: Vec::new(),
    }
}

/// A registration the poll did not write. These tests exercise the status
/// *shape*, not the launchd side effect, so they hand in a fixed answer —
/// which is the reason `from_snapshot` takes it as an argument.
fn unregistered() -> beekeeper_host::install::Registration {
    beekeeper_host::install::Registration {
        installed: false,
        path: std::path::PathBuf::from("/home/agent/Library/LaunchAgents/host.plist"),
        program: None,
        warnings: Vec::new(),
        domain: beekeeper_host::install::Domain::User,
    }
}

fn reachable(pubkey: &str, provider: ProviderChildState) -> HostSnapshot {
    let status = host_status(pubkey, provider);
    HostSnapshot {
        reachability: HostReachability::Reachable,
        message: status.provider.message(),
        status: Some(status),
    }
}

fn absent() -> HostSnapshot {
    HostSnapshot {
        reachability: HostReachability::Absent {
            socket: std::path::PathBuf::from("/tmp/host.sock"),
        },
        status: None,
        message: "the agent host is not running (nothing is listening on /tmp/host.sock)"
            .to_string(),
    }
}

/// The whole reason `running` is nullable: an unreachable host must not read
/// as a stopped provider.
#[test]
fn running_is_null_when_the_host_could_not_be_reached() {
    let pubkey = "a".repeat(64);
    let host = AgentHost::new();
    let status = CodingSessionProviderStatus::from_snapshot(
        Some(&record(&pubkey)),
        &host,
        absent(),
        unregistered(),
        beekeeper_host::install::LoginAutostart::ShouldAsk,
    );
    assert_eq!(status.running, None);
    assert!(status.provisioned, "the record still exists on disk");
    assert!(!status.host.reachability.is_reachable());
    assert!(
        status.host.message.contains("not running"),
        "{}",
        status.host.message
    );
    // The JSON must carry a real null, not omit the field: an absent key reads
    // as `undefined` in TypeScript and every `=== false` check would be wrong
    // in the same silent way.
    let json = serde_json::to_string(&status).expect("encode");
    assert!(json.contains("\"running\":null"), "{json}");
}

#[test]
fn a_live_child_for_this_identity_is_running_and_another_identity_is_not() {
    let ours = "a".repeat(64);
    let theirs = "b".repeat(64);
    let host = AgentHost::new();
    let live = ProviderChildState::Live {
        pid: 42,
        started_at: "2026-09-30T00:00:00Z".to_string(),
    };

    let status = CodingSessionProviderStatus::from_snapshot(
        Some(&record(&ours)),
        &host,
        reachable(&ours, live.clone()),
        unregistered(),
        beekeeper_host::install::LoginAutostart::ShouldAsk,
    );
    assert_eq!(status.running, Some(true));
    assert_eq!(
        status.process_state(),
        CodingSessionProviderProcessState::Live { pid: 42 }
    );

    // The host is serving a *different* identity: live, but not ours.
    let status = CodingSessionProviderStatus::from_snapshot(
        Some(&record(&ours)),
        &host,
        reachable(&theirs, live),
        unregistered(),
        beekeeper_host::install::LoginAutostart::ShouldAsk,
    );
    assert_eq!(
        status.running,
        Some(false),
        "a live provider for another identity is not this one running"
    );
}

/// An unprovisioned app with a reachable host is a definite "not running" —
/// unknown is reserved for a host that did not answer.
#[test]
fn nothing_provisioned_is_false_when_the_host_answered_and_null_when_it_did_not() {
    let host = AgentHost::new();
    let pubkey = "a".repeat(64);
    assert_eq!(
        CodingSessionProviderStatus::from_snapshot(
            None,
            &host,
            reachable(&pubkey, ProviderChildState::NotSupervised),
            unregistered(),
            beekeeper_host::install::LoginAutostart::ShouldAsk,
        )
        .running,
        Some(false)
    );
    assert_eq!(
        CodingSessionProviderStatus::from_snapshot(
            None,
            &host,
            absent(),
            unregistered(),
            beekeeper_host::install::LoginAutostart::ShouldAsk
        )
        .running,
        None
    );
}

/// Every host-side refusal has to reach readiness as something it can block
/// on, carrying the host's own words. Folding them into `NotSupervised` would
/// tell a person to "use Prepare" when the real problem is a missing key or
/// another host holding the lock.
#[test]
fn a_host_that_refuses_to_start_reports_its_reason_rather_than_not_supervised() {
    let pubkey = "a".repeat(64);
    let host = AgentHost::new();
    let refusals = [
        ProviderChildState::KeyUnresolved {
            reason: beekeeper_host::identity::KeyUnresolved::NotFound {
                tried: vec!["BEEKEEPER_HOST_PRIVATE_KEY is not set".to_string()],
            },
        },
        ProviderChildState::GaveUp {
            failures: 5,
            at: "2026-09-30T00:10:00Z".to_string(),
        },
        ProviderChildState::LockHeldElsewhere {
            pid: Some(7),
            kind: LockOwnerKind::AnotherHost,
        },
    ];
    for refusal in refusals {
        let expected = refusal.message();
        let status = CodingSessionProviderStatus::from_snapshot(
            Some(&record(&pubkey)),
            &host,
            reachable(&pubkey, refusal.clone()),
            unregistered(),
            beekeeper_host::install::LoginAutostart::ShouldAsk,
        );
        assert_eq!(status.running, Some(false), "{refusal:?}");
        match status.process_state() {
            CodingSessionProviderProcessState::HostUnreachable { reason } => {
                assert_eq!(reason, expected);
                assert!(
                    !reason.contains("Use Prepare"),
                    "the host's own words, not a generic remedy: {reason}"
                );
            }
            other => panic!("{refusal:?} must not read as {other:?}"),
        }
    }
}

/// A backoff is the host working, not the host failing: it must stay
/// distinguishable so a surface can say "restarting" instead of "stopped".
#[test]
fn a_backoff_stays_a_backoff() {
    let pubkey = "a".repeat(64);
    let host = AgentHost::new();
    let status = CodingSessionProviderStatus::from_snapshot(
        Some(&record(&pubkey)),
        &host,
        reachable(
            &pubkey,
            ProviderChildState::Backoff {
                failures: 3,
                next_at: "2026-09-30T00:00:08Z".to_string(),
            },
        ),
        unregistered(),
        beekeeper_host::install::LoginAutostart::ShouldAsk,
    );
    assert_eq!(status.running, Some(false));
    assert_eq!(
        status.process_state(),
        CodingSessionProviderProcessState::Backoff
    );
    assert!(
        status.host.message.contains("attempt 3"),
        "{}",
        status.host.message
    );
}

/// An unreachable host must be its own process state, never `NotSupervised`.
#[test]
fn an_unreachable_host_is_not_the_same_process_state_as_a_stopped_provider() {
    let pubkey = "a".repeat(64);
    let host = AgentHost::new();
    let unreachable = CodingSessionProviderStatus::from_snapshot(
        Some(&record(&pubkey)),
        &host,
        absent(),
        unregistered(),
        beekeeper_host::install::LoginAutostart::ShouldAsk,
    )
    .process_state();
    let stopped = CodingSessionProviderStatus::from_snapshot(
        Some(&record(&pubkey)),
        &host,
        reachable(&pubkey, ProviderChildState::NotSupervised),
        unregistered(),
        beekeeper_host::install::LoginAutostart::ShouldAsk,
    )
    .process_state();
    assert_ne!(unreachable, stopped);
    assert!(matches!(
        unreachable,
        CodingSessionProviderProcessState::HostUnreachable { .. }
    ));
    assert_eq!(stopped, CodingSessionProviderProcessState::NotSupervised);
}
