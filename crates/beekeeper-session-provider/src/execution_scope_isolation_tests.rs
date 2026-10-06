//! The provider's session isolation settings ([`crate::session_isolation`]),
//! exercised through the production [`prepare`]: what the rendered scope
//! grants and exports with each setting and without it, what it discloses,
//! and — under the real boundary — which connections a session can open.

use std::ffi::OsString;

use super::tests::launched::run_in;
use super::tests::{fixture, inputs, Fixture};
use super::*;
use crate::execution_scope_git::TEST_OPERATOR_CONFIG;
use crate::session_isolation::{isolation_status_items, SessionIsolation};

fn prepared(plan: Result<ExecutionPlan, CreateFailure>) -> Box<PreparedExecution> {
    match plan {
        Ok(ExecutionPlan::Prepared(plan)) => plan,
        other => panic!("prepared: {other:?}"),
    }
}

fn env_value(plan: &PreparedExecution, name: &str) -> Option<String> {
    plan.launch.env().get(name).map(str::to_owned)
}

/// A disposable operator Git configuration naming a credential helper and a
/// key file, so the default and the withheld scope differ only by setting.
struct OperatorGit {
    key: PathBuf,
    helper: PathBuf,
}

fn operator_git(fx: &Fixture) -> OperatorGit {
    use std::os::unix::fs::PermissionsExt;
    let dir = fx.root.join("operator");
    std::fs::create_dir_all(&dir).expect("operator dir");
    let helper = dir.join("git-credential-fixture");
    std::fs::write(&helper, "#!/bin/sh\nexit 0\n").expect("helper");
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    let key = dir.join("operator.key");
    std::fs::write(&key, "nsec-fixture-not-a-key\n").expect("key");
    let config = dir.join("gitconfig");
    for (name, value) in [
        ("user.name", "Operator"),
        ("credential.helper", helper.to_str().expect("utf-8")),
        ("nostr.keyfile", key.to_str().expect("utf-8")),
    ] {
        let status = std::process::Command::new("git")
            .args(["config", "--file"])
            .arg(&config)
            .args(["--add", name, value])
            .status()
            .expect("git config");
        assert!(status.success());
    }
    TEST_OPERATOR_CONFIG.with(|file| *file.borrow_mut() = Some(config));
    OperatorGit { key, helper }
}

fn grants_path(plan: &PreparedExecution, path: &Path) -> bool {
    plan.launch
        .boundary()
        .grants()
        .iter()
        .any(|grant| grant.target.path() == path)
}

/// Unseated sessions get the operator's Git transport by default; with the
/// setting, none of it is staged, granted or exported, and the record and
/// transcript say so.
#[test]
fn withheld_operator_git_reaches_no_session_and_is_disclosed() {
    let fx = fixture();
    let operator = operator_git(&fx);
    let agent_sock = fx.seat_a.join("agent.sock");
    std::fs::write(&agent_sock, "").expect("stand-in agent socket");
    let ambient: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        ("SSH_AUTH_SOCK".into(), agent_sock.clone().into_os_string()),
    ];

    let mut open = inputs(&fx, "default", &fx.seat_a, &[], &[]);
    open.actor = None;
    open.ambient_env = Some(&ambient);
    let open = prepared(prepare(&open));
    assert!(grants_path(&open, &operator.key), "default stages the key");
    assert!(grants_path(&open, &operator.helper), "and the helper");
    assert_eq!(
        env_value(&open, "SSH_AUTH_SOCK").as_deref(),
        agent_sock.to_str()
    );
    let BoundaryState::Enforced { isolation, .. } = open.state() else {
        panic!("enforced");
    };
    assert_eq!(isolation, SessionIsolation::default());
    let recorded = recorded_state(&open.state()).expect("recorded");
    assert_eq!((recorded.operator_git, recorded.egress), (None, None));
    assert!(isolation_status_items(&open.state()).is_empty());

    let mut withheld = inputs(&fx, "withheld", &fx.seat_a, &[], &[]);
    withheld.actor = None;
    withheld.ambient_env = Some(&ambient);
    withheld.isolation.withhold_operator_git = true;
    let withheld = prepared(prepare(&withheld));
    assert!(!grants_path(&withheld, &operator.key));
    assert!(!grants_path(&withheld, &operator.helper));
    assert!(!withheld
        .launch
        .boundary()
        .grants()
        .iter()
        .any(|grant| grant.reason.contains("keychain") || grant.reason.contains("ssh agent")));
    assert_eq!(env_value(&withheld, "SSH_AUTH_SOCK"), None);
    let staged = env_value(&withheld, "GIT_CONFIG_GLOBAL").expect("staged config");
    let staged = std::fs::read_to_string(staged).expect("read staged");
    assert!(!staged.contains("credential"), "{staged}");
    assert!(!staged.contains("keyfile"), "{staged}");
    assert!(staged.contains("name = Operator"), "{staged}");
    // The child cannot read the key either, whatever it is told.
    let output = run_in(
        &withheld,
        &fx.seat_a,
        &format!("cat '{}'", operator.key.display()),
    );
    assert!(!output.status.success(), "{output:?}");

    let state = withheld.state();
    let recorded = recorded_state(&state).expect("recorded");
    assert_eq!(recorded.operator_git.as_deref(), Some("withheld"));
    assert_eq!(recorded.egress, None);
    let items = isolation_status_items(&state);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["status"], "operator_git_withheld");
    TEST_OPERATOR_CONFIG.with(|file| *file.borrow_mut() = None);
}

/// With an egress proxy, every proxy variable names it — overriding the
/// host's own — exemptions are emptied, the verified boundary carries the
/// rule, and from inside it only the proxy's port connects.
#[test]
fn an_egress_proxy_confines_the_session_and_is_disclosed() {
    let fx = fixture();
    let proxy = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("proxy");
    let other = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("other");
    let proxy_addr = proxy.local_addr().expect("addr");
    let other_port = other.local_addr().expect("addr").port();
    let ambient: Vec<(OsString, OsString)> = vec![
        ("PATH".into(), "/usr/bin:/bin".into()),
        (
            "HTTPS_PROXY".into(),
            "http://corp-proxy.invalid:3128".into(),
        ),
        ("NO_PROXY".into(), "*".into()),
    ];
    let mut confined = inputs(&fx, "egress", &fx.seat_a, &[], &[]);
    confined.ambient_env = Some(&ambient);
    confined.isolation.egress =
        beekeeper_acp::exec_boundary::Egress::loopback_proxy(proxy_addr).expect("loopback");
    let plan = prepared(prepare(&confined));
    let url = format!("http://{proxy_addr}");
    for name in crate::session_isolation::PROXY_VARS {
        assert_eq!(
            env_value(&plan, name).as_deref(),
            Some(url.as_str()),
            "{name}"
        );
    }
    for name in crate::session_isolation::NO_PROXY_VARS {
        assert_eq!(env_value(&plan, name).as_deref(), Some(""), "{name}");
    }
    assert_eq!(plan.launch.boundary().egress().proxy(), Some(proxy_addr));
    let state = plan.state();
    assert_eq!(
        recorded_state(&state).and_then(|r| r.egress).as_deref(),
        Some("loopback-proxy")
    );
    let items = isolation_status_items(&state);
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["status"], "network_egress_proxy_only");

    let connect = |port: u16| {
        run_in(
            &plan,
            &fx.seat_a,
            &format!("/bin/bash -c 'exec 3<>/dev/tcp/127.0.0.1/{port}'"),
        )
    };
    let allowed = connect(proxy_addr.port());
    assert!(allowed.status.success(), "{allowed:?}");
    let refused = connect(other_port);
    assert!(!refused.status.success());
    assert!(
        String::from_utf8_lossy(&refused.stderr).contains("Operation not permitted"),
        "{refused:?}"
    );

    // Host commands keep their own network: the setting is for sessions.
    let mut host = inputs(&fx, "egress-host", &fx.seat_a, &[], &[]);
    host.purpose = ScopePurpose::HostCommand;
    host.actor = None;
    host.isolation = confined.isolation;
    let host = prepared(prepare(&host));
    assert_eq!(host.launch.boundary().egress().proxy(), None);
    drop((proxy, other));
}

/// Without either setting the policy and environment are what they were:
/// no network rule, no proxy variables the host did not have.
#[test]
fn absent_settings_leave_the_scope_unchanged() {
    let fx = fixture();
    let ambient: Vec<(OsString, OsString)> = vec![("PATH".into(), "/usr/bin:/bin".into())];
    let mut plain = inputs(&fx, "plain", &fx.seat_a, &[], &[]);
    plain.ambient_env = Some(&ambient);
    let plan = prepared(prepare(&plain));
    assert_eq!(plan.launch.boundary().egress().proxy(), None);
    for name in crate::session_isolation::PROXY_VARS {
        assert_eq!(env_value(&plan, name), None, "{name}");
    }
    assert!(!plan.operator_git_withheld);
}
