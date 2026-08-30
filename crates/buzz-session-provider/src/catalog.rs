//! Provider catalog (kind 44222) — what this adapter offers, canonically.
//!
//! The catalog is how an operator's picker learns that this provider exists,
//! which models it will accept, and what it can be asked to do. The create
//! command that follows names this event's signer as its
//! `providerAuthorityPubkey`, which is what stops a command addressed to one
//! adapter from being served by another sharing the channel.
//!
//! **The wire schema itself lives in
//! [`buzz_core::coding_session_catalog`]**, with the reader that enforces it.
//! This module is the publisher: it turns this host's configuration into one
//! of those catalogs and decides when a new revision is due. Keeping the
//! definition in `buzz-core` is what lets `bee sessions catalog` and `bee
//! sessions rubric check` read the offer without holding a second idea of
//! what it is.
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

use serde::Serialize;
use sha2::{Digest, Sha256};

pub use buzz_core::coding_session_catalog::{
    to_canonical_json, Catalog, CatalogModel, CatalogProject, CatalogProvider, CATALOG_SCHEMA,
    MAX_ALLOWED_MODELS, MAX_CATALOG_CONTENT_BYTES, MAX_PROJECTS, MAX_PROVIDERS,
};

use crate::commands::ProjectsFile;
use crate::config::Config;
use crate::context_window::{context_window_for_model, model_family_for_model};
use crate::payload::Capabilities;

/// The one model vendor a runtime slug or driver slug can serve.
///
/// A runtime that takes its provider from configuration — Goose, `buzz-agent`
/// — can serve several, so it is deliberately absent and its models publish no
/// `vendor` at all. This mirrors `CODING_SESSION_RUNTIME_VENDORS` in
/// `desktop/src/features/coding-sessions/lib/codingSessionCrew.ts`, which is
/// the rule the family check already applies to a seat.
const RUNTIME_VENDORS: &[(&str, &str)] = &[
    ("claude", "anthropic"),
    ("claude-agent-acp", "anthropic"),
    ("claude-code-acp", "anthropic"),
    ("codex", "openai"),
    ("codex-acp", "openai"),
];

/// The vendor behind a runtime, or `None` when the runtime can serve several.
fn vendor_for_runtime(runtime: &str, driver: &str) -> Option<&'static str> {
    let runtime = runtime.trim().to_ascii_lowercase();
    let driver = driver.trim().to_ascii_lowercase();
    RUNTIME_VENDORS
        .iter()
        .find(|(slug, _)| *slug == runtime || *slug == driver)
        .map(|(_, vendor)| *vendor)
}

/// Describe one offered model id with everything this host actually knows.
///
/// Every field is a fact from a named source or it is absent: the window comes
/// from [`context_window_for_model`], the family from
/// [`model_family_for_model`], and the vendor from the runtime that can only
/// serve one. Nothing is inferred from the spelling of an id, because a reader
/// cannot tell an inferred fact from a measured one — the whole reason the
/// context-window table refuses to guess.
fn describe_model(id: &str, runtime: &str, driver: &str) -> CatalogModel {
    CatalogModel {
        context_window: context_window_for_model(id),
        family: model_family_for_model(id).map(str::to_owned),
        vendor: vendor_for_runtime(runtime, driver).map(str::to_owned),
        deprecated: None,
        ..CatalogModel::new(id)
    }
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
            // Sparse by construction: an id this host knows nothing about
            // contributes no row, so the table never implies knowledge.
            let models: Vec<CatalogModel> = allowed_models
                .iter()
                .map(|model| describe_model(model, &descriptor.runtime, &descriptor.driver))
                .filter(|model| !model.is_bare())
                .collect();
            CatalogProvider {
                provider_instance_ref: descriptor.instance_ref.clone(),
                driver: descriptor.driver.clone(),
                runtime: descriptor.runtime.clone(),
                default_model: descriptor.default_model.clone(),
                allowed_models,
                capabilities: descriptor
                    .capabilities
                    .unwrap_or_else(|| Capabilities::v1_for_runtime(&descriptor.runtime)),
                models,
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

    let mut catalog = Catalog {
        schema: CATALOG_SCHEMA.to_owned(),
        revision,
        providers,
        projects,
    };
    drop_metadata_if_oversized(&mut catalog);
    catalog
}

/// Drop every per-model row rather than publish a body the relay will refuse.
///
/// The offer outranks its description. 32 providers × 64 described models can
/// push the signed body past the relay's 256 KiB ceiling for kind 44222, and a
/// rejected catalog makes *every* model on this host invisible — a far worse
/// outcome than losing the context windows. So when the full body will not
/// fit, the metadata goes and `allowedModels` stays.
///
/// This is all-or-nothing on purpose: trimming rows until it fits would make
/// which models carry a window depend on how many other providers happen to be
/// configured, and a reader has no way to tell that kind of silent truncation
/// from "nobody knows this id".
fn drop_metadata_if_oversized(catalog: &mut Catalog) {
    let fits = to_canonical_json(catalog)
        .map(|json| json.len() <= MAX_CATALOG_CONTENT_BYTES)
        .unwrap_or(false);
    if fits {
        return;
    }
    for provider in &mut catalog.providers {
        provider.models.clear();
    }
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
            actor_seats_file: None,
            context_mcp_command: None,
            instance_id: "instance-1".into(),
            runtimes,
            max_sessions: 4,
            turn_budget: crate::config::UNLIMITED_TURN_BUDGET,
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
            r#"{"schema":"buzz-coding-session-provider-catalog/v1","revision":3,"providers":[{"providerInstanceRef":"claude-primary","driver":"claude-agent-acp","runtime":"claude","defaultModel":"model-b","allowedModels":["model-b","model-a","model-c"],"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true},"models":[{"id":"model-b","vendor":"anthropic"},{"id":"model-a","vendor":"anthropic"},{"id":"model-c","vendor":"anthropic"}]}]"#
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
            r#"{"schema":"buzz-coding-session-provider-catalog/v1","revision":1,"providers":[{"providerInstanceRef":"claude-primary","driver":"claude-agent-acp","runtime":"claude","defaultModel":"default","allowedModels":["default"],"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true},"models":[{"id":"default","vendor":"anthropic"}]},{"providerInstanceRef":"codex-primary","driver":"codex-acp","runtime":"codex","defaultModel":"default","allowedModels":["default"],"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":false},"models":[{"id":"default","vendor":"openai"}]}]"#
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

    /// The metadata is a description of the offer, never an addition to it:
    /// every row names an `allowedModels` id, in that order, and the whole
    /// thing re-parses through the canonical reader that ships it.
    #[test]
    fn per_model_metadata_describes_the_real_ids_and_reparses() {
        let catalog = build(
            &config("opus[1m]", &["sonnet", "haiku", "does-not-exist"]),
            &ProjectsFile::default(),
            2,
        );
        let provider = &catalog.providers[0];
        assert_eq!(
            provider.allowed_models,
            vec!["opus[1m]", "does-not-exist", "haiku", "sonnet"]
        );
        let described: Vec<&str> = provider
            .models
            .iter()
            .map(|model| model.id.as_str())
            .collect();
        // `does-not-exist` still gets a row, because the runtime's single
        // vendor is a fact about it even when nothing knows its window.
        assert_eq!(
            described,
            vec!["opus[1m]", "does-not-exist", "haiku", "sonnet"]
        );
        let opus = &provider.models[0];
        assert_eq!(opus.context_window, Some(1_000_000));
        assert_eq!(opus.family.as_deref(), Some("opus"));
        assert_eq!(opus.vendor.as_deref(), Some("anthropic"));
        let unknown = &provider.models[1];
        assert_eq!(unknown.context_window, None);
        assert_eq!(unknown.family, None);
        assert_eq!(unknown.vendor.as_deref(), Some("anthropic"));
        // No adapter reports deprecation, so nothing claims it.
        assert!(provider
            .models
            .iter()
            .all(|model| model.deprecated.is_none()));

        let json = to_canonical_json(&catalog).expect("serialize");
        let reparsed =
            buzz_core::coding_session_catalog::parse_catalog(&json).expect("canonical reader");
        assert_eq!(reparsed, catalog);
    }

    /// A runtime that takes its provider from configuration can serve several
    /// vendors, so it names none — and with nothing else known about the id,
    /// the row disappears rather than being padded out.
    #[test]
    fn a_multi_vendor_runtime_publishes_no_vendor_and_no_empty_rows() {
        let mut goose = claude_descriptor("house-model", &[]);
        goose.instance_ref = "goose-primary".into();
        goose.driver = "goose-acp".into();
        goose.runtime = "goose".into();
        let catalog = build(
            &config_with_runtimes(vec![goose]),
            &ProjectsFile::default(),
            1,
        );
        assert_eq!(catalog.providers[0].allowed_models, vec!["house-model"]);
        assert!(catalog.providers[0].models.is_empty());
        let json = to_canonical_json(&catalog).expect("serialize");
        assert!(
            !json.contains("\"models\""),
            "unexpected models key: {json}"
        );
        buzz_core::coding_session_catalog::parse_catalog(&json).expect("canonical reader");
    }

    /// Two ids on the same runtime, one recognized and one not: the sparse
    /// half is what proves the table is not filling itself in.
    #[test]
    fn an_unrecognized_id_publishes_no_window_and_no_family() {
        let mut codex = claude_descriptor("gpt-5.6-sol", &["gpt-9-unreleased"]);
        codex.instance_ref = "codex-primary".into();
        codex.driver = "codex-acp".into();
        codex.runtime = "codex".into();
        let catalog = build(
            &config_with_runtimes(vec![codex]),
            &ProjectsFile::default(),
            1,
        );
        let models = &catalog.providers[0].models;
        assert_eq!(models[0].id, "gpt-5.6-sol");
        assert_eq!(models[0].context_window, Some(400_000));
        assert_eq!(models[0].family.as_deref(), Some("gpt-5.6"));
        assert_eq!(models[0].vendor.as_deref(), Some("openai"));
        assert_eq!(models[1].id, "gpt-9-unreleased");
        assert_eq!(models[1].context_window, None);
        assert_eq!(models[1].family, None);
        assert_eq!(models[1].vendor.as_deref(), Some("openai"));
    }

    /// The digest tracks the metadata too: a host that learns a window has
    /// changed what it advertises and owes a revision bump.
    #[test]
    fn the_body_digest_tracks_per_model_metadata() {
        let known = build(&config("opus[1m]", &[]), &ProjectsFile::default(), 1);
        let unknown = build(&config("opus-unheard-of", &[]), &ProjectsFile::default(), 1);
        assert_ne!(body_digest(&known), body_digest(&unknown));
    }

    /// The offer outranks its description: a body that would not fit under the
    /// relay's 256 KiB ceiling sheds its metadata rather than being refused
    /// whole, which would make every model on the host invisible.
    #[test]
    fn an_oversized_body_sheds_its_metadata_and_keeps_the_offer() {
        // 32 providers each offering the full 64 ids, every one of them an id
        // the provider knows a window, a family and a vendor for — the worst
        // case the bounds allow. The ids are sized so the *offer* fits
        // comfortably and only the description pushes it over.
        let runtimes: Vec<RuntimeDescriptor> = (0..MAX_PROVIDERS)
            .map(|index| {
                let mut descriptor = claude_descriptor("gpt-5.6-sol", &[]);
                descriptor.instance_ref = format!("codex-{index:03}");
                descriptor.driver = "codex-acp".into();
                descriptor.runtime = "codex".into();
                descriptor.allowed_models = (0..MAX_ALLOWED_MODELS)
                    .map(|model| format!("gpt-5.6-{}{model:03}", "m".repeat(24)))
                    .collect();
                descriptor.default_model = descriptor.allowed_models[0].clone();
                descriptor
            })
            .collect();
        let catalog = build(&config_with_runtimes(runtimes), &ProjectsFile::default(), 1);
        let json = to_canonical_json(&catalog).expect("serialize");
        assert!(
            json.len() <= MAX_CATALOG_CONTENT_BYTES,
            "body is {} bytes",
            json.len()
        );
        assert!(catalog.providers.iter().all(|p| p.models.is_empty()));
        // The offer itself is untouched — that is the half that must survive.
        assert_eq!(catalog.providers.len(), MAX_PROVIDERS);
        assert_eq!(
            catalog.providers[0].allowed_models.len(),
            MAX_ALLOWED_MODELS
        );
        buzz_core::coding_session_catalog::parse_catalog(&json).expect("canonical reader");
    }
}
