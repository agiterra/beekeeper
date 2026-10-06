//! The four drop rules of [`still_owed`], one at a time, plus the pointer
//! readers the dequeue side depends on.

use super::*;

fn fact(id: &str, author: Option<&str>, created_at: Option<u64>, reopens: bool) -> WakeFact {
    WakeFact {
        fact_id: id.to_owned(),
        author: author.map(str::to_owned),
        created_at,
        reopens_work: reopens,
    }
}

fn view(recipient: &str) -> RelevanceView {
    RelevanceView {
        recipient: recipient.to_owned(),
        ..RelevanceView::default()
    }
}

#[test]
fn a_foreign_unconsumed_fact_on_an_open_umbrella_is_owed() {
    assert_eq!(
        still_owed(&fact("aa", Some("builder"), Some(10), false), &view("lead")),
        Ok(())
    );
}

#[test]
fn the_recipients_own_fact_is_self_authored() {
    assert_eq!(
        still_owed(&fact("aa", Some("lead"), Some(10), true), &view("lead")),
        Err(WakeDropReason::SelfAuthored)
    );
}

#[test]
fn a_fact_the_recipient_cites_is_already_consumed() {
    let mut view = view("lead");
    view.cited_by_recipient.insert("aa".into());
    assert_eq!(
        still_owed(&fact("aa", Some("host"), None, true), &view),
        Err(WakeDropReason::AlreadyConsumed)
    );
}

#[test]
fn a_delivered_fact_is_a_duplicate() {
    let mut view = view("lead");
    view.delivered.insert("aa".into());
    assert_eq!(
        still_owed(&fact("aa", Some("builder"), Some(1), true), &view),
        Err(WakeDropReason::Duplicate)
    );
}

#[test]
fn after_a_terminal_only_newer_contesting_facts_are_owed() {
    let mut view = view("lead");
    view.terminal = Some(TerminalMark {
        event_id: "tt".into(),
        created_at: 100,
    });
    // Bookkeeping, whenever it was signed.
    assert_eq!(
        still_owed(&fact("r", Some("builder"), Some(200), false), &view),
        Err(WakeDropReason::PostTerminal)
    );
    // Contesting, but already known when the lead completed.
    assert_eq!(
        still_owed(&fact("d", Some("verifier"), Some(99), true), &view),
        Err(WakeDropReason::PostTerminal)
    );
    // Contesting and newer, or of unprovable order: owed.
    assert_eq!(
        still_owed(&fact("d", Some("verifier"), Some(101), true), &view),
        Ok(())
    );
    assert_eq!(
        still_owed(&fact("d", Some("verifier"), Some(100), true), &view),
        Ok(())
    );
    assert_eq!(still_owed(&fact("h", None, None, true), &view), Ok(()));
}

#[test]
fn cited_by_reads_every_event_id_the_author_signed_and_nothing_else() {
    let lead = nostr::Keys::generate();
    let other = nostr::Keys::generate();
    let cited = "ab".repeat(32);
    let foreign = "cd".repeat(32);
    let own = nostr::EventBuilder::new(
        nostr::Kind::Custom(1),
        serde_json::json!({"body": {"evidenceRefs": [{"eventId": cited}]}}).to_string(),
    )
    .sign_with_keys(&lead)
    .expect("sign");
    let theirs = nostr::EventBuilder::new(
        nostr::Kind::Custom(1),
        serde_json::json!({"reportRef": foreign}).to_string(),
    )
    .sign_with_keys(&other)
    .expect("sign");
    let ids = cited_by(&[own, theirs], &lead.public_key().to_hex());
    assert!(ids.contains(&cited));
    assert!(
        !ids.contains(&foreign),
        "another author's citations are not the lead's"
    );
}

#[test]
fn a_host_result_pointer_reopens_only_when_it_did_not_exit_clean() {
    let pointer = |disposition: &str, exit: i64| {
        serde_json::json!({
            "schema": crate::host_result_wake::HOST_RESULT_WAKE_SCHEMA,
            "type": crate::host_result_wake::HOST_RESULT_WAKE_TYPE,
            "runId": "run-1",
            "stepId": "verify",
            "disposition": disposition,
            "resultEventId": "ef".repeat(32),
            "exitCode": exit,
        })
        .to_string()
    };
    let clean = fact_of_pointer(&pointer("exited", 0), &[]).expect("a pointer");
    assert_eq!(clean.fact_id, "ef".repeat(32));
    assert!(!clean.reopens_work);
    let failed = fact_of_pointer(&pointer("exited", 1), &[]).expect("a pointer");
    assert!(failed.reopens_work);
    assert!(fact_of_pointer("prose", &[]).is_none());
}

#[test]
fn every_drop_reason_has_its_stable_slug() {
    assert_eq!(
        [
            WakeDropReason::AlreadyConsumed,
            WakeDropReason::SelfAuthored,
            WakeDropReason::PostTerminal,
            WakeDropReason::Duplicate,
        ]
        .map(WakeDropReason::as_str),
        [
            "already_consumed",
            "self_authored",
            "post_terminal",
            "duplicate",
        ]
    );
}

// ── Ledger 272(d) / 275 A3: delivery is proved, never inferred ───────────────────────────
//
// Run 11 (lead seq 88–104): the verifier's refutation 252522f7 of builder
// report 516bc23c woke the lead, whose turn ran 1257–1290 s. The verifier's
// settlement report 7dc53a6b — on its own assignment b97fc46c, whose base
// is the refuted report's head — was signed at 1269, inside that turn, and
// woke the lead again at 1291 "just to say it was already waiting"
// ($0.0517). Both facts are about one obligation: the verifier's assignment.

mod obligation {
    use super::*;
    use beekeeper_core::coding_session_command::CodingSessionTarget;
    use beekeeper_core::coding_session_context::{
        CodingSessionContextHistoryItem, CodingSessionContextIdentity,
        CodingSessionContextInboxItem, CodingSessionContextPackage, CodingSessionContextProvenance,
        CodingSessionContextRole, CodingSessionContextRosterEntry, CodingSessionContextSeatStatus,
        CODING_SESSION_CONTEXT_PACKAGE_VERSION,
    };
    use beekeeper_core::coding_session_payload::ReceiptStatus;
    use beekeeper_core::coding_session_team_transaction::{
        fold_coding_session_team_transactions, CodingSessionTeamActiveSeat,
        CodingSessionTeamAssignment, CodingSessionTeamRefutationDecision, CodingSessionTeamReport,
        CodingSessionTeamSettlementLink, CodingSessionTeamTransactionBody,
        CodingSessionTeamVerdict,
    };
    use beekeeper_sdk::coding_session_team_transaction::{
        build_coding_session_team_transaction, coding_session_team_transaction_payload,
    };
    use nostr::{Keys, Timestamp};

    const CHANNEL: &str = "04a7d016-57e3-4c15-b262-4655ffd8549f";
    const SESSION: &str = "25dd86fe-0364-4bde-80ca-428af75ce584";
    const HEAD: &str = "7c54cd1ff680aaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    fn genesis() -> String {
        "9f".repeat(32)
    }

    fn signed(body: CodingSessionTeamTransactionBody, keys: &Keys, at: u64) -> Event {
        let payload = coding_session_team_transaction_payload(
            SESSION.to_owned(),
            genesis(),
            None,
            None,
            body,
        );
        build_coding_session_team_transaction(CHANNEL, payload)
            .expect("the record builds")
            .custom_created_at(Timestamp::from_secs(at))
            .sign_with_keys(keys)
            .expect("the record signs")
    }

    fn assignment(
        assignee: &Keys,
        role: &str,
        base_sha: Option<&str>,
    ) -> CodingSessionTeamTransactionBody {
        CodingSessionTeamTransactionBody::Assignment(CodingSessionTeamAssignment {
            assignee_actor: assignee.public_key().to_hex(),
            assignee_role: role.into(),
            objective: "Deliver the slice".into(),
            brief: "Do the bounded work.".into(),
            branch: None,
            base_sha: base_sha.map(str::to_owned),
            file_ownership: vec!["src".into()],
            acceptance_steps: vec!["run the tests".into()],
        })
    }

    fn report(assignment_ref: &str, head_sha: Option<&str>) -> CodingSessionTeamTransactionBody {
        CodingSessionTeamTransactionBody::Report(CodingSessionTeamReport {
            assignment_ref: assignment_ref.into(),
            summary: "Done".into(),
            branch: None,
            base_sha: head_sha.map(str::to_owned),
            head_sha: head_sha.map(str::to_owned),
            files: Vec::new(),
            tests: Vec::new(),
            red_before_green: None,
            deviations: Vec::new(),
            residuals: Vec::new(),
            anomalies: Vec::new(),
        })
    }

    fn refutation(assignment_ref: &str, report_ref: &str) -> CodingSessionTeamTransactionBody {
        CodingSessionTeamTransactionBody::Verdict(CodingSessionTeamVerdict::Refutation {
            assignment_ref: assignment_ref.into(),
            report_ref: report_ref.into(),
            decision: CodingSessionTeamRefutationDecision::Confirmed,
            summary: "Line numbers are wrong".into(),
            findings: vec!["bad UTF-8 on line 3 reports line 2".into()],
            required_action: Some("fix the line count".into()),
        })
    }

    fn lead_target() -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "claude-agent-acp".into(),
            instance_id: "1958c6c448e05eed".into(),
            session_id: "3c66204a-bb4e-4e36-98fe-e3b4ce7e68d5".into(),
            generation: 1,
        }
    }

    struct Run {
        lead: Keys,
        builder: Keys,
        verifier: Keys,
        events: Vec<Event>,
        verifier_assignment: String,
        verdict: Event,
        verifier_report: Event,
    }

    /// The run-11 sequence, with the verifier's settlement report signed at
    /// `report_at`.
    fn run(report_at: u64) -> Run {
        let (lead, builder, verifier) = (Keys::generate(), Keys::generate(), Keys::generate());
        let builder_assignment = signed(assignment(&builder, "builder", None), &lead, 100);
        let builder_report = signed(
            report(&builder_assignment.id.to_hex(), Some(HEAD)),
            &builder,
            155,
        );
        let verifier_assignment = signed(assignment(&verifier, "verifier", Some(HEAD)), &lead, 174);
        let verdict = signed(
            refutation(&builder_assignment.id.to_hex(), &builder_report.id.to_hex()),
            &verifier,
            256,
        );
        let verifier_report = signed(
            report(&verifier_assignment.id.to_hex(), Some(HEAD)),
            &verifier,
            report_at,
        );
        Run {
            events: vec![
                builder_assignment,
                builder_report,
                verifier_assignment.clone(),
                verdict.clone(),
                verifier_report.clone(),
            ],
            verifier_assignment: verifier_assignment.id.to_hex(),
            lead,
            builder,
            verifier,
            verdict,
            verifier_report,
        }
    }

    /// The lead's package after the verdict's wake ran as turn `t-verdict`,
    /// 257–290 s. `ended` false leaves the turn without its `result`.
    fn package(lead: &Keys, verdict: &str, ended: bool) -> CodingSessionContextPackage {
        let command_id = format!("cli-wake-v1:{verdict}:f1e8076482ec");
        let pointer = serde_json::json!({ "operationId": verdict, "type": "verdict" }).to_string();
        let target = lead_target();
        let history_item = |seq: u64, at: u64, kind: &str, content: serde_json::Value| {
            CodingSessionContextHistoryItem {
                event_id: format!("{seq:064x}"),
                created_at: at,
                author: "1958c6c448e05eed".repeat(4),
                source_kind: 44225,
                target: target.clone(),
                event_seq: seq,
                turn_id: Some("t-verdict".into()),
                role: if kind == "user_prompt" {
                    CodingSessionContextRole::User
                } else {
                    CodingSessionContextRole::Lifecycle
                },
                item_kind: kind.into(),
                content,
            }
        };
        let mut history = vec![history_item(
            88,
            257,
            "user_prompt",
            serde_json::json!({ "kind": "user_prompt", "commandId": command_id, "content": pointer }),
        )];
        if ended {
            history.push(history_item(
                98,
                290,
                "result",
                serde_json::json!({ "kind": "result", "subtype": "success" }),
            ));
        }
        CodingSessionContextPackage {
            v: CODING_SESSION_CONTEXT_PACKAGE_VERSION,
            session: CodingSessionContextIdentity {
                session_ref: SESSION.into(),
                genesis_ref: genesis(),
                channel_id: CHANNEL.parse().expect("uuid"),
                name: None,
                goal: None,
                project_ref: None,
            },
            provenance: CodingSessionContextProvenance {
                generated_at: 1,
                complete_as_of: Some(1),
                complete: true,
                truncated: false,
                source_event_count: 0,
                source_event_breakdown: None,
                included_history_items: 0,
                omitted_history_items: 0,
                total_history_items: Some(0),
                notes: Vec::new(),
            },
            history,
            roster: vec![CodingSessionContextRosterEntry {
                target: target.clone(),
                actor: Some(lead.public_key().to_hex()),
                role: Some("lead".into()),
                status: CodingSessionContextSeatStatus::Active,
                last_signed_seq: Some(98),
                last_signed_at_ms: Some(290_000),
            }],
            inbox: vec![CodingSessionContextInboxItem {
                event_id: "1f".repeat(32),
                created_at: 256,
                command_id,
                sender: "2f".repeat(32),
                sender_role: None,
                target,
                delivery: "boundary".into(),
                content: pointer,
                stage: Some(ReceiptStatus::TurnStarted),
                stage_at: Some(257),
                stage_code: None,
            }],
            policy: None,
        }
    }

    fn view(run: &Run, ended: bool) -> RelevanceView {
        let lead = run.lead.public_key().to_hex();
        let package = package(&run.lead, &run.verdict.id.to_hex(), ended);
        let mut view = view_for(&lead, None, &[&run.events]);
        view.delivered = delivered_to(&package, &lead);
        view
    }

    /// Astra's audit A3 (ledger 275): run 11's settlement report, signed
    /// while the lead's verdict turn was running, may have arrived after the
    /// lead's last read — its signing time proves neither delivery nor
    /// consumption, and its disposition is still owed. It keeps its wake; only
    /// the verdict itself, re-sent, is a duplicate.
    #[test]
    fn a_report_signed_during_the_verdicts_turn_keeps_its_wake() {
        let run = run(269);
        let view = view(&run, true);
        assert!(
            view.delivered.contains(&run.verdict.id.to_hex()),
            "the verdict's own id is delivered: {:?}",
            view.delivered
        );
        let report = team_fact(&run.verifier_report.id.to_hex(), &run.events);
        assert_eq!(still_owed(&report, &view), Ok(()));
        let verdict = team_fact(&run.verdict.id.to_hex(), &run.events);
        assert_eq!(still_owed(&verdict, &view), Err(WakeDropReason::Duplicate));
    }

    /// A delivering turn that failed delivered nothing — the same fact re-sent
    /// (a person's explicit re-wake included) gets a turn.
    #[test]
    fn a_failed_delivering_turn_delivers_nothing() {
        let run = run(269);
        let lead = run.lead.public_key().to_hex();
        let mut package = package(&run.lead, &run.verdict.id.to_hex(), true);
        for item in &mut package.history {
            if item.item_kind == "result" {
                item.content = serde_json::json!({ "kind": "result", "subtype": "error" });
            }
        }
        let mut view = view_for(&lead, None, &[&run.events]);
        view.delivered = delivered_to(&package, &lead);
        assert!(view.delivered.is_empty());
        let verdict = team_fact(&run.verdict.id.to_hex(), &run.events);
        assert_eq!(still_owed(&verdict, &view), Ok(()));
    }

    /// A delivering turn with no `result` yet has not delivered its fact.
    #[test]
    fn an_unended_delivering_turn_delivers_nothing() {
        let run = run(269);
        let view = view(&run, false);
        assert!(view.delivered.is_empty());
        let verdict = team_fact(&run.verdict.id.to_hex(), &run.events);
        assert_eq!(still_owed(&verdict, &view), Ok(()));
    }

    /// Why that report's wake is kept: the lead's ruling on it is still owed
    /// in the fold's awaiting set, and no later event is guaranteed to wake
    /// the lead for it.
    #[test]
    fn the_reports_ruling_is_still_owed_so_its_wake_is_kept() {
        let run = run(269);
        let context =
            beekeeper_core::coding_session_team_transaction::CodingSessionTeamFoldContext {
                channel_ref: CHANNEL.into(),
                session_ref: SESSION.into(),
                genesis_ref: genesis(),
                founder_pubkey: Keys::generate().public_key().to_hex(),
                active_seats: [
                    (&run.lead, "lead"),
                    (&run.builder, "builder"),
                    (&run.verifier, "verifier"),
                ]
                .into_iter()
                .map(|(keys, role)| CodingSessionTeamActiveSeat {
                    actor_pubkey: keys.public_key().to_hex(),
                    role: role.into(),
                })
                .collect(),
                active_grants: Vec::new(),
                verifier_required: false,
            };
        let fold = fold_coding_session_team_transactions(&run.events, &context).expect("fold");
        let settlement = fold
            .assignments
            .iter()
            .find(|settlement| settlement.assignment_event_id == run.verifier_assignment)
            .expect("the verifier's assignment is folded");
        let awaiting = settlement.awaiting.as_ref().expect("still awaiting");
        assert_eq!(awaiting.link, CodingSessionTeamSettlementLink::Disposition);
        assert_eq!(awaiting.owed_by_role, "lead");
    }
}
