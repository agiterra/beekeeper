//! SV-56 / D9 acceptance for the provider: "Off produces no title on a new
//! session; the default titles one with nothing configured." This
//! computer's session-title mode is read before a job starts and again just
//! before it signs; only `agent` (or no file) publishes a 44252, and the
//! host-wide `BUZZ_CSP_AUTO_TITLE=off` wins over every mode.

use std::time::Duration;

use nostr::Keys;

use super::*;
use crate::session_title_mode::{write as write_mode, SessionTitleMode};

// The SV-31 tests keep their copy private; this module loads its own rather
// than widen theirs.
#[allow(dead_code, clippy::duplicate_mod)]
#[path = "tests/auto_title_stub_adapter.rs"]
mod stub;

use stub::{stub_adapter, StubPrompt};

const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CREATE_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn job(founder: &Keys) -> TitleJob {
    TitleJob {
        channel_id: Uuid::new_v4(),
        session_ref: SESSION_REF.to_owned(),
        founder: founder.public_key().to_hex(),
        target: CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "8f14e45f-ceea-4672-9b5c-2b9a3a6f0d11".into(),
            generation: 1,
        },
        first_message: "Fix the flaky login test in the desktop app".into(),
        attachments: Vec::new(),
        source_command: None,
        create_event_id: CREATE_ID.into(),
    }
}

fn fast() -> RetryPolicy {
    RetryPolicy {
        attempt_timeout: Duration::from_secs(20),
        retries: 0,
        backoff_base: Duration::from_millis(10),
    }
}

/// Nothing names the session, every time it is asked.
struct Unnamed;

impl TitlePreflight for Unnamed {
    async fn already_named(&self, _job: &TitleJob) -> Result<bool, String> {
        Ok(false)
    }
}

/// A generator that answers with a title, first doing `during` to the mode
/// file — the person changing their mind while the model is thinking.
struct ChangesMindGenerator {
    state_dir: PathBuf,
    during: Option<SessionTitleMode>,
    corrupt: bool,
}

impl TitleGenerator for ChangesMindGenerator {
    async fn generate(&self, _message: &str) -> Result<Option<GeneratedTitle>, String> {
        if let Some(mode) = self.during {
            write_mode(&self.state_dir, mode)?;
        }
        if self.corrupt {
            std::fs::write(self.state_dir.join(SESSION_TITLE_MODE_FILE), "{")
                .map_err(|error| error.to_string())?;
        }
        Ok(Some(GeneratedTitle {
            title: "Fix flaky login test".into(),
            model: "claude-haiku-4-5".into(),
        }))
    }
}

async fn run_with_mind_change(
    state_dir: &Path,
    during: Option<SessionTitleMode>,
    corrupt: bool,
) -> TitleOutcome {
    let generator = ChangesMindGenerator {
        state_dir: state_dir.to_path_buf(),
        during,
        corrupt,
    };
    run_title_job_in(
        Some(state_dir),
        &job(&Keys::generate()),
        &Unnamed,
        &generator,
        fast(),
        &Semaphore::new(1),
        &Keys::generate(),
    )
    .await
}

#[tokio::test]
async fn flipping_to_off_while_the_title_is_generated_drops_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_with_mind_change(dir.path(), Some(SessionTitleMode::Off), false).await;
    assert!(
        matches!(outcome, TitleOutcome::ModeDeclined(SessionTitleMode::Off)),
        "{outcome:?}"
    );
    let outcome = run_with_mind_change(dir.path(), Some(SessionTitleMode::MyModel), false).await;
    assert!(
        matches!(
            outcome,
            TitleOutcome::ModeDeclined(SessionTitleMode::MyModel)
        ),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn a_mode_file_broken_mid_generation_fails_closed() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_with_mind_change(dir.path(), None, true).await;
    assert!(
        matches!(outcome, TitleOutcome::ModeUnreadable(_)),
        "{outcome:?}"
    );
}

#[tokio::test]
async fn agent_or_no_file_still_signs_the_title() {
    let dir = tempfile::tempdir().expect("tempdir");
    let outcome = run_with_mind_change(dir.path(), None, false).await;
    assert!(matches!(outcome, TitleOutcome::Ready(_)), "{outcome:?}");
    let outcome = run_with_mind_change(dir.path(), Some(SessionTitleMode::Agent), false).await;
    assert!(matches!(outcome, TitleOutcome::Ready(_)), "{outcome:?}");
}

/// A detached job under Off publishes nothing back to the loop.
#[tokio::test]
async fn a_job_under_off_queues_nothing_for_the_outbox() {
    for mode in [SessionTitleMode::Off, SessionTitleMode::MyModel] {
        let dir = tempfile::tempdir().expect("tempdir");
        write_mode(dir.path(), mode).expect("mode");
        let mut titler = AutoTitler::new(true).with_policy(fast());
        let mut ready = titler.take_ready().expect("queue");
        titler.start_in(
            Some(dir.path().to_path_buf()),
            "session-1".into(),
            job(&Keys::generate()),
            Unnamed,
            ChangesMindGenerator {
                state_dir: dir.path().to_path_buf(),
                during: None,
                corrupt: false,
            },
            Keys::generate(),
        );
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while titler.running() > 0 && std::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(titler.running(), 0, "the job finished");
        assert!(ready.try_recv().is_err(), "{mode:?} must publish nothing");
    }
}

/// The real spawn against the stub adapter, with the mode file saying
/// `agent`: SV-31 behaviour is unchanged.
#[tokio::test]
async fn the_stub_adapter_still_titles_under_agent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).expect("state");
    write_mode(&state, SessionTitleMode::Agent).expect("mode");
    let script = stub_adapter(
        dir.path(),
        "titler.sh",
        StubPrompt::Reply("Title: Fix flaky login test."),
    );
    let generator = AcpTitleGenerator {
        state_dir: state.clone(),
        driver: "claude-agent-acp".into(),
        runtime: "claude".into(),
        agent_command: "bash".into(),
        agent_args: vec![script],
        cli_env: Vec::new(),
        model: "haiku".into(),
        runtime_profile_override: Some(RuntimeProfile::TestDouble),
    };
    let outcome = run_title_job_in(
        Some(&state),
        &job(&Keys::generate()),
        &Unnamed,
        &generator,
        fast(),
        &Semaphore::new(1),
        &Keys::generate(),
    )
    .await;
    let TitleOutcome::Ready(event) = outcome else {
        panic!("expected a title, got {outcome:?}");
    };
    assert_eq!(
        u32::from(event.kind.as_u16()),
        KIND_CODING_SESSION_GENERATED_TITLE
    );
}

// --- The provider's trigger ------------------------------------------------

fn test_provider(state_dir: &Path, auto_title: &str) -> crate::Provider {
    let state = state_dir.to_string_lossy().into_owned();
    let auto_title = auto_title.to_owned();
    let config = crate::config::Config::from_lookup(move |name| match name {
        "BUZZ_PRIVATE_KEY" => {
            Some("0000000000000000000000000000000000000000000000000000000000000001".into())
        }
        "BUZZ_RELAY_URL" => Some("ws://127.0.0.1:9".into()),
        "BUZZ_CSP_STATE_DIR" => Some(state.clone()),
        "BUZZ_CSP_AUTO_TITLE" => Some(auto_title.clone()),
        _ => None,
    })
    .expect("config");
    let mut provider = crate::Provider::new(config).expect("provider");
    provider.set_rest_client(RestClient {
        http: reqwest::Client::new(),
        // Nothing listens here: a started job's preflight fails.
        base_url: "http://127.0.0.1:9".into(),
        keys: Keys::generate(),
        auth_tag_json: None,
    });
    provider
}

fn record(founder: &str) -> crate::state::SessionRecord {
    crate::state::SessionRecord {
        execution_binding: None,
        execution_boundary: None,
        authority_withdrawn: None,
        project_head_seen_at: None,
        session_id: "session-1".into(),
        generation: 1,
        channel_id: Uuid::new_v4(),
        command_id: CREATE_ID.into(),
        generation_command_id: None,
        provider_instance_ref: "claude-primary".into(),
        runtime: "claude".into(),
        driver: "claude-agent-acp".into(),
        cwd: std::path::PathBuf::from("/tmp"),
        project_ref: None,
        repo_ref: None,
        session_ref: Some(SESSION_REF.into()),
        genesis_ref: None,
        actor: None,
        role: None,
        founder_pubkey: Some(founder.to_owned()),
        granted_operators: Default::default(),
        granted_viewers: Default::default(),
        authority_seq: 0,
        model: None,
        model_requested: None,
        model_effective: None,
        routing: None,
        resume_cursor: None,
        title: None,
        created_at_ms: 1_700_000_000_000,
        next_seq: 1,
        next_lease_sequence: 1,
        bootstrap_transport: None,
        open_turn: None,
        closed: false,
        created_by: Some(founder.to_owned()),
        handover: beekeeper_core::coding_session_authority_claim::ClaimState::NoClaim,
        retired: None,
        pack_ref: None,
        compose_ref: None,
    }
}

/// Start the founder's first turn on a provider whose state directory holds
/// `mode` (or no file), with the host switch `env`; return whether a job is
/// running and whether the umbrella was decided.
async fn first_turn(mode: Option<&str>, env: &str) -> (usize, bool) {
    let dir = tempfile::tempdir().expect("tempdir");
    if let Some(body) = mode {
        std::fs::write(dir.path().join(SESSION_TITLE_MODE_FILE), body).expect("mode");
    }
    let founder = "ab".repeat(32);
    let mut provider = test_provider(dir.path(), env);
    provider
        .state
        .insert_session(record(&founder))
        .expect("record");
    provider.maybe_start_auto_title(
        "session-1",
        CREATE_ID,
        "Fix the login test",
        &[],
        Some(&founder),
    );
    let running = provider.auto_title.running();
    let decided = provider.auto_title.started.contains(SESSION_REF);
    assert!(!provider
        .outbox
        .pending_events()
        .any(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_GENERATED_TITLE));
    (running, decided)
}

#[tokio::test]
async fn off_and_my_model_start_no_job_and_decide_once() {
    for body in [
        r#"{"version":1,"mode":"off"}"#,
        r#"{"version":1,"mode":"my-model"}"#,
    ] {
        let (running, decided) = first_turn(Some(body), "on").await;
        assert_eq!(running, 0, "{body} must start no job");
        assert!(decided, "{body} decides the umbrella, so it is logged once");
    }
}

#[tokio::test]
async fn an_unreadable_mode_file_starts_no_job() {
    for body in ["not json", r#"{"version":2,"mode":"agent"}"#] {
        let (running, decided) = first_turn(Some(body), "on").await;
        assert_eq!(running, 0, "{body} must fail closed");
        assert!(decided);
    }
}

#[tokio::test]
async fn the_default_titles_with_nothing_configured() {
    let (running, decided) = first_turn(None, "on").await;
    assert_eq!(running, 1, "no file is mode agent: a job starts");
    assert!(decided);
    let (running, _) = first_turn(Some(r#"{"version":1,"mode":"agent"}"#), "on").await;
    assert_eq!(running, 1);
}

#[tokio::test]
async fn the_host_switch_wins_over_a_file_saying_agent() {
    let (running, decided) = first_turn(Some(r#"{"version":1,"mode":"agent"}"#), "off").await;
    assert_eq!(running, 0);
    assert!(!decided, "the host switch stops it before the mode is read");
}
