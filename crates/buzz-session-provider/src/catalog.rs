//! Provider catalog (kind 44222) — what this adapter offers, canonically.
//!
//! The catalog is how an operator's picker learns that this provider exists,
//! which models it will accept, and what it can be asked to do. The create
//! command that follows names this event's signer as its
//! `providerAuthorityPubkey`, which is what stops a command addressed to one
//! adapter from being served by another sharing the channel.
//!
//! # Why canonical bytes, not just a revision
//!
//! `cspc-key` digests the exact signed content, and the consumer re-serializes
//! its parse and compares byte for byte. So the serialization is part of the
//! contract, not an implementation detail: two providers advertising the same
//! capabilities must produce identical bytes, and any difference in bytes must
//! be a real difference in what is on offer. Ordering is therefore fixed rather
//! than incidental — see [`build`].
//!
//! It also gives the revision an honest definition. `revision` bumps when the
//! canonical body changes and at no other time, so a restart, a reconnect, or a
//! projects-file rewrite that changes nothing advertises nothing new. A consumer
//! keeping the highest revision per signer therefore never has to guess whether
//! two revisions differ in substance.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::commands::ProjectsFile;
use crate::config::Config;
use crate::payload::Capabilities;

/// Schema string on every catalog advertisement.
pub const CATALOG_SCHEMA: &str = "buzz-coding-session-provider-catalog/v1";

/// NIP-CSPC bound on `providers[]`.
pub const MAX_PROVIDERS: usize = 32;
/// NIP-CSPC bound on `allowedModels[]`.
pub const MAX_ALLOWED_MODELS: usize = 64;
/// NIP-CSPC bound on `projects[]`.
pub const MAX_PROJECTS: usize = 512;

/// One catalog advertisement.
///
/// Field order is the wire order; `serde_json::to_string` preserves declaration
/// order, which is what makes [`to_canonical_json`] canonical.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Catalog {
    /// Always [`CATALOG_SCHEMA`].
    pub schema: String,
    /// Monotonic per (channel, signer). Positive.
    pub revision: u64,
    /// Every session target this signer offers, sorted by `providerInstanceRef`.
    pub providers: Vec<CatalogProvider>,
    /// Optional narrowing by project. Omitted entirely when empty, which is the
    /// standalone-session case: every listed provider serves every session.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub projects: Vec<CatalogProject>,
}

/// One offered provider instance.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogProvider {
    /// Unique within the catalog; a create command names it.
    pub provider_instance_ref: String,
    /// Driver slug, matching the `cs-target` this provider mints.
    pub driver: String,
    /// Runtime behind the driver.
    pub runtime: String,
    /// Model used when a create names none.
    pub default_model: String,
    /// Accepted models. `allowedModels[0] == defaultModel`, remainder sorted.
    pub allowed_models: Vec<String>,
    /// What this provider can be asked to do.
    pub capabilities: Capabilities,
}

/// One project-scoped narrowing of `providers[]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogProject {
    /// NIP-MP project coordinate.
    pub project_ref: String,
    /// Repository within it, or `null`.
    pub repo_ref: Option<String>,
    /// `providerInstanceRef` values, sorted, no duplicates.
    pub providers: Vec<String>,
}

/// The revision-independent body, digested to decide whether a bump is due.
#[derive(Serialize)]
struct CatalogBody<'a> {
    providers: &'a [CatalogProvider],
    projects: &'a [CatalogProject],
}

/// Build the canonical catalog this provider advertises at `revision`.
///
/// Canonical means, per NIP-CSPC: keys in declaration order; `providers[]`
/// sorted by `providerInstanceRef`; `allowedModels[0]` equal to `defaultModel`
/// with the remainder sorted; `projects[]` sorted by `(projectRef, repoRef ?? "")`
/// with no duplicate pair; each project's provider list sorted and deduplicated.
pub fn build(config: &Config, projects_file: &ProjectsFile, revision: u64) -> Catalog {
    let mut providers: Vec<CatalogProvider> = config
        .runtimes
        .iter()
        .map(|descriptor| {
            let mut allowed_models: Vec<String> = descriptor
                .allowed_models
                .iter()
                .filter(|model| *model != &descriptor.default_model)
                .cloned()
                .collect();
            allowed_models.sort();
            allowed_models.dedup();
            allowed_models.truncate(MAX_ALLOWED_MODELS.saturating_sub(1));
            allowed_models.insert(0, descriptor.default_model.clone());
            CatalogProvider {
                provider_instance_ref: descriptor.instance_ref.clone(),
                driver: descriptor.driver.clone(),
                runtime: descriptor.runtime.clone(),
                default_model: descriptor.default_model.clone(),
                allowed_models,
                capabilities: descriptor
                    .capabilities
                    .unwrap_or_else(|| Capabilities::v1_for_runtime(&descriptor.runtime)),
            }
        })
        .collect();
    providers.sort_by(|left, right| left.provider_instance_ref.cmp(&right.provider_instance_ref));
    providers.truncate(MAX_PROVIDERS);

    // In v1 every runtime serves every configured project, so each project
    // advertises the full (already sorted) instance-ref list.
    let provider_refs: Vec<String> = providers
        .iter()
        .map(|provider| provider.provider_instance_ref.clone())
        .collect();

    // Every project the host has configured a working directory for is one this
    // provider can actually serve — a project with no directory would only fail
    // later with PROJECT_CWD_UNRESOLVED, so advertising it would be a lie.
    let mut projects: Vec<CatalogProject> = projects_file
        .project_refs()
        .filter(|project_ref| {
            buzz_core::coding_session_lifecycle_command::validate_project_ref(project_ref).is_ok()
        })
        .map(|project_ref| CatalogProject {
            project_ref: project_ref.to_owned(),
            repo_ref: None,
            providers: provider_refs.clone(),
        })
        .collect();
    projects.sort_by(|left, right| {
        left.project_ref.cmp(&right.project_ref).then_with(|| {
            left.repo_ref
                .as_deref()
                .unwrap_or_default()
                .cmp(right.repo_ref.as_deref().unwrap_or_default())
        })
    });
    projects.dedup_by(|left, right| {
        left.project_ref == right.project_ref && left.repo_ref == right.repo_ref
    });
    projects.truncate(MAX_PROJECTS);

    Catalog {
        schema: CATALOG_SCHEMA.to_owned(),
        revision,
        providers,
        projects,
    }
}

/// Serialize a catalog to its exact signed bytes.
pub fn to_canonical_json(catalog: &Catalog) -> Result<String, serde_json::Error> {
    serde_json::to_string(catalog)
}

/// Digest everything except the revision.
///
/// This is what decides whether a revision bump is due: a restart or a
/// projects-file rewrite that changes nothing must not advertise a new
/// revision, or "the highest revision I have seen" stops meaning "the newest
/// thing on offer".
pub fn body_digest(catalog: &Catalog) -> String {
    let body = CatalogBody {
        providers: &catalog.providers,
        projects: &catalog.projects,
    };
    let bytes = serde_json::to_vec(&body).unwrap_or_default();
    hex::encode(Sha256::digest(&bytes))
}

/// Cheap change detector for the projects file: modified time and size.
///
/// Rebuilding the catalog on every tick would mean re-reading and re-hashing a
/// file that almost never changes; this narrows that to a `stat`.
pub fn fingerprint(path: Option<&std::path::Path>) -> Option<(std::time::SystemTime, u64)> {
    let metadata = std::fs::metadata(path?).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::path::PathBuf;
    use std::time::Duration;

    use nostr::Keys;

    use buzz_core::coding_session_runtime::RuntimeDescriptor;

    fn claude_descriptor(default_model: &str, allowed: &[&str]) -> RuntimeDescriptor {
        RuntimeDescriptor {
            instance_ref: "claude-primary".into(),
            driver: "claude-agent-acp".into(),
            runtime: "claude".into(),
            agent_command: "claude-agent-acp".into(),
            agent_args: Vec::new(),
            cli_env: None,
            default_model: default_model.to_owned(),
            allowed_models: allowed.iter().map(|model| (*model).to_owned()).collect(),
            discover_models: false,
            capabilities: None,
        }
    }

    fn config_with_runtimes(runtimes: Vec<RuntimeDescriptor>) -> Config {
        Config {
            keys: Keys::generate(),
            relay_url: "ws://localhost:3000".into(),
            auth_tag: None,
            state_dir: PathBuf::from("/tmp/csp"),
            projects_file: None,
            context_mcp_command: None,
            instance_id: "instance-1".into(),
            runtimes,
            max_sessions: 4,
            session_idle_shutdown: Duration::from_secs(1800),
            idle_timeout: Duration::from_secs(900),
            answer_stall_timeout: Some(Duration::from_secs(120)),
            max_turn_duration: Duration::from_secs(7200),
            include_thoughts: true,
            emit_raw_sdk_frames: false,
            redaction_retention: crate::redaction_vault::RetentionPolicy::default(),
            command_horizon: Duration::from_secs(86_400),
        }
    }

    fn config(default_model: &str, allowed: &[&str]) -> Config {
        config_with_runtimes(vec![claude_descriptor(default_model, allowed)])
    }

    fn projects(refs: &[&str]) -> ProjectsFile {
        ProjectsFile {
            version: 1,
            projects: refs
                .iter()
                .map(|project_ref| ((*project_ref).to_owned(), PathBuf::from("/tmp")))
                .collect(),
            ..ProjectsFile::default()
        }
    }

    fn coordinate(d: &str) -> String {
        format!("30621:{}:{d}", "cd".repeat(32))
    }

    /// The bytes are the contract: the consumer re-serializes its parse and
    /// compares byte for byte, so the exact key order is pinned here.
    #[test]
    fn the_canonical_form_is_byte_stable_and_ordered() {
        let catalog = build(
            &config("model-b", &["model-c", "model-a", "model-b"]),
            &projects(&[&coordinate("zeta"), &coordinate("alpha")]),
            3,
        );
        let json = to_canonical_json(&catalog).expect("serialize");
        assert!(json.starts_with(
            r#"{"schema":"buzz-coding-session-provider-catalog/v1","revision":3,"providers":[{"providerInstanceRef":"claude-primary","driver":"claude-agent-acp","runtime":"claude","defaultModel":"model-b","allowedModels":["model-b","model-a","model-c"],"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true}}]"#
        ));
        // projects[] sorted by projectRef, each with a null repoRef.
        let alpha = json
            .find(&coordinate("alpha"))
            .expect("alpha is advertised");
        let zeta = json.find(&coordinate("zeta")).expect("zeta is advertised");
        assert!(alpha < zeta, "projects must be sorted by coordinate");
        assert!(json.contains(r#""repoRef":null,"providers":["claude-primary"]"#));

        // Same input, same bytes — the property `cspc-key` depends on.
        let again = to_canonical_json(&build(
            &config("model-b", &["model-a", "model-b", "model-c"]),
            &projects(&[&coordinate("alpha"), &coordinate("zeta")]),
            3,
        ))
        .expect("serialize");
        assert_eq!(json, again);
    }

    /// The multi-runtime form: providers sorted by the full instance ref, one
    /// entry per descriptor, and every project offering every runtime.
    #[test]
    fn two_runtimes_advertise_sorted_providers_and_shared_projects() {
        let codex = RuntimeDescriptor {
            instance_ref: "codex-primary".into(),
            driver: "codex-acp".into(),
            runtime: "codex".into(),
            agent_command: "codex-acp".into(),
            agent_args: Vec::new(),
            cli_env: None,
            default_model: "default".into(),
            allowed_models: vec!["default".into()],
            discover_models: false,
            capabilities: None,
        };
        // Deliberately out of order: the catalog sorts by instance ref.
        let catalog = build(
            &config_with_runtimes(vec![codex, claude_descriptor("default", &["default"])]),
            &projects(&[&coordinate("alpha")]),
            1,
        );
        let json = to_canonical_json(&catalog).expect("serialize");
        assert!(json.starts_with(
            r#"{"schema":"buzz-coding-session-provider-catalog/v1","revision":1,"providers":[{"providerInstanceRef":"claude-primary","driver":"claude-agent-acp","runtime":"claude","defaultModel":"default","allowedModels":["default"],"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true}},{"providerInstanceRef":"codex-primary","driver":"codex-acp","runtime":"codex","defaultModel":"default","allowedModels":["default"],"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":false}}]"#
        ));
        assert!(json.contains(r#""providers":["claude-primary","codex-primary"]"#));
    }

    /// A descriptor's explicit capability vector overrides the per-runtime v1
    /// default — the vector must be per provider, not shared.
    #[test]
    fn an_explicit_capability_vector_wins_over_the_runtime_default() {
        let mut goose = claude_descriptor("default", &[]);
        goose.instance_ref = "goose-primary".into();
        goose.driver = "goose-acp".into();
        goose.runtime = "goose".into();
        goose.capabilities = Some(Capabilities {
            thread_steer: true,
            ..Capabilities::v1_baseline()
        });
        let catalog = build(
            &config_with_runtimes(vec![goose]),
            &ProjectsFile::default(),
            1,
        );
        assert!(catalog.providers[0].capabilities.thread_steer);
        assert!(!catalog.providers[0].capabilities.plan);
    }

    #[test]
    fn allowed_models_always_lead_with_the_default() {
        let catalog = build(
            &config("m-default", &["m-z", "m-a"]),
            &ProjectsFile::default(),
            1,
        );
        assert_eq!(
            catalog.providers[0].allowed_models,
            vec!["m-default", "m-a", "m-z"]
        );

        // A default missing from the configured list is still offered: refusing
        // the model the catalog advertises as its default would be incoherent.
        let catalog = build(&config("m-default", &[]), &ProjectsFile::default(), 1);
        assert_eq!(catalog.providers[0].allowed_models, vec!["m-default"]);
    }

    #[test]
    fn an_empty_projects_list_is_omitted_rather_than_sent_as_an_empty_array() {
        let catalog = build(&config("m", &[]), &ProjectsFile::default(), 1);
        let json = to_canonical_json(&catalog).expect("serialize");
        assert!(!json.contains("projects"));
    }

    /// A directory keyed by something that is not a NIP-MP coordinate is a host
    /// mistake, not an offer — advertising it would put an unusable target in
    /// the operator's picker.
    #[test]
    fn only_valid_project_coordinates_are_advertised() {
        let catalog = build(
            &config("m", &[]),
            &projects(&["not-a-coordinate", &coordinate("real")]),
            1,
        );
        assert_eq!(catalog.projects.len(), 1);
        assert_eq!(catalog.projects[0].project_ref, coordinate("real"));
    }

    /// The digest ignores the revision, which is what makes a bump mean
    /// something: re-advertising identical capabilities must not look new.
    #[test]
    fn the_body_digest_ignores_the_revision_and_tracks_everything_else() {
        let base = build(&config("m", &[]), &ProjectsFile::default(), 1);
        let bumped = build(&config("m", &[]), &ProjectsFile::default(), 99);
        assert_eq!(body_digest(&base), body_digest(&bumped));

        let other_model = build(&config("m2", &[]), &ProjectsFile::default(), 1);
        assert_ne!(body_digest(&base), body_digest(&other_model));

        let with_project = build(&config("m", &[]), &projects(&[&coordinate("a")]), 1);
        assert_ne!(body_digest(&base), body_digest(&with_project));
    }

    #[test]
    fn duplicate_project_entries_collapse() {
        let mut file = ProjectsFile::default();
        let mut map = BTreeMap::new();
        map.insert(coordinate("a"), PathBuf::from("/tmp"));
        file.projects = map;
        let catalog = build(&config("m", &[]), &file, 1);
        assert_eq!(catalog.projects.len(), 1);
    }

    #[test]
    fn the_fingerprint_changes_when_the_file_does() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("projects.json");
        assert!(fingerprint(None).is_none());
        assert!(fingerprint(Some(&path)).is_none());

        std::fs::write(&path, b"{}").expect("write");
        let first = fingerprint(Some(&path)).expect("fingerprint");
        std::fs::write(&path, b"{\"version\":1}").expect("write");
        assert_ne!(first, fingerprint(Some(&path)).expect("fingerprint"));
    }
}
