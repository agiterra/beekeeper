//! Serialize `ManagedAgentRecord` ↔ kind:30177 managed-agent events and
//! publish via relay.
//!
//! Managed-agent events are NIP-33 parameterized replaceable events keyed by
//! `(pubkey, kind, d_tag)` where `d_tag` is the agent's pubkey. They mirror the
//! persona event flow (see `persona_events`): the same retention store and
//! flush loop publish them — this module only owns the kind-specific
//! projection, build and content-hash. Shared management owns retirement.
//!
//! # Security: opt-IN allowlist, never record-minus-denylist
//!
//! These events are world-readable on the relay. [`ManagedAgentEventContent`]
//! is an explicit opt-IN projection: it lists exactly the fields that are safe
//! to publish. A field added to [`super::ManagedAgentRecord`] later defaults to
//! NOT published unless it is deliberately added here. The flush loop is a
//! blind pre-signed pipe — this projection is the ONLY guard. It MUST NEVER
//! carry:
//! - `private_key_nsec` — the agent's secret key.
//! - `auth_tag` — the NIP-OA owner attestation.
//! - `env_vars` — may hold API keys / credentials.
//! - `backend` — `Provider { config }` is an opaque blob that may hold secrets.
//! - any runtime field (`runtime_pid`, `last_*`, `backend_agent_id`, …) — these
//!   mutate on every start/stop and describe transient process state.

#[cfg(test)]
use beekeeper_core_pkg::kind::KIND_MANAGED_AGENT;
use nostr::EventBuilder;

use super::ManagedAgentRecord;
#[cfg(test)]
use super::RespondTo;

pub use beekeeper_core_pkg::managed_agent::ManagedAgentEventContent;

/// Project a `ManagedAgentRecord` onto the content fields published in
/// managed-agent events.
///
/// This is the single source of truth for what leaves the device. The
/// retention upsert compares the serialized projection to suppress a
/// re-publish when only excluded runtime/local fields changed, so an
/// operational start/stop produces an identical projection and never
/// republishes.
pub fn agent_event_content(record: &ManagedAgentRecord) -> ManagedAgentEventContent {
    ManagedAgentEventContent {
        name: record.name.clone(),
        persona_id: record.persona_id.clone(),
        system_prompt: record.system_prompt.clone(),
        model: record.model.clone(),
        provider: record.provider.clone(),
        persona_source_version: record.persona_source_version.clone(),
        parallelism: record.parallelism,
        respond_to: record.respond_to,
        respond_to_allowlist: record.respond_to_allowlist.clone(),
        home_role: record.home_role.clone(),
        // A private project is never announced; an unverified or absent
        // association republishes only the digest this record carries, so
        // this computer never withdraws another computer's association.
        project_digest: super::project_association_carry::published_project_digest(record),
        // A withdrawal is marked, so it is not mistaken for a stale host.
        project_withdrawn: super::project_association_carry::published_project_withdrawn(record),
    }
    .public_projection()
}

/// Build a kind:30177 event from a `ManagedAgentRecord`.
///
/// Returns an unsigned `EventBuilder` — the caller signs and submits. The
/// `d_tag` is the agent's pubkey.
pub fn build_agent_event(record: &ManagedAgentRecord) -> Result<EventBuilder, String> {
    beekeeper_sdk_pkg::managed_agent::build_managed_agent_event(
        &record.pubkey,
        &agent_event_content(record),
    )
    .map_err(|error| error.to_string())
}

/// Parse a kind:30177 event's content into the projection — the inbound
/// counterpart of [`agent_event_content`].
///
/// # Security: the projection type is the structural guard
///
/// Returns [`ManagedAgentEventContent`], NOT a [`ManagedAgentRecord`]. The
/// return type physically cannot represent `private_key_nsec`, `auth_tag`,
/// `env_vars`, `backend`, `agent_command`/`agent_command_override`, or any
/// runtime field, so a malicious inbound event carrying those keys cannot
/// smuggle them onto the apply path — they are dropped at deserialization, not
/// filtered afterward. The caller patches only these fields onto the local
/// record (see `apply_inbound_managed_agent`), matching on the d-tag (the
/// agent's pubkey).
pub fn managed_agent_content_from_event(
    event: &nostr::Event,
) -> Result<ManagedAgentEventContent, String> {
    serde_json::from_str(event.content.as_ref())
        .map_err(|e| format!("failed to parse managed-agent event content: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core_pkg::project_agent_association::project_agent_digest;
    use std::collections::BTreeMap;

    fn sample_agent() -> ManagedAgentRecord {
        ManagedAgentRecord {
            reserves_name_globally: false,
            pubkey: "ab".repeat(32),
            name: "Test Agent".to_string(),
            persona_id: Some("persona-1".to_string()),
            private_key_nsec: "nsec1secretdonotpublish".to_string(),
            auth_tag: Some("authtagsecret".to_string()),
            relay_url: "wss://relay.example".to_string(),
            avatar_url: Some("https://example.com/a.png".to_string()),
            acp_command: "beekeeper-acp".to_string(),
            agent_command: "goose".to_string(),
            agent_command_override: None,
            agent_args: vec!["--flag".to_string()],
            mcp_command: "beekeeper-dev-mcp".to_string(),
            turn_timeout_seconds: 320,
            idle_timeout_seconds: None,
            max_turn_duration_seconds: None,
            parallelism: 24,
            system_prompt: Some("You are a test agent.".to_string()),
            model: Some("claude-opus-4".to_string()),
            provider: Some("anthropic".to_string()),
            persona_source_version: Some("abc123".to_string()),
            env_vars: BTreeMap::from([("OPENAI_API_KEY".to_string(), "sk-secret".to_string())]),
            start_on_app_launch: true,
            auto_restart_on_config_change: true,
            runtime_pid: Some(4242),
            backend: super::super::BackendKind::Provider {
                id: "buzz-backend-x".to_string(),
                config: serde_json::json!({ "api_key": "sk-provider-secret" }),
            },
            backend_agent_id: Some("remote-id".to_string()),
            provider_policy_pending: false,
            provider_binary_path: Some("/path/to/binary".to_string()),
            team_id: None,
            persona_team_dir: None,
            persona_name_in_team: None,
            home_role: None,
            project_ref: None,
            project_public: None,
            carried_project_digest: None,
            project_publication_withdrawn: false,
            created_at: "2025-01-01T00:00:00Z".to_string(),
            updated_at: "2025-01-01T00:00:00Z".to_string(),
            last_started_at: Some("2025-01-02T00:00:00Z".to_string()),
            last_stopped_at: Some("2025-01-03T00:00:00Z".to_string()),
            last_exit_code: Some(0),
            last_error: Some("some runtime error".to_string()),
            last_error_code: None,
            respond_to: RespondTo::Allowlist,
            respond_to_allowlist: vec!["79be667e".to_string()],
            // Unified-model fields carry real values so the exclusion test
            // proves they are absent from the wire, not vacuously empty.
            display_name: Some("Display Name Secretish".to_string()),
            slug: Some("sample-slug".to_string()),
            runtime: Some("goose".to_string()),
            name_pool: vec!["poolname".to_string()],
            is_builtin: true,
            is_active: false,
            shared: false,
            source_team: None,
            source_team_persona_slug: None,
            catalog_source: None,
            definition_respond_to: None,
            definition_respond_to_allowlist: Vec::new(),
            definition_parallelism: None,
            relay_mesh: None,
        }
    }

    #[test]
    fn build_agent_event_produces_correct_kind() {
        let builder = build_agent_event(&sample_agent()).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        assert_eq!(event.kind.as_u16() as u32, KIND_MANAGED_AGENT);
    }

    #[test]
    fn publication_rejects_unsafe_definition_less_name_and_prompt() {
        let mut unsafe_name = sample_agent();
        unsafe_name.persona_id = None;
        unsafe_name.name = "Review\u{200B}er".to_string();
        let error = build_agent_event(&unsafe_name)
            .expect_err("publication must reject an invisible agent name");
        assert!(error.contains("U+200B"), "unexpected error: {error}");

        let mut unsafe_prompt = sample_agent();
        unsafe_prompt.persona_id = None;
        unsafe_prompt.system_prompt = Some("Review\u{202E} code.".to_string());
        let error = build_agent_event(&unsafe_prompt)
            .expect_err("publication must reject bidi formatting in instructions");
        assert!(error.contains("U+202E"), "unexpected error: {error}");
    }

    #[test]
    fn publication_ignores_inert_linked_record_prompt() {
        let mut linked = sample_agent();
        linked.system_prompt = Some("stale\u{200B} prompt".to_string());
        build_agent_event(&linked)
            .expect("linked record prompt is omitted in favor of the validated persona");
    }

    #[test]
    fn d_tag_is_agent_pubkey() {
        let builder = build_agent_event(&sample_agent()).unwrap();
        let keys = nostr::Keys::generate();
        let event = builder.sign_with_keys(&keys).unwrap();
        let d = event
            .tags
            .iter()
            .find_map(|t| {
                let v: Vec<&str> = t.as_slice().iter().map(|s| s.as_str()).collect();
                (v.first() == Some(&"d"))
                    .then(|| v.get(1).map(|s| s.to_string()))
                    .flatten()
            })
            .unwrap();
        assert_eq!(
            d,
            "abababababababababababababababababababababababababababababababab"
        );
    }

    /// The security-critical assertion: the published projection must NEVER
    /// carry secrets, the provider backend blob, env vars, or runtime fields.
    #[test]
    fn content_excludes_secrets_and_runtime_fields() {
        let json = serde_json::to_string(&agent_event_content(&sample_agent())).unwrap();

        // Secrets — must never appear.
        assert!(
            !json.contains("nsec1secretdonotpublish"),
            "leaked private key"
        );
        assert!(!json.contains("private_key"), "leaked private key field");
        assert!(!json.contains("authtagsecret"), "leaked auth tag value");
        assert!(!json.contains("auth_tag"), "leaked auth tag field");
        assert!(!json.contains("OPENAI_API_KEY"), "leaked env var key");
        assert!(!json.contains("sk-secret"), "leaked env var value");
        assert!(!json.contains("env_vars"), "leaked env var field");
        assert!(
            !json.contains("sk-provider-secret"),
            "leaked provider config secret"
        );
        assert!(!json.contains("backend"), "leaked backend blob");

        // Runtime / install fields — must never appear.
        assert!(!json.contains("runtime_pid"), "leaked runtime pid");
        assert!(!json.contains("4242"), "leaked runtime pid value");
        assert!(!json.contains("last_started_at"));
        assert!(!json.contains("last_stopped_at"));
        assert!(!json.contains("last_exit_code"));
        assert!(!json.contains("last_error"));
        assert!(!json.contains("some runtime error"));

        // Unified-agent-model fields (Phase 1A) — deliberately NOT published
        // yet; adding them to the wire projection is a Phase 2 (relay
        // canonicalization) decision, not a record-shape side effect.
        assert!(!json.contains("display_name"), "leaked display_name");
        assert!(!json.contains("\"slug\""), "leaked slug");
        assert!(!json.contains("\"runtime\""), "leaked runtime");
        assert!(!json.contains("name_pool"), "leaked name_pool");
        assert!(!json.contains("is_builtin"), "leaked is_builtin");
        assert!(!json.contains("is_active"), "leaked is_active");
        assert!(!json.contains("backend_agent_id"));
        assert!(!json.contains("provider_binary_path"));
        assert!(!json.contains("relay_url"));

        // Identity fields — must appear.
        assert!(json.contains("\"name\""));
        assert!(json.contains("Test Agent"));
        assert!(json.contains("persona_id"));
        // Slimmed projection: a definition-linked record resolves its prompt
        // through the definition, so the wire must NOT carry it.
        assert!(
            !json.contains("system_prompt"),
            "definition-linked projection must omit system_prompt"
        );
    }

    /// Slimming (NIP-AP): definition-linked records omit the definition quad;
    /// definition-less records keep emitting it (they ARE their own
    /// definition — old readers would otherwise wipe fields with no restore
    /// path).
    #[test]
    fn projection_slims_definition_quad_only_when_linked() {
        let linked = sample_agent(); // persona_id: Some
        let json = serde_json::to_string(&agent_event_content(&linked)).unwrap();
        assert!(!json.contains("system_prompt"));
        assert!(!json.contains("\"model\""));
        assert!(!json.contains("\"provider\""));
        assert!(!json.contains("persona_source_version"));
        // Instance fields stay on the wire.
        assert!(json.contains("parallelism"));
        assert!(json.contains("respond_to"));

        let mut standalone = sample_agent();
        standalone.persona_id = None;
        let json = serde_json::to_string(&agent_event_content(&standalone)).unwrap();
        assert!(json.contains("system_prompt"), "standalone keeps prompt");
        assert!(json.contains("\"model\""), "standalone keeps model");
        assert!(json.contains("\"provider\""), "standalone keeps provider");
        assert!(
            json.contains("persona_source_version"),
            "standalone keeps source_version"
        );
    }

    #[test]
    fn projection_is_deterministic() {
        let agent = sample_agent();
        let a = serde_json::to_string(&agent_event_content(&agent)).unwrap();
        let b = serde_json::to_string(&agent_event_content(&agent)).unwrap();
        assert_eq!(a, b);
    }

    /// Mutating only runtime fields must NOT change the projection — the
    /// guarantee that operational start/stop never republishes.
    #[test]
    fn projection_ignores_runtime_field_churn() {
        let agent = sample_agent();
        let mut churned = agent.clone();
        churned.runtime_pid = Some(9999);
        churned.last_started_at = Some("2099-01-01T00:00:00Z".to_string());
        churned.last_exit_code = Some(1);
        churned.last_error = Some("different error".to_string());
        churned.updated_at = "2099-12-31T00:00:00Z".to_string();
        assert_eq!(
            agent_event_content(&agent),
            agent_event_content(&churned),
            "runtime field churn must not alter the published projection"
        );
    }

    #[test]
    fn projection_changes_on_meaningful_edit() {
        // Instance-level edit surfaces for every record.
        let agent = sample_agent();
        let mut edited = agent.clone();
        edited.parallelism += 1;
        assert_ne!(agent_event_content(&agent), agent_event_content(&edited));

        // Definition-level edit surfaces only for definition-less records —
        // linked records resolve the prompt through their definition.
        let mut standalone = sample_agent();
        standalone.persona_id = None;
        let mut edited = standalone.clone();
        edited.system_prompt = Some("A different prompt.".to_string());
        assert_ne!(
            agent_event_content(&standalone),
            agent_event_content(&edited)
        );
        let mut linked_edit = sample_agent();
        linked_edit.system_prompt = Some("A different prompt.".to_string());
        assert_eq!(
            agent_event_content(&sample_agent()),
            agent_event_content(&linked_edit),
            "a linked record's local prompt snapshot is not wire state"
        );
    }

    /// The inbound structural guard: a foreign event whose content JSON crams
    /// in secrets and harness fields parses successfully but the projection type
    /// silently DROPS every non-projected key — they can never reach the apply
    /// path. This is the inbound mirror of
    /// `content_excludes_secrets_and_runtime_fields`.
    #[test]
    fn from_event_drops_injected_secret_and_harness_keys() {
        use nostr::{EventBuilder, JsonUtil, Keys, Kind, Tag};
        let content = serde_json::json!({
            "name": "Agent",
            "parallelism": 1,
            "respond_to": "owner-only",
            // Injected — not part of the projection, must be dropped.
            "private_key_nsec": "nsec1leak",
            "auth_tag": "leak",
            "env_vars": { "K": "leak" },
            "agent_command": "leak",
            "agent_command_override": "leak",
            "backend": { "type": "local" },
        });
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(KIND_MANAGED_AGENT as u16), content.to_string())
            .tags(vec![Tag::parse([
                "d",
                "abababababababababababababababababababababababababababababababab",
            ])
            .unwrap()])
            .sign_with_keys(&keys)
            .unwrap();
        let event = nostr::Event::from_json(event.as_json()).unwrap();

        let parsed = managed_agent_content_from_event(&event).unwrap();
        // The projected fields parse through.
        assert_eq!(parsed.name, "Agent");
        assert_eq!(parsed.parallelism, 1);
        assert_eq!(parsed.respond_to, RespondTo::OwnerOnly);
        // Re-serializing the projection contains no injected key.
        let json = serde_json::to_string(&parsed).unwrap();
        assert!(!json.contains("nsec1leak"));
        assert!(!json.contains("private_key"));
        assert!(!json.contains("auth_tag"));
        assert!(!json.contains("env_vars"));
        assert!(!json.contains("agent_command"));
        assert!(!json.contains("backend"));
    }

    const PROJECT: &str =
        "30621:ABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABABAB:tank-loop";

    /// The opt-IN allowlist, pinned as a key set: a new projected key must be
    /// added here deliberately, next to the exclusion test above.
    #[test]
    fn projection_keys_are_exactly_the_allowlist() {
        use beekeeper_core_pkg::project_agent_association::{
            PROJECT_AGENT_DIGEST_CONTENT_KEY, PROJECT_AGENT_ROLE_CONTENT_KEY,
        };
        let mut agent = sample_agent();
        agent.persona_id = None;
        agent.home_role = Some("builder".to_string());
        agent.project_ref = Some(PROJECT.to_string());
        agent.project_public = Some(true);
        let value = serde_json::to_value(agent_event_content(&agent)).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        let mut allowed = vec![
            "name",
            "system_prompt",
            "model",
            "provider",
            "persona_source_version",
            "parallelism",
            "respond_to",
            "respond_to_allowlist",
            PROJECT_AGENT_ROLE_CONTENT_KEY,
            PROJECT_AGENT_DIGEST_CONTENT_KEY,
        ];
        allowed.sort_unstable();
        assert_eq!(keys, allowed);
    }

    /// The project association rides as a digest under the core key names;
    /// the coordinate — owner and slug — never appears anywhere in content.
    #[test]
    fn project_association_publishes_digest_never_the_coordinate() {
        use beekeeper_core_pkg::project_agent_association::{
            PROJECT_AGENT_DIGEST_CONTENT_KEY, PROJECT_AGENT_ROLE_CONTENT_KEY,
        };
        let mut agent = sample_agent();
        agent.home_role = Some("builder".to_string());
        agent.project_ref = Some(PROJECT.to_string());
        agent.project_public = Some(true);
        let event = build_agent_event(&agent)
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&event.content).unwrap();
        assert_eq!(
            value[PROJECT_AGENT_DIGEST_CONTENT_KEY],
            serde_json::json!(project_agent_digest(PROJECT).unwrap())
        );
        assert_eq!(value[PROJECT_AGENT_ROLE_CONTENT_KEY], "builder");
        let lowered = event.content.to_ascii_lowercase();
        assert!(!lowered.contains("tank-loop"), "leaked the project slug");
        assert!(
            !lowered.contains(&"ab".repeat(32)),
            "leaked the project owner"
        );
        assert!(!event.content.contains("30621:"), "leaked a coordinate");
        assert!(!event.content.contains("project_ref"));
    }

    #[test]
    fn unassociated_or_malformed_association_publishes_no_digest() {
        let mut agent = sample_agent();
        let json = serde_json::to_string(&agent_event_content(&agent)).unwrap();
        assert!(!json.contains("project_digest"));
        assert!(
            !json.contains("project_withdrawn"),
            "false is never serialized"
        );
        assert!(!json.contains("home_role"));
        agent.project_ref = Some("30621:not-hex:tank-loop".to_string());
        agent.project_public = Some(true);
        agent.home_role = Some("  ".to_string());
        let json = serde_json::to_string(&agent_event_content(&agent)).unwrap();
        assert!(!json.contains("project_digest"), "digest of a guess");
        assert!(!json.contains("home_role"), "blank role is no role");
    }

    /// Retention republishes when content differs, so an association change
    /// must change the projection.
    #[test]
    fn projection_changes_when_association_changes() {
        let agent = sample_agent();
        let mut associated = agent.clone();
        associated.project_ref = Some(PROJECT.to_string());
        associated.project_public = Some(true);
        assert_ne!(
            agent_event_content(&agent),
            agent_event_content(&associated)
        );
        let mut other = associated.clone();
        other.project_ref = Some(PROJECT.replace("tank-loop", "other"));
        assert_ne!(
            agent_event_content(&associated),
            agent_event_content(&other)
        );
        let mut roled = agent.clone();
        roled.home_role = Some("verifier".to_string());
        assert_ne!(agent_event_content(&agent), agent_event_content(&roled));
    }

    /// A private project is never announced: no digest, no coordinate, no
    /// owner or slug — even when the record carries a digest from elsewhere,
    /// which the private verdict withdraws on purpose, under the marker key.
    #[test]
    fn private_project_publishes_no_digest_and_no_coordinate() {
        use beekeeper_core_pkg::project_agent_association::{
            PROJECT_AGENT_DIGEST_CONTENT_KEY, PROJECT_AGENT_WITHDRAWN_CONTENT_KEY,
        };
        let mut agent = sample_agent();
        agent.home_role = Some("builder".to_string());
        agent.project_ref = Some(PROJECT.to_string());
        agent.project_public = Some(false);
        agent.carried_project_digest = project_agent_digest(PROJECT);
        let event = build_agent_event(&agent)
            .unwrap()
            .sign_with_keys(&nostr::Keys::generate())
            .unwrap();
        let value: serde_json::Value = serde_json::from_str(&event.content).unwrap();
        assert!(value.get(PROJECT_AGENT_DIGEST_CONTENT_KEY).is_none());
        assert_eq!(value[PROJECT_AGENT_WITHDRAWN_CONTENT_KEY], true);
        assert!(!event
            .content
            .contains(&project_agent_digest(PROJECT).unwrap()));
        let lowered = event.content.to_ascii_lowercase();
        assert!(!lowered.contains("tank-loop"), "leaked the project slug");
        assert!(!lowered.contains(&"ab".repeat(32)), "leaked the owner");
        assert!(!event.content.contains("30621:"), "leaked a coordinate");
        assert_eq!(value["home_role"], "builder", "the role still publishes");
    }

    /// An association whose visibility is not yet verified newly publishes
    /// nothing, and republishes a carried digest rather than withdrawing it.
    #[test]
    fn unverified_project_publishes_only_a_carried_digest() {
        let mut agent = sample_agent();
        agent.project_ref = Some(PROJECT.to_string());
        assert_eq!(agent_event_content(&agent).project_digest, None);
        let carried = "c".repeat(64);
        agent.carried_project_digest = Some(carried.clone());
        assert_eq!(agent_event_content(&agent).project_digest, Some(carried));
    }

    /// Inbound parsing tolerates the new keys and events that predate them.
    #[test]
    fn from_event_tolerates_association_fields_and_their_absence() {
        use nostr::{EventBuilder, Keys, Kind, Tag};
        let parse = |content: serde_json::Value| {
            let event =
                EventBuilder::new(Kind::Custom(KIND_MANAGED_AGENT as u16), content.to_string())
                    .tags(vec![Tag::parse([
                        "d",
                        "abababababababababababababababababababababababababababababababab",
                    ])
                    .unwrap()])
                    .sign_with_keys(&Keys::generate())
                    .unwrap();
            managed_agent_content_from_event(&event).unwrap()
        };
        let digest = project_agent_digest(PROJECT).unwrap();
        let parsed = parse(serde_json::json!({
            "name": "Agent", "parallelism": 1, "respond_to": "owner-only",
            "home_role": "builder", "project_digest": digest,
        }));
        assert_eq!(parsed.home_role.as_deref(), Some("builder"));
        assert_eq!(parsed.project_digest.as_deref(), Some(digest.as_str()));
        let legacy = parse(serde_json::json!({
            "name": "Agent", "parallelism": 1, "respond_to": "owner-only",
        }));
        assert_eq!(legacy.home_role, None);
        assert_eq!(legacy.project_digest, None);
        assert!(!legacy.project_withdrawn);
        let withdrawn = parse(serde_json::json!({
            "name": "Agent", "parallelism": 1, "respond_to": "owner-only",
            (beekeeper_core_pkg::project_agent_association::PROJECT_AGENT_WITHDRAWN_CONTENT_KEY): true,
        }));
        assert!(withdrawn.project_withdrawn, "the marker key parses");
        assert_eq!(withdrawn.project_digest, None);
    }
}
