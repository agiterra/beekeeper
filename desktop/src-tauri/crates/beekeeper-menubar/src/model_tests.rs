//! The honesty rules, as tests. Every one of these is a sentence that must or
//! must not appear over a given state.

use super::*;
use beekeeper_host::activity::PushedActivity;
use beekeeper_host::protocol::Warning;
use beekeeper_host::sessions::{SessionRow, SessionSnapshot};
use beekeeper_host::state::{LockOwnerKind, RelayConnectionState};

const NOW: i64 = 1_750_000_060_000;

fn status(provider: ProviderChildState) -> Box<Status> {
    Box::new(Status {
        protocol_version: beekeeper_host::protocol::PROTOCOL_VERSION,
        host_version: "0.1.0".into(),
        host_pid: 42,
        host_started_at: "2026-09-30T00:00:00Z".into(),
        relay_url: "wss://hive.example.org".into(),
        provider_pubkey: "a".repeat(64),
        provider_state_dir: std::path::PathBuf::from("/data/session-provider/aaaa"),
        provider,
        provider_settings_in_force: None,
        relay_connection: RelayConnectionState::unknowable(),
        // Zero, and the tests that add rows set it from the rows they add:
        // a fixture where the count disagrees with the rows would be
        // describing a status the host cannot produce.
        turns_in_flight: 0,
        sessions: SessionSnapshot {
            read_at: "2026-09-30T00:00:00Z".into(),
            unavailable: None,
            sessions: Vec::new(),
        },
        app_activity: Vec::new(),
        app_activity_leased: false,
        warnings: Vec::new(),
    })
}

/// Put rows on a status *and* keep `turns_in_flight` in step with them.
///
/// The host derives that count from the rows when it answers, so a fixture
/// that sets one without the other describes a status the host cannot produce
/// — and a menu asserted against it would be asserted against fiction.
fn with_sessions(status: &mut Status, rows: Vec<SessionRow>) {
    status.turns_in_flight = rows
        .iter()
        .filter(|row| row.turn_started_at_ms.is_some())
        .count();
    status.sessions.sessions = rows;
}

fn live() -> ProviderChildState {
    ProviderChildState::Live {
        pid: 4242,
        started_at: "2026-09-30T00:00:00Z".into(),
    }
}

fn session(id: &str, role: Option<&str>, started_at_ms: Option<i64>) -> SessionRow {
    SessionRow {
        session_id: id.into(),
        generation: 1,
        channel_id: Some("11111111-1111-1111-1111-111111111111".into()),
        runtime: Some("claude".into()),
        actor: Some("b".repeat(64)),
        role: role.map(str::to_string),
        turn_started_at_ms: started_at_ms,
    }
}

/// Rule 1, the load-bearing one. The desktop tray said "No agents are running"
/// whenever its list was empty, which was fine while the app *was* the agent
/// manager. This process usually cannot make that claim.
#[test]
fn an_unreachable_host_never_claims_anything_about_agents() {
    for installed in [true, false] {
        let model = model(
            &HostView::Unreachable {
                installed,
                reason: "the agent host is not running (nothing is listening on …)".into(),
            },
            NOW,
        );
        let text = format!("{} {}", model.header, model.notices.join(" "));
        assert!(
            !text.to_lowercase().contains("no agents are running"),
            "{text}"
        );
        assert!(!text.to_lowercase().contains("idle"), "{text}");
        assert!(model.running.is_empty() && model.recent.is_empty());
        assert!(!model.host_reachable, "controls must not be offered");
        assert!(!model.provider_live);
    }
}

/// Rule 2. The socket's absence cannot tell these apart; the registration can,
/// and telling somebody to start something they never installed is worse than
/// saying nothing.
#[test]
fn not_installed_and_not_running_are_different_sentences() {
    let absent = model(
        &HostView::Unreachable {
            installed: false,
            reason: "r".into(),
        },
        NOW,
    );
    let stopped = model(
        &HostView::Unreachable {
            installed: true,
            reason: "r".into(),
        },
        NOW,
    );
    assert_ne!(absent.header, stopped.header);
    assert!(absent.header.contains("not installed"), "{}", absent.header);
    assert!(
        stopped.header.contains("installed, not running"),
        "{}",
        stopped.header
    );
}

/// Rule 3: the elapsed time is computed here, from the absolute start the host
/// sent. Sixty seconds after the turn began, the row says a minute.
#[test]
fn elapsed_is_computed_from_the_absolute_start() {
    let mut status = status(live());
    with_sessions(
        &mut status,
        vec![session("s-1", Some("lead"), Some(NOW - 192_000))],
    );
    let model = model(&HostView::Reachable(status), NOW);
    assert_eq!(model.running.len(), 1);
    assert_eq!(model.running[0].elapsed, "3m 12s");
    // And the same model at a later instant says something later, without the
    // host being asked again — which is why the wire carries no duration.
    let mut status = status_with_turn(NOW - 192_000);
    status.app_activity_leased = true;
    let later = model_at(status, NOW + 60_000);
    assert_eq!(later.running[0].elapsed, "4m 12s");
}

fn status_with_turn(started_at_ms: i64) -> Box<Status> {
    let mut status = status(live());
    with_sessions(
        &mut status,
        vec![session("s-1", Some("lead"), Some(started_at_ms))],
    );
    status
}

fn model_at(status: Box<Status>, now_ms: i64) -> MenuModel {
    model(&HostView::Reachable(status), now_ms)
}

/// A clock that disagrees, or a snapshot read across a second boundary, must
/// not produce a negative duration or a panic.
#[test]
fn a_start_in_the_future_reads_as_zero() {
    let model = model_at(status_with_turn(NOW + 5_000), NOW);
    assert_eq!(model.running[0].elapsed, "0s");
}

/// A session with no open turn is not work in flight. Showing it with a
/// ticking clock would be a lie about what the machine is doing.
#[test]
fn a_session_with_no_open_turn_is_not_a_row() {
    let mut status = status(live());
    with_sessions(
        &mut status,
        vec![
            session("s-1", Some("lead"), None),
            session("s-2", Some("builder"), Some(NOW - 1_000)),
        ],
    );
    let model = model(&HostView::Reachable(status), NOW);
    assert_eq!(model.running.len(), 1);
    assert_eq!(model.running[0].agent_name, "builder");
}

/// A coding-session row names the seat's role, then its runtime, then nothing
/// invented. It deliberately does **not** name a channel: the host reads a
/// channel *id* out of the provider's state file and names live on the relay,
/// which this process does not talk to.
#[test]
fn a_session_row_names_what_is_knowable_here() {
    let mut status = status(live());
    with_sessions(
        &mut status,
        vec![
            session("s-1", Some("lead"), Some(NOW)),
            session("s-2", None, Some(NOW)),
            SessionRow {
                runtime: None,
                ..session("s-3", None, Some(NOW))
            },
        ],
    );
    let model = model(&HostView::Reachable(status), NOW);
    let names: Vec<&str> = model
        .running
        .iter()
        .map(|row| row.agent_name.as_str())
        .collect();
    assert_eq!(names, vec!["lead", "claude", "session"]);
    // The channel column is a short id, not a made-up name.
    assert_eq!(model.running[0].channel_name, "11111111");
}

/// Rule 4. Every state the host refuses in carries its own sentence, which
/// names a cause and a fix; a category invented here would lose both.
#[test]
fn a_refusing_host_gets_its_own_words_in_the_header() {
    let refusals = [
        ProviderChildState::KeyUnresolved {
            reason: beekeeper_host::identity::KeyUnresolved::NotFound {
                tried: vec!["BEEKEEPER_HOST_PRIVATE_KEY is not set".into()],
            },
        },
        ProviderChildState::GaveUp {
            failures: 5,
            at: "2026-09-30T00:10:00Z".into(),
        },
        ProviderChildState::LockHeldElsewhere {
            pid: Some(7),
            kind: LockOwnerKind::AnotherHost,
        },
    ];
    for refusal in refusals {
        let expected = refusal.message();
        let model = model(&HostView::Reachable(status(refusal.clone())), NOW);
        assert!(
            model.header.contains(&expected),
            "{refusal:?}: header {:?} does not carry {expected:?}",
            model.header
        );
        // Precisely: the header *is* the host's sentence, not a category
        // beside it. A bare label like "stopped" would lose both the cause
        // and the fix — `GaveUp`'s own message says "stopped trying … restart
        // it to try again", which is the specific version and is fine.
        assert_eq!(model.header, format!("Agent host: {expected}"));
        assert!(
            model.header.len() > "Agent host: stopped".len(),
            "a refusal's header must carry more than a category: {}",
            model.header
        );
        assert!(model.host_reachable, "the host answered, so controls work");
        assert!(!model.provider_live);
    }
}

/// A backoff is the host working, and says which attempt — so a person can see
/// it climbing rather than wonder whether anything is happening.
#[test]
fn a_backoff_says_which_attempt() {
    let model = model(
        &HostView::Reachable(status(ProviderChildState::Backoff {
            failures: 3,
            next_at: "2026-09-30T00:00:08Z".into(),
        })),
        NOW,
    );
    assert_eq!(model.header, "Agent host: restarting (attempt 3)");
}

/// Counts are singular where they should be, because "1 agents" is the kind of
/// detail that makes a person distrust everything else on the menu.
#[test]
fn the_header_counts_agents_and_says_idle_for_none() {
    let mut status = status(live());
    assert_eq!(
        model(&HostView::Reachable(status.clone()), NOW).header,
        "Agent host: running · idle"
    );
    with_sessions(&mut status, vec![session("s-1", Some("lead"), Some(NOW))]);
    assert_eq!(
        model(&HostView::Reachable(status.clone()), NOW).header,
        "Agent host: running · 1 agent"
    );
    status
        .sessions
        .sessions
        .push(session("s-2", Some("b"), Some(NOW)));
    assert_eq!(
        model(&HostView::Reachable(status), NOW).header,
        "Agent host: running · 2 agents"
    );
}

/// The app's rows appear while it holds a lease, split into running and
/// recent, and its absence is said out loud — a machine with agents under a
/// closed app looks identical to one with none.
#[test]
fn app_rows_are_shown_under_lease_and_their_absence_is_disclosed() {
    let mut status = status(live());
    status.app_activity_leased = true;
    status.app_activity = vec![
        PushedActivity {
            activity_id: "running".into(),
            agent_name: "Scout".into(),
            agent_pubkey: "c".repeat(64),
            channel_id: "chan".into(),
            channel_name: "planning".into(),
            started_at_ms: NOW - 60_000,
            recent: false,
        },
        PushedActivity {
            activity_id: "done".into(),
            agent_name: "Reviewer".into(),
            agent_pubkey: "d".repeat(64),
            channel_id: "chan".into(),
            channel_name: "design".into(),
            started_at_ms: NOW - 265_000,
            recent: true,
        },
    ];
    let with_app = model(&HostView::Reachable(status), NOW);
    assert_eq!(with_app.running.len(), 1);
    assert_eq!(with_app.recent.len(), 1);
    assert_eq!(with_app.running[0].elapsed, "1m 0s");
    assert_eq!(with_app.recent[0].elapsed, "4m 25s");
    assert!(
        !with_app
            .notices
            .iter()
            .any(|notice| notice.contains("Beekeeper is not running")),
        "{:?}",
        with_app.notices
    );

    let without_app = model(&HostView::Reachable(status_with_turn(NOW)), NOW);
    assert!(
        without_app
            .notices
            .iter()
            .any(|notice| notice.contains("Beekeeper is not running")),
        "{:?}",
        without_app.notices
    );
}

/// The host's warnings are the host's; they reach the menu verbatim.
#[test]
fn host_warnings_become_notices() {
    let mut status = status(live());
    status.warnings = vec![Warning::seat_restage_requires_desktop(3)];
    status.sessions.unavailable = Some("the provider has not written state.json yet".into());
    let model = model(&HostView::Reachable(status), NOW);
    assert!(model
        .notices
        .iter()
        .any(|notice| notice.contains("waiting to be re-staged")));
    assert!(model
        .notices
        .iter()
        .any(|notice| notice.contains("has not written state.json")));
}
