//! A record cut at each point it can be cut, retried to the same one revision.
use super::*;
use crate::management::configuration::cas_tests::{fixture, Relay};
use crate::management::configuration::update;
use agent_setup::{AgentSetupSpec, SetupAccess, AGENT_SETUP_VERSION};
use beekeeper_host_core::config::HostConfig;
use std::sync::atomic::Ordering;

const FROM: &str = "sonnet";
const TO: &str = "opus";

struct Rig {
    _home: tempfile::TempDir,
    context: Context,
    relay: Relay,
    actor: String,
    config: HostConfig,
    setup_id: Uuid,
}

async fn rig() -> Rig {
    let (home, context, relay, actor, head, private) = fixture();
    // A head that names the setup's model, as a started setup's does.
    let from = UpdateRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: actor.clone(),
        expected_revision: head.id.to_hex(),
        display_name: None,
        system_prompt: None,
        model: Some(format!("{FROM}[low]")),
        home: None,
        public_definition: None,
        private_config: Some(private),
    };
    update(&from, &relay, &context).await.expect("head");
    let spec = AgentSetupSpec {
        project: "30621:aa:slug".into(),
        channel: Uuid::from_u128(1),
        role: "lead".into(),
        display_name: "Lead".into(),
        provider_instance: "acme-primary".into(),
        model: FROM.into(),
        effort: Some("low".into()),
        parallelism: 1,
        access: SetupAccess::OwnerOnly,
        existing_actor: Some(actor.clone()),
    };
    let setup_id = Uuid::new_v4();
    let mut setup = AgentSetup {
        version: AGENT_SETUP_VERSION,
        setup_id,
        spec_digest: spec.digest().unwrap(),
        spec,
        owner: relay.keys().public_key().to_hex(),
        relay_url: relay.relay_url().into(),
        created_at: String::new(),
        updated_at: String::new(),
        state: SetupState::Stopped,
        install_operation: Uuid::from_u128(3),
        start_operation: Uuid::from_u128(4),
        confirmed_start: None,
        context_operation: Uuid::from_u128(5),
        session_ref: Uuid::from_u128(6),
        genesis_event_id: None,
        project_context: None,
        pending_action: None,
        attempts: Vec::new(),
        agent_pubkey: Some(actor.clone()),
        revision: None,
        receipt_event_id: None,
        steps: Vec::new(),
        last_refusal: None,
        readback: None,
        model_change: None,
    };
    let config = config_for(&context, &relay).expect("config");
    save(&config, &mut setup).expect("journal");
    Rig {
        _home: home,
        context,
        relay,
        actor,
        config,
        setup_id,
    }
}

impl Rig {
    async fn record(&self) -> Result<AgentSetup, Error> {
        record_model_change(&self.context, &self.relay, self.setup_id, TO, None).await
    }

    fn journal_path(&self) -> std::path::PathBuf {
        self.config
            .provider_state_dir
            .join("agent-setups")
            .join(format!("{}.json", self.setup_id))
    }

    /// Distinct definitions that name the new model.
    fn revisions_on_new_model(&self) -> usize {
        let wanted = format!("{TO}[low]");
        let events = self.relay.events.lock().expect("events");
        let ids: std::collections::BTreeSet<_> = events
            .iter()
            .filter(|event| {
                serde_json::from_str::<ManagedAgentEventContent>(&event.content)
                    .is_ok_and(|content| content.model.as_deref() == Some(wanted.as_str()))
            })
            .map(|event| event.id)
            .collect();
        ids.len()
    }

    fn assert_recorded(&self, setup: &AgentSetup) {
        assert_eq!(setup.spec.selection(), format!("{TO}[low]"));
        assert_eq!(setup.spec.effort.as_deref(), Some("low"), "effort kept");
        assert!(setup.model_change.is_none(), "hold cleared");
        assert_eq!(setup.agent_pubkey.as_deref(), Some(self.actor.as_str()));
        assert_eq!(self.revisions_on_new_model(), 1, "one definition revision");
        let snapshot = private_configuration_snapshot(&self.context, &self.relay, &self.actor)
            .expect("snapshot");
        assert_eq!(snapshot.private.record["model"], format!("{TO}[low]"));
        let reloaded = load(&self.config, self.setup_id).expect("journal");
        assert_eq!(reloaded.spec_digest, reloaded.spec.digest().unwrap());
    }
}

/// The definition is published and the reply lost: the private settings and the
/// journal have not moved, and the head is no longer the one the request named.
#[tokio::test]
async fn a_cut_after_submit_retries_the_held_request_to_one_revision() {
    let rig = rig().await;
    rig.relay
        .lose_publication_reply
        .store(true, Ordering::SeqCst);
    assert!(rig.record().await.is_err());
    let held = load(&rig.config, rig.setup_id)
        .expect("journal")
        .model_change
        .expect("held");
    assert_eq!(rig.revisions_on_new_model(), 1);
    assert_eq!(held.model, TO);
    let setup = rig.record().await.expect("the retry finishes");
    rig.assert_recorded(&setup);
}

/// Everything but the journal moved: the snapshot hash has changed too. This is the
/// journal as a crash there leaves it, the bytes it held before the last save.
#[tokio::test]
async fn a_cut_after_commit_update_retries_the_held_request_to_one_revision() {
    let rig = rig().await;
    rig.relay
        .lose_publication_reply
        .store(true, Ordering::SeqCst);
    assert!(rig.record().await.is_err());
    let held_journal = std::fs::read(rig.journal_path()).expect("held journal");
    let done = rig.record().await.expect("finishes");
    rig.assert_recorded(&done);
    std::fs::write(rig.journal_path(), held_journal).expect("cut");
    let setup = rig.record().await.expect("the retry finishes");
    rig.assert_recorded(&setup);
}

/// A held change was confirmed, or had nothing to confirm it. It asks for no switch,
/// even where this setup reads as running and nothing is live or open.
#[tokio::test]
async fn a_held_change_asks_for_no_switch_and_no_provider() {
    let rig = rig().await;
    let mut setup = load(&rig.config, rig.setup_id).expect("journal");
    setup.state = SetupState::Running;
    setup.model_change = Some(ModelChange {
        operation_id: Uuid::new_v4(),
        model: TO.into(),
        effort: Some("low".into()),
        expected_revision: "ab".repeat(32),
        snapshot_hash: "cd".repeat(32),
    });
    save(&rig.config, &mut setup).expect("journal");
    let no_provider = crate::project_agent_setup::ProviderProbe {
        live_pid: None,
        state: "not running".into(),
    };
    let plan = plan_model_change(
        &rig.context,
        &rig.relay,
        &no_provider,
        rig.setup_id,
        TO,
        None,
        Path::new("/nonexistent"),
    )
    .await
    .expect("planned");
    assert_eq!(plan.session, None);
    assert_eq!(plan.selection, format!("{TO}[low]"));
    let other = plan_model_change(
        &rig.context,
        &rig.relay,
        &no_provider,
        rig.setup_id,
        "another",
        None,
        Path::new("/nonexistent"),
    )
    .await
    .expect_err("refused");
    assert!(
        other.to_string().contains("MODEL_CHANGE_IN_PROGRESS"),
        "{other}"
    );
}

/// A definition the owner changed creates no hold it could not leave.
#[tokio::test]
async fn an_owner_revised_definition_refuses_before_any_hold() {
    let rig = rig().await;
    let snapshot =
        private_configuration_snapshot(&rig.context, &rig.relay, &rig.actor).expect("snapshot");
    let revised = UpdateRequest {
        operation_id: Uuid::new_v4(),
        agent_pubkey: rig.actor.clone(),
        expected_revision: snapshot.definition.id.to_hex(),
        display_name: None,
        system_prompt: None,
        model: Some("owner-choice".into()),
        home: None,
        public_definition: None,
        private_config: Some(snapshot.private),
    };
    update(&revised, &rig.relay, &rig.context)
        .await
        .expect("owner");
    let refused = rig.record().await.expect_err("refused");
    assert!(
        refused.to_string().contains("OWNER_CHANGED_DEFINITION"),
        "{refused}"
    );
    let setup = load(&rig.config, rig.setup_id).expect("journal");
    assert!(setup.model_change.is_none());
}

/// The provider's own record decides, exactly: both fields, open, this session.
#[test]
fn only_an_open_record_on_both_fields_counts_as_already_on_it() {
    let dir = tempfile::tempdir().expect("dir");
    let write = |value: serde_json::Value| {
        std::fs::write(dir.path().join("state.json"), value.to_string()).expect("state")
    };
    let on = |session: &str| agent_setup::execution_already_on(dir.path(), session, "opus[low]");
    assert!(!on("s"), "no state reads as no");
    write(
        serde_json::json!({"sessions":{"s":{"closed":false,"model":"opus[low]","modelRequested":"opus[low]"}}}),
    );
    assert!(on("s"));
    assert!(!on("other"));
    write(
        serde_json::json!({"sessions":{"s":{"closed":false,"model":"sonnet[low]","modelRequested":"opus[low]"}}}),
    );
    assert!(!on("s"), "asked, not acknowledged");
    write(
        serde_json::json!({"sessions":{"s":{"closed":false,"model":"opus[low]","modelRequested":"sonnet[low]"}}}),
    );
    assert!(!on("s"));
    write(
        serde_json::json!({"sessions":{"s":{"closed":true,"model":"opus[low]","modelRequested":"opus[low]"}}}),
    );
    assert!(!on("s"), "closed");
}
