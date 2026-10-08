//! SV-33 S2: an unseated session given the desktop broker's socket can
//! connect to that one socket under the proxy egress — and to no other Unix
//! socket — through the production [`prepare`], with no file grant on it.

use std::ffi::OsString;

use super::tests::launched::run_in;
use super::tests::{fixture, inputs};
use super::*;
use beekeeper_core::preview_grant::{PREVIEW_GRANT_ENV, SESSION_BROKER_SOCK_ENV};

#[test]
fn the_broker_socket_is_the_one_unix_connect_a_confined_session_makes() {
    let fx = fixture();
    let broker = fx.root.join("broker.sock");
    let other = fx.root.join("other.sock");
    let _broker = std::os::unix::net::UnixListener::bind(&broker).expect("broker listener");
    let _other = std::os::unix::net::UnixListener::bind(&other).expect("other listener");
    let proxy = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("proxy");
    let ambient: Vec<(OsString, OsString)> = vec![("PATH".into(), "/usr/bin:/bin".into())];
    let identity = vec![
        (PREVIEW_GRANT_ENV.to_owned(), "bkpg1.fixture".to_owned()),
        (
            SESSION_BROKER_SOCK_ENV.to_owned(),
            broker.display().to_string(),
        ),
    ];
    let mut confined = inputs(&fx, "preview", &fx.seat_a, &[], &identity);
    confined.actor = None;
    confined.ambient_env = Some(&ambient);
    confined.isolation.egress =
        beekeeper_acp::exec_boundary::Egress::loopback_proxy(proxy.local_addr().expect("addr"))
            .expect("loopback");
    let plan = match prepare(&confined) {
        Ok(ExecutionPlan::Prepared(plan)) => plan,
        other => panic!("prepared: {other:?}"),
    };
    // Both variables reach the unseated session, after the fence.
    assert_eq!(
        plan.launch.env().get(PREVIEW_GRANT_ENV),
        Some("bkpg1.fixture")
    );
    assert_eq!(
        plan.launch.env().get(SESSION_BROKER_SOCK_ENV),
        broker.to_str()
    );
    let socket_grants: Vec<_> = plan
        .launch
        .boundary()
        .grants()
        .iter()
        .filter(|grant| grant.target.path() == broker)
        .collect();
    assert_eq!(socket_grants.len(), 1, "{socket_grants:?}");
    assert_eq!(
        socket_grants[0].target,
        beekeeper_acp::exec_boundary::GrantTarget::UnixSocket(broker.clone())
    );

    let connect = |socket: &Path| {
        run_in(
            &plan,
            &fx.seat_a,
            &format!("/usr/bin/nc -w 1 -U '{}' </dev/null", socket.display()),
        )
    };
    let reached = connect(&broker);
    assert!(reached.status.success(), "{reached:?}");
    let refused = connect(&other);
    assert!(!refused.status.success(), "{refused:?}");
    drop(proxy);
}
