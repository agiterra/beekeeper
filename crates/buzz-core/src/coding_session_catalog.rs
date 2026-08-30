//! The kind:44222 provider catalog — its wire schema, and the reader that
//! refuses anything non-canonical.
//!
//! The catalog is the **only** list of models this product offers. A create,
//! a hire, or a registry row that names an id the catalog does not carry is naming
//! something nobody is serving; the honest answer is to say so and refuse,
//! never to translate the id onto a neighbouring one. That rule is why the
//! schema lives here rather than inside the publisher: the provider
//! (`buzz-session-provider`) writes these bytes, and `bee sessions catalog`,
//! `bee sessions registry check` and `bee sessions route` read them, and none
//! of them may hold its own idea of what is on offer. The router in
//! [`crate::coding_session_routing`] intersects the model registry with this
//! catalog before it applies a single gate: a registry row nothing here offers
//! is dormant and drops out, and an id offered here that no row covers is
//! staleness the check reports.
//!
//! # Canonical bytes are the contract
//!
//! `cspc-key` digests the exact signed content, so a reader re-serializes its
//! parse and compares byte for byte — see [`parse_catalog`]. Two providers
//! advertising the same capabilities must produce identical bytes, and any
//! difference in bytes must be a real difference in what is on offer. Key
//! order is therefore declaration order, and every list has one legal
//! ordering.
//!
//! # Per-model metadata
//!
//! `allowedModels` is the offer. [`CatalogProvider::models`] is optional
//! per-model *description* alongside it — a context window, a family, a
//! vendor, a deprecation flag — carried only for ids something actually knows
//! a fact about. A row is never synthesised to fill the table out: an id with
//! nothing known about it simply has no row, because a reader cannot tell a
//! guessed context window from a measured one and would render both as a
//! percentage.

use serde::{Deserialize, Serialize};

/// Schema string on every catalog advertisement.
pub const CATALOG_SCHEMA: &str = "buzz-coding-session-provider-catalog/v1";

/// NIP-CSPC bound on `providers[]`.
pub const MAX_PROVIDERS: usize = 32;
/// NIP-CSPC bound on `allowedModels[]`.
pub const MAX_ALLOWED_MODELS: usize = 64;
/// NIP-CSPC bound on `projects[]`.
pub const MAX_PROJECTS: usize = 512;
/// NIP-CSPC bound on the signed content, in bytes.
pub const MAX_CATALOG_CONTENT_BYTES: usize = 256 * 1024;
/// NIP-CSPC bound on any single reference string, in bytes.
pub const MAX_REFERENCE_BYTES: usize = 2 * 1024;

/// One catalog advertisement.
///
/// Field order is the wire order; `serde_json::to_string` preserves
/// declaration order, which is what makes [`to_canonical_json`] canonical.
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
    ///
    /// This is the offer, and the whole offer. [`CatalogProvider::models`]
    /// describes ids on this list; it never adds one.
    pub allowed_models: Vec<String>,
    /// What this provider can be asked to do.
    pub capabilities: crate::coding_session_payload::Capabilities,
    /// Per-model description, for the ids something knows a fact about.
    ///
    /// Sparse and optional: entries appear in `allowedModels` order, name only
    /// ids on that list, and each carries at least one fact beyond its id. An
    /// id nothing knows anything about has no entry, and the whole key is
    /// omitted when no id does. Trailing position is deliberate — a reader
    /// that predates this field parses the object up to `capabilities` and
    /// treats the tail as optional.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub models: Vec<CatalogModel>,
}

/// What is known about one offered model id.
///
/// Every field beyond `id` is optional and is omitted rather than guessed —
/// see the module docs. A reader must treat an absent field as "nobody said",
/// never as a default.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CatalogModel {
    /// The model id, exactly as it appears in `allowedModels`.
    pub id: String,
    /// Context window in tokens, when the driver states one or the publisher
    /// holds a recorded figure for this exact id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_window: Option<u64>,
    /// Model family (`opus`, `sonnet`, `gpt-5.6`), when the id's family is
    /// recorded rather than inferred from its spelling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family: Option<String>,
    /// Model vendor (`anthropic`, `openai`), when the runtime behind this
    /// provider can serve exactly one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vendor: Option<String>,
    /// `true` when the publisher has been told this id is going away.
    ///
    /// No adapter reports this today, so nothing sets it; the field exists so
    /// a driver that starts saying so needs no schema change. `None` means
    /// "nobody said", which is not the same as `Some(false)`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deprecated: Option<bool>,
}

impl CatalogModel {
    /// An entry for `id` carrying nothing else — not itself publishable.
    ///
    /// Callers fill the optional fields and then drop the entry if
    /// [`CatalogModel::is_bare`] still holds.
    pub fn new(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            context_window: None,
            family: None,
            vendor: None,
            deprecated: None,
        }
    }

    /// `true` when this entry says nothing beyond naming an id.
    ///
    /// A bare entry is not canonical: `allowedModels` already named the id, so
    /// a row that adds no fact would be a second way to encode one offer.
    pub fn is_bare(&self) -> bool {
        self.context_window.is_none()
            && self.family.is_none()
            && self.vendor.is_none()
            && self.deprecated.is_none()
    }
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

/// Serialize a catalog to its exact signed bytes.
pub fn to_canonical_json(catalog: &Catalog) -> Result<String, serde_json::Error> {
    serde_json::to_string(catalog)
}

/// Why a 44222 body was refused.
///
/// One variant per rule so a refusal can name what it refused; the CLI prints
/// this verbatim rather than reporting "malformed".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CatalogParseError {
    /// Longer than [`MAX_CATALOG_CONTENT_BYTES`].
    TooLarge {
        /// Actual byte length of the content.
        bytes: usize,
    },
    /// Not JSON, or not this schema's shape. Carries serde's own message.
    Malformed(String),
    /// `schema` is not [`CATALOG_SCHEMA`].
    WrongSchema(String),
    /// `revision` is zero.
    NonPositiveRevision,
    /// A structural rule was broken; the string names which.
    NotCanonical(String),
    /// The parse re-serialized to different bytes than it was given.
    ///
    /// This is the `cspc-key` contract failing: whatever the difference is,
    /// the digest a consumer computes will not match the one the signer
    /// published, so the body cannot be trusted as canonical.
    ByteMismatch,
}

impl std::fmt::Display for CatalogParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::TooLarge { bytes } => write!(
                formatter,
                "catalog body is {bytes} bytes, over the {MAX_CATALOG_CONTENT_BYTES}-byte limit"
            ),
            Self::Malformed(detail) => write!(formatter, "catalog body is malformed: {detail}"),
            Self::WrongSchema(schema) => {
                write!(formatter, "catalog body declares schema {schema:?}")
            }
            Self::NonPositiveRevision => {
                write!(formatter, "catalog revision must be greater than zero")
            }
            Self::NotCanonical(detail) => {
                write!(formatter, "catalog body is not canonical: {detail}")
            }
            Self::ByteMismatch => write!(
                formatter,
                "catalog body does not re-serialize to its own bytes, so its cspc-key cannot match"
            ),
        }
    }
}

impl std::error::Error for CatalogParseError {}

/// Parse one 44222 body, enforcing every canonical rule.
///
/// The final check is the load-bearing one: the parse is re-serialized and
/// compared to the input byte for byte. A body that decodes to the same values
/// but different bytes — reordered keys, a duplicated key, `1.0` for `1` — is
/// refused, because `cspc-key` digests bytes and a consumer that accepted it
/// would compute a digest the signer never published.
///
/// # Errors
///
/// [`CatalogParseError`] names the rule that refused it.
pub fn parse_catalog(content: &str) -> Result<Catalog, CatalogParseError> {
    if content.len() > MAX_CATALOG_CONTENT_BYTES {
        return Err(CatalogParseError::TooLarge {
            bytes: content.len(),
        });
    }
    let catalog: Catalog = serde_json::from_str(content)
        .map_err(|error| CatalogParseError::Malformed(error.to_string()))?;
    if catalog.schema != CATALOG_SCHEMA {
        return Err(CatalogParseError::WrongSchema(catalog.schema));
    }
    if catalog.revision == 0 {
        return Err(CatalogParseError::NonPositiveRevision);
    }
    check_providers(&catalog.providers)?;
    check_projects(&catalog.projects, &catalog.providers)?;

    let reserialized = to_canonical_json(&catalog)
        .map_err(|error| CatalogParseError::Malformed(error.to_string()))?;
    if reserialized != content {
        return Err(CatalogParseError::ByteMismatch);
    }
    Ok(catalog)
}

fn bounded_nonblank(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= MAX_REFERENCE_BYTES
}

fn check_providers(providers: &[CatalogProvider]) -> Result<(), CatalogParseError> {
    if providers.is_empty() || providers.len() > MAX_PROVIDERS {
        return Err(CatalogParseError::NotCanonical(format!(
            "providers[] holds {} entries; 1..={MAX_PROVIDERS} allowed",
            providers.len()
        )));
    }
    for (index, provider) in providers.iter().enumerate() {
        if !bounded_nonblank(&provider.provider_instance_ref)
            || !bounded_nonblank(&provider.driver)
            || !bounded_nonblank(&provider.runtime)
            || !bounded_nonblank(&provider.default_model)
        {
            return Err(CatalogParseError::NotCanonical(format!(
                "providers[{index}] carries a blank or oversized reference"
            )));
        }
        if index > 0 && providers[index - 1].provider_instance_ref >= provider.provider_instance_ref
        {
            return Err(CatalogParseError::NotCanonical(
                "providers[] must be sorted by providerInstanceRef with no duplicates".to_owned(),
            ));
        }
        check_allowed_models(index, provider)?;
        check_models(index, provider)?;
    }
    Ok(())
}

fn check_allowed_models(index: usize, provider: &CatalogProvider) -> Result<(), CatalogParseError> {
    let models = &provider.allowed_models;
    if models.is_empty() || models.len() > MAX_ALLOWED_MODELS {
        return Err(CatalogParseError::NotCanonical(format!(
            "providers[{index}].allowedModels holds {} entries; 1..={MAX_ALLOWED_MODELS} allowed",
            models.len()
        )));
    }
    if models[0] != provider.default_model {
        return Err(CatalogParseError::NotCanonical(format!(
            "providers[{index}].allowedModels[0] must be the defaultModel"
        )));
    }
    for (position, model) in models.iter().enumerate() {
        if !bounded_nonblank(model) {
            return Err(CatalogParseError::NotCanonical(format!(
                "providers[{index}].allowedModels[{position}] is blank or oversized"
            )));
        }
        if position > 1 && models[position - 1] >= *model {
            return Err(CatalogParseError::NotCanonical(format!(
                "providers[{index}].allowedModels must be sorted after the default, with no duplicates"
            )));
        }
        if position > 0 && *model == provider.default_model {
            return Err(CatalogParseError::NotCanonical(format!(
                "providers[{index}].allowedModels repeats the defaultModel"
            )));
        }
    }
    Ok(())
}

fn check_models(index: usize, provider: &CatalogProvider) -> Result<(), CatalogParseError> {
    if provider.models.len() > provider.allowed_models.len() {
        return Err(CatalogParseError::NotCanonical(format!(
            "providers[{index}].models describes more ids than allowedModels offers"
        )));
    }
    // `models[]` walks `allowedModels` in order, skipping ids nothing knows
    // anything about. One cursor over the offer therefore validates order,
    // membership, and uniqueness at once.
    let mut cursor = 0usize;
    for (position, model) in provider.models.iter().enumerate() {
        if model.is_bare() {
            return Err(CatalogParseError::NotCanonical(format!(
                "providers[{index}].models[{position}] adds no fact beyond the id"
            )));
        }
        if let Some(family) = &model.family {
            if !bounded_nonblank(family) {
                return Err(CatalogParseError::NotCanonical(format!(
                    "providers[{index}].models[{position}].family is blank or oversized"
                )));
            }
        }
        if let Some(vendor) = &model.vendor {
            if !bounded_nonblank(vendor) {
                return Err(CatalogParseError::NotCanonical(format!(
                    "providers[{index}].models[{position}].vendor is blank or oversized"
                )));
            }
        }
        if model.context_window == Some(0) {
            return Err(CatalogParseError::NotCanonical(format!(
                "providers[{index}].models[{position}].contextWindow is zero; omit it instead"
            )));
        }
        let found = provider.allowed_models[cursor..]
            .iter()
            .position(|offered| *offered == model.id);
        match found {
            Some(offset) => cursor += offset + 1,
            None => {
                return Err(CatalogParseError::NotCanonical(format!(
                    "providers[{index}].models[{position}] names {:?}, which allowedModels does not offer in order",
                    model.id
                )))
            }
        }
    }
    Ok(())
}

fn check_projects(
    projects: &[CatalogProject],
    providers: &[CatalogProvider],
) -> Result<(), CatalogParseError> {
    if projects.is_empty() {
        return Ok(());
    }
    if projects.len() > MAX_PROJECTS {
        return Err(CatalogParseError::NotCanonical(format!(
            "projects[] holds {} entries; at most {MAX_PROJECTS} allowed",
            projects.len()
        )));
    }
    for (index, project) in projects.iter().enumerate() {
        if !bounded_nonblank(&project.project_ref) {
            return Err(CatalogParseError::NotCanonical(format!(
                "projects[{index}].projectRef is blank or oversized"
            )));
        }
        if let Some(repo) = &project.repo_ref {
            if !bounded_nonblank(repo) {
                return Err(CatalogParseError::NotCanonical(format!(
                    "projects[{index}].repoRef is blank or oversized"
                )));
            }
        }
        if project.providers.is_empty() || project.providers.len() > MAX_PROVIDERS {
            return Err(CatalogParseError::NotCanonical(format!(
                "projects[{index}].providers holds {} entries; 1..={MAX_PROVIDERS} allowed",
                project.providers.len()
            )));
        }
        for (position, provider_ref) in project.providers.iter().enumerate() {
            if position > 0 && project.providers[position - 1] >= *provider_ref {
                return Err(CatalogParseError::NotCanonical(format!(
                    "projects[{index}].providers must be sorted with no duplicates"
                )));
            }
            // A narrowing that names a provider this catalog never offered is
            // not a narrowing — it is a claim about something the signer did
            // not declare.
            if !providers
                .iter()
                .any(|provider| provider.provider_instance_ref == *provider_ref)
            {
                return Err(CatalogParseError::NotCanonical(format!(
                    "projects[{index}].providers names {provider_ref:?}, which this catalog does not offer"
                )));
            }
        }
        if index > 0 {
            let previous = &projects[index - 1];
            let left = (
                previous.project_ref.as_str(),
                previous.repo_ref.as_deref().unwrap_or_default(),
            );
            let right = (
                project.project_ref.as_str(),
                project.repo_ref.as_deref().unwrap_or_default(),
            );
            if left >= right {
                return Err(CatalogParseError::NotCanonical(
                    "projects[] must be sorted by (projectRef, repoRef) with no duplicates"
                        .to_owned(),
                ));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coding_session_payload::Capabilities;

    fn provider(models: Vec<CatalogModel>) -> CatalogProvider {
        CatalogProvider {
            provider_instance_ref: "claude-primary".into(),
            driver: "claude-agent-acp".into(),
            runtime: "claude".into(),
            default_model: "opus[1m]".into(),
            allowed_models: vec!["opus[1m]".into(), "haiku".into(), "sonnet".into()],
            capabilities: Capabilities::v1_claude(),
            models,
        }
    }

    fn catalog(models: Vec<CatalogModel>) -> Catalog {
        Catalog {
            schema: CATALOG_SCHEMA.to_owned(),
            revision: 4,
            providers: vec![provider(models)],
            projects: Vec::new(),
        }
    }

    fn described(id: &str, window: u64) -> CatalogModel {
        CatalogModel {
            context_window: Some(window),
            family: Some("opus".into()),
            vendor: Some("anthropic".into()),
            ..CatalogModel::new(id)
        }
    }

    /// The wire order, pinned: `models` sits after `capabilities`, so a reader
    /// written before this field parses the head unchanged and finds the new
    /// key in the optional tail.
    #[test]
    fn per_model_metadata_serializes_after_capabilities() {
        let json =
            to_canonical_json(&catalog(vec![described("opus[1m]", 1_000_000)])).expect("serialize");
        assert!(
            json.contains(
                r#""allowedModels":["opus[1m]","haiku","sonnet"],"capabilities":{"threadTurnStart":true,"threadTurnInterrupt":true,"threadSteer":false,"context":false,"diff":false,"plan":true},"models":[{"id":"opus[1m]","contextWindow":1000000,"family":"opus","vendor":"anthropic"}]"#
            ),
            "unexpected wire form: {json}"
        );
        assert_eq!(
            parse_catalog(&json).expect("parse"),
            catalog(vec![described("opus[1m]", 1_000_000)])
        );
    }

    /// An id nothing knows a fact about has no row at all, and a catalog where
    /// that is true of every id omits the key.
    #[test]
    fn a_catalog_with_nothing_known_omits_the_models_key() {
        let json = to_canonical_json(&catalog(Vec::new())).expect("serialize");
        assert!(
            !json.contains("\"models\""),
            "unexpected models key: {json}"
        );
        parse_catalog(&json).expect("parse");
    }

    /// Sparse is legal: describing one of three offered ids says nothing about
    /// the other two, which is exactly the honest state.
    #[test]
    fn models_may_describe_a_subset_of_the_offer() {
        let json = to_canonical_json(&catalog(vec![described("sonnet", 200_000)])).expect("json");
        let parsed = parse_catalog(&json).expect("parse");
        assert_eq!(parsed.providers[0].models.len(), 1);
        assert_eq!(parsed.providers[0].allowed_models.len(), 3);
    }

    /// A row naming an id the offer does not carry is the exact lie this
    /// schema exists to prevent — a model that looks offered and is not.
    #[test]
    fn a_model_row_outside_the_offer_is_refused() {
        let json =
            to_canonical_json(&catalog(vec![described("gpt-5.6-sol", 400_000)])).expect("json");
        let error = parse_catalog(&json).expect_err("must refuse");
        assert!(
            matches!(&error, CatalogParseError::NotCanonical(detail) if detail.contains("gpt-5.6-sol")),
            "unexpected error: {error}"
        );
    }

    /// Two ways to encode one offer is one too many.
    #[test]
    fn a_row_that_adds_no_fact_is_refused() {
        let json = to_canonical_json(&catalog(vec![CatalogModel::new("opus[1m]")])).expect("json");
        assert!(matches!(
            parse_catalog(&json),
            Err(CatalogParseError::NotCanonical(_))
        ));
    }

    /// `models[]` walks the offer in order; out-of-order or duplicated rows
    /// would give one catalog two byte forms.
    #[test]
    fn model_rows_follow_allowed_model_order() {
        let out_of_order = catalog(vec![
            described("sonnet", 200_000),
            described("haiku", 200_000),
        ]);
        let json = to_canonical_json(&out_of_order).expect("json");
        assert!(matches!(
            parse_catalog(&json),
            Err(CatalogParseError::NotCanonical(_))
        ));

        let duplicated = catalog(vec![
            described("haiku", 200_000),
            described("haiku", 200_000),
        ]);
        let json = to_canonical_json(&duplicated).expect("json");
        assert!(matches!(
            parse_catalog(&json),
            Err(CatalogParseError::NotCanonical(_))
        ));
    }

    /// A zero window is not a small window; it is a measurement nobody made.
    #[test]
    fn a_zero_context_window_is_refused_rather_than_published_as_a_number() {
        let zero = CatalogModel {
            context_window: Some(0),
            ..CatalogModel::new("haiku")
        };
        let json = to_canonical_json(&catalog(vec![zero])).expect("json");
        assert!(matches!(
            parse_catalog(&json),
            Err(CatalogParseError::NotCanonical(_))
        ));
    }

    /// The byte contract: same values, different bytes, refused.
    #[test]
    fn reordered_keys_are_refused_even_though_they_decode() {
        let json = to_canonical_json(&catalog(Vec::new())).expect("json");
        let reordered = json.replace(
            r#"{"schema":"buzz-coding-session-provider-catalog/v1","revision":4"#,
            r#"{"revision":4,"schema":"buzz-coding-session-provider-catalog/v1""#,
        );
        assert_ne!(json, reordered);
        assert_eq!(
            parse_catalog(&reordered),
            Err(CatalogParseError::ByteMismatch)
        );
    }

    #[test]
    fn an_unknown_field_is_refused_rather_than_ignored() {
        let json = to_canonical_json(&catalog(Vec::new()))
            .expect("json")
            .replace(r#""revision":4"#, r#""revision":4,"surprise":true"#);
        assert!(matches!(
            parse_catalog(&json),
            Err(CatalogParseError::Malformed(_))
        ));
    }

    #[test]
    fn a_zero_revision_is_refused() {
        let mut zero = catalog(Vec::new());
        zero.revision = 0;
        let json = to_canonical_json(&zero).expect("json");
        assert_eq!(
            parse_catalog(&json),
            Err(CatalogParseError::NonPositiveRevision)
        );
    }

    #[test]
    fn a_foreign_schema_is_refused_by_name() {
        let mut foreign = catalog(Vec::new());
        foreign.schema = "something-else/v9".into();
        let json = to_canonical_json(&foreign).expect("json");
        assert_eq!(
            parse_catalog(&json),
            Err(CatalogParseError::WrongSchema("something-else/v9".into()))
        );
    }

    #[test]
    fn allowed_models_must_lead_with_the_default() {
        let mut wrong = catalog(Vec::new());
        wrong.providers[0].allowed_models = vec!["haiku".into(), "opus[1m]".into()];
        let json = to_canonical_json(&wrong).expect("json");
        assert!(matches!(
            parse_catalog(&json),
            Err(CatalogParseError::NotCanonical(_))
        ));
    }

    #[test]
    fn a_project_naming_an_unoffered_provider_is_refused() {
        let mut narrowed = catalog(Vec::new());
        narrowed.projects = vec![CatalogProject {
            project_ref: format!("30621:{}:alpha", "cd".repeat(32)),
            repo_ref: None,
            providers: vec!["codex-primary".into()],
        }];
        let json = to_canonical_json(&narrowed).expect("json");
        assert!(matches!(
            parse_catalog(&json),
            Err(CatalogParseError::NotCanonical(_))
        ));
    }

    #[test]
    fn an_oversized_body_is_refused_before_it_is_parsed() {
        let body = "x".repeat(MAX_CATALOG_CONTENT_BYTES + 1);
        assert_eq!(
            parse_catalog(&body),
            Err(CatalogParseError::TooLarge {
                bytes: MAX_CATALOG_CONTENT_BYTES + 1
            })
        );
    }
}
