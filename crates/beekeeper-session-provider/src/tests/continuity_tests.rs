//! Deterministic private bootstrap, read-time evidence and real ACP wire tests.
use super::*;
use crate::continuity_bootstrap::{prepare, Phase, Refusal, RequiredFacts, MAX_BOOTSTRAP_BYTES};
use crate::continuity_collect::{collect, Custody, Knowledge};
use crate::session::testing::{fake_agent, legacy_request, GOOD_AGENT};
use beekeeper_core::continuity::ClaimState;
use beekeeper_core::continuity::*;

fn scope() -> Scope {
    Scope::new("https://fixture.invalid", &Uuid::nil().to_string(), None).unwrap()
}
fn facts() -> RequiredFacts {
    RequiredFacts {
        target: CodingSessionTarget {
            driver: "fixture".into(),
            instance_id: "host".into(),
            session_id: Uuid::new_v4().to_string(),
            generation: 2,
        },
        command_id: "new-command".into(),
        scope: scope(),
        actor: "11".repeat(32),
        owner: "22".repeat(32),
        directions: "Private direction; revision abc. Keep permissions.".into(),
        work: "Required decision and unfinished responsibility.".into(),
        checkpoint: "No prior verified report; unknown.".into(),
        read_at: 100,
        source_ids: vec![],
        runtime: crate::continuity_bootstrap::RuntimeBinding {
            bytes: 5325,
            digest: "ab".repeat(32),
            transport: "first_turn",
        },
    }
}
fn unknown() -> Knowledge {
    Knowledge {
        status: "unknown/query_failed".into(),
        claims: vec![],
        source_ids: vec![],
        result_count: 0,
        read_at: 100,
    }
}
#[test]
fn continuity_combined_byte_bound_never_drops_required_sections() {
    assert_eq!(MAX_BOOTSTRAP_BYTES, 16384);
    for transport in ["system_prompt", "first_turn"] {
        let base = || {
            let mut f = facts();
            f.runtime = runtime(19897, transport);
            f
        };
        let overhead = prepare(base(), &unknown()).unwrap().payload().len();
        // Exactly at the bound: admitted whole, required sections intact.
        let mut f = base();
        f.work.push_str(&"x".repeat(MAX_BOOTSTRAP_BYTES - overhead));
        let bounded = prepare(f, &unknown()).unwrap();
        assert_eq!(bounded.payload().len(), 16384, "{transport}");
        assert!(bounded.payload().contains("Required decision"));
        assert!(bounded.payload().contains("Keep permissions"));
        let receipt = bounded.receipt().unwrap();
        assert_eq!(receipt.bytes, 16384);
        assert_eq!(receipt.runtime_transport, transport);
        // One byte over: refused by name with its count, nothing truncated.
        let mut f = base();
        f.work
            .push_str(&"x".repeat(MAX_BOOTSTRAP_BYTES - overhead + 1));
        let refusal = prepare(f, &unknown()).unwrap_err();
        assert_eq!(refusal, Refusal::TooLarge { bytes: 16385 }, "{transport}");
        assert_eq!(refusal.code(), "CONTINUITY_BOOTSTRAP_TOO_LARGE");
        assert_eq!(
            refusal.overflow().unwrap(),
            "dynamic continuity block is 16385 UTF-8 bytes, over its 16384-byte bound by 1"
        );
        // Multibyte text is counted in bytes, not characters.
        let mut f = base();
        f.work
            .push_str(&"🐝".repeat((MAX_BOOTSTRAP_BYTES - overhead) / 4 + 1));
        assert!(matches!(
            prepare(f, &unknown()).unwrap_err(),
            Refusal::TooLarge { bytes } if bytes > 16384
        ));
    }
}
#[test]
fn continuity_receipts_bind_command_generation_and_do_not_infer_confirmation() {
    let prepared = prepare(facts(), &unknown()).unwrap();
    let r = prepared.receipt().unwrap();
    assert_eq!(r.phase, Phase::Prepared);
    assert!(r.confirmation.is_none());
    let mut target = r.target.clone();
    target.generation += 1;
    assert_eq!(
        prepared.check_binding(&target, &r.command_id),
        Err(Refusal::Unavailable)
    );
    assert_eq!(
        prepared.check_binding(&r.target, "old-command"),
        Err(Refusal::Unavailable)
    );
    prepared.submitted().unwrap();
    assert_eq!(prepared.receipt().unwrap().phase, Phase::Submitted);
    assert!(prepared.receipt().unwrap().confirmation.is_none());
    prepared.confirmed().unwrap();
    assert_eq!(prepared.receipt().unwrap().phase, Phase::Confirmed);
    assert!(prepared.submitted().is_err());
    assert!(!format!("{prepared:?} {r:?}").contains("Private direction"));
}
#[test]
fn continuity_private_receipts_are_cleaned_with_execution_and_have_no_plaintext() {
    let dir = tempfile::tempdir().unwrap();
    let prepared = prepare(facts(), &unknown()).unwrap();
    let r = prepared.receipt().unwrap();
    crate::context_store::write_continuity_receipt(dir.path(), &r).unwrap();
    let package = dir
        .path()
        .join("context-packages")
        .join(&r.target.session_id);
    let path = std::fs::read_dir(&package)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("Private direction"));
    assert!(!raw.contains("Required decision"));
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
    crate::context_store::remove_context_packages(dir.path(), &r.target.session_id).unwrap();
    assert!(!package.exists());
}
#[tokio::test]
async fn continuity_exact_evidence_is_reread_and_correction_shadows_replaced_history() {
    let actor = Keys::generate();
    let owner = Keys::generate();
    let (relay, control, server) = spawn_recording_test_relay(&actor, vec![]).await;
    let mut rest = relay.rest_client();
    rest.auth_tag_json =
        Some(beekeeper_sdk::nip_oa::compute_auth_tag(&owner, &actor.public_key(), "").unwrap());
    let scope = Scope::new(&rest.base_url, &Uuid::nil().to_string(), None).unwrap();
    let source = nostr::EventBuilder::new(Kind::Custom(9), "observed B 🐝")
        .tags([nostr::Tag::parse(["h", &scope.channel]).unwrap()])
        .sign_with_keys(&owner)
        .unwrap();
    let record = Record {
        schema: SCHEMA.into(),
        scope: scope.clone(),
        base_event_id: Some("ab".repeat(32)),
        claims: vec![Claim {
            id: "deployment".into(),
            state: ClaimState::Asserted,
            text: "B is the reported observation".into(),
            observed_at: 10,
            evidence: vec![Evidence {
                kind: 9,
                event_id: source.id.to_hex(),
                channel: scope.channel.clone(),
                sha256: value_hash(source.content.as_bytes()),
                excerpt: "observed B 🐝".into(),
            }],
            correction: Some(Correction {
                prior_head: "ab".repeat(32),
                prior_text: "Old A".into(),
                reason: "newer observation".into(),
            }),
        }],
    };
    let head = beekeeper_core::engram::build_event(
        &actor,
        &owner.public_key(),
        &record.to_body().unwrap(),
        20,
    )
    .unwrap();
    *control.events.lock().unwrap() = vec![head.clone(), source.clone()];
    let custody = Custody {
        rest,
        owner: owner.public_key(),
    };
    let first = collect(&custody, &scope).await;
    assert!(matches!(
        first.claims.as_slice(),
        [ClaimOutcome::AssertionWithReadableEvidence { .. }]
    ));
    let mut f = facts();
    f.scope = scope.clone();
    let bootstrap = prepare(f, &first).unwrap();
    assert!(bootstrap
        .payload()
        .contains("B is the reported observation"));
    assert!(bootstrap.payload().contains("prior_text"));
    *control.events.lock().unwrap() = vec![head];
    let second = collect(&custody, &scope).await;
    assert!(matches!(
        second.claims.as_slice(),
        [ClaimOutcome::Unknown { .. }]
    ));
    let queries = control.queries.lock().unwrap();
    assert!(queries.len() >= 4);
    let exact: Vec<_> = queries
        .iter()
        .filter(|q| q.to_string().contains(&source.id.to_hex()))
        .collect();
    assert_eq!(exact.len(), 2);
    for q in exact {
        assert!(q.to_string().contains("#h"));
        assert!(q.to_string().contains("kinds"));
    }
    server.abort();
}

#[cfg(unix)]
/// The admission answer a provider gives when it cannot split the
/// generation's automatic output: the owner-private channel did not open and
/// verify (`bind_failed`), or the split could not be persisted.
fn split_refusal(bind_failed: bool) -> Refusal {
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let mut provider = provider(&state, None);
    let record = governed_record(Uuid::new_v4(), dir.path(), &"ab".repeat(32));
    let session_id = record.session_id.clone();
    provider.state.insert_session(record).unwrap();
    let opened = if bind_failed {
        Err(crate::private_output::BindFailure::MembershipUnverified)
    } else {
        // A channel opened and verified, then a state directory the split
        // cannot be written to.
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o500)).unwrap();
        Ok(Uuid::new_v4())
    };
    let refusal = provider
        .record_private_output(&session_id, 1, "22".repeat(32), opened)
        .unwrap_err();
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&state, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    // Nothing was split, so nothing may be sent as if it were.
    assert!(provider.state.private_output(&session_id, 1).is_none());
    refusal
}

async fn adapter_case(mode: u8) {
    let fails = mode == 1;
    let refused = mode == 2 || mode == 3;
    let storage_failed = mode == 4;
    // 5: the split's channel did not bind; 6: the split did not persist.
    let split_failed = (mode == 5 || mode == 6).then(|| split_refusal(mode == 5));
    let dir = tempfile::tempdir().unwrap();
    let wire = dir.path().join("wire.jsonl");
    let script = GOOD_AGENT.replace(
        "while IFS= read -r line; do",
        &format!(
            "while IFS= read -r line; do\n printf '%s\\n' \"$line\" >> '{}'",
            wire.display()
        ),
    );
    let script = if fails {
        script.replace("\"result\":{\"stopReason\":\"end_turn\"}","\"error\":{\"code\":-32603,\"message\":\"Private direction; revision abc. Keep permissions.\"}")
    } else {
        script
    };
    let agent = fake_agent(dir.path(), "recording-agent", &script);
    let mut request = legacy_request(&agent, dir.path(), &Uuid::new_v4().to_string());
    request.seat = Some(crate::session::SeatIdentity {
        actor_pubkey: "11".repeat(32),
        role: String::new(),
        relay_url: "https://fixture.invalid".into(),
    });
    let target = request.target.clone();
    let (events, mut rx) = tokio::sync::mpsc::channel(128);
    let (started, _) =
        crate::session::SessionManager::start_observed_reconstructed(request, events.clone(), true)
            .await;
    let mut manager = crate::session::SessionManager::new(events);
    manager.attach(started.unwrap());
    let mut f = facts();
    f.target = target.clone();
    if mode == 2 {
        f.target.generation += 1;
    }
    if mode == 3 {
        f.command_id = "consumed-previous-command".into();
    }
    let mut prepared = prepare(f, &unknown()).unwrap();
    if storage_failed {
        prepared.persist_prepared(dir.path()).unwrap();
        crate::context_store::remove_context_packages(dir.path(), &target.session_id).unwrap();
    }
    manager
        .handle(&target.session_id)
        .unwrap()
        .deliver(crate::session::SessionCommand::Turn {
            command_id: "new-command".into(),
            text: "signed user text".into(),
            // 7: an attachment this host cannot read back, so the adapter is
            // also handed the undelivered note as its own text block.
            attachments: if mode == 7 {
                vec![beekeeper_core::coding_session_command::TurnAttachment {
                    sha256: "ab".repeat(32),
                    mime: "text/plain".into(),
                    size: 12,
                    dim: None,
                    filename: Some("notes.txt".into()),
                }]
            } else {
                vec![]
            },
            operator_pubkey: Some("22".repeat(32)),
            framing: None,
        })
        .unwrap();
    let mut phases = vec![];
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await.unwrap() {
                SessionEvent::ContinuityAdmissionRequested { decision, .. } => {
                    let answer = match split_failed {
                        Some(refusal) => Err(refusal),
                        None => Ok(prepared.clone()),
                    };
                    decision.send(Some(answer)).unwrap();
                }
                SessionEvent::ContinuityReceipt { receipt } => phases.push(receipt.phase),
                SessionEvent::TurnBaselineRequested { reply, .. } => {
                    drop(reply);
                }
                SessionEvent::TurnFinished { outcome, .. } => {
                    assert!(!format!("{outcome:?}").contains("Private direction"));
                    if fails || storage_failed {
                        assert!(matches!(
                            outcome,
                            crate::session::TurnOutcome::Failed { .. }
                        ));
                    }
                    break;
                }
                SessionEvent::ContinuityRefused { reason, .. } => {
                    match split_failed {
                        Some(expected) => {
                            assert_eq!(expected, Refusal::PrivateDeliveryUnavailable);
                            assert_eq!(reason, expected);
                        }
                        None => {
                            assert!(refused, "unexpected refusal {reason:?}");
                            assert_eq!(reason, Refusal::Unavailable);
                        }
                    }
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let raw = std::fs::read_to_string(wire).unwrap();
    if refused || storage_failed || split_failed.is_some() {
        assert!(!raw.contains("session/prompt"));
        assert!(phases.is_empty());
        assert_eq!(prepared.receipt().unwrap().phase, Phase::Prepared);
        manager.shutdown(&target.session_id);
        return;
    }
    let prompt: serde_json::Value = raw
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["method"] == "session/prompt")
        .unwrap();
    let text = prompt["params"]["prompt"][0]["text"].as_str().unwrap();
    let session_new = raw
        .lines()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .find(|v| v["method"] == "session/new")
        .unwrap();
    // Composed once: a first-turn briefing is kept (never cleared by the
    // continuity path), then the block, then the signed words, untruncated.
    let expected = match session_new["params"]["systemPrompt"].as_str() {
        Some(_) => format!("{}signed user text", prepared.payload()),
        None => {
            let (preamble, rest) = text
                .split_once(&format!("\n\n{}", prepared.payload()))
                .expect("the block follows the briefing");
            assert!(
                preamble.contains("Beekeeper"),
                "the runtime briefing leads: {preamble}"
            );
            assert_eq!(rest, "signed user text");
            assert_eq!(text.matches(preamble).count(), 1);
            text.to_owned()
        }
    };
    assert_eq!(text, expected);
    // The receipt counts what the adapter was handed: every text block,
    // after interleaving and the undelivered-attachment note.
    let handed: Vec<&str> = prompt["params"]["prompt"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|block| block["text"].as_str())
        .collect();
    assert_eq!(handed.len(), if mode == 7 { 2 } else { 1 });
    if mode == 7 {
        assert!(handed[1].starts_with("[Attachments]"), "{}", handed[1]);
    }
    assert_eq!(
        prepared.receipt().unwrap().prompt_bytes,
        Some(handed.iter().map(|t| t.len()).sum())
    );
    assert_eq!(
        phases,
        if fails {
            vec![Phase::Submitted]
        } else {
            vec![Phase::Submitted, Phase::Confirmed]
        }
    );
    assert_eq!(
        prepared.receipt().unwrap().phase,
        if fails {
            Phase::Submitted
        } else {
            Phase::Confirmed
        }
    );
    manager.shutdown(&target.session_id);
}

#[tokio::test]
async fn continuity_wrong_scope_and_missing_custody_perform_zero_queries() {
    let actor = Keys::generate();
    let owner = Keys::generate();
    let (relay, control, server) = spawn_recording_test_relay(&actor, vec![]).await;
    let mut rest = relay.rest_client();
    let right = Scope::new(&rest.base_url, &Uuid::nil().to_string(), None).unwrap();
    rest.auth_tag_json =
        Some(beekeeper_sdk::nip_oa::compute_auth_tag(&owner, &actor.public_key(), "").unwrap());
    let custody = Custody {
        rest: rest.clone(),
        owner: owner.public_key(),
    };
    assert_eq!(
        collect(&custody, &scope()).await.status,
        "unknown/custody_or_scope"
    );
    rest.auth_tag_json = None;
    assert_eq!(
        collect(
            &Custody {
                rest,
                owner: owner.public_key()
            },
            &right
        )
        .await
        .status,
        "unknown/custody_or_scope"
    );
    assert!(control.queries.lock().unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn continuity_role_current_pin_changes_or_disappears_refuse_old_staging() {
    let owner = Keys::generate();
    let (relay, control, server) = spawn_recording_test_relay(&owner, vec![]).await;
    let rest = relay.rest_client();
    let project = format!("30621:{}:fixture", owner.public_key().to_hex());
    let repo = format!("30617:{}:fixture-agents", owner.public_key().to_hex());
    let pack = crate::actor_seats::PackRef {
        repo: repo.clone(),
        sha: "a".repeat(40),
        role: "builder".into(),
        path: "roles/builder".into(),
    };
    let source = |sha: &str| {
        nostr::EventBuilder::new(
            Kind::Custom(30624),
            r#"{"schema":"buzz-project-pack-source/v1"}"#,
        )
        .tags([
            nostr::Tag::parse(["d", &project]).unwrap(),
            nostr::Tag::parse(["repo", &repo]).unwrap(),
            nostr::Tag::parse(["sha", sha]).unwrap(),
            nostr::Tag::parse(["path", "."]).unwrap(),
        ])
        .sign_with_keys(&owner)
        .unwrap()
    };
    *control.events.lock().unwrap() = vec![source(&pack.sha)];
    assert!(
        crate::continuity_collect::current_role_source(&rest, None, &project, &pack)
            .await
            .is_ok()
    );
    *control.events.lock().unwrap() = vec![source(&"b".repeat(40))];
    assert_eq!(
        crate::continuity_collect::current_role_source(&rest, None, &project, &pack).await,
        Err(Refusal::Unavailable)
    );
    control.events.lock().unwrap().clear();
    assert_eq!(
        crate::continuity_collect::current_role_source(&rest, None, &project, &pack).await,
        Err(Refusal::Unavailable)
    );
    server.abort();
}

#[tokio::test]
async fn continuity_stale_target_refuses_before_any_required_source_read() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(&dir.path().join("state"), None);
    let request = crate::continuity_delivery::Request {
        target: facts().target,
        command_id: "new-command".into(),
        text: "not an assignment".into(),
        operator: Some("22".repeat(32)),
        custody: Err(Refusal::Unavailable),
        role_definition: None,
        runtime_briefing: None,
        checkpoint: "none".into(),
    };
    assert!(matches!(
        provider.prepare_continuity(&request).await,
        Err(Refusal::Unavailable)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn continuity_adapter_receives_exact_private_bytes_and_phase_signals() {
    adapter_case(0).await;
}
#[cfg(unix)]
#[tokio::test]
async fn continuity_raw_adapter_error_does_not_leak_or_confirm() {
    adapter_case(1).await;
}

fn archived_fixture(
    dir: &std::path::Path,
) -> (
    SessionRecord,
    crate::continuity_archive::Archive,
    Vec<(String, String)>,
) {
    let actor = Keys::generate();
    let owner = Keys::generate();
    let mut record = governed_record(Uuid::nil(), dir, &"aa".repeat(32));
    record.actor = Some(actor.public_key().to_hex());
    record.resume_cursor = Some("acp-session-1".into());
    let binding = crate::continuity_archive::Binding::for_record(
        &record,
        "instance-1",
        "https://fixture.invalid",
        &owner.public_key().to_hex(),
    )
    .unwrap();
    let archive = crate::continuity_archive::Archive {
        binding,
        fragments: vec!["PRIVATE_RESTORE_CANARY".into()],
    };
    let env = vec![(
        "BEEKEEPER_AUTH_TAG".into(),
        beekeeper_sdk::nip_oa::compute_auth_tag(&owner, &actor.public_key(), "").unwrap(),
    )];
    (record, archive, env)
}
#[test]
fn continuity_archive_restart_preflight_refuses_missing_corrupt_or_mismatched_private_state() {
    let dir = tempfile::tempdir().unwrap();
    let (record, archive, env) = archived_fixture(dir.path());
    let restore = |r: &SessionRecord| {
        crate::continuity_archive::restore(
            dir.path(),
            r,
            "instance-1",
            "https://fixture.invalid",
            &env,
        )
    };
    assert!(
        restore(&record).unwrap().is_none(),
        "never continuity-bearing stays ordinary"
    );
    crate::context_store::write_continuity_archive(dir.path(), &archive).unwrap();
    assert_eq!(
        restore(&record).unwrap().unwrap().fragments,
        archive.fragments
    );
    // Ordinary context cleanup and a provider startup package sweep must not
    // erase a native conversation's still-required output privacy boundary.
    crate::context_store::remove_all_context_packages(dir.path()).unwrap();
    assert!(restore(&record).unwrap().is_some());
    let mut stale = record.clone();
    stale.generation += 1;
    assert!(restore(&stale).is_err());
    stale = record.clone();
    stale.resume_cursor = Some("other-native".into());
    assert!(restore(&stale).is_err());
    assert!(crate::continuity_archive::restore(
        dir.path(),
        &record,
        "instance-1",
        "https://another.invalid",
        &env
    )
    .is_err());
    assert!(crate::continuity_archive::restore(
        dir.path(),
        &record,
        "instance-1",
        "https://fixture.invalid",
        &[]
    )
    .is_err());
    let path = dir
        .path()
        .join("continuity-private")
        .join(format!("{}.archive", record.session_id));
    let pristine = std::fs::read(&path).unwrap();
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(restore(&record).is_err());
    std::fs::write(&path, &pristine).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(restore(&record).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    assert!(
        restore(&record).is_err(),
        "durable required marker cannot become ordinary"
    );
    crate::context_store::remove_continuity_archive(dir.path(), &record.session_id).unwrap();
}
#[cfg(unix)]
#[tokio::test]
async fn continuity_native_replacement_registers_before_output_without_reinjecting_bootstrap() {
    let dir = tempfile::tempdir().unwrap();
    let (record, archive, env) = archived_fixture(dir.path());
    crate::context_store::write_continuity_archive(dir.path(), &archive).unwrap();
    // A new provider/process reads the on-disk boundary, not an in-memory cache.
    let restored = crate::continuity_archive::restore(
        dir.path(),
        &record,
        "instance-1",
        "https://fixture.invalid",
        &env,
    )
    .unwrap()
    .unwrap();
    let wire = dir.path().join("native-wire.jsonl");
    let script=GOOD_AGENT.replace("\"protocolVersion\":2","\"protocolVersion\":2,\"agentCapabilities\":{\"loadSession\":true}")
        .replace("    *'\"method\":\"session/new\"'*)","    *'\"method\":\"session/load\"'*)\n      printf '{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{}}\\n' \"$id\" ;;\n    *'\"method\":\"session/new\"'*)")
        .replace("\"text\":\"working\"","\"text\":\"PRIVATE_RESTORE_CANARY\"")
        .replace("while IFS= read -r line; do",&format!("while IFS= read -r line; do\n printf '%s\\n' \"$line\" >> '{}'",wire.display()));
    let agent = fake_agent(dir.path(), "native-private", &script);
    let mut request = legacy_request(&agent, dir.path(), &record.session_id);
    request.resume_cursor = record.resume_cursor.clone();
    request.strict_native = true;
    let (events, mut rx) = tokio::sync::mpsc::channel(128);
    let (started, _) = crate::session::SessionManager::start_observed_private(
        request,
        events.clone(),
        false,
        restored.fragments,
    )
    .await;
    let mut manager = crate::session::SessionManager::new(events);
    let startup = manager.attach(started.unwrap());
    assert_eq!(
        startup.continuity,
        crate::session::SessionContinuity::Loaded
    );
    manager
        .handle(&record.session_id)
        .unwrap()
        .deliver(crate::session::SessionCommand::Turn {
            command_id: "native-command".into(),
            text: "ordinary followup".into(),
            attachments: vec![],
            operator_pubkey: None,
            framing: None,
        })
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let event = rx.recv().await.unwrap();
            assert!(!format!("{event:?}").contains("PRIVATE_RESTORE_CANARY"));
            match event {
                SessionEvent::TurnBaselineRequested { reply, .. } => drop(reply),
                SessionEvent::ContinuityAdmissionRequested { .. } => {
                    panic!("native restore must not inject reconstruction")
                }
                SessionEvent::TurnFinished { .. } => break,
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let raw = std::fs::read_to_string(wire).unwrap();
    assert!(!raw.contains("[Private continuity"));
    assert!(raw.contains("session/load"));
    assert!(!raw.contains("session/new"));
    manager.shutdown(&record.session_id);
}

#[cfg(unix)]
#[tokio::test]
async fn continuity_stale_generation_or_command_attachment_makes_zero_adapter_calls() {
    adapter_case(2).await;
    adapter_case(3).await;
}

#[tokio::test]
async fn continuity_consumed_command_is_not_reconstructed_or_replayed() {
    let dir = tempfile::tempdir().unwrap();
    let mut provider = provider(&dir.path().join("state"), None);
    let mut record = governed_record(Uuid::nil(), dir.path(), &"aa".repeat(32));
    record.actor = Some("11".repeat(32));
    let target = provider.target_for(&record);
    provider.state.insert_session(record).unwrap();
    provider
        .state
        .consume_command("already-consumed", now_secs())
        .unwrap();
    let request = crate::continuity_delivery::Request {
        target,
        command_id: "already-consumed".into(),
        text: "assignment pointer".into(),
        operator: Some("22".repeat(32)),
        custody: Err(Refusal::Unavailable),
        role_definition: None,
        runtime_briefing: None,
        checkpoint: "none".into(),
    };
    assert!(matches!(
        provider.prepare_continuity(&request).await,
        Err(Refusal::Unavailable)
    ));
    assert!(provider.state.is_command_consumed("already-consumed"));
    assert!(!dir.path().join("state/continuity-private").exists());
}

#[cfg(unix)]
#[tokio::test]
async fn continuity_failed_native_restore_never_silently_opens_new() {
    for private in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let (record, archive, env) = archived_fixture(dir.path());
        crate::context_store::write_continuity_archive(dir.path(), &archive).unwrap();
        let restored = crate::continuity_archive::restore(
            dir.path(),
            &record,
            "instance-1",
            "https://fixture.invalid",
            &env,
        )
        .unwrap()
        .unwrap();
        let wire = dir.path().join("wire");
        let script = GOOD_AGENT
            .replace("\"protocolVersion\":2", "\"protocolVersion\":2,\"agentCapabilities\":{\"loadSession\":true,\"sessionCapabilities\":{\"resume\":{}}}")
            .replace("    *'\"method\":\"session/new\"'*)", "    *'\"method\":\"session/resume\"'*|*'\"method\":\"session/load\"'*)\n      printf '{\"jsonrpc\":\"2.0\",\"id\":%s,\"error\":{\"code\":-32603,\"message\":\"rejected\"}}\\n' \"$id\" ;;\n    *'\"method\":\"session/new\"'*)")
            .replace("while IFS= read -r line; do", &format!("while IFS= read -r line; do\n printf '%s\\n' \"$line\" >> '{}'", wire.display()));
        let agent = fake_agent(dir.path(), "reject-native", &script);
        let mut request = legacy_request(&agent, dir.path(), &record.session_id);
        request.resume_cursor = record.resume_cursor;
        // Exercise the original permissive caller: private history must force
        // strict restore at the common open boundary too.
        request.strict_native = false;
        let (events, _rx) = tokio::sync::mpsc::channel(128);
        let (result, _) = crate::session::SessionManager::start_observed_private(
            request,
            events.clone(),
            false,
            if private { restored.fragments } else { vec![] },
        )
        .await;
        let raw = std::fs::read_to_string(wire).unwrap();
        assert!(raw.contains("session/resume"));
        assert!(raw.contains("session/load"));
        assert!(!raw.contains("session/prompt"));
        if private {
            assert!(
                result.is_err(),
                "private native failure silently opened new"
            );
            assert!(!raw.contains("session/new"));
            assert_eq!(
                result.err().unwrap().code,
                crate::native_restore::NATIVE_RESTORE_REJECTED
            );
        } else {
            let mut manager = crate::session::SessionManager::new(events);
            let startup = manager.attach(result.unwrap());
            assert!(matches!(
                startup.continuity,
                crate::session::SessionContinuity::RestartedWithoutContext { .. }
            ));
            assert!(raw.contains("session/new"));
            manager.shutdown(&record.session_id);
        }
    }
}

#[cfg(unix)]
#[tokio::test]
async fn continuity_saturated_receipt_queue_keeps_durable_unknown_after_error_cancel_restart() {
    for cancel in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(dir.path())
            .status()
            .unwrap()
            .success());
        let wire = dir.path().join("wire");
        let script = GOOD_AGENT.replace(
            "while IFS= read -r line; do",
            &format!(
                "while IFS= read -r line; do\n printf '%s\\n' \"$line\" >> '{}'",
                wire.display()
            ),
        );
        let response = "      printf '{\"jsonrpc\":\"2.0\",\"id\":%s,\"result\":{\"stopReason\":\"end_turn\"}}\\n' \"$id\" ;;";
        let script = if cancel {
            script.replace(response, "      : ;;")
        } else {
            script.replace(
                "\"result\":{\"stopReason\":\"end_turn\"}",
                "\"error\":{\"code\":-32603,\"message\":\"rejected\"}",
            )
        };
        let agent = fake_agent(dir.path(), "phase-agent", &script);
        let mut request = legacy_request(&agent, dir.path(), &Uuid::new_v4().to_string());
        request.seat = Some(crate::session::SeatIdentity {
            actor_pubkey: "11".repeat(32),
            role: String::new(),
            relay_url: "https://fixture.invalid".into(),
        });
        let target = request.target.clone();
        // BaselineSettled fills this sole slot before the prompt is polled.
        let (events, mut rx) = tokio::sync::mpsc::channel(1);
        let (started, _) = crate::session::SessionManager::start_observed_reconstructed(
            request,
            events.clone(),
            true,
        )
        .await;
        let mut manager = crate::session::SessionManager::new(events);
        manager.attach(started.unwrap());
        let mut f = facts();
        f.target = target.clone();
        let mut prepared = prepare(f, &unknown()).unwrap();
        prepared.persist_prepared(dir.path()).unwrap();
        manager
            .handle(&target.session_id)
            .unwrap()
            .deliver(crate::session::SessionCommand::Turn {
                command_id: "new-command".into(),
                text: "ordinary request".into(),
                attachments: vec![],
                operator_pubkey: None,
                framing: None,
            })
            .unwrap();
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                match rx.recv().await.unwrap() {
                    SessionEvent::ContinuityAdmissionRequested { decision, .. } => {
                        decision.send(Some(Ok(prepared.clone()))).unwrap();
                    }
                    SessionEvent::TurnBaselineRequested { reply, .. } => {
                        drop(reply);
                        break;
                    }
                    _ => {}
                }
            }
            // Deliberately do not drain the provider queue until the adapter
            // actually sees the prompt. Queue admission cannot be the evidence.
            loop {
                if std::fs::read_to_string(&wire)
                    .unwrap_or_default()
                    .contains("session/prompt")
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(rx.len(), 1);
        assert!(matches!(
            rx.try_recv(),
            Ok(SessionEvent::BaselineSettled { .. })
        ));
        let package = dir.path().join("context-packages").join(&target.session_id);
        let disk = || {
            std::fs::read_dir(&package)
                .unwrap()
                .map(|entry| std::fs::read_to_string(entry.unwrap().path()).unwrap())
                .collect::<Vec<_>>()
                .join("\n")
        };
        let receipts = disk();
        assert!(
            receipts.contains("\"phase\":\"submission_intent\""),
            "no durable conservative intent"
        );
        assert!(
            receipts.contains("\"phase\":\"submitted\""),
            "full queue lost submission evidence"
        );
        assert!(!receipts.contains("\"phase\":\"confirmed\""));
        if cancel {
            manager
                .handle(&target.session_id)
                .unwrap()
                .deliver(crate::session::SessionCommand::Interrupt {
                    command_id: "cancel-command".into(),
                })
                .unwrap();
        }
        tokio::time::timeout(Duration::from_secs(10), async {
            loop {
                if matches!(rx.recv().await.unwrap(), SessionEvent::TurnFinished { .. }) {
                    break;
                }
            }
        })
        .await
        .unwrap();
        manager.shutdown(&target.session_id);
        assert!(!disk().contains("\"phase\":\"confirmed\""));
        // A fresh attachment after restart cannot overwrite the consumed phase
        // files and rearm this command. No implicit replay from receipt data.
        let mut restarted_facts = facts();
        restarted_facts.target = target;
        let mut restarted = prepare(restarted_facts, &unknown()).unwrap();
        assert!(restarted.persist_prepared(dir.path()).is_err());
        assert_eq!(
            std::fs::read_to_string(wire)
                .unwrap()
                .lines()
                .filter(|l| l.contains("session/prompt"))
                .count(),
            1
        );
    }
}

#[cfg(unix)]
#[tokio::test]
async fn continuity_receipt_storage_failure_prevents_adapter_send() {
    adapter_case(4).await;
}

/// `promptBytes` was measured before attachment interleaving and the note, so
/// it described a prompt the adapter was never handed.
#[cfg(unix)]
#[tokio::test]
async fn continuity_receipt_counts_the_final_prompt_including_an_undelivered_attachment_note() {
    adapter_case(7).await;
}

/// A generation is admitted private continuity only once its automatic output
/// is split. A channel that does not bind, or a split that does not persist,
/// is a refusal by name and the adapter is never prompted.
#[cfg(unix)]
#[tokio::test]
async fn continuity_split_bind_or_persist_failure_makes_zero_adapter_calls() {
    adapter_case(5).await;
    adapter_case(6).await;
}

fn runtime(bytes: usize, transport: &'static str) -> crate::continuity_bootstrap::RuntimeBinding {
    crate::continuity_bootstrap::RuntimeBinding {
        bytes,
        digest: "cd".repeat(32),
        transport,
    }
}

#[test]
fn continuity_dynamic_bytes_are_identical_across_system_slot_and_first_turn_transports() {
    let mut system = facts();
    system.runtime = runtime(6000, "system_prompt");
    let mut first = facts();
    first.runtime = runtime(6000, "first_turn");
    let (system, first) = (
        prepare(system, &unknown()).unwrap(),
        prepare(first, &unknown()).unwrap(),
    );
    assert_eq!(system.payload(), first.payload());
    assert_eq!(system.receipt().unwrap().runtime_transport, "system_prompt");
    assert_eq!(first.receipt().unwrap().runtime_transport, "first_turn");
    assert_eq!(first.receipt().unwrap().runtime_bytes, 6000);
    // The briefing is bound, not copied, and is not counted in the block's bound.
    assert!(first.payload().contains(&"cd".repeat(32)));
    assert!(first.receipt().unwrap().bytes < MAX_BOOTSTRAP_BYTES);
}

#[test]
fn continuity_runtime_briefing_has_its_own_32768_bound_on_both_transports() {
    use crate::continuity_bootstrap::MAX_RUNTIME_BRIEFING_BYTES;
    assert_eq!(MAX_RUNTIME_BRIEFING_BYTES, 32768);
    for transport in ["system_prompt", "first_turn"] {
        // The measured canonical role seat (ledger 394) now fits.
        let mut f = facts();
        f.runtime = runtime(19897, transport);
        assert!(prepare(f, &unknown()).is_ok(), "{transport}");
        let mut f = facts();
        f.runtime = runtime(32768, transport);
        let prepared = prepare(f, &unknown()).expect(transport);
        // The receipt still carries the briefing's actual bytes and transport.
        let receipt = prepared.receipt().unwrap();
        assert_eq!(receipt.runtime_bytes, 32768);
        assert_eq!(receipt.runtime_transport, transport);
        assert_eq!(receipt.runtime_digest, "cd".repeat(32));
        let mut f = facts();
        f.runtime = runtime(32769, transport);
        let refusal = prepare(f, &unknown()).unwrap_err();
        assert_eq!(refusal, Refusal::RuntimeBriefingTooLarge { bytes: 32769 });
        assert_eq!(refusal.code(), "CONTINUITY_RUNTIME_BRIEFING_TOO_LARGE");
        assert_eq!(
            refusal.overflow().unwrap(),
            "runtime briefing is 32769 UTF-8 bytes, over its 32768-byte bound by 1"
        );
    }
    // A large briefing never consumes the dynamic block's bound.
    let big = || {
        let mut f = facts();
        f.runtime = runtime(32768, "first_turn");
        f
    };
    let overhead = prepare(big(), &unknown()).unwrap().payload().len();
    let mut f = big();
    f.work.push_str(&"x".repeat(MAX_BOOTSTRAP_BYTES - overhead));
    assert_eq!(prepare(f, &unknown()).unwrap().payload().len(), 16384);
}

#[test]
fn continuity_role_text_is_bound_by_digest_and_delivered_once() {
    use crate::continuity_delivery::{RoleDefinition, RuntimeBriefing};
    use crate::session::BootstrapTransport;
    let definition = "You are the builder. Private role rule R-77: never share bench codes.";
    let role = RoleDefinition {
        pack: None,
        compose: None,
        text: format!("{definition}\n"),
    };
    let briefing = RuntimeBriefing {
        text: format!(
            "Beekeeper session briefing.\nYour role pack:\n{definition}\nBoundary: fenced."
        ),
        transport: BootstrapTransport::FirstTurn,
    };
    let line = crate::continuity_collect::role_binding(
        Some("builder"),
        &role,
        &briefing,
        "cmd",
        &"22".repeat(32),
    )
    .unwrap();
    assert!(
        !line.contains("R-77"),
        "the dynamic block repeats the role: {line}"
    );
    assert!(line.contains(&value_hash(definition.as_bytes())));
    assert!(line.contains(&value_hash(briefing.text.as_bytes())));
    let mut f = facts();
    f.directions = line;
    f.runtime = briefing.binding();
    let prepared = prepare(f, &unknown()).unwrap();
    let composed = format!("{}\n\n{}signed", briefing.text, prepared.payload());
    assert_eq!(composed.matches("R-77").count(), 1);
    // Verified, not asserted: a briefing without the staged text refuses.
    let other = RuntimeBriefing {
        text: "Beekeeper session briefing without the role.".into(),
        transport: BootstrapTransport::SystemPrompt,
    };
    assert_eq!(
        crate::continuity_collect::role_binding(Some("builder"), &role, &other, "cmd", "op"),
        Err(Refusal::Unavailable)
    );
    let empty = RoleDefinition {
        pack: None,
        compose: None,
        text: "  \n".into(),
    };
    assert_eq!(
        crate::continuity_collect::role_binding(Some("builder"), &empty, &briefing, "cmd", "op"),
        Err(Refusal::Unavailable)
    );
}

#[test]
fn continuity_block_states_the_enforced_split_and_the_deliberate_rule() {
    let prepared = prepare(facts(), &unknown()).unwrap();
    let payload = prepared.payload();
    assert!(payload.contains(crate::continuity_bootstrap::AUDIENCE));
    assert!(payload.contains("That split is enforced."));
    assert!(payload.contains("is not filtered"));
}
