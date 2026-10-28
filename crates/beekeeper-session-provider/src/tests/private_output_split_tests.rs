//! Ledger 388(c), the actual mechanism: a generation that received private
//! continuity echoed a model-derived substring of a registered claim through
//! its Write tool into the shared 44225 stream. Whole-fragment redaction cannot
//! catch a fragment the model made up, so the split removes the field instead:
//! the full item goes to the owner-private channel, the shared channel gets a
//! deterministic twin, and an ordinary session is unchanged.
use super::*;

/// Registered as a whole claim sentence; the model then emits only parts.
const CLAIM: &str = "The current bench calibration code is C4B-9a8b7c6d5e4f.";
const DERIVED: &str = "C4B-9a8b7c6d5e4f";
const PARAPHRASE: &str = "calibration now nine-a eight-b";

fn leaky_turn() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({"kind":"user_prompt","content":"@agent proceed","commandId":"cmd-2"}),
        serde_json::json!({"kind":"assistant_text","text":format!("Current code {DERIVED}; {PARAPHRASE}.")}),
        serde_json::json!({"kind":"tool_call","tool":{"toolName":"Write","toolKind":"edit","toolId":"toolu_w","input":{"file_path":"report.txt","content":format!("{DERIVED}\nrevision 2\n")}}}),
        serde_json::json!({"kind":"tool_result","toolId":"toolu_w","toolName":"Write","toolKind":"edit","edit":{"paths":["report.txt"],"changes":[{"path":"report.txt","newText":DERIVED}]},"content":format!("wrote {PARAPHRASE}"),"isError":false}),
        serde_json::json!({"kind":"result","subtype":"success","isError":false,"durationMs":900,"result":format!("Done with {DERIVED}")}),
    ]
}

fn fixture(dir: &tempfile::TempDir, split: bool) -> (Provider, Uuid, Uuid, String) {
    let cwd = dir.path().join("checkout");
    std::fs::create_dir_all(&cwd).unwrap();
    let shared = Uuid::new_v4();
    let private = Uuid::new_v4();
    let mut provider = provider(&dir.path().join("state"), None);
    let record = governed_record(shared, &cwd, &"ab".repeat(32));
    let session_id = record.session_id.clone();
    let generation = record.generation;
    provider.state.insert_session(record).unwrap();
    if split {
        provider
            .state
            .bind_private_output(
                &session_id,
                crate::private_output::PrivateOutput {
                    channel: private,
                    owner: "22".repeat(32),
                    from_generation: generation,
                },
            )
            .unwrap();
    }
    (provider, shared, private, session_id)
}

fn feed(provider: &mut Provider, session_id: &str, items: Vec<serde_json::Value>) {
    provider
        .handle_session_event(session::SessionEvent::TranscriptItems {
            session_id: session_id.to_owned(),
            turn_id: "turn-2".to_owned(),
            items,
        })
        .unwrap();
}

fn channel_of(event: &Event) -> Option<Uuid> {
    event
        .tags
        .iter()
        .find(|t| t.as_slice().first().map(String::as_str) == Some("h"))
        .and_then(|t| Uuid::parse_str(&t.as_slice()[1]).ok())
}

/// Every flushed event, as `(channel, kind, content)`.
async fn published(provider: &mut Provider) -> Vec<(Option<Uuid>, u32, String)> {
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    sink.all()
        .into_iter()
        .map(|e| {
            (
                channel_of(&e),
                u32::from(e.kind.as_u16()),
                e.content.clone(),
            )
        })
        .collect()
}

fn seqs(events: &[(Option<Uuid>, u32, String)], channel: Uuid) -> Vec<u64> {
    let mut seqs: Vec<u64> = events
        .iter()
        .filter(|(c, k, _)| *c == Some(channel) && *k == KIND_CODING_SESSION_TRANSCRIPT)
        .map(|(_, _, content)| {
            serde_json::from_str::<serde_json::Value>(content).unwrap()["eventSeq"]
                .as_u64()
                .unwrap()
        })
        .collect();
    seqs.sort_unstable();
    seqs
}

#[tokio::test]
async fn a_private_generations_derived_substrings_never_reach_the_shared_channel_but_reach_the_owner(
) {
    let dir = tempfile::tempdir().unwrap();
    let (mut provider, shared, private, session_id) = fixture(&dir, true);
    // Defence in depth stays on: the whole claim is registered, as C3 does.
    feed(&mut provider, &session_id, leaky_turn());
    let events = published(&mut provider).await;
    let shared_text: String = events
        .iter()
        .filter(|(c, _, _)| *c != Some(private))
        .map(|(_, _, content)| content.as_str())
        .collect();
    for leaked in [
        DERIVED,
        "9a8b7c6d5e4f",
        PARAPHRASE,
        "nine-a",
        "report.txt",
        CLAIM,
    ] {
        assert!(
            !shared_text.contains(leaked),
            "{leaked} reached a teammate-readable event"
        );
    }
    let private_text: String = events
        .iter()
        .filter(|(c, _, _)| *c == Some(private))
        .map(|(_, _, content)| content.as_str())
        .collect();
    for detail in [DERIVED, PARAPHRASE, "report.txt", "revision 2"] {
        assert!(private_text.contains(detail), "the owner lost {detail}");
    }
    // One numbering: every shared twin names a sequence the owner also holds,
    // and the owner's copy is complete.
    let owner_seqs = seqs(&events, private);
    assert_eq!(owner_seqs.len(), leaky_turn().len());
    let shared_seqs = seqs(&events, shared);
    assert!(!shared_seqs.is_empty());
    assert!(shared_seqs.iter().all(|s| owner_seqs.contains(s)));
    // Signed operator input keeps its existing visibility.
    assert!(shared_text.contains("@agent proceed"));
    assert!(shared_text.contains(
        crate::private_output::PRIVATE_PROSE_NOTICE
            .split(':')
            .next()
            .unwrap()
    ));
}

#[tokio::test]
async fn an_ordinary_generations_transcript_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (mut provider, shared, _private, session_id) = fixture(&dir, false);
    feed(&mut provider, &session_id, leaky_turn());
    let events = published(&mut provider).await;
    assert!(events.iter().all(|(c, _, _)| *c == Some(shared)));
    let text: String = events.iter().map(|(_, _, c)| c.as_str()).collect();
    assert!(text.contains(DERIVED) && text.contains(PARAPHRASE));
    assert!(!text.contains(crate::private_output::PRIVATE_PROSE_NOTICE));
    assert_eq!(seqs(&events, shared).len(), leaky_turn().len());
}

#[tokio::test]
async fn the_split_survives_a_provider_restart_and_covers_later_generations() {
    let dir = tempfile::tempdir().unwrap();
    let (provider, _shared, private, session_id) = fixture(&dir, true);
    let generation = provider.state.session(&session_id).unwrap().generation;
    drop(provider);
    // A restore or replay after restart reads the persisted split.
    let mut reopened = super::provider(&dir.path().join("state"), None);
    let split = reopened
        .state
        .private_output(&session_id, generation)
        .cloned()
        .expect("persisted");
    assert_eq!(split.channel, private);
    assert!(reopened
        .state
        .private_output(&session_id, generation + 1)
        .is_some());
    feed(&mut reopened, &session_id, leaky_turn());
    let events = published(&mut reopened).await;
    assert!(events
        .iter()
        .filter(|(c, _, _)| *c != Some(private))
        .all(|(_, _, content)| !content.contains(DERIVED)));
}

#[tokio::test]
async fn a_split_generations_observed_gate_row_is_owner_private() {
    let dir = tempfile::tempdir().unwrap();
    let (mut provider, shared, private, session_id) = fixture(&dir, true);
    init_repo(&dir.path().join("checkout"), "gate-branch");
    feed(
        &mut provider,
        &session_id,
        vec![
            serde_json::json!({"kind":"tool_call","tool":{"toolName":"Bash","toolId":"t1","toolKind":"execute","input":{"command":format!("cargo test {DERIVED}")}}}),
            serde_json::json!({"kind":"tool_result","toolId":"t1","toolName":"Bash","toolKind":"execute","content":format!("ok {DERIVED}"),"isError":false}),
        ],
    );
    pump_until(&mut provider, |event| {
        matches!(event, session::SessionEvent::GateObserved { .. })
    })
    .await;
    let events = published(&mut provider).await;
    let rows: Vec<_> = events
        .iter()
        .filter(|(_, k, _)| *k == KIND_CODING_SESSION_OBSERVATION)
        .collect();
    assert!(!rows.is_empty(), "the gate row must still be observed");
    assert!(rows.iter().all(|(c, _, _)| *c == Some(private)));
    assert!(events
        .iter()
        .filter(|(c, _, _)| *c == Some(shared))
        .all(|(_, _, content)| !content.contains(DERIVED)));
}

/// Make the fixture's session a seated, non-lead turn whose end wakes the lead.
fn seat_open_turn(provider: &mut Provider, session_id: &str, command_id: &str) {
    provider
        .state
        .update_session(session_id, |record| {
            record.actor = Some("33".repeat(32));
            record.role = Some("builder".into());
            record.open_turn = Some(OpenTurn {
                turn_id: "turn-2".into(),
                command_id: Some(command_id.into()),
                team_wake_eligible: true,
                started_at_ms: now_ms(),
                operator_pubkey: None,
            });
        })
        .unwrap();
}

fn finish_failed(
    provider: &mut Provider,
    session_id: &str,
    cause: Option<session::TurnFailureCause>,
) {
    provider
        .handle_session_event(session::SessionEvent::TurnFinished {
            session_id: session_id.to_owned(),
            turn_id: "turn-2".to_owned(),
            outcome: session::TurnOutcome::Failed {
                // What adapters really put here: their own words, which can
                // carry whatever the model was working on.
                message: format!("tool crashed writing {DERIVED} ({PARAPHRASE})"),
                agent_gone: false,
                cause,
            },
            duration_ms: 1200,
            usage: None,
            tool_calls: 1,
        })
        .unwrap();
}

/// Every flushed event, as `(id, channel, kind, content)`.
async fn published_with_ids(provider: &mut Provider) -> Vec<(String, Option<Uuid>, u32, String)> {
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    sink.all()
        .into_iter()
        .map(|e| {
            (
                e.id.to_hex(),
                channel_of(&e),
                u32::from(e.kind.as_u16()),
                e.content.clone(),
            )
        })
        .collect()
}

fn results_in(
    events: &[(String, Option<Uuid>, u32, String)],
    channel: Uuid,
) -> Vec<(String, serde_json::Value)> {
    events
        .iter()
        .filter(|(_, c, k, _)| *c == Some(channel) && *k == KIND_CODING_SESSION_TRANSCRIPT)
        .map(|(id, _, _, content)| (id.clone(), serde_json::from_str(content).unwrap()))
        .filter(|(_, envelope): &(String, serde_json::Value)| envelope["item"]["kind"] == "result")
        .collect()
}

/// The terminal result is automatic output too: an adapter's failure text
/// (`result` when unclassified, `detail` when classified) went verbatim into
/// the shared channel and the owner got no result at all.
#[tokio::test]
async fn a_private_generations_failed_turn_result_is_split_at_one_sequence() {
    for cause in [
        None,
        Some(session::TurnFailureCause::CancelNotConfirmed { shutdown: false }),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let (mut provider, shared, private, session_id) = fixture(&dir, true);
        seat_open_turn(&mut provider, &session_id, "cmd-fail");
        finish_failed(&mut provider, &session_id, cause);
        let events = published_with_ids(&mut provider).await;
        let shared_text: String = events
            .iter()
            .filter(|(_, c, _, _)| *c != Some(private))
            .map(|(_, _, _, content)| content.as_str())
            .collect();
        for leaked in [DERIVED, "9a8b7c6d5e4f", PARAPHRASE, "tool crashed"] {
            assert!(
                !shared_text.contains(leaked),
                "{leaked} reached a teammate-readable event ({cause:?})"
            );
        }
        let owner = results_in(&events, private);
        let twin = results_in(&events, shared);
        assert_eq!(owner.len(), 1, "the owner must get the full result");
        assert_eq!(twin.len(), 1, "teammates must still see the turn end");
        let (owner_item, twin_item) = (&owner[0].1["item"], &twin[0].1["item"]);
        assert!(owner_item.to_string().contains(DERIVED), "{owner_item}");
        assert_eq!(owner[0].1["eventSeq"], twin[0].1["eventSeq"]);
        // Status semantics survive: still an error, still the same class.
        assert_eq!(twin_item["subtype"], "error");
        assert_eq!(twin_item["isError"], true);
        assert_eq!(
            twin_item["result"],
            crate::private_output::PRIVATE_RESULT_NOTICE
        );
        assert!(twin_item.get("detail").is_none());
        assert_eq!(twin_item.get("errorCode"), owner_item.get("errorCode"));
        // The lead's wake cites the event the lead can read, not the DM copy.
        assert_eq!(
            provider.team_wakes.terminal_event_ids(shared),
            vec![twin[0].0.clone()],
            "{cause:?}"
        );
        let status = events
            .iter()
            .filter(|(_, c, k, _)| *c == Some(shared) && *k == KIND_CODING_SESSION_METADATA)
            .map(|(_, _, _, content)| serde_json::from_str::<serde_json::Value>(content).unwrap())
            .next_back()
            .expect("metadata");
        assert_eq!(status["status"], "failed");
    }
}

#[tokio::test]
async fn an_ordinary_generations_failed_turn_result_is_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let (mut provider, shared, _private, session_id) = fixture(&dir, false);
    seat_open_turn(&mut provider, &session_id, "cmd-fail");
    finish_failed(&mut provider, &session_id, None);
    let events = published_with_ids(&mut provider).await;
    let results = results_in(&events, shared);
    assert_eq!(results.len(), 1);
    assert_eq!(
        results[0].1["item"]["result"],
        format!("tool crashed writing {DERIVED} ({PARAPHRASE})")
    );
    assert_eq!(
        provider.team_wakes.terminal_event_ids(shared),
        vec![results[0].0.clone()]
    );
}

/// Feed one gate call and its result, and return every flushed event.
async fn run_gate(
    provider: &mut Provider,
    session_id: &str,
    tool_id: &str,
    command: &str,
) -> Vec<Event> {
    feed(
        provider,
        session_id,
        vec![
            serde_json::json!({"kind":"tool_call","tool":{"toolName":"Bash","toolId":tool_id,"toolKind":"execute","input":{"command":command}}}),
            serde_json::json!({"kind":"tool_result","toolId":tool_id,"toolName":"Bash","toolKind":"execute","content":format!("test result: ok. {DERIVED}"),"isError":false}),
        ],
    );
    pump_until(provider, |event| {
        matches!(event, session::SessionEvent::GateObserved { .. })
    })
    .await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    sink.all()
}

/// The 44246 fold the push gate runs, over one channel's rows.
fn fold_gates(
    provider: &Provider,
    session_id: &str,
    events: &[Event],
    channel: Uuid,
) -> Vec<beekeeper_core::coding_session_observation::CodingSessionObservationGateEntry> {
    let record = provider.state.session(session_id).unwrap();
    let rows: Vec<Event> = events
        .iter()
        .filter(|e| u32::from(e.kind.as_u16()) == KIND_CODING_SESSION_OBSERVATION)
        .filter(|e| channel_of(e) == Some(channel))
        .cloned()
        .collect();
    beekeeper_core::coding_session_observation::fold_coding_session_observations(
        &rows,
        &beekeeper_core::coding_session_observation::CodingSessionObservationFoldContext {
            session_ref: record.session_ref.clone().unwrap(),
            genesis_ref: record.genesis_ref.clone().unwrap(),
            known_assignment_refs: Vec::new(),
            provider_pubkeys: Some(vec![provider.config.keys.public_key().to_hex()]),
        },
    )
    .gates
}

/// Moving every row into the DM took the gate facts away from the push gate,
/// which reads only the mission's channel. A fixed gate command keeps its
/// row there with exactly the facts an ordinary generation's row gives the
/// fold — gate, outcome, `observed`, signer, `headSha`/`dirty` — and only the
/// tool's summary withheld; a command carrying anything else stays private.
#[tokio::test]
async fn a_split_generations_fixed_gate_command_keeps_its_admission_facts_in_the_shared_channel() {
    let ordinary_dir = tempfile::tempdir().unwrap();
    let (mut ordinary, ordinary_shared, _, ordinary_id) = fixture(&ordinary_dir, false);
    init_repo(&ordinary_dir.path().join("checkout"), "gate-branch");
    let ordinary_events = run_gate(&mut ordinary, &ordinary_id, "t1", "cargo test").await;
    let ordinary_gates = fold_gates(&ordinary, &ordinary_id, &ordinary_events, ordinary_shared);
    assert_eq!(ordinary_gates.len(), 1);

    let dir = tempfile::tempdir().unwrap();
    let (mut provider, shared, private, session_id) = fixture(&dir, true);
    init_repo(&dir.path().join("checkout"), "gate-branch");
    let events = run_gate(&mut provider, &session_id, "t1", "cargo   test").await;
    let shared_gates = fold_gates(&provider, &session_id, &events, shared);
    let owner_gates = fold_gates(&provider, &session_id, &events, private);
    assert_eq!(
        shared_gates.len(),
        1,
        "the push gate's channel lost the row"
    );
    assert_eq!(owner_gates.len(), 1, "the owner lost the full row");
    let (s, o) = (&shared_gates[0], &ordinary_gates[0]);
    assert_eq!(
        s.source,
        beekeeper_core::coding_session_observation::CodingSessionObservationSource::Observed
    );
    // Everything verdict admission reads is the ordinary generation's.
    assert_eq!(
        (
            &s.row.gate,
            &s.row.outcome,
            &s.row.command,
            &s.row.head_sha,
            &s.row.dirty,
            &s.source
        ),
        (
            &o.row.gate,
            &o.row.outcome,
            &o.row.command,
            &o.row.head_sha,
            &o.row.dirty,
            &o.source
        )
    );
    assert_eq!(s.author_pubkey, provider.config.keys.public_key().to_hex());
    // The tool output stays with the owner.
    assert_eq!(s.row.summary, None);
    assert!(owner_gates[0]
        .row
        .summary
        .as_deref()
        .is_some_and(|t| t.contains("ok")));
    assert!(events
        .iter()
        .filter(|e| channel_of(e) != Some(private))
        .all(|e| !e.content.contains(DERIVED)));

    // A command the model shaped is not repeated, and no substitute is.
    let more = run_gate(
        &mut provider,
        &session_id,
        "t2",
        &format!("cargo test {DERIVED}"),
    )
    .await;
    assert!(fold_gates(&provider, &session_id, &more, shared).is_empty());
    assert_eq!(fold_gates(&provider, &session_id, &more, private).len(), 1);
}

/// Probe the fixture's worktree and return the newest shared 44223 and every
/// flushed event.
async fn probe_branch(
    provider: &mut Provider,
    session_id: &str,
    shared: Uuid,
) -> (Option<serde_json::Value>, Vec<Event>) {
    let target = provider
        .state
        .session(session_id)
        .unwrap()
        .target("instance-1");
    provider
        .publish_metadata(shared, &target, SessionStatus::Idle)
        .unwrap();
    provider.spawn_git_probe(session_id);
    pump_until_worktree_observed(provider).await;
    let sink = CollectingSink::new();
    provider.flush(&sink).await.unwrap();
    let metadata = sink
        .contents_of(KIND_CODING_SESSION_METADATA)
        .into_iter()
        .next_back();
    (metadata, sink.all())
}

/// A branch name is the agent's words: `git checkout -b <derived value>`
/// published it in every shared 44223 after the transcript was split.
#[tokio::test]
async fn a_split_generations_branch_is_withheld_from_shared_metadata_and_reaches_the_owner() {
    let branch = format!("work-{}", DERIVED.to_ascii_lowercase());
    let dir = tempfile::tempdir().unwrap();
    let (mut provider, shared, private, session_id) = fixture(&dir, true);
    init_repo(&dir.path().join("checkout"), &branch);
    let (metadata, events) = probe_branch(&mut provider, &session_id, shared).await;
    let metadata = metadata.expect("metadata");
    assert!(metadata["branch"].is_null(), "{metadata}");
    // The provider's own coordinate facts stay.
    assert_eq!(metadata["observedCommit"].as_str().map(str::len), Some(40));
    let derived = DERIVED.to_ascii_lowercase();
    assert!(events
        .iter()
        .filter(|e| channel_of(e) != Some(private))
        .all(|e| !e.content.contains(&derived)));
    let owner: Vec<serde_json::Value> = events
        .iter()
        .filter(|e| channel_of(e) == Some(private))
        .map(|e| serde_json::from_str::<serde_json::Value>(&e.content).unwrap()["item"].clone())
        .collect();
    assert_eq!(owner.len(), 1, "{owner:?}");
    assert_eq!(owner[0]["status"], "branch_observed");
    assert_eq!(owner[0]["branch"], branch);
    // Once per change: the same branch again costs the owner nothing.
    let (_, again) = probe_branch(&mut provider, &session_id, shared).await;
    assert!(again.iter().all(|e| channel_of(e) != Some(private)));
}

#[tokio::test]
async fn an_ordinary_generations_metadata_still_names_its_branch() {
    let dir = tempfile::tempdir().unwrap();
    let (mut provider, shared, private, session_id) = fixture(&dir, false);
    init_repo(&dir.path().join("checkout"), "work-ordinary");
    let (metadata, events) = probe_branch(&mut provider, &session_id, shared).await;
    assert_eq!(metadata.expect("metadata")["branch"], "work-ordinary");
    assert!(events.iter().all(|e| channel_of(e) != Some(private)));
}
