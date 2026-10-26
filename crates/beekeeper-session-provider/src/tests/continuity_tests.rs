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
    let mut f = facts();
    let overhead = prepare(facts(), &unknown()).unwrap().payload().len();
    f.work.push_str(&"x".repeat(MAX_BOOTSTRAP_BYTES - overhead));
    let bounded = prepare(f, &unknown()).unwrap();
    assert_eq!(bounded.payload().len(), 8192);
    assert!(bounded.payload().contains("Required decision"));
    assert!(bounded.payload().contains("Keep permissions"));
    let mut f = facts();
    f.work
        .push_str(&"x".repeat(MAX_BOOTSTRAP_BYTES - overhead + 1));
    assert_eq!(prepare(f, &unknown()).unwrap_err(), Refusal::TooLarge);
    let mut f = facts();
    f.work
        .push_str(&"🐝".repeat((MAX_BOOTSTRAP_BYTES - overhead) / 4 + 1));
    assert_eq!(prepare(f, &unknown()).unwrap_err(), Refusal::TooLarge);
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
async fn adapter_case(mode: u8) {
    let fails = mode == 1;
    let refused = mode == 2 || mode == 3;
    let storage_failed = mode == 4;
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
            attachments: vec![],
            operator_pubkey: Some("22".repeat(32)),
            framing: None,
        })
        .unwrap();
    let mut phases = vec![];
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match rx.recv().await.unwrap() {
                SessionEvent::ContinuityAdmissionRequested { decision, .. } => {
                    decision.send(Some(Ok(prepared.clone()))).unwrap();
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
                    assert!(refused, "unexpected refusal {reason:?}");
                    assert_eq!(reason, Refusal::Unavailable);
                    break;
                }
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    let raw = std::fs::read_to_string(wire).unwrap();
    if refused || storage_failed {
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
    assert_eq!(text, format!("{}signed user text", prepared.payload()));
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
        runtime_preface: None,
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
        runtime_preface: None,
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
