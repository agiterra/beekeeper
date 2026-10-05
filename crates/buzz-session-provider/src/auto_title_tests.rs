//! SV-31 acceptance for the host producer, against fakes for the pure job
//! logic and against a stub ACP adapter process for the spawn.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use buzz_core::coding_session_title::{
    parse_coding_session_title_content, validate_coding_session_title_envelope,
    CodingSessionTitleBasis,
};
use nostr::{Keys, Tag};

use super::*;

#[path = "tests/auto_title_stub_adapter.rs"]
mod stub;

use stub::{stub_adapter, stub_adapter_with, StubPrompt};

const SESSION_REF: &str = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CREATE_ID: &str = "1111111111111111111111111111111111111111111111111111111111111111";
const TURN_ID: &str = "2222222222222222222222222222222222222222222222222222222222222222";

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
        attempt_timeout: Duration::from_millis(200),
        retries: 2,
        backoff_base: Duration::from_millis(10),
    }
}

/// A preflight that answers from a script: one answer per call, the last
/// repeated.
struct ScriptedPreflight {
    answers: Vec<Result<bool, String>>,
    calls: AtomicU32,
}

impl ScriptedPreflight {
    fn new(answers: Vec<Result<bool, String>>) -> Self {
        Self {
            answers,
            calls: AtomicU32::new(0),
        }
    }
}

impl TitlePreflight for ScriptedPreflight {
    async fn already_named(&self, _job: &TitleJob) -> Result<bool, String> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst) as usize;
        self.answers[call.min(self.answers.len() - 1)].clone()
    }
}

#[derive(Clone, Copy)]
enum Answer {
    Title,
    Nothing,
    Fail,
    Hang,
}

struct FakeGenerator {
    answer: Answer,
    calls: Arc<AtomicU32>,
}

impl FakeGenerator {
    fn new(answer: Answer) -> (Self, Arc<AtomicU32>) {
        let calls = Arc::new(AtomicU32::new(0));
        (
            Self {
                answer,
                calls: Arc::clone(&calls),
            },
            calls,
        )
    }
}

impl TitleGenerator for FakeGenerator {
    async fn generate(&self, message: &str) -> Result<Option<GeneratedTitle>, String> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(
            message.chars().count() <= MAX_TITLE_MESSAGE_CHARS + MAX_ATTACHMENT_SECTION_CHARS + 64
        );
        match self.answer {
            Answer::Title => Ok(Some(GeneratedTitle {
                title: "Fix flaky login test".into(),
                model: "claude-haiku-4-5".into(),
            })),
            Answer::Nothing => Ok(None),
            Answer::Fail => Err("adapter exited".into()),
            Answer::Hang => std::future::pending().await,
        }
    }
}

fn tag_value(event: &Event, name: &str) -> Option<String> {
    event
        .tags
        .iter()
        .map(Tag::as_slice)
        .find(|tag| tag.first().map(String::as_str) == Some(name))
        .and_then(|tag| tag.get(1).cloned())
}

#[test]
fn a_long_first_message_keeps_its_head_and_tail_within_the_budget() {
    assert_eq!(limit_title_message("short", 2_000), "short");
    let message = format!("{}{}", "a".repeat(3_000), "z".repeat(3_000));
    let limited = limit_title_message(&message, MAX_TITLE_MESSAGE_CHARS);
    assert_eq!(limited.chars().count(), MAX_TITLE_MESSAGE_CHARS);
    assert!(limited.starts_with('a'));
    assert!(limited.ends_with('z'));
    assert!(limited.contains("[Content truncated]"));
    // Characters, not bytes: a multi-byte message is cut on a boundary.
    let wide = "é".repeat(5_000);
    assert_eq!(
        limit_title_message(&wide, MAX_TITLE_MESSAGE_CHARS)
            .chars()
            .count(),
        MAX_TITLE_MESSAGE_CHARS
    );
    assert_eq!(limit_title_message(&message, 5), "");
}

#[test]
fn only_the_founders_first_turn_of_generation_one_is_titled() {
    let founder = "ab".repeat(32);
    let base = TriggerFacts {
        generation: 1,
        turns_used_before: 0,
        operator: Some(&founder),
        founder: Some(&founder),
        text: "Fix the login test",
        attachments: 0,
        already_started: false,
    };
    assert!(is_title_turn(&base));
    // A pasted screenshot with no words is still the founder's first message.
    assert!(is_title_turn(&TriggerFacts {
        text: "",
        attachments: 1,
        ..base
    }));
    let stranger = "cd".repeat(32);
    for (label, facts) in [
        (
            "another operator",
            TriggerFacts {
                operator: Some(&stranger),
                ..base
            },
        ),
        (
            "an unknown sender",
            TriggerFacts {
                operator: None,
                ..base
            },
        ),
        (
            "no recorded founder",
            TriggerFacts {
                founder: None,
                ..base
            },
        ),
        (
            "generation 2",
            TriggerFacts {
                generation: 2,
                ..base
            },
        ),
        (
            "a second turn",
            TriggerFacts {
                turns_used_before: 1,
                ..base
            },
        ),
        (
            "an umbrella already started",
            TriggerFacts {
                already_started: true,
                ..base
            },
        ),
        (
            "a blank message",
            TriggerFacts {
                text: "  \n",
                ..base
            },
        ),
    ] {
        assert!(!is_title_turn(&facts), "{label} must not be titled");
    }
}

#[tokio::test]
async fn a_named_session_spawns_nothing() {
    let keys = Keys::generate();
    let founder = Keys::generate();
    let preflight = ScriptedPreflight::new(vec![Ok(true)]);
    let (generator, calls) = FakeGenerator::new(Answer::Title);
    let outcome = run_title_job(
        &job(&founder),
        &preflight,
        &generator,
        fast(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    assert!(matches!(outcome, TitleOutcome::AlreadyNamed), "{outcome:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn a_name_that_lands_mid_generation_drops_the_title_unsigned() {
    let keys = Keys::generate();
    let founder = Keys::generate();
    let preflight = ScriptedPreflight::new(vec![Ok(false), Ok(true)]);
    let (generator, calls) = FakeGenerator::new(Answer::Title);
    let outcome = run_title_job(
        &job(&founder),
        &preflight,
        &generator,
        fast(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    assert!(
        matches!(outcome, TitleOutcome::NamedMeanwhile),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(preflight.calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn a_preflight_that_cannot_answer_publishes_nothing() {
    let keys = Keys::generate();
    let founder = Keys::generate();
    let (generator, calls) = FakeGenerator::new(Answer::Title);
    let outcome = run_title_job(
        &job(&founder),
        &ScriptedPreflight::new(vec![Err("relay down".into())]),
        &generator,
        fast(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    assert!(matches!(outcome, TitleOutcome::Failed(_)), "{outcome:?}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);

    let (generator, _) = FakeGenerator::new(Answer::Title);
    let outcome = run_title_job(
        &job(&founder),
        &ScriptedPreflight::new(vec![Ok(false), Err("relay down".into())]),
        &generator,
        fast(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    assert!(
        matches!(&outcome, TitleOutcome::Failed(error) if error.contains("recheck")),
        "{outcome:?}"
    );
}

/// Two retries, backing off 2 s then 4 s, exactly as T3's initial title does.
#[tokio::test(start_paused = true)]
async fn failures_retry_twice_with_exponential_backoff_from_two_seconds() {
    let keys = Keys::generate();
    let founder = Keys::generate();
    let (generator, calls) = FakeGenerator::new(Answer::Fail);
    let started = tokio::time::Instant::now();
    let outcome = run_title_job(
        &job(&founder),
        &ScriptedPreflight::new(vec![Ok(false)]),
        &generator,
        RetryPolicy::default(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    assert!(
        matches!(&outcome, TitleOutcome::Failed(error) if error.contains("3 attempt(s)")),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1 + RETRIES);
    assert_eq!(started.elapsed(), Duration::from_secs(2 + 4));
}

/// Each attempt has its own 30 s ceiling; a hung runtime costs three of them
/// plus the backoff, and then nothing is published.
#[tokio::test(start_paused = true)]
async fn a_hung_runtime_times_out_each_attempt() {
    let keys = Keys::generate();
    let founder = Keys::generate();
    let (generator, calls) = FakeGenerator::new(Answer::Hang);
    let started = tokio::time::Instant::now();
    let outcome = run_title_job(
        &job(&founder),
        &ScriptedPreflight::new(vec![Ok(false)]),
        &generator,
        RetryPolicy::default(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    assert!(
        matches!(&outcome, TitleOutcome::Failed(error) if error.contains("timed out")),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    assert_eq!(started.elapsed(), Duration::from_secs(3 * 30 + 2 + 4));
}

#[tokio::test]
async fn an_answer_that_names_nothing_is_not_retried() {
    let keys = Keys::generate();
    let founder = Keys::generate();
    let (generator, calls) = FakeGenerator::new(Answer::Nothing);
    let outcome = run_title_job(
        &job(&founder),
        &ScriptedPreflight::new(vec![Ok(false)]),
        &generator,
        fast(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    assert!(
        matches!(outcome, TitleOutcome::NothingUsable),
        "{outcome:?}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

/// The published event is the provider's own 44252 in the exact S1 envelope:
/// `sourceCommand` null for a create's initial turn, `createEventId`, and the
/// signer's execution in `cs-target`.
#[tokio::test]
async fn a_title_is_a_provider_signed_44252_in_the_s1_envelope() {
    let keys = Keys::generate();
    let founder = Keys::generate();
    let job = job(&founder);
    let (generator, _) = FakeGenerator::new(Answer::Title);
    let outcome = run_title_job(
        &job,
        &ScriptedPreflight::new(vec![Ok(false)]),
        &generator,
        fast(),
        &Semaphore::new(1),
        &keys,
    )
    .await;
    let TitleOutcome::Ready(event) = outcome else {
        panic!("expected a title, got {outcome:?}");
    };
    assert_eq!(
        u32::from(event.kind.as_u16()),
        KIND_CODING_SESSION_GENERATED_TITLE
    );
    assert_eq!(event.pubkey, keys.public_key());
    assert_ne!(event.pubkey, founder.public_key());
    event.verify().expect("signed");
    validate_coding_session_title_envelope(&event).expect("the relay's own check");
    assert_eq!(
        tag_value(&event, "h").as_deref(),
        Some(job.channel_id.to_string().as_str())
    );
    assert_eq!(tag_value(&event, "d").as_deref(), Some(SESSION_REF));
    assert_eq!(
        tag_value(&event, "cs-target"),
        Some(buzz_core::coding_session_command::coding_session_target_key(&job.target))
    );
    let payload = parse_coding_session_title_content(&event.content).expect("payload");
    assert_eq!(payload.title, "Fix flaky login test");
    assert_eq!(payload.model, "claude-haiku-4-5");
    assert_eq!(payload.basis, CodingSessionTitleBasis::FirstMessage);
    assert_eq!(payload.source_command, None);
    assert_eq!(payload.create_event_id, CREATE_ID);
    let raw: serde_json::Value = serde_json::from_str(&event.content).expect("json");
    assert!(raw
        .get("sourceCommand")
        .is_some_and(serde_json::Value::is_null));

    // A title from a later 44220 names that command.
    let mut from_turn = job.clone();
    from_turn.source_command = Some(TURN_ID.into());
    let (generator, _) = FakeGenerator::new(Answer::Title);
    let TitleOutcome::Ready(event) = run_title_job(
        &from_turn,
        &ScriptedPreflight::new(vec![Ok(false)]),
        &generator,
        fast(),
        &Semaphore::new(1),
        &keys,
    )
    .await
    else {
        panic!("expected a title");
    };
    let payload = parse_coding_session_title_content(&event.content).expect("payload");
    assert_eq!(payload.source_command.as_deref(), Some(TURN_ID));
}

#[test]
fn the_preflight_reads_a_founder_name_or_any_title_and_nothing_else() {
    let founder = Keys::generate();
    let job = job(&founder);
    let stranger = Keys::generate();
    let name = |keys: &Keys, session_ref: &str| {
        buzz_sdk::builders::build_coding_session_name(job.channel_id, session_ref, "Named")
            .expect("builder")
            .sign_with_keys(keys)
            .expect("sign")
    };
    let row = |event: &Event| serde_json::to_value(event).expect("row");
    assert!(names_the_session(&row(&name(&founder, SESSION_REF)), &job));
    assert!(!names_the_session(
        &row(&name(&stranger, SESSION_REF)),
        &job
    ));
    let other_ref = "6c8f2d3b-01e5-4c1f-b2a4-8d3e9f7a5b21";
    assert!(!names_the_session(&row(&name(&founder, other_ref)), &job));

    let title = sign_title(
        &job,
        &GeneratedTitle {
            title: "Something".into(),
            model: "m".into(),
        },
        &stranger,
    )
    .expect("title");
    assert!(names_the_session(&row(&title), &job));

    // A forged row (content changed after signing) proves nothing.
    let mut forged = row(&name(&founder, SESSION_REF));
    forged["content"] = serde_json::json!("Changed");
    assert!(!names_the_session(&forged, &job));
}

/// The detached job never holds up its caller: `start` returns at once even
/// while the runtime is hung. This covers only `start` itself; the turn's
/// transcript is proved ungated by
/// [`the_founders_first_turn_streams_while_its_title_is_still_running`].
#[tokio::test]
async fn starting_a_job_does_not_wait_for_the_title() {
    let mut titler = AutoTitler::new(true).with_policy(fast());
    let mut ready = titler.take_ready().expect("queue");
    let founder = Keys::generate();
    let (generator, calls) = FakeGenerator::new(Answer::Hang);
    let before = Instant::now();
    titler.start(
        "session-1".into(),
        job(&founder),
        ScriptedPreflight::new(vec![Ok(false)]),
        generator,
        Keys::generate(),
    );
    assert!(before.elapsed() < Duration::from_millis(50));
    assert_eq!(titler.running(), 1);
    tokio::time::sleep(Duration::from_millis(30)).await;
    assert!(calls.load(Ordering::SeqCst) >= 1);
    assert!(ready.try_recv().is_err(), "nothing is ready while hung");
}

#[tokio::test]
async fn a_finished_job_reports_back_to_the_loop() {
    let mut titler = AutoTitler::new(true).with_policy(fast());
    let mut ready = titler.take_ready().expect("queue");
    let founder = Keys::generate();
    let (generator, _) = FakeGenerator::new(Answer::Title);
    titler.start(
        "session-1".into(),
        job(&founder),
        ScriptedPreflight::new(vec![Ok(false)]),
        generator,
        Keys::generate(),
    );
    let title = tokio::time::timeout(Duration::from_secs(5), ready.recv())
        .await
        .expect("in time")
        .expect("a title");
    assert_eq!(title.session_ref, SESSION_REF);
    assert_eq!(title.session_id, "session-1");
}

/// The one-shot runs on a single slot: a second umbrella's job waits for the
/// first rather than spawning a second adapter beside it.
#[tokio::test]
async fn one_title_spawn_at_a_time() {
    let slot = Arc::new(Semaphore::new(1));
    let held = Arc::clone(&slot).acquire_owned().await.expect("slot");
    let founder = Keys::generate();
    let keys = Keys::generate();
    let (generator, calls) = FakeGenerator::new(Answer::Title);
    let job = job(&founder);
    let preflight = ScriptedPreflight::new(vec![Ok(false)]);
    let waiting = {
        let slot = Arc::clone(&slot);
        async move { run_title_job(&job, &preflight, &generator, fast(), &slot, &keys).await }
    };
    tokio::pin!(waiting);
    assert!(
        tokio::time::timeout(Duration::from_millis(50), &mut waiting)
            .await
            .is_err(),
        "the job must wait for the slot"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    drop(held);
    assert!(matches!(waiting.await, TitleOutcome::Ready(_)));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

fn generator_for(state_dir: &Path, script: String) -> AcpTitleGenerator {
    AcpTitleGenerator {
        state_dir: state_dir.to_path_buf(),
        driver: "claude-agent-acp".into(),
        runtime: "claude".into(),
        agent_command: "bash".into(),
        agent_args: vec![script],
        cli_env: Vec::new(),
        model: "haiku".into(),
        runtime_profile_override: Some(RuntimeProfile::TestDouble),
    }
}

fn leftovers(state_dir: &Path) -> Vec<String> {
    [DISCOVERY_DIR, EXECUTIONS_DIR]
        .iter()
        .flat_map(|dir| std::fs::read_dir(state_dir.join(dir)).into_iter().flatten())
        .flatten()
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(SCRATCH_PREFIX))
        .collect()
}

/// The real spawn, inside the real boundary, against the stub adapter: one
/// `session/new` with the instruction and no MCP server, no terminal offered,
/// the alias switched to the offered model and every permission rejected —
/// the stub answers with the title only when each of those held — then the
/// reply cleaned, the process gone and its scratch removed.
#[tokio::test]
async fn the_stub_adapter_names_the_session_and_leaves_nothing_behind() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).expect("state");
    let script = stub_adapter(
        dir.path(),
        "titler.sh",
        StubPrompt::Reply("Title: Fix flaky login test."),
    );
    let generated = generator_for(&state, script)
        .generate("Fix the flaky login test")
        .await
        .expect("an attempt that answered")
        .expect("a usable title");
    assert_eq!(
        generated,
        GeneratedTitle {
            title: "Fix flaky login test".into(),
            model: "claude-haiku-4-5".into(),
        }
    );
    assert!(leftovers(&state).is_empty(), "{:?}", leftovers(&state));
    assert!(
        std::fs::read_dir(state.join(DISCOVERY_DIR))
            .expect("discovery dir")
            .next()
            .is_none(),
        "the scratch directory survived"
    );
}

#[tokio::test]
async fn an_adapter_that_says_nothing_is_a_failed_attempt() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).expect("state");
    let script = stub_adapter(dir.path(), "silent.sh", StubPrompt::Exit);
    let error = generator_for(&state, script)
        .generate("Fix the flaky login test")
        .await
        .expect_err("no answer");
    assert!(error.contains("exited"), "{error}");
    assert!(leftovers(&state).is_empty(), "{:?}", leftovers(&state));
}

/// A timeout drops the attempt mid-exchange: the process group is killed and
/// the scratch is still removed.
#[tokio::test]
async fn a_timed_out_attempt_is_killed_and_cleaned_up() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).expect("state");
    let script = stub_adapter(dir.path(), "hung.sh", StubPrompt::Hang);
    let generator = generator_for(&state, script);
    let mut attempt = Box::pin(generator.generate("Fix it"));
    assert!(
        tokio::time::timeout(Duration::from_millis(1500), &mut attempt)
            .await
            .is_err()
    );
    // The stub is mid-prompt now: its scratch exists until the attempt is
    // dropped, exactly as a timeout in `run_title_job` drops it.
    assert!(!leftovers(&state).is_empty(), "the attempt never started");
    drop(attempt);
    assert!(leftovers(&state).is_empty(), "{:?}", leftovers(&state));
}

/// An adapter that reports no model and offers none to switch to: the title
/// must not be attributed to the requested alias (`haiku`), which the session
/// never ran on — it names the disclosed non-answer instead.
#[tokio::test]
async fn an_adapter_that_reports_no_model_is_not_credited_to_the_request() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    std::fs::create_dir_all(&state).expect("state");
    let script = stub_adapter_with(
        dir.path(),
        "modelless.sh",
        StubPrompt::Reply("Fix flaky login test"),
        false,
    );
    let generated = generator_for(&state, script)
        .generate("Fix the flaky login test")
        .await
        .expect("an attempt that answered")
        .expect("a usable title");
    assert_eq!(generated.title, "Fix flaky login test");
    assert_ne!(generated.model, "haiku");
    assert_eq!(generated.model, acp::UNREPORTED_MODEL);
}

fn attachment(filename: Option<&str>) -> TurnAttachment {
    TurnAttachment {
        sha256: "ab".repeat(32),
        mime: "image/png".into(),
        size: 48_213,
        dim: Some("1280x720".into()),
        filename: filename.map(str::to_owned),
    }
}

/// T3's initial-title prompt: the message, then one metadata line per
/// attachment. An attachment-only message is shown its metadata alone.
#[test]
fn attachments_reach_the_namer_as_metadata() {
    assert_eq!(title_message("Fix it", &[]), "Fix it");
    let shown = title_message(
        "",
        &[attachment(Some("login\nerror.png")), attachment(None)],
    );
    assert_eq!(
        shown,
        "\n\nAttachment metadata:\n\
         - loginerror.png (image/png, 48213 bytes)\n\
         - abababab.png (image/png, 48213 bytes)"
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
        // Nothing listens here: the preflight fails and publishes nothing.
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
        handover: buzz_core::coding_session_authority_claim::ClaimState::NoClaim,
        retired: None,
        pack_ref: None,
        compose_ref: None,
    }
}

#[tokio::test]
async fn the_provider_starts_a_job_only_for_the_founders_first_turn() {
    let dir = tempfile::tempdir().expect("tempdir");
    let founder = "ab".repeat(32);
    let mut provider = test_provider(dir.path(), "on");
    provider
        .state
        .insert_session(record(&founder))
        .expect("record");

    // Someone else's turn first: not titled.
    let stranger = "cd".repeat(32);
    provider.maybe_start_auto_title("session-1", CREATE_ID, "Hello", &[], Some(&stranger));
    assert_eq!(provider.auto_title.running(), 0);

    provider.maybe_start_auto_title(
        "session-1",
        CREATE_ID,
        "Fix the login test",
        &[],
        Some(&founder),
    );
    assert!(provider.auto_title.started.contains(SESSION_REF));

    // Once per umbrella, and never after the first turn is charged.
    let other = tempfile::tempdir().expect("tempdir");
    let mut again = test_provider(other.path(), "on");
    again
        .state
        .insert_session(record(&founder))
        .expect("record");
    again.state.record_turn_spend(SESSION_REF).expect("spend");
    again.maybe_start_auto_title("session-1", TURN_ID, "And another", &[], Some(&founder));
    assert!(!again.auto_title.started.contains(SESSION_REF));
}

#[tokio::test]
async fn an_attachment_only_first_message_starts_a_job() {
    let dir = tempfile::tempdir().expect("tempdir");
    let founder = "ab".repeat(32);
    let mut provider = test_provider(dir.path(), "on");
    provider
        .state
        .insert_session(record(&founder))
        .expect("record");
    provider.maybe_start_auto_title("session-1", CREATE_ID, "  ", &[], Some(&founder));
    assert!(!provider.auto_title.started.contains(SESSION_REF));
    provider.maybe_start_auto_title(
        "session-1",
        CREATE_ID,
        "",
        &[attachment(Some("screenshot.png"))],
        Some(&founder),
    );
    assert!(provider.auto_title.started.contains(SESSION_REF));
}

#[tokio::test]
async fn off_starts_nothing() {
    let dir = tempfile::tempdir().expect("tempdir");
    let founder = "ab".repeat(32);
    let mut provider = test_provider(dir.path(), "off");
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
    assert!(!provider.auto_title.enabled());
    assert!(provider.auto_title.started.is_empty());
    assert_eq!(provider.auto_title.running(), 0);
}

#[tokio::test]
async fn a_finished_title_goes_into_the_outbox_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let founder = Keys::generate();
    let mut provider = test_provider(dir.path(), "on");
    let job = job(&founder);
    let event = sign_title(
        &job,
        &GeneratedTitle {
            title: "Fix flaky login test".into(),
            model: "claude-haiku-4-5".into(),
        },
        &provider.config.keys,
    )
    .expect("title");
    let ready = |event: &Event| TitleReady {
        channel_id: job.channel_id,
        session_ref: SESSION_REF.into(),
        session_id: "session-1".into(),
        event: Box::new(event.clone()),
    };
    provider
        .enqueue_generated_title(ready(&event))
        .expect("enqueue");
    let key = format!("title:{SESSION_REF}");
    assert!(provider
        .outbox
        .contains(KIND_CODING_SESSION_GENERATED_TITLE, &key));
    assert_eq!(provider.outbox.pending_len(), 1);
    provider
        .enqueue_generated_title(ready(&event))
        .expect("enqueue");
    assert_eq!(provider.outbox.pending_len(), 1, "fenced by its key");
}

/// SV-31 acceptance "first transcript chunk not gated", through the real path:
/// the provider's own `TurnStarted` handler starts the job, the job's relay
/// preflight hangs (a relay that accepts the connection and never answers),
/// and the turn's transcript chunk is still queued as a 44225 at once — before
/// and while the title job is in flight.
#[tokio::test]
async fn the_founders_first_turn_streams_while_its_title_is_still_running() {
    let dir = tempfile::tempdir().expect("tempdir");
    let founder = "ab".repeat(32);
    let mut provider = test_provider(dir.path(), "on");
    // A relay that takes the preflight's connection and never replies.
    let relay = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let address = relay.local_addr().expect("address");
    provider.set_rest_client(RestClient {
        http: reqwest::Client::new(),
        base_url: format!("http://{address}"),
        keys: Keys::generate(),
        auth_tag_json: None,
    });
    provider
        .state
        .insert_session(record(&founder))
        .expect("record");
    let transcripts = |provider: &crate::Provider| {
        provider
            .outbox
            .pending_events()
            .filter(|event| {
                u32::from(event.kind.as_u16()) == buzz_core::kind::KIND_CODING_SESSION_TRANSCRIPT
            })
            .count()
    };
    let chunk = |n: usize| crate::session::SessionEvent::TranscriptItems {
        session_id: "session-1".into(),
        turn_id: "turn-1".into(),
        items: vec![serde_json::json!({ "kind": "assistant_text", "text": format!("chunk {n}") })],
    };

    let before = Instant::now();
    // The create's initial turn: its command is the record's own, so the
    // operator resolves to `created_by`, the founder.
    provider
        .handle_session_event(crate::session::SessionEvent::TurnStarted {
            session_id: "session-1".into(),
            turn_id: "turn-1".into(),
            command_id: CREATE_ID.into(),
            text: "Fix the login test".into(),
            attachments: Vec::new(),
        })
        .expect("turn started");
    provider.handle_session_event(chunk(1)).expect("chunk");
    assert!(
        before.elapsed() < Duration::from_millis(200),
        "the turn's first chunk waited {:?}",
        before.elapsed()
    );
    assert!(
        provider.auto_title.started.contains(SESSION_REF),
        "a job began"
    );
    assert_eq!(provider.auto_title.running(), 1);
    assert_eq!(transcripts(&provider), 1, "the first chunk is queued");

    // The job is really in flight: its preflight reached the relay and is
    // waiting on it.
    let (_held, _) = tokio::time::timeout(Duration::from_secs(5), relay.accept())
        .await
        .expect("the preflight reached the relay")
        .expect("accept");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(
        provider.auto_title.running(),
        1,
        "still waiting on the relay"
    );

    let before = Instant::now();
    provider.handle_session_event(chunk(2)).expect("chunk");
    assert!(before.elapsed() < Duration::from_millis(200));
    assert_eq!(transcripts(&provider), 2, "the turn keeps streaming");
    assert_eq!(provider.auto_title.running(), 1);
    assert!(!provider
        .outbox
        .pending_events()
        .any(|event| u32::from(event.kind.as_u16()) == KIND_CODING_SESSION_GENERATED_TITLE));
}
