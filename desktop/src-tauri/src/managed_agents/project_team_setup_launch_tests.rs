use super::*;
use buzz_core_pkg::coding_session_payload::LifecycleReceipt;
use uuid::Uuid;

struct Fixture {
    _root: tempfile::TempDir,
    draft: ProjectTeamSetupDraft,
    owner: Keys,
    provider: Keys,
    saved: journal::LaunchJournal,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().expect("root");
        let owner = Keys::generate();
        let provider = Keys::generate();
        let directory = root.path().join("draft");
        std::fs::create_dir(&directory).expect("draft");
        let draft = ProjectTeamSetupDraft {
            setup_id: Uuid::new_v4().to_string(),
            project_ref: format!("30621:{}:garden", owner.public_key().to_hex()),
            project_directory: root
                .path()
                .join("private-project")
                .to_string_lossy()
                .into_owned(),
            draft_directory: directory.to_string_lossy().into_owned(),
            roles_directory: directory
                .join("personas/roles")
                .to_string_lossy()
                .into_owned(),
            status: super::super::SetupStatus::Draft,
            intent: "Private project intent must stay local".into(),
            owner_pubkey: owner.public_key().to_hex(),
            relay_url: "wss://garden.example".into(),
            roles: vec![],
            expected_roles: vec![],
            created_at: crate::util::now_iso(),
            latest_snapshot_id: None,
        };
        let channel = Uuid::new_v4();
        let session_ref = Uuid::new_v4().to_string();
        let genesis = buzz_sdk_pkg::build_coding_session_genesis(
            channel,
            &buzz_core_pkg::coding_session_genesis::CodingSessionGenesisPayload::new(&session_ref),
        )
        .expect("genesis")
        .sign_with_keys(&owner)
        .expect("sign");
        let reservation = authoring::AuthoringReservation {
            authoring_id: Uuid::new_v4().to_string(),
            session_ref,
            create_command_id: format!("csl-{}", Uuid::new_v4()),
            channel_id: channel.to_string(),
            status: authoring::AuthoringStatus::Reserved,
            genesis_event_id: genesis.id.to_hex(),
            genesis_event: genesis,
        };
        let choice = LaunchChoice {
            provider_pubkey: provider.public_key().to_hex(),
            provider_instance_ref: "codex-primary".into(),
            runtime: "codex".into(),
            model: "default".into(),
        };
        let actor = actor::PreparedSetupActor {
            actor_pubkey: Keys::generate().public_key().to_hex(),
            pack_ref: PackRef {
                repo: "app:shipped".into(),
                sha: "1.0".into(),
                path: "personas/roles/project-setup".into(),
                role: "project-setup".into(),
            },
            pack_digest: "a".repeat(64),
            authoring_directory: root.path().join("authoring").to_string_lossy().into_owned(),
        };
        let event = create_event(&draft, &reservation, &choice, &actor, &owner).expect("create");
        let saved = journal::LaunchJournal::new(
            &draft,
            reservation,
            choice,
            actor,
            "instance-1".into(),
            "codex-acp".into(),
            event,
        );
        Self {
            _root: root,
            draft,
            owner,
            provider,
            saved,
        }
    }
    fn target(&self) -> CodingSessionTarget {
        CodingSessionTarget {
            driver: "codex-acp".into(),
            instance_id: "instance-1".into(),
            session_id: "session-1".into(),
            generation: 1,
        }
    }
    fn receipt(&self, receipt: &LifecycleReceipt, signer: &Keys) -> Event {
        buzz_sdk_pkg::builders::build_coding_session_lifecycle_receipt(
            Uuid::parse_str(&self.saved.reservation.channel_id).expect("channel"),
            &receipt.command_id,
            &serde_json::to_string(receipt).expect("json"),
        )
        .expect("receipt")
        .sign_with_keys(signer)
        .expect("sign")
    }
}

#[test]
fn persisted_choice_and_signed_bytes_survive_read_and_transition() {
    let mut f = Fixture::new();
    assert!(journal::read(&f.draft, &f.saved.reservation)
        .expect("read")
        .is_none());
    assert!(!f._root.path().join("launch.json").exists());
    journal::save(&f.draft, &f.saved, &f.owner).expect("save");
    let original = f.saved.create_event.clone();
    let read = journal::read(&f.draft, &f.saved.reservation)
        .expect("read")
        .expect("saved");
    assert_eq!(read.create_event, original);
    assert_eq!(read.choice, f.saved.choice);
    f.saved.status = LaunchStatus::Ambiguous;
    journal::save(&f.draft, &f.saved, &f.owner).expect("transition");
    assert_eq!(
        journal::read(&f.draft, &f.saved.reservation)
            .expect("read")
            .expect("saved")
            .create_event,
        original
    );
}
#[test]
fn private_paths_and_intent_do_not_appear_in_published_create() {
    let f = Fixture::new();
    let text = &f.saved.create_event.content;
    assert!(!text.contains(&f.draft.project_directory));
    assert!(!text.contains(&f.draft.draft_directory));
    assert!(!text.contains(&f.draft.intent));
    assert!(text.contains("PROJECT_TEAM_SETUP.md"));
}
#[test]
fn launch_scope_and_mutated_runtime_or_event_are_refused() {
    let f = Fixture::new();
    journal::save(&f.draft, &f.saved, &f.owner).expect("save");
    let path = f._root.path().join("launch.json");
    let original = std::fs::read(&path).expect("bytes");
    for field in ["runtime", "model", "providerPubkey"] {
        let mut value: serde_json::Value = serde_json::from_slice(&original).expect("json");
        value["choice"][field] = "changed".into();
        std::fs::write(&path, serde_json::to_vec(&value).expect("json")).expect("write");
        assert!(journal::read(&f.draft, &f.saved.reservation).is_err());
    }
    std::fs::write(&path, &original).expect("restore");
    let mut other = f.draft.clone();
    other.relay_url = "wss://elsewhere.example".into();
    assert!(journal::read(&other, &f.saved.reservation).is_err());
    let mut reservation = f.saved.reservation.clone();
    reservation.create_command_id = format!("csl-{}", Uuid::new_v4());
    assert!(journal::read(&f.draft, &reservation).is_err());
}
#[test]
fn created_failed_turn_and_failed_are_distinct_verified_receipts() {
    let f = Fixture::new();
    let id = &f.saved.reservation.create_command_id;
    for receipt in [
        LifecycleReceipt::created(id, &f.target()),
        LifecycleReceipt::created_with_failed_initial_turn(id, &f.target(), "failed first turn"),
        LifecycleReceipt::failed(id, "runtime_unavailable", "No runtime"),
    ] {
        let event = f.receipt(&receipt, &f.provider);
        let folded = wire::fold_receipts(&f.saved, &[event])
            .expect("fold")
            .expect("receipt");
        assert_eq!(folded.1, receipt);
    }
}
#[test]
fn wrong_signer_command_channel_instance_driver_and_turn_receipts_are_ignored() {
    let f = Fixture::new();
    let receipt = LifecycleReceipt::created(&f.saved.reservation.create_command_id, &f.target());
    let foreign = f.receipt(&receipt, &Keys::generate());
    assert!(wire::verified_receipt(&foreign, &f.saved).is_none());
    let wrong = LifecycleReceipt::created("another-command", &f.target());
    assert!(wire::verified_receipt(&f.receipt(&wrong, &f.provider), &f.saved).is_none());
    for field in ["driver", "instance"] {
        let mut target = f.target();
        if field == "driver" {
            target.driver = "other".into();
        } else {
            target.instance_id = "alias-is-not-instance".into();
        }
        let wrong = LifecycleReceipt::created(&f.saved.reservation.create_command_id, &target);
        assert!(wire::verified_receipt(&f.receipt(&wrong, &f.provider), &f.saved).is_none());
    }
    let turn = LifecycleReceipt::turn_queued(&f.saved.reservation.create_command_id, &f.target());
    assert!(wire::verified_receipt(&f.receipt(&turn, &f.provider), &f.saved).is_none());
    let other = buzz_sdk_pkg::builders::build_coding_session_lifecycle_receipt(
        Uuid::new_v4(),
        &receipt.command_id,
        &serde_json::to_string(&receipt).expect("json"),
    )
    .expect("build")
    .sign_with_keys(&f.provider)
    .expect("sign");
    assert!(wire::verified_receipt(&other, &f.saved).is_none());
}
#[test]
fn contradictory_provider_receipts_remain_unknown() {
    let f = Fixture::new();
    let id = &f.saved.reservation.create_command_id;
    let first = f.receipt(&LifecycleReceipt::created(id, &f.target()), &f.provider);
    let failed = f.receipt(
        &LifecycleReceipt::failed(id, "failed", "Failed"),
        &f.provider,
    );
    assert!(
        wire::fold_receipts(&f.saved, &[first.clone(), first.clone()])
            .expect("duplicate")
            .is_some()
    );
    assert!(wire::fold_receipts(&f.saved, &[first, failed]).is_err());
}
#[test]
fn signed_project_forward_and_channel_backlinks_are_bound() {
    let f = Fixture::new();
    let relay = Keys::generate();
    let channel = &f.saved.reservation.channel_id;
    let make = |keys: &Keys, kind: u16, tags: Vec<Vec<&str>>| {
        nostr::EventBuilder::new(nostr::Kind::Custom(kind), "")
            .tags(
                tags.into_iter()
                    .map(|tag| nostr::Tag::parse(tag).expect("tag")),
            )
            .sign_with_keys(keys)
            .expect("sign")
    };
    let project = make(&f.owner, 30621, vec![vec!["d", "garden"]]);
    let metadata = make(
        &relay,
        39000,
        vec![vec!["d", channel], vec!["project", &f.draft.project_ref]],
    );
    assert!(wire::project_channel_proof(
        &project,
        &metadata,
        &f.draft.project_ref,
        channel,
        &relay.public_key().to_hex()
    )
    .is_ok());
    let wrong = make(
        &relay,
        39000,
        vec![vec!["d", channel], vec!["project", "30621:other:project"]],
    );
    assert!(wire::project_channel_proof(
        &project,
        &wrong,
        &f.draft.project_ref,
        channel,
        &relay.public_key().to_hex()
    )
    .is_err());
    let forward = make(
        &f.owner,
        30621,
        vec![vec!["d", "garden"], vec!["channel", channel]],
    );
    let bare = make(&relay, 39000, vec![vec!["d", channel]]);
    assert!(wire::project_channel_proof(
        &forward,
        &bare,
        &f.draft.project_ref,
        channel,
        &relay.public_key().to_hex()
    )
    .is_ok());
    assert!(wire::project_channel_proof(
        &project,
        &bare,
        &f.draft.project_ref,
        channel,
        &relay.public_key().to_hex()
    )
    .is_err());
    assert!(wire::project_channel_proof(
        &forward,
        &wrong,
        &f.draft.project_ref,
        channel,
        &relay.public_key().to_hex()
    )
    .is_err());
}
#[cfg(unix)]
#[test]
fn symlink_and_oversize_journals_refuse_without_effects() {
    let f = Fixture::new();
    let path = f._root.path().join("launch.json");
    let elsewhere = f._root.path().join("elsewhere");
    std::fs::write(&elsewhere, "preserve").expect("write");
    std::os::unix::fs::symlink(&elsewhere, &path).expect("link");
    assert!(journal::save(&f.draft, &f.saved, &f.owner).is_err());
    assert_eq!(
        std::fs::read_to_string(&elsewhere).expect("read"),
        "preserve"
    );
    std::fs::remove_file(&path).expect("remove link");
    std::fs::write(&path, vec![b'x'; 65 * 1024]).expect("large");
    assert!(journal::read(&f.draft, &f.saved.reservation).is_err());
}

#[derive(Default)]
struct RelayStub {
    events: std::sync::Mutex<Vec<Event>>,
    receipts: std::sync::Mutex<Vec<Event>>,
    fail_create: std::sync::atomic::AtomicBool,
    fail_query: std::sync::atomic::AtomicBool,
}
async fn relay_stub(stub: std::sync::Arc<RelayStub>) -> (String, tokio::task::JoinHandle<()>) {
    use axum::{extract::State, http::StatusCode, routing::post, Router};
    async fn publish(
        State(stub): State<std::sync::Arc<RelayStub>>,
        body: String,
    ) -> (StatusCode, String) {
        let event: Event = serde_json::from_str(&body).expect("signed event");
        event.verify().expect("signature");
        stub.events.lock().expect("events").push(event.clone());
        if event.kind.as_u16() == buzz_core_pkg::kind::KIND_CODING_SESSION_LIFECYCLE_COMMAND as u16
            && stub.fail_create.load(std::sync::atomic::Ordering::SeqCst)
        {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                "stored before connection failure".into(),
            );
        }
        (
            StatusCode::OK,
            serde_json::json!({"event_id":event.id.to_hex(),"accepted":true,"message":"stored"})
                .to_string(),
        )
    }
    async fn query(
        State(stub): State<std::sync::Arc<RelayStub>>,
        body: String,
    ) -> (StatusCode, String) {
        if stub.fail_query.load(std::sync::atomic::Ordering::SeqCst) {
            return (StatusCode::SERVICE_UNAVAILABLE, "unavailable".into());
        }
        let filters: serde_json::Value = serde_json::from_str(&body).expect("query");
        let filter = &filters[0];
        let events = if let Some(id) = filter["ids"][0].as_str() {
            stub.events
                .lock()
                .expect("events")
                .iter()
                .filter(|event| event.id.to_hex() == id)
                .cloned()
                .collect::<Vec<_>>()
        } else {
            stub.receipts.lock().expect("receipts").clone()
        };
        (
            StatusCode::OK,
            serde_json::to_string(&events).expect("json"),
        )
    }
    let router = Router::new()
        .route("/events", post(publish))
        .route("/query", post(query))
        .with_state(stub);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind");
    let url = format!("http://{}", listener.local_addr().expect("address"));
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.expect("serve");
    });
    (url, task)
}
#[tokio::test]
async fn accepted_but_failed_http_retry_reuses_exact_create_and_stored_genesis() {
    use std::sync::{atomic::Ordering, Arc};
    let mut f = Fixture::new();
    let stub = Arc::new(RelayStub::default());
    stub.fail_create.store(true, Ordering::SeqCst);
    let (url, server) = relay_stub(stub.clone()).await;
    f.draft.relay_url = url;
    let state = crate::app_state::build_app_state();
    *state.keys.lock().expect("keys") = f.owner.clone();
    // Deliberately a different active community: all network calls stay on the captured destination.
    *state.relay_url_override.lock().expect("relay") = Some("http://127.0.0.1:1".into());
    assert!(wire::publish(&state, &f.draft, &f.saved, &f.owner)
        .await
        .is_err());
    stub.fail_create.store(false, Ordering::SeqCst);
    wire::publish(&state, &f.draft, &f.saved, &f.owner)
        .await
        .expect("retry");
    let events = stub.events.lock().expect("events");
    assert_eq!(events.len(), 3);
    assert_eq!(events[0], f.saved.reservation.genesis_event);
    assert_eq!(events[1], f.saved.create_event);
    assert_eq!(events[2], f.saved.create_event);
    server.abort();
}
#[tokio::test]
async fn unavailable_receipt_read_is_ambiguous_and_reconciliation_performs_no_writes() {
    use std::sync::{atomic::Ordering, Arc};
    let mut f = Fixture::new();
    let stub = Arc::new(RelayStub::default());
    let (url, server) = relay_stub(stub.clone()).await;
    f.draft.relay_url = url;
    let state = crate::app_state::build_app_state();
    stub.fail_query.store(true, Ordering::SeqCst);
    let unknown = wire::observe(&state, &f.draft, &f.saved, &f.owner).await;
    assert_eq!(unknown.status, LaunchStatus::Ambiguous);
    assert!(unknown.target.is_none());
    stub.fail_query.store(false, Ordering::SeqCst);
    let receipt = LifecycleReceipt::created_with_failed_initial_turn(
        &f.saved.reservation.create_command_id,
        &f.target(),
        "initial request failed",
    );
    stub.receipts
        .lock()
        .expect("receipts")
        .push(f.receipt(&receipt, &f.provider));
    let observed = wire::observe(&state, &f.draft, &f.saved, &f.owner).await;
    assert_eq!(observed.status, LaunchStatus::InitialTurnFailed);
    assert_eq!(observed.target, Some(f.target()));
    assert!(observed.receipt_event_id.is_some());
    assert!(stub.events.lock().expect("events").is_empty());
    assert!(!f._root.path().join("launch.json").exists());
    server.abort();
}

#[test]
fn missing_launch_journal_or_mismatched_marker_cannot_resign_the_request() {
    let f = Fixture::new();
    journal::save(&f.draft, &f.saved, &f.owner).expect("save");
    let path = f._root.path().join("launch.json");
    let marker = f._root.path().join("launch-event-id");
    let original = std::fs::read(&path).expect("saved");
    assert_eq!(
        std::fs::read_to_string(&marker).expect("marker"),
        f.saved.create_event.id.to_hex()
    );
    std::fs::remove_file(&path).expect("remove journal");
    assert!(journal::read(&f.draft, &f.saved.reservation).is_err());
    assert!(journal::save(&f.draft, &f.saved, &f.owner).is_err());
    assert!(!path.exists());
    std::fs::write(&path, &original).expect("restore");
    std::fs::write(&marker, "b".repeat(64)).expect("wrong marker");
    assert!(journal::read(&f.draft, &f.saved.reservation).is_err());
    assert!(journal::save(&f.draft, &f.saved, &f.owner).is_err());
    assert_eq!(std::fs::read(&path).expect("preserved"), original);
}
#[test]
fn interrupted_before_marker_keeps_exact_journal_and_read_does_not_create_marker() {
    let f = Fixture::new();
    journal::save(&f.draft, &f.saved, &f.owner).expect("save");
    let marker = f._root.path().join("launch-event-id");
    std::fs::remove_file(&marker).expect("interrupt");
    let recovered = journal::read(&f.draft, &f.saved.reservation)
        .expect("read")
        .expect("saved");
    assert_eq!(recovered.create_event, f.saved.create_event);
    assert!(!marker.exists());
    journal::ensure_marker(&f.draft, &recovered.create_event.id.to_hex()).expect("repair marker");
    assert_eq!(
        std::fs::read_to_string(&marker).expect("marker"),
        f.saved.create_event.id.to_hex()
    );
}
#[test]
fn transition_cannot_replace_a_validly_signed_create_or_change_the_fixed_brief() {
    let f = Fixture::new();
    journal::save(&f.draft, &f.saved, &f.owner).expect("save");
    let mut changed = f.saved.clone();
    changed.choice.model = "another-model".into();
    changed.create_event = create_event(
        &f.draft,
        &changed.reservation,
        &changed.choice,
        &changed.actor,
        &f.owner,
    )
    .expect("sign");
    assert!(journal::save(&f.draft, &changed, &f.owner).is_err());
    let mut payload = create_payload(
        &f.draft,
        &f.saved.reservation,
        &f.saved.choice,
        &f.saved.actor,
    )
    .expect("payload");
    if let CodingSessionLifecycleAction::SessionCreate { initial_turn, .. } = &mut payload.action {
        *initial_turn = Some("Override local instructions".into());
    }
    changed = f.saved.clone();
    changed.create_event = buzz_sdk_pkg::builders::build_coding_session_lifecycle_command(
        Uuid::parse_str(&changed.reservation.channel_id).expect("channel"),
        &payload,
    )
    .expect("build")
    .sign_with_keys(&f.owner)
    .expect("sign");
    assert!(journal::save(&f.draft, &changed, &f.owner).is_err());
    assert_eq!(
        journal::read(&f.draft, &f.saved.reservation)
            .expect("read")
            .expect("saved")
            .create_event,
        f.saved.create_event
    );
}

/// Every production path that starts a provider or hands a session to one
/// admits the host through the one seam,
/// `project_admission::admit_host_for_session` (ledger 266).
///
/// Run 7 (2026-09-24) wired the roster repair into one of two
/// provider-starting call sites; run 8 (2026-09-25) had both, but the
/// provider had been running since app launch, the founding never passed
/// through a start path, and the host stayed off the roster. So:
///
/// - every production `ensure_running(` call outside `supervisor` is either
///   `ensure_host_serving_project` (which admits the named project) or is
///   followed by `reconcile_saved_project_admissions` (which admits every
///   project this computer has a saved association with);
/// - `supervisor::start_provider_if_provisioned` (app launch, community
///   switch) reconciles too;
/// - the create flow admits before it publishes, whatever the provider's
///   state, and project creation pre-provisions;
/// - there is one implementation: the pre-266 `ensure_host_on_private_roster`
///   is gone.
///
/// RED on the base (bbb4adc1c): `session_provider/commands.rs` called
/// `ensure_running(` twice with no admission after it, `supervisor.rs`'s
/// launch start admitted nothing, and the create flow never named the
/// project to the host at all.
#[test]
fn every_provider_start_routes_through_the_roster_repair_helper() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut sources: Vec<(String, String)> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        for entry in std::fs::read_dir(&dir).expect("read src").flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !name.ends_with(".rs") || name.contains("test") {
                continue;
            }
            let rel = path
                .strip_prefix(&root)
                .map(|p| p.to_string_lossy().replace('\\', "/"))
                .unwrap_or_default();
            sources.push((rel, std::fs::read_to_string(&path).expect("read")));
        }
    }
    let source = |rel: &str| {
        sources
            .iter()
            .find(|(path, _)| path == rel)
            .map(|(_, text)| text.as_str())
            .unwrap_or_else(|| panic!("{rel} is missing"))
    };
    // Production code only: stop at the first `#[cfg(test)]`.
    let production = |text: &str| text.split("#[cfg(test)]").next().unwrap_or("").to_string();

    // Who calls `ensure_running(` in production, and how often.
    let mut callers: Vec<(String, usize)> = sources
        .iter()
        .map(|(path, text)| {
            let body = production(text);
            let calls = body.matches("ensure_running(").count()
                - body.matches("fn ensure_running(").count();
            (path.clone(), calls)
        })
        .filter(|(_, calls)| *calls > 0)
        .collect();
    callers.sort();
    assert_eq!(
        callers,
        vec![
            ("managed_agents/project_roster.rs".to_string(), 1),
            ("session_provider/commands.rs".to_string(), 2),
            ("session_provider/supervisor.rs".to_string(), 1),
        ],
        "a new provider start must admit the host: route it through \
         project_roster::ensure_host_serving_project or follow it with \
         project_admission::reconcile_saved_project_admissions"
    );
    let commands = production(source("session_provider/commands.rs"));
    assert_eq!(
        commands
            .matches("project_admission::reconcile_saved_project_admissions(")
            .count(),
        2,
        "both provider commands reconcile saved associations after a start"
    );
    let supervisor = production(source("session_provider/supervisor.rs"));
    let launch_start = supervisor
        .split("fn start_provider_if_provisioned(")
        .nth(1)
        .expect("start_provider_if_provisioned exists");
    assert!(
        launch_start
            .split("\n}\n")
            .next()
            .unwrap_or("")
            .contains("project_admission::reconcile_saved_project_admissions("),
        "a provider started at app launch reconciles its saved projects"
    );
    let roster = production(source("managed_agents/project_roster.rs"));
    assert!(
        roster.contains("project_admission::admit_host_for_session("),
        "ensure_host_serving_project is a thin wrapper of the seam"
    );
    assert!(
        production(source("managed_agents/project_verify_setup.rs"))
            .contains("project_admission::admit_host_for_session("),
        "project creation pre-provisions admission"
    );
    for (path, text) in &sources {
        assert!(
            !text.contains("ensure_host_on_private_roster"),
            "{path}: the pre-266 repair is gone; there is one implementation"
        );
        if path != "managed_agents/project_admission.rs" {
            assert!(
                !text.contains("fn admit_host_to_project"),
                "{path}: admission has one implementation"
            );
        }
    }

    // Both team-setup provider starts route through the wrapper.
    for rel in [
        "managed_agents/project_team_setup_launch.rs",
        "managed_agents/project_team_setup_activation.rs",
    ] {
        assert_eq!(
            source(rel)
                .matches("project_roster::ensure_host_serving_project(")
                .count(),
            1,
            "{rel} must route through the helper"
        );
    }

    // The create flow admits this computer before it publishes, whatever
    // the provider's state (founding and joining share `submit`).
    let create =
        include_str!("../../../src/features/coding-sessions/ui/useNewCodingSessionCreate.ts");
    let admit_at = create
        .find("admitThisComputerToProject(")
        .expect("the create flow admits the host");
    let publish_at = create
        .find("await publishSeatedCodingSessionCreate(")
        .expect("the create flow publishes");
    assert!(admit_at < publish_at, "admission comes before the publish");
}
