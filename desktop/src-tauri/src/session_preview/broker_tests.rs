use super::*;
use beekeeper_core_pkg::coding_session_command::CodingSessionTarget;
use beekeeper_core_pkg::preview_grant::{
    mint_preview_grant_with_nonce, PreviewGrantRequest, PREVIEW_GRANT_DEFAULT_TTL_SECS,
};
use nostr::Keys;
use uuid::Uuid;

const NOW: u64 = 1_790_000_000;
const CHANNEL: &str = "6f1c1c0e-6a8e-4c38-9d0f-1a2b3c4d5e6f";

struct Fixture {
    provider: Keys,
    owner: Keys,
}

impl Fixture {
    fn new() -> Self {
        Self {
            provider: Keys::generate(),
            owner: Keys::generate(),
        }
    }

    fn token(&self, channel: &str, session: &str, generation: u64, execution: &str) -> String {
        let request = PreviewGrantRequest {
            channel_id: Uuid::parse_str(channel).expect("uuid"),
            target: target(session, generation),
            execution_id: execution.into(),
            audience: preview_grant_audience(&self.owner.public_key()),
            ttl_secs: PREVIEW_GRANT_DEFAULT_TTL_SECS,
        };
        mint_preview_grant_with_nonce(
            &self.provider,
            &request,
            NOW,
            "00112233445566778899aabbccddeeff",
        )
        .expect("mint")
    }

    fn grant(&self, session: &str, generation: u64, execution: &str) -> VerifiedPreviewGrant {
        self.verify(&self.token(CHANNEL, session, generation, execution))
            .expect("verify")
    }

    fn verify(&self, token: &str) -> Result<VerifiedPreviewGrant, PreviewGrantError> {
        verify_preview_grant(
            Some(token),
            &[self.provider.public_key()],
            &preview_grant_audience(&self.owner.public_key()),
            NOW + 10,
        )
    }
}

fn target(session: &str, generation: u64) -> CodingSessionTarget {
    CodingSessionTarget {
        driver: "claude".into(),
        instance_id: "inst-1".into(),
        session_id: session.into(),
        generation,
    }
}

fn person(session: &str, generation: u64) -> Binding {
    Binding::Person {
        target: target(session, generation),
    }
}

#[test]
fn two_sessions_one_channel_the_sibling_is_refused() {
    let fixture = Fixture::new();
    // The person opened the preview in session A's Browser surface.
    let binding = person("A", 1);
    let own = fixture.grant("A", 1, "exec-a");
    let sibling = fixture.grant("B", 1, "exec-b");
    assert_eq!(
        authorize(PreviewStatus::Ready, &binding, &own, Access::Drive),
        Ok(None)
    );
    let refused = authorize(PreviewStatus::Ready, &binding, &sibling, Access::Drive)
        .expect_err("a sibling session must not drive");
    assert_eq!(refused.code(), "preview_wrong_session");
    // Nor may it take the preview over by re-opening it.
    let refused = authorize(PreviewStatus::Ready, &binding, &sibling, Access::Open)
        .expect_err("a sibling session must not reopen it");
    assert_eq!(refused.code(), "preview_wrong_session");
}

#[test]
fn agent_opened_preview_is_bound_to_that_agents_session() {
    let fixture = Fixture::new();
    let opener = fixture.grant("A", 2, "exec-a");
    let bound = authorize(PreviewStatus::Absent, &Binding::None, &opener, Access::Open)
        .expect("open")
        .expect("binds");
    assert_eq!(
        bound,
        Binding::Agent {
            target: target("A", 2),
            execution_id: "exec-a".into()
        }
    );
    let sibling = fixture.grant("B", 9, "exec-b");
    assert_eq!(
        authorize(PreviewStatus::Ready, &bound, &sibling, Access::Drive)
            .expect_err("sibling")
            .code(),
        "preview_wrong_session"
    );
}

#[test]
fn newer_generation_drives_and_rebinds_older_is_refused() {
    let fixture = Fixture::new();
    let binding = person("A", 2);
    let newer = fixture.grant("A", 3, "exec-a3");
    assert_eq!(
        authorize(PreviewStatus::Ready, &binding, &newer, Access::Drive),
        Ok(Some(person("A", 3)))
    );
    let older = fixture.grant("A", 1, "exec-a1");
    assert_eq!(
        authorize(PreviewStatus::Ready, &binding, &older, Access::Drive)
            .expect_err("older")
            .code(),
        "preview_wrong_session"
    );
    let agent = Binding::Agent {
        target: target("A", 2),
        execution_id: "exec-a2".into(),
    };
    assert_eq!(
        authorize(PreviewStatus::Ready, &agent, &newer, Access::Drive),
        Ok(Some(Binding::Agent {
            target: target("A", 3),
            execution_id: "exec-a3".into()
        }))
    );
}

#[test]
fn unbound_preview_is_undriveable_until_an_agent_opens_it() {
    let fixture = Fixture::new();
    let grant = fixture.grant("A", 1, "exec-a");
    assert_eq!(
        authorize(PreviewStatus::Ready, &Binding::None, &grant, Access::Drive)
            .expect_err("undriveable")
            .code(),
        "preview_wrong_session"
    );
    assert!(matches!(
        authorize(PreviewStatus::Ready, &Binding::None, &grant, Access::Open),
        Ok(Some(Binding::Agent { .. }))
    ));
}

#[test]
fn absent_preview_belongs_to_nobody() {
    let fixture = Fixture::new();
    let grant = fixture.grant("B", 1, "exec-b");
    // A stale binding from before a close does not keep the channel.
    assert!(matches!(
        authorize(PreviewStatus::Absent, &person("A", 1), &grant, Access::Open),
        Ok(Some(Binding::Agent { .. }))
    ));
    // Driving an absent preview is left to the not-open check.
    assert_eq!(
        authorize(
            PreviewStatus::Absent,
            &person("A", 1),
            &grant,
            Access::Drive
        ),
        Ok(None)
    );
}

#[test]
fn closed_by_person_keeps_its_binding_for_reopen() {
    let fixture = Fixture::new();
    let binding = person("A", 1);
    let own = fixture.grant("A", 1, "exec-a");
    assert_eq!(
        authorize(PreviewStatus::ClosedByPerson, &binding, &own, Access::Open),
        Ok(None)
    );
    let sibling = fixture.grant("B", 1, "exec-b");
    assert!(authorize(
        PreviewStatus::ClosedByPerson,
        &binding,
        &sibling,
        Access::Open
    )
    .is_err());
}

#[test]
fn grants_for_another_channel_or_desktop_are_refused_before_binding() {
    let fixture = Fixture::new();
    let other_desktop = Fixture {
        provider: fixture.provider.clone(),
        owner: Keys::generate(),
    };
    let token = fixture.token(CHANNEL, "A", 1, "exec-a");
    assert_eq!(
        other_desktop.verify(&token).expect_err("audience").code(),
        "preview_wrong_audience"
    );
    let stranger = Fixture {
        provider: Keys::generate(),
        owner: fixture.owner.clone(),
    };
    assert_eq!(
        fixture
            .verify(&stranger.token(CHANNEL, "A", 1, "exec-a"))
            .expect_err("issuer")
            .code(),
        "preview_wrong_issuer"
    );
    // A grant for another channel is resolved to that channel's preview, and
    // the binding check refuses a channel mismatch outright.
    let elsewhere = fixture
        .verify(&fixture.token("11111111-2222-4333-8444-555555555555", "A", 1, "exec-a"))
        .expect("verify");
    let refused = elsewhere
        .check_binding(&PreviewSessionBinding {
            channel_id: Uuid::parse_str(CHANNEL).expect("uuid"),
            target: Some(target("A", 1)),
        })
        .expect_err("other channel");
    assert_eq!(refused.code(), "preview_wrong_session");
}

#[test]
fn actions_parse_from_the_wire_shape() {
    let parse = |value: Value| serde_json::from_value::<PreviewAction>(value);
    assert_eq!(
        parse(json!({"verb": "open", "port": 5173})).expect("open"),
        PreviewAction::Open {
            url: None,
            port: Some(5173),
            wait: None,
        }
    );
    assert!(matches!(
        parse(json!({"verb": "navigate", "reload": true, "wait": false})),
        Ok(PreviewAction::Navigate {
            reload: true,
            wait: Some(false),
            ..
        })
    ));
    assert_eq!(
        parse(
            json!({"verb": "click", "target": {"role": "button", "name": "Save"}, "clickCount": 2})
        )
        .expect("click"),
        PreviewAction::Click {
            target: json!({"role": "button", "name": "Save"}),
            button: None,
            click_count: Some(2)
        }
    );
    assert!(matches!(
        parse(json!({"verb": "wait_for", "urlIncludes": "/done", "timeoutMs": 2000})),
        Ok(PreviewAction::WaitFor {
            url_includes: Some(_),
            timeout_ms: Some(2000),
            ..
        })
    ));
    assert_eq!(
        parse(json!({"verb": "status"})).expect("status").verb(),
        "status"
    );
    assert!(parse(json!({"verb": "fly"})).is_err());
    assert!(parse(json!({"verb": "type", "target": {"label": "Email"}})).is_err());
}

#[test]
fn grant_sentences_name_the_problem() {
    assert!(grant_sentence(&PreviewGrantError::Missing).contains("BEEKEEPER_PREVIEW_GRANT"));
    assert!(grant_sentence(&PreviewGrantError::WrongSession("x".into()))
        .starts_with("That preview belongs to a different session"));
}

#[tokio::test]
async fn a_refused_op_records_no_activity() {
    let mut noted = false;
    let refused: Result<(), PreviewError> = noting_success(
        async { Err(PreviewError::new("element_not_found", "no such ref")) },
        || noted = true,
    )
    .await;
    assert!(refused.is_err());
    assert!(!noted, "a refused op must not show the agent as driving");
}

#[tokio::test]
async fn a_successful_op_records_activity_after_it_runs() {
    let ran = std::cell::Cell::new(false);
    let mut noted_after_run = None;
    let done = noting_success(
        async {
            ran.set(true);
            Ok::<_, PreviewError>(7)
        },
        || noted_after_run = Some(ran.get()),
    )
    .await;
    assert_eq!(done.ok(), Some(7));
    assert_eq!(noted_after_run, Some(true));
}

#[test]
fn a_navigation_that_never_started_is_a_timeout_and_a_slow_one_is_not() {
    use super::{arrival_check, arrival_note, Arrival};
    // Not started yet, time left: keep waiting.
    assert!(arrival_check(3, 3, PreviewStatus::Loading, true, false).is_none());
    // Never started: it did not get there.
    let refused = arrival_check(3, 3, PreviewStatus::Ready, true, true)
        .expect("decided")
        .expect_err("timeout");
    assert_eq!(refused.code, "preview_timeout");
    // Started and loaded.
    assert_eq!(
        arrival_check(3, 4, PreviewStatus::Ready, true, false).map(|r| r.ok()),
        Some(Some(Arrival::Loaded))
    );
    // Started, still loading, waiting: keep waiting, then answer honestly.
    assert!(arrival_check(3, 4, PreviewStatus::Loading, true, false).is_none());
    assert_eq!(
        arrival_check(3, 4, PreviewStatus::Loading, true, true).map(|r| r.ok()),
        Some(Some(Arrival::StillLoading))
    );
    // --no-wait answers as soon as it started.
    assert_eq!(
        arrival_check(3, 4, PreviewStatus::Loading, false, false).map(|r| r.ok()),
        Some(Some(Arrival::Started))
    );
    // Closed under it: not open.
    let closed = arrival_check(3, 3, PreviewStatus::ClosedByPerson, true, false)
        .expect("decided")
        .expect_err("closed");
    assert_eq!(closed.code, "preview_not_open");
    assert!(arrival_note(Arrival::Loaded).is_none());
    assert!(arrival_note(Arrival::StillLoading)
        .is_some_and(|note| note.contains("may still be loading")));
}

#[test]
fn a_snapshot_of_a_page_still_loading_says_so() {
    use super::snapshot_loading;
    assert!(!snapshot_loading(Some("complete"), PreviewStatus::Ready));
    assert!(snapshot_loading(Some("interactive"), PreviewStatus::Ready));
    assert!(snapshot_loading(Some("loading"), PreviewStatus::Ready));
    assert!(snapshot_loading(Some("complete"), PreviewStatus::Loading));
    // An older driver reports no readyState: the load state decides.
    assert!(!snapshot_loading(None, PreviewStatus::Ready));
    assert!(snapshot_loading(None, PreviewStatus::Loading));
}
