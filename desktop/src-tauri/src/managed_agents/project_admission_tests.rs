//! Project-service admission of this computer's host key (ledger 266),
//! against the `agents_repo` stub relay. Split from `agents_repo_tests.rs`
//! for the file-size gate.
//!
//! The seam under test, `admit_host_to_project`, takes no provider state at
//! all: it runs whether the provider was started a moment ago or has been
//! running since app launch — run 8's shape, where the founding never
//! passed through a start path and the old start-hooked repair never ran.

use super::stub::*;
use super::*;
use crate::managed_agents::project_admission::{
    admit_host_to_project, host_can_read_project, HostAdmission, ADMITTED_BUT_UNREAD,
};
use std::sync::atomic::{AtomicUsize, Ordering};

/// A verification probe that must never run (nothing to verify).
async fn never() -> Result<bool, String> {
    Err("the host read must not run for this outcome".to_string())
}

/// (a) Run 8's exact shape: a private project, the provider already
/// serving, the host absent from the roster. RED on the base: the only
/// repair hung off `ensure_host_serving_project`'s provider start, and the
/// founding path of a running provider never reached it — the host stayed
/// off the roster and the autobind read 0 rows. GREEN: the host is put on
/// as a collaborator in one kind:9010, and `Admitted` is returned only
/// after a read of the project made with the host's own key succeeds.
#[tokio::test]
async fn a_private_project_without_the_host_admits_it_and_verifies_as_the_host() {
    let owner = Keys::generate();
    let host = Keys::generate();
    let host_hex = host.public_key().to_hex();
    let viewer = owner.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");
    let (relay_url, stored) = spawn_stub_relay(
        vec![private_project_head_json(&owner, "demo", "Demo Project")],
        None,
        None,
    )
    .await;
    let state = stubbed_state(relay_url, owner.clone()).await;
    let reads = AtomicUsize::new(0);

    let admission = admit_host_to_project(&state, &owner, &project, &host_hex, || {
        reads.fetch_add(1, Ordering::SeqCst);
        host_can_read_project(&state, &host, None, &viewer, "demo")
    })
    .await;

    assert_eq!(admission, HostAdmission::Admitted { already: false });
    assert_eq!(
        reads.load(Ordering::SeqCst),
        1,
        "verified once, as the host"
    );
    let puts = stored_of_kind(&stored, 9010);
    assert_eq!(puts.len(), 1, "exactly one 9010");
    assert_eq!(
        p_tags(&puts[0]),
        vec![vec![
            "p".to_string(),
            host_hex.clone(),
            String::new(),
            "collaborator".to_string(),
        ]]
    );
}

/// The roster names the host but its own read still comes back empty: that
/// is never reported as admitted.
#[tokio::test]
async fn an_admitted_host_that_cannot_read_the_project_is_unreadable() {
    let owner = Keys::generate();
    let host = "c".repeat(64);
    let viewer = owner.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");
    let (relay_url, stored) = spawn_stub_relay(
        vec![private_project_head_json(&owner, "demo", "Demo Project")],
        None,
        None,
    )
    .await;
    let state = stubbed_state(relay_url, owner.clone()).await;
    let admission =
        admit_host_to_project(&state, &owner, &project, &host, || async { Ok(false) }).await;
    assert_eq!(
        admission,
        HostAdmission::Unreadable {
            reason: ADMITTED_BUT_UNREAD.to_string()
        }
    );
    assert_eq!(
        stored_of_kind(&stored, 9010).len(),
        1,
        "the op still went out"
    );
}

/// (b) A public project's repositories are not roster-gated: `Public`,
/// nothing published, nothing to verify.
#[tokio::test]
async fn a_public_project_is_public_and_nothing_is_published() {
    let owner = Keys::generate();
    let viewer = owner.public_key().to_hex();
    let project = format!("30621:{viewer}:open");
    let (relay_url, stored) = spawn_stub_relay(
        vec![project_head_json(&owner, "open", "Open Project")],
        None,
        None,
    )
    .await;
    let state = stubbed_state(relay_url, owner.clone()).await;
    let admission = admit_host_to_project(&state, &owner, &project, &"c".repeat(64), never).await;
    assert_eq!(admission, HostAdmission::Public);
    assert!(stored_of_kind(&stored, 9010).is_empty());
}

/// (c) This computer's person is neither the creator nor a roster owner:
/// `Unauthorized` with the words, and no op is attempted.
#[tokio::test]
async fn a_key_that_may_not_add_members_is_unauthorized_and_publishes_nothing() {
    let creator = Keys::generate();
    let stranger = Keys::generate();
    let project = format!("30621:{}:demo", creator.public_key().to_hex());
    let (relay_url, stored) = spawn_stub_relay(
        vec![private_project_head_json(&creator, "demo", "Demo Project")],
        None,
        None,
    )
    .await;
    let state = stubbed_state(relay_url, stranger.clone()).await;
    let admission =
        admit_host_to_project(&state, &stranger, &project, &"c".repeat(64), never).await;
    match admission {
        HostAdmission::Unauthorized { reason } => {
            assert!(reason.contains("not a member"), "{reason}")
        }
        other => panic!("expected Unauthorized, got {other:?}"),
    }
    assert!(stored_of_kind(&stored, 9010).is_empty());
}

/// An owner removed this computer: it is not put back silently.
#[tokio::test]
async fn a_host_an_owner_removed_is_not_re_added() {
    let owner = Keys::generate();
    let host = "c".repeat(64);
    let viewer = owner.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");
    let removal =
        crate::managed_agents::project_roster::build_remove_member_event(&owner, &project, &host)
            .expect("builds");
    let (relay_url, stored) = spawn_stub_relay(
        vec![
            private_project_head_json(&owner, "demo", "Demo Project"),
            event_json(&removal),
        ],
        None,
        None,
    )
    .await;
    let state = stubbed_state(relay_url, owner.clone()).await;
    let admission = admit_host_to_project(&state, &owner, &project, &host, never).await;
    assert!(
        matches!(&admission, HostAdmission::Unauthorized { reason } if reason.contains("removed")),
        "{admission:?}"
    );
    assert!(stored_of_kind(&stored, 9010).is_empty());
}

/// (d) The relay cannot be reached, or refuses the op: `Unreadable` with
/// the relay's words — never `None`, never a silent success.
#[tokio::test]
async fn a_relay_failure_is_unreadable() {
    let owner = Keys::generate();
    let viewer = owner.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let dead_url = format!("http://{}", closed.local_addr().expect("addr"));
    drop(closed);
    let state = stubbed_state(dead_url, owner.clone()).await;
    let admission = admit_host_to_project(&state, &owner, &project, &"c".repeat(64), never).await;
    assert!(
        matches!(&admission, HostAdmission::Unreadable { reason } if reason.contains("project head")),
        "{admission:?}"
    );

    let (relay_url, stored) = spawn_stub_relay(
        vec![private_project_head_json(&owner, "demo", "Demo Project")],
        Some(9010),
        None,
    )
    .await;
    let state = stubbed_state(relay_url, owner.clone()).await;
    let admission = admit_host_to_project(&state, &owner, &project, &"c".repeat(64), never).await;
    assert!(
        matches!(&admission, HostAdmission::Unreadable { reason } if reason.contains("refused the roster update")),
        "{admission:?}"
    );
    assert!(stored_of_kind(&stored, 9010).is_empty());
}

/// (e) The host is already on the roster: nothing is published, and it is
/// still verified as the host before `Admitted { already: true }`.
#[tokio::test]
async fn a_host_already_on_the_roster_publishes_nothing() {
    let owner = Keys::generate();
    let host = Keys::generate();
    let host_hex = host.public_key().to_hex();
    let viewer = owner.public_key().to_hex();
    let project = format!("30621:{viewer}:demo");
    let (relay_url, stored) = spawn_stub_relay(
        vec![
            private_project_head_json(&owner, "demo", "Demo Project"),
            roster_projection_json(&project, &[(&host_hex, "collaborator")]),
        ],
        None,
        None,
    )
    .await;
    let state = stubbed_state(relay_url, owner.clone()).await;
    let admission = admit_host_to_project(&state, &owner, &project, &host_hex, || {
        host_can_read_project(&state, &host, None, &viewer, "demo")
    })
    .await;
    assert_eq!(admission, HostAdmission::Admitted { already: true });
    assert!(stored_of_kind(&stored, 9010).is_empty());
}
