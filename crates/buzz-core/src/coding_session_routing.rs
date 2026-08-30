//! The model registry and the router — Brian's ruling of 2026-08-30.
//!
//! Two rules govern this module, and they are quoted here because everything
//! below is an implementation of them:
//!
//! > "The lead chooses the capability required. The router chooses the
//! > execution target."
//!
//! > "Select the least expensive execution target whose expected failure mode
//! > is acceptable for the task."
//!
//! Not "the smartest available model", and **never** a weighted average. The
//! ruling is explicit that ranking by `capability_match × cost_efficiency ×
//! velocity` is wrong: a target must first clear every material capability
//! requirement, and cost and speed then choose among the survivors. Cost can
//! never compensate for a capability deficit, so it is not a factor in any
//! gate — it appears only in [`expected_cost`], which runs after every gate
//! has already passed.
//!
//! # What is routed
//!
//! An **execution target**: harness/provider + model + effort. `codex-primary
//! / gpt-5.6-sol / high` and `claude-primary / sonnet / medium` are two
//! targets. The same model in a materially different harness is a different
//! target and may eventually earn different telemetry.
//!
//! # The order, and why it is an order and not a score
//!
//! [`route`] runs spec §7 in sequence:
//!
//! 1. Intersect the registry with the **live catalog**. A registry row nothing
//!    offers today is *dormant*, which is legal — see [`CandidateState::Dormant`].
//! 2. Hard requirements: modality, tools, context window, known failure modes,
//!    and any per-target constraint (Spark's bounded-only rule).
//! 3. Class gates: **every** numeric minimum the class lists, plus any extra
//!    minimum the lead passed in a profile.
//! 4. Risk tier: `Risk = impact × uncertainty × irreversibility`, each 1–5, so
//!    1..=125. FAST 1–8, STANDARD 9–39, DEEP 40–125 — `seed_policy`, not truth.
//! 5. Effort from the tier: FAST→low, STANDARD→medium, DEEP→high. The router
//!    **never** selects `xhigh`, `max` or `ultra`; those are human override
//!    only. It also never escalates effort after a failure — a failed high
//!    goes to a different target or to a reviewer.
//! 6. Standing: a challenger does not hold a route (spec §8). It is sampled
//!    deliberately, by rule, and the routing record marks the sample.
//! 7. Among what is left, the cheapest expected accepted completion.
//!
//! # Honesty
//!
//! Every score in the registry is an *opinion* — each row carries a
//! [`Rating`] saying `operational_opinion` / `confidence: low` with an author
//! and a date, and nothing here may present one as a measurement. When no
//! target clears the bar the answer is [`RouteError::NoEligibleTarget`], which
//! names the binding trait, the number it wanted, and the best score anything
//! available actually has. There is no silent fallback to the smartest model.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

/// The `default` alias every provider publishes for "whatever this host is set
/// to". It is a pointer, not a model, so it is never a registry row and never
/// a routable target.
pub const DEFAULT_ALIAS: &str = "default";

/// Repository-relative path of the registry the router reads.
pub const DEFAULT_REGISTRY_RELATIVE_PATH: &str = "team/model-registry.yaml";

/// Longest `reason` a routing record may carry on the wire.
pub const MAX_ROUTING_REASON_BYTES: usize = 1024;
/// Longest class, tier, provider, model, effort or review-reason token.
pub const MAX_ROUTING_TOKEN_BYTES: usize = 256;
/// Most review reasons one record may list.
pub const MAX_ROUTING_REVIEW_REASONS: usize = 16;
/// Most trait entries one record's `profile` may list.
pub const MAX_ROUTING_PROFILE_ENTRIES: usize = 32;

// ── the registry file ────────────────────────────────────────────────────────

/// The parsed `team/model-registry.yaml`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Registry {
    /// Schema version of this file; echoed onto every routing record.
    pub version: u32,
    /// The day these rows were last reviewed.
    pub updated_at: String,
    /// The ten scored traits, in the order the ruling lists them.
    pub traits: Vec<String>,
    /// Where each `facts` field came from — printed beside any fact that
    /// gated a candidate, so a refusal never rests on an unattributable number.
    #[serde(default)]
    pub facts_provenance: BTreeMap<String, String>,
    /// Risk band → tier policy.
    pub tiers: BTreeMap<String, TierPolicy>,
    /// Class name → its hard gate.
    pub classes: BTreeMap<String, ClassGate>,
    /// Every execution target this registry has an opinion about.
    pub targets: Vec<RegistryTarget>,
}

/// One risk band and the effort it buys.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TierPolicy {
    /// Inclusive `[low, high]` risk score bounds.
    pub risk: [u32; 2],
    /// The one effort this tier buys.
    pub effort: Effort,
    /// `true` while these bounds are a seed policy rather than measured truth.
    #[serde(default)]
    pub seed_policy: bool,
}

/// The three efforts the autonomous router may purchase.
///
/// `xhigh`, `max` and `ultra` are deliberately absent from this enum rather
/// than present and filtered: a value that cannot be constructed cannot be
/// selected by accident, and the ruling is that they are human override only
/// until our own evaluations show they buy something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effort {
    /// FAST.
    Low,
    /// STANDARD.
    Medium,
    /// DEEP — the ceiling for anything the router chooses on its own.
    High,
}

impl Effort {
    /// The wire token: `low`, `medium`, `high`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

impl std::fmt::Display for Effort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One class's hard gate.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassGate {
    /// Trait → minimum. A target must clear **every** entry.
    pub minimums: BTreeMap<String, f64>,
    /// Non-scored requirements: modality, tools.
    #[serde(default)]
    pub requires: Option<ClassRequires>,
    /// When set, a target for this class must not share a provider with the
    /// named class's chosen target — diversity of failure mode (spec §4).
    #[serde(default)]
    pub cross_provider_of: Option<String>,
    /// A caution printed with any decision for this class.
    #[serde(default)]
    pub note: Option<String>,
    /// Set when the class is not one Brian ruled on, naming who drafted it.
    #[serde(default)]
    pub drafted_by: Option<String>,
}

/// A class's non-scored requirements.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ClassRequires {
    /// `Some(true)` when the class cannot be done without image input.
    #[serde(default)]
    pub multimodal: Option<bool>,
    /// Tool families the class needs, e.g. `search`.
    #[serde(default)]
    pub tools: Vec<String>,
}

/// One execution target's row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RegistryTarget {
    /// `providerInstanceRef` exactly as the 44222 catalog publishes it.
    pub provider: String,
    /// Model id exactly as the catalog publishes it; effort variants inherit
    /// this row.
    pub model: String,
    /// Trait → 1–5 prior, or `null` for a trait nobody has an opinion about.
    pub scores: BTreeMap<String, Option<f64>>,
    /// Non-scored claims about the world.
    #[serde(default)]
    pub facts: TargetFacts,
    /// Class (or `class-tier`) → `incumbent` | `challenger`.
    #[serde(default)]
    pub status: BTreeMap<String, String>,
    /// Who holds this opinion, how confident, and when.
    pub rating: Rating,
}

impl RegistryTarget {
    /// `provider/model`, the label a report names this row by.
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider, self.model)
    }

    /// This row's prior for one trait, or `None` when nobody scored it.
    ///
    /// An absent key and an explicit `null` are the same fact — nobody said —
    /// and neither is ever read as zero.
    pub fn score(&self, trait_name: &str) -> Option<f64> {
        self.scores.get(trait_name).copied().flatten()
    }
}

/// Non-scored, factual capabilities — deliberately kept apart from the scores.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetFacts {
    /// `Some(true)` when the target accepts image input. `None` means nobody
    /// said, which never satisfies a modality requirement.
    #[serde(default)]
    pub multimodal: Option<bool>,
    /// Tool families the harness advertises for this target.
    #[serde(default)]
    pub tools: Vec<String>,
    /// Context window in tokens, when the registry states one. Usually absent:
    /// the live catalog's per-model figure is preferred.
    #[serde(default)]
    pub context_window: Option<u64>,
    /// List price, recorded for disclosure. **Not** in the selection formula —
    /// the ruling says our real cost is quota lanes, not API list price.
    #[serde(default)]
    pub price_usd_per_m: Option<Price>,
    /// Which quota lane this target's cost is drawn from.
    #[serde(default)]
    pub quota_class: Option<String>,
    /// Failure modes somebody has recorded for this target. Empty is the
    /// honest state, not a claim that none exist.
    #[serde(default)]
    pub known_failure_modes: Vec<String>,
    /// A hard constraint that applies to this target and no other.
    #[serde(default)]
    pub constraints: Option<TargetConstraints>,
}

/// Published list price per million tokens.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Price {
    /// Input tokens, USD per million.
    pub input: f64,
    /// Output tokens, USD per million.
    pub output: f64,
}

/// A per-target hard constraint (spec §4, RUNNER: Spark).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetConstraints {
    /// Highest task ambiguity this target may be sent. Our wire records
    /// ambiguity as the risk triple's `uncertainty`.
    #[serde(default)]
    pub ambiguity_max: Option<u8>,
    /// Highest irreversibility this target may be sent.
    #[serde(default)]
    pub irreversibility_max: Option<u8>,
    /// The one scope word this target may be sent, e.g. `bounded`. A task
    /// whose scope nobody stated does not satisfy it.
    #[serde(default)]
    pub scope: Option<String>,
}

/// Who holds a row's opinion, and how strongly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rating {
    /// Always `operational_opinion` today. Traits become
    /// `{ prior, measured, samples }` once telemetry exists.
    pub status: String,
    /// `low` until measured cost per accepted task replaces the priors.
    pub confidence: String,
    /// Who rated it.
    pub author: String,
    /// When.
    pub date: String,
}

/// Why a registry file was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryParseError {
    /// Not YAML, or not this schema's shape. Carries serde's own message.
    Malformed(String),
    /// A structural rule was broken; the string names which.
    NotCanonical(String),
}

impl std::fmt::Display for RegistryParseError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Malformed(detail) => write!(formatter, "registry is malformed: {detail}"),
            Self::NotCanonical(detail) => write!(formatter, "registry is not canonical: {detail}"),
        }
    }
}

impl std::error::Error for RegistryParseError {}

/// Parse `team/model-registry.yaml`.
///
/// Unknown keys are refused rather than ignored: a typo in a trait name would
/// otherwise silently drop a gate, and a gate that quietly stops applying is
/// exactly the failure this whole module exists to prevent.
///
/// # Errors
///
/// [`RegistryParseError`] names the rule that refused it.
pub fn parse_registry(text: &str) -> Result<Registry, RegistryParseError> {
    let registry: Registry = serde_yaml::from_str(text)
        .map_err(|error| RegistryParseError::Malformed(error.to_string()))?;
    check_registry(&registry)?;
    Ok(registry)
}

fn check_registry(registry: &Registry) -> Result<(), RegistryParseError> {
    if registry.version == 0 {
        return Err(RegistryParseError::NotCanonical(
            "version must be greater than zero".to_owned(),
        ));
    }
    if registry.targets.is_empty() {
        return Err(RegistryParseError::NotCanonical(
            "targets[] is empty; a registry that knows no target can route nothing".to_owned(),
        ));
    }
    let known: BTreeSet<&str> = registry.traits.iter().map(String::as_str).collect();
    for (name, tier) in &registry.tiers {
        if tier.risk[0] > tier.risk[1] {
            return Err(RegistryParseError::NotCanonical(format!(
                "tiers.{name}.risk is inverted: {:?}",
                tier.risk
            )));
        }
    }
    for (name, class) in &registry.classes {
        if class.minimums.is_empty() {
            return Err(RegistryParseError::NotCanonical(format!(
                "classes.{name}.minimums is empty; a class with no gate gates nothing"
            )));
        }
        for trait_name in class.minimums.keys() {
            if !known.contains(trait_name.as_str()) {
                return Err(RegistryParseError::NotCanonical(format!(
                    "classes.{name}.minimums names {trait_name:?}, which is not one of the \
                     scored traits"
                )));
            }
        }
        if let Some(of) = &class.cross_provider_of {
            if !registry.classes.contains_key(of) {
                return Err(RegistryParseError::NotCanonical(format!(
                    "classes.{name}.crossProviderOf names {of:?}, which is not a class"
                )));
            }
        }
    }
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for target in &registry.targets {
        if target.provider.trim().is_empty() || target.model.trim().is_empty() {
            return Err(RegistryParseError::NotCanonical(
                "a target carries a blank provider or model".to_owned(),
            ));
        }
        if target.model.eq_ignore_ascii_case(DEFAULT_ALIAS) {
            return Err(RegistryParseError::NotCanonical(format!(
                "{} names the {DEFAULT_ALIAS:?} alias, which is a pointer at whatever the host \
                 is set to rather than a model",
                target.label()
            )));
        }
        if !seen.insert((target.provider.as_str(), target.model.as_str())) {
            return Err(RegistryParseError::NotCanonical(format!(
                "{} appears twice",
                target.label()
            )));
        }
        for (trait_name, score) in &target.scores {
            if !known.contains(trait_name.as_str()) {
                return Err(RegistryParseError::NotCanonical(format!(
                    "{} scores {trait_name:?}, which is not one of the scored traits",
                    target.label()
                )));
            }
            if let Some(score) = score {
                if !(1.0..=5.0).contains(score) {
                    return Err(RegistryParseError::NotCanonical(format!(
                        "{} scores {trait_name} at {score}, outside 1..=5",
                        target.label()
                    )));
                }
            }
        }
        for (key, standing) in &target.status {
            if !matches!(standing.as_str(), "incumbent" | "challenger")
                && !standing.starts_with("incumbent-")
                && !standing.starts_with("challenger-")
            {
                return Err(RegistryParseError::NotCanonical(format!(
                    "{} status.{key} is {standing:?}; expected incumbent or challenger",
                    target.label()
                )));
            }
            let (class, _) = split_class_tier(key, registry);
            if !registry.classes.contains_key(class) {
                return Err(RegistryParseError::NotCanonical(format!(
                    "{} has a standing for {class:?}, which is not a class",
                    target.label()
                )));
            }
        }
    }
    Ok(())
}

/// Split a status key into `(class, tier)`.
///
/// Brian's contract writes the same fact two ways — `builder: incumbent-deep`
/// on one row and `builder-fast: incumbent` on another — so both spellings are
/// accepted and normalized here rather than in eleven hand-edited rows.
fn split_class_tier<'a>(key: &'a str, registry: &Registry) -> (&'a str, Option<&'a str>) {
    if registry.classes.contains_key(key) {
        return (key, None);
    }
    if let Some((class, tier)) = key.rsplit_once('-') {
        if registry.tiers.contains_key(tier) {
            return (class, Some(tier));
        }
    }
    (key, None)
}

// ── the live offer ───────────────────────────────────────────────────────────

/// One `(provider, model)` the live 44222 catalog offers, with what the
/// catalog itself says about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OfferedTarget {
    /// `providerInstanceRef`.
    pub provider: String,
    /// Model id, exactly as published — variants included.
    pub model: String,
    /// The catalog's own context window for this id, when it carries one.
    pub context_window: Option<u64>,
}

impl OfferedTarget {
    /// An offer with no metadata attached.
    pub fn new(provider: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            provider: provider.into(),
            model: model.into(),
            context_window: None,
        }
    }
}

/// A model id with its bracket suffix removed.
///
/// `gpt-5.6-sol[high]` and `gpt-5.6-sol[max]` are one model at two effort
/// levels; `opus[1m]` and `opus` are one model at two context windows. The
/// bracket is a knob on a model, not a different model, so a registry row
/// decides at the base and this is how the decision is read.
pub fn base_id(model: &str) -> &str {
    model.split_once('[').map_or(model, |(base, _)| base)
}

// ── the request ──────────────────────────────────────────────────────────────

/// Impact × uncertainty × irreversibility, each 1–5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Risk {
    /// How much it matters if this is right.
    pub impact: u8,
    /// How unsure we are how to do it. Our wire reads this as the task's
    /// ambiguity where a target constrains ambiguity.
    pub uncertainty: u8,
    /// How hard it is to undo.
    pub irreversibility: u8,
}

impl Risk {
    /// `impact × uncertainty × irreversibility`, 1..=125.
    pub fn score(self) -> u32 {
        u32::from(self.impact) * u32::from(self.uncertainty) * u32::from(self.irreversibility)
    }

    /// Refuse anything outside 1–5 rather than clamping it.
    ///
    /// # Errors
    ///
    /// A sentence naming the component that was out of range.
    pub fn validate(self) -> Result<(), String> {
        for (name, value) in [
            ("impact", self.impact),
            ("uncertainty", self.uncertainty),
            ("irreversibility", self.irreversibility),
        ] {
            if !(1..=5).contains(&value) {
                return Err(format!("risk {name} is {value}; each component is 1..=5"));
            }
        }
        Ok(())
    }
}

/// Hard, non-scored requirements the task itself imposes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskNeeds {
    /// `Some(true)` when the task cannot be done without image input.
    pub multimodal: Option<bool>,
    /// Tool families the task needs.
    pub tools: Vec<String>,
    /// Tokens of context the task needs. A target whose window nobody stated
    /// cannot satisfy a stated need.
    pub context_window: Option<u64>,
    /// The task's scope word, e.g. `bounded`. Unstated satisfies no
    /// scope-constrained target.
    pub scope: Option<String>,
    /// Failure modes this task cannot tolerate; a target that records one is
    /// dropped.
    pub incompatible_failure_modes: Vec<String>,
}

/// The spec §6 review triggers the lead can assert. Risk and irreversibility
/// are read from [`Risk`] and are not repeated here.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReviewFlags {
    /// A security, auth or data-loss boundary is touched.
    pub security_boundary: bool,
    /// Architecture, a schema, or a public contract changes.
    pub contract_change: bool,
    /// The builder operated outside an approved plan.
    pub outside_plan: bool,
    /// The builder reports uncertainty.
    pub builder_uncertain: bool,
    /// Tests cannot adequately verify correctness.
    pub tests_insufficient: bool,
    /// The lead explicitly asked for independent judgment.
    pub lead_requests: bool,
}

/// Every review-flag token the CLI accepts, in spec §6 order.
pub const REVIEW_FLAG_NAMES: &[&str] = &[
    "securityBoundary",
    "contractChange",
    "outsidePlan",
    "builderUncertain",
    "testsInsufficient",
    "leadRequests",
];

impl ReviewFlags {
    /// Set one flag by its wire name.
    ///
    /// # Errors
    ///
    /// The unknown token, with the accepted list.
    pub fn set(&mut self, name: &str) -> Result<(), String> {
        match name {
            "securityBoundary" => self.security_boundary = true,
            "contractChange" => self.contract_change = true,
            "outsidePlan" => self.outside_plan = true,
            "builderUncertain" => self.builder_uncertain = true,
            "testsInsufficient" => self.tests_insufficient = true,
            "leadRequests" => self.lead_requests = true,
            other => {
                return Err(format!(
                    "unknown review flag {other:?}; the spec §6 triggers are {}",
                    REVIEW_FLAG_NAMES.join(", ")
                ))
            }
        }
        Ok(())
    }
}

/// One routing question.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RouteRequest {
    /// The class the lead named. The lead names a capability, never a model.
    pub class: String,
    /// The risk triple. The tier is derived from it and is never passed in.
    pub risk: Risk,
    /// Extra per-trait minimums on top of the class gate.
    pub profile: BTreeMap<String, f64>,
    /// Hard, non-scored requirements.
    pub needs: TaskNeeds,
    /// Spec §6 triggers the lead asserts.
    pub review_flags: ReviewFlags,
    /// Deliberately sample a challenger for this class (spec §8).
    pub challenger_sample: bool,
    /// For a cross-provider class: the provider the compared class chose.
    pub counterpart_provider: Option<String>,
}

impl Default for Risk {
    fn default() -> Self {
        Self {
            impact: 1,
            uncertainty: 1,
            irreversibility: 1,
        }
    }
}

// ── the outcome ──────────────────────────────────────────────────────────────

/// Why one candidate is or is not eligible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CandidateState {
    /// Cleared every gate.
    Eligible,
    /// The registry knows it; today's catalog does not offer it. Legal, and
    /// **not** staleness — see the module docs.
    Dormant,
    /// A gate refused it; the string names which and why.
    Rejected(String),
}

/// The standing a target holds for the requested class and tier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Standing {
    /// Seeded as this class's incumbent at exactly this tier.
    IncumbentAtTier,
    /// Seeded as this class's incumbent at every tier.
    Incumbent,
    /// No standing recorded for this class: eligible, but not a seeded route.
    Unranked,
    /// A challenger for this class. Never routed except on a deliberate
    /// sample (spec §8).
    Challenger,
}

impl Standing {
    /// The wire token.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::IncumbentAtTier => "incumbent-at-tier",
            Self::Incumbent => "incumbent",
            Self::Unranked => "unranked",
            Self::Challenger => "challenger",
        }
    }
}

/// One registry row, evaluated against one request.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    /// `providerInstanceRef`.
    pub provider: String,
    /// The registry row's model id.
    pub registry_model: String,
    /// The catalog id that would actually be hired, effort included where the
    /// catalog offers an effort variant. `None` when nothing is offered.
    pub catalog_model: Option<String>,
    /// `true` when [`Self::catalog_model`] itself carries the chosen effort.
    /// `false` means the catalog offers no effort variant of this id, so the
    /// effort is the tier's policy rather than a setting on the wire.
    pub effort_on_the_wire: bool,
    /// Standing for the requested class and tier.
    pub standing: Standing,
    /// Eligible, dormant, or rejected with a reason.
    pub state: CandidateState,
    /// `6 − costEfficiency`; `None` when nobody scored cost efficiency.
    pub cost_prior: Option<f64>,
    /// `6 − velocity`; `None` when nobody scored velocity.
    pub latency_prior: Option<f64>,
    /// Expected attempts to an accepted completion. 1.0 until telemetry.
    pub retry_prior: f64,
    /// `retry × (cost_prior + latency_prior)`.
    ///
    /// **`None` whenever no cost prior is recorded.** The expected cost of an
    /// accepted completion cannot be computed from a latency prior alone, and
    /// printing the latency figure in this field would put a number beside a
    /// target that was not chosen and make the choice look wrong. `None` is
    /// the fact: not comparable on cost. See [`Candidate::rank_score`].
    pub expected_cost: Option<f64>,
    /// Ordering only, never displayed as a cost.
    ///
    /// A target with a cost prior ranks on [`Self::expected_cost`]; a target
    /// without one ranks on latency alone and, by [`rank_key`], only after
    /// every target that has a cost prior. It is never guessed cheap and never
    /// guessed dear.
    pub rank_score: f64,
    /// Which quota lane this target's cost comes out of.
    pub quota_class: Option<String>,
    /// List price, for disclosure only — never in the formula.
    pub price: Option<Price>,
}

impl Candidate {
    /// `provider/model` for the registry row.
    pub fn label(&self) -> String {
        format!("{}/{}", self.provider, self.registry_model)
    }
}

/// A completed routing decision.
#[derive(Debug, Clone, PartialEq)]
pub struct RoutingDecision {
    /// Every candidate considered, eligible first and cheapest first within a
    /// standing band, then the dormant and rejected rows in label order.
    pub candidates: Vec<Candidate>,
    /// The decision, ready for the wire.
    pub record: Routing,
    /// The class gate that was applied.
    pub minimums: BTreeMap<String, f64>,
    /// A caution the class carries, if any.
    pub class_note: Option<String>,
    /// Set when the class itself was drafted by a lane rather than ruled on.
    pub class_drafted_by: Option<String>,
    /// `factsProvenance` entries for the facts that actually gated something.
    pub facts_used: BTreeMap<String, String>,
}

impl RoutingDecision {
    /// The eligible candidates, in the order they were ranked.
    pub fn eligible(&self) -> impl Iterator<Item = &Candidate> {
        self.candidates
            .iter()
            .filter(|candidate| candidate.state == CandidateState::Eligible)
    }
}

/// Why no execution target could be chosen.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteError {
    /// The class is not in the registry.
    UnknownClass {
        /// What was asked for.
        class: String,
        /// What the registry has.
        known: Vec<String>,
    },
    /// The risk triple is out of range; the string is the reason.
    BadRisk(String),
    /// No tier band contains this risk score.
    NoTier {
        /// The computed score.
        score: u32,
    },
    /// Nothing cleared the bar. Never a silent fallback to the smartest model.
    NoEligibleTarget {
        /// The class asked for.
        class: String,
        /// The derived tier.
        tier: String,
        /// The trait that bound the most candidates, and its minimum.
        binding_trait: Option<(String, f64)>,
        /// The best score anything available actually has for that trait.
        best_available: Option<(String, f64)>,
        /// One line per rejected row, in label order.
        rejections: Vec<String>,
    },
}

impl std::fmt::Display for RouteError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownClass { class, known } => write!(
                formatter,
                "no such class {class:?}; the registry knows {}",
                known.join(", ")
            ),
            Self::BadRisk(detail) => formatter.write_str(detail),
            Self::NoTier { score } => write!(
                formatter,
                "risk score {score} falls in no tier band; check tiers[] in the registry"
            ),
            Self::NoEligibleTarget {
                class,
                tier,
                binding_trait,
                best_available,
                ..
            } => {
                write!(formatter, "no eligible model: {class}/{tier}")?;
                let mut minimum_wanted = None;
                if let Some((trait_name, minimum)) = binding_trait {
                    write!(formatter, " needs {trait_name}>={minimum}")?;
                    minimum_wanted = Some(*minimum);
                }
                match best_available {
                    Some((label, score)) => {
                        write!(formatter, "; best available {label} scores {score}")?;
                        // The trait that rejected the most candidates can still
                        // be one something clears on its own — then it is the
                        // combination that is impossible, and saying only
                        // "needs x>=n" would send a reader hunting for a model
                        // that already exists.
                        if minimum_wanted.is_some_and(|minimum| *score >= minimum) {
                            formatter
                                .write_str("; no single target clears every minimum at once")?;
                        }
                    }
                    None => formatter.write_str("; nothing offered today scores it")?,
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for RouteError {}

// ── the wire record ──────────────────────────────────────────────────────────

/// The `routing` object carried on `session.hire`, echoed onto the resulting
/// `session.create` and onto the seat's kind:44223 metadata.
///
/// **One key set, always.** Every field is written, `null` where the answer is
/// not known yet, so a strict observer has exactly one shape to accept. A hire
/// signed by a lead may carry only the question — `class`, `tier`, `risk`,
/// `profile`, `override` — with the router-filled answers `null`; the create
/// the host publishes must carry all of them ([`Routing::is_complete`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Routing {
    /// The capability class the lead named.
    pub class: String,
    /// `fast` | `standard` | `deep`, derived from `risk`, never passed in.
    pub tier: String,
    /// The risk triple and its product.
    pub risk: RoutingRisk,
    /// Extra per-trait minimums the lead asked for, or `null`.
    pub profile: Option<BTreeMap<String, f64>>,
    /// The chosen execution target, or `null` on a hire the router has not
    /// answered yet.
    pub chosen: Option<RoutingTarget>,
    /// The next-best target, or `null` when there was no second.
    pub runner_up: Option<RoutingTarget>,
    /// One sentence naming the gates cleared and why this was cheapest.
    pub reason: Option<String>,
    /// Whether spec §6 requires independent review. `null` when not computed.
    pub review_required: Option<bool>,
    /// The §6 triggers that fired, in spec order. Always an array.
    #[serde(default)]
    pub review_reasons: Vec<String>,
    /// `true` when this decision deliberately sampled a challenger.
    pub challenger_sample: bool,
    /// A human's explicit override of the router, or `null`.
    pub r#override: Option<RoutingOverride>,
    /// `version` of the registry that produced this, or `null`.
    pub registry_version: Option<u32>,
    /// The catalog revision it was intersected with, or `null` when more than
    /// one signer published and there is therefore no single number.
    pub catalog_revision: Option<u64>,
}

/// The risk triple as it rides on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutingRisk {
    /// 1–5.
    pub impact: u8,
    /// 1–5.
    pub uncertainty: u8,
    /// 1–5.
    pub irreversibility: u8,
    /// Their product, 1–125. Written out so a reader never has to multiply.
    pub score: u32,
}

/// One execution target on the wire: harness/provider + model + effort.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutingTarget {
    /// `providerInstanceRef`.
    pub provider: String,
    /// The catalog id that will be hired.
    pub model: String,
    /// `low` | `medium` | `high`. Never above high from the router.
    pub effort: String,
}

/// A human overriding the router.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutingOverride {
    /// The catalog id the human named.
    pub model: String,
    /// The effort the human named, or `null` to take the tier's.
    pub effort: Option<String>,
    /// Why. Required: an unexplained override is indistinguishable from a bug.
    pub because: String,
}

/// `profile` carries `f64` minimums, so `Eq` cannot be derived — but the
/// enclosing lifecycle action is `Eq`, and this record must ride inside it.
///
/// The implementation is sound because a non-reflexive float can never get
/// here: JSON has no `NaN` literal, so a decoded record's minimums are always
/// finite, and [`Routing::validate`] refuses a hand-built record whose minimum
/// is outside `1..=5` — a range check `NaN` also fails.
impl Eq for Routing {}

impl Routing {
    /// `true` when every router-filled field is answered — the shape a
    /// `session.create` and a kind:44223 must carry.
    pub fn is_complete(&self) -> bool {
        self.chosen.is_some()
            && self.reason.is_some()
            && self.review_required.is_some()
            && self.registry_version.is_some()
    }

    /// Check every bound and token.
    ///
    /// # Errors
    ///
    /// A sentence naming the field that was wrong.
    pub fn validate(&self) -> Result<(), String> {
        bounded_token("routing.class", &self.class)?;
        bounded_token("routing.tier", &self.tier)?;
        if self.risk.score
            != u32::from(self.risk.impact)
                * u32::from(self.risk.uncertainty)
                * u32::from(self.risk.irreversibility)
        {
            return Err(
                "routing.risk.score must equal impact × uncertainty × irreversibility".to_owned(),
            );
        }
        Risk {
            impact: self.risk.impact,
            uncertainty: self.risk.uncertainty,
            irreversibility: self.risk.irreversibility,
        }
        .validate()
        .map_err(|detail| format!("routing.{detail}"))?;
        if let Some(profile) = &self.profile {
            if profile.len() > MAX_ROUTING_PROFILE_ENTRIES {
                return Err(format!(
                    "routing.profile holds {} entries; at most {MAX_ROUTING_PROFILE_ENTRIES}",
                    profile.len()
                ));
            }
            for (name, minimum) in profile {
                bounded_token("routing.profile key", name)?;
                if !(1.0..=5.0).contains(minimum) {
                    return Err(format!(
                        "routing.profile.{name} is {minimum}, outside 1..=5"
                    ));
                }
            }
        }
        for (field, target) in [("chosen", &self.chosen), ("runnerUp", &self.runner_up)] {
            if let Some(target) = target {
                bounded_token(&format!("routing.{field}.provider"), &target.provider)?;
                bounded_token(&format!("routing.{field}.model"), &target.model)?;
                if !matches!(target.effort.as_str(), "low" | "medium" | "high") {
                    return Err(format!(
                        "routing.{field}.effort is {:?}; the router only ever purchases low, \
                         medium or high — xhigh, max and ultra are human override only",
                        target.effort
                    ));
                }
            }
        }
        if let Some(reason) = &self.reason {
            if reason.trim().is_empty() || reason.len() > MAX_ROUTING_REASON_BYTES {
                return Err(format!(
                    "routing.reason must be 1..={MAX_ROUTING_REASON_BYTES} non-blank bytes"
                ));
            }
        }
        if self.review_reasons.len() > MAX_ROUTING_REVIEW_REASONS {
            return Err(format!(
                "routing.reviewReasons holds {} entries; at most {MAX_ROUTING_REVIEW_REASONS}",
                self.review_reasons.len()
            ));
        }
        for reason in &self.review_reasons {
            bounded_token("routing.reviewReasons entry", reason)?;
        }
        if self.review_required == Some(false) && !self.review_reasons.is_empty() {
            return Err(
                "routing.reviewRequired is false while reviewReasons names triggers that fired"
                    .to_owned(),
            );
        }
        if let Some(over) = &self.r#override {
            bounded_token("routing.override.model", &over.model)?;
            if let Some(effort) = &over.effort {
                bounded_token("routing.override.effort", effort)?;
            }
            if over.because.trim().is_empty() || over.because.len() > MAX_ROUTING_REASON_BYTES {
                return Err(format!(
                    "routing.override.because must be 1..={MAX_ROUTING_REASON_BYTES} non-blank \
                     bytes: an unexplained override is indistinguishable from a bug"
                ));
            }
        }
        Ok(())
    }
}

fn bounded_token(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_ROUTING_TOKEN_BYTES {
        return Err(format!(
            "{field} must be 1..={MAX_ROUTING_TOKEN_BYTES} non-blank bytes"
        ));
    }
    Ok(())
}

// ── the router ───────────────────────────────────────────────────────────────

/// The retry prior every target carries until telemetry replaces it.
///
/// It is the expected number of attempts to an *accepted* completion, so it
/// multiplies the per-attempt priors rather than adding to them. One flat
/// value for everything is an admission that we have measured nothing yet; it
/// is deliberately not a guess that varies by model.
pub const DEFAULT_RETRY_PRIOR: f64 = 1.0;

/// The cost of one accepted completion, in unit-free priors — **lower is
/// better**.
///
/// ```text
/// cost_prior    = 6 − costEfficiency     (1.0 cheapest … 5.0 dearest)
/// latency_prior = 6 − velocity           (1.0 fastest  … 5.0 slowest)
/// retry_prior   = expected attempts to acceptance (1.0 until telemetry)
///
/// expected_cost = retry_prior × (cost_prior + latency_prior)
/// ```
///
/// The two priors add because they are two costs paid on the same attempt; the
/// retry prior multiplies because it is a count of attempts. List price is
/// **not** in here: the ruling says our real cost is quota lanes, so price is
/// recorded and printed as a fact and never scored.
///
/// A target whose `costEfficiency` nobody scored gets `None` for `cost_prior`
/// and is ranked on latency alone, *after* every target that has a cost prior.
/// It is never guessed cheap and never guessed dear.
pub fn expected_cost(
    cost_prior: Option<f64>,
    latency_prior: Option<f64>,
    retry: f64,
) -> Option<f64> {
    match (cost_prior, latency_prior) {
        (Some(cost), Some(latency)) => Some(retry * (cost + latency)),
        (Some(cost), None) => Some(retry * cost),
        (None, Some(latency)) => Some(retry * latency),
        (None, None) => None,
    }
}

/// Choose the execution target for one request.
///
/// See the module docs for the order. Nothing about capability is traded
/// against cost: [`expected_cost`] is consulted only after every gate has
/// already passed.
///
/// # Errors
///
/// [`RouteError`] — an unknown class, an out-of-range risk, or
/// [`RouteError::NoEligibleTarget`], which names the binding trait and the
/// best score anything available actually has. Boxed because the refusal
/// carries one line per rejected row: the reasons are the point of it, and a
/// refusal that dropped them to stay small would be the silent fallback this
/// module exists to prevent.
pub fn route(
    registry: &Registry,
    offered: &[OfferedTarget],
    request: &RouteRequest,
    catalog_revision: Option<u64>,
) -> Result<RoutingDecision, Box<RouteError>> {
    request.risk.validate().map_err(RouteError::BadRisk)?;
    let class = registry
        .classes
        .get(&request.class)
        .ok_or_else(|| RouteError::UnknownClass {
            class: request.class.clone(),
            known: registry.classes.keys().cloned().collect(),
        })?;
    let score = request.risk.score();
    let (tier_name, tier) = registry
        .tiers
        .iter()
        .find(|(_, tier)| (tier.risk[0]..=tier.risk[1]).contains(&score))
        .ok_or(RouteError::NoTier { score })?;
    let effort = tier.effort;

    let mut minimums = class.minimums.clone();
    for (trait_name, minimum) in &request.profile {
        let entry = minimums.entry(trait_name.clone()).or_insert(*minimum);
        // The stricter of the two wins: a profile may tighten a class gate,
        // never loosen it.
        if *minimum > *entry {
            *entry = *minimum;
        }
    }

    let mut facts_used: BTreeMap<String, String> = BTreeMap::new();
    let mut note_fact = |registry: &Registry, field: &str| {
        if let Some(note) = registry.facts_provenance.get(field) {
            facts_used.insert(field.to_owned(), note.clone());
        }
    };

    let mut candidates: Vec<Candidate> = Vec::with_capacity(registry.targets.len());
    for target in &registry.targets {
        let mut candidate = Candidate {
            provider: target.provider.clone(),
            registry_model: target.model.clone(),
            catalog_model: None,
            effort_on_the_wire: false,
            standing: standing_for(target, registry, &request.class, tier_name),
            state: CandidateState::Eligible,
            cost_prior: target.score("costEfficiency").map(|value| 6.0 - value),
            latency_prior: target.score("velocity").map(|value| 6.0 - value),
            retry_prior: DEFAULT_RETRY_PRIOR,
            expected_cost: None,
            rank_score: f64::MAX,
            quota_class: target.facts.quota_class.clone(),
            price: target.facts.price_usd_per_m,
        };
        let combined = expected_cost(
            candidate.cost_prior,
            candidate.latency_prior,
            candidate.retry_prior,
        );
        candidate.rank_score = combined.unwrap_or(f64::MAX);
        // Only a target with a cost prior has an expected cost. The others
        // rank, but they do not compare.
        candidate.expected_cost = combined.filter(|_| candidate.cost_prior.is_some());

        // 1 — intersect with the live catalog.
        let Some((catalog_model, on_the_wire, catalog_window)) =
            resolve_offer(target, offered, effort)
        else {
            candidate.state = CandidateState::Dormant;
            candidates.push(candidate);
            continue;
        };
        candidate.catalog_model = Some(catalog_model);
        candidate.effort_on_the_wire = on_the_wire;

        // 2 — hard requirements.
        if let Some(reason) =
            hard_requirement_failure(target, class, request, catalog_window, &mut |field| {
                note_fact(registry, field)
            })
        {
            candidate.state = CandidateState::Rejected(reason);
            candidates.push(candidate);
            continue;
        }

        // 3 — class gates, every one of them.
        if let Some(reason) = minimum_failure(target, &minimums) {
            candidate.state = CandidateState::Rejected(reason);
            candidates.push(candidate);
            continue;
        }

        candidates.push(candidate);
    }

    // 4 — standing. A challenger holds no route; it is sampled by rule.
    let sampling = request.challenger_sample
        && candidates
            .iter()
            .any(|c| c.state == CandidateState::Eligible && c.standing == Standing::Challenger);
    for candidate in &mut candidates {
        if candidate.state != CandidateState::Eligible {
            continue;
        }
        let is_challenger = candidate.standing == Standing::Challenger;
        if sampling && !is_challenger {
            candidate.state = CandidateState::Rejected(
                "not sampled: this run deliberately samples a challenger for this class".to_owned(),
            );
        } else if !sampling && is_challenger {
            candidate.state = CandidateState::Rejected(format!(
                "challenger for {}: a challenger earns an incumbent route only through measured \
                 results (spec §8), so it is routed only on a deliberate sample",
                request.class
            ));
        }
    }

    // 5 — cross-provider diversity, when the class asks for it and a
    // counterpart provider is known. Applied only when it leaves somebody
    // standing: the ruling says prefer cross-provider "when an eligible
    // cross-provider verifier exists", not "refuse otherwise".
    let mut cross_provider_applied = false;
    if let (Some(_), Some(counterpart)) = (&class.cross_provider_of, &request.counterpart_provider)
    {
        let survivors = candidates
            .iter()
            .filter(|c| c.state == CandidateState::Eligible && c.provider != *counterpart)
            .count();
        if survivors > 0 {
            cross_provider_applied = true;
            for candidate in &mut candidates {
                if candidate.state == CandidateState::Eligible && candidate.provider == *counterpart
                {
                    candidate.state = CandidateState::Rejected(format!(
                        "same provider as the {} it must review ({counterpart}); an eligible \
                         cross-provider target exists, so failure-mode diversity applies",
                        class.cross_provider_of.clone().unwrap_or_default()
                    ));
                }
            }
        }
    }

    // 6 — rank: standing band first, then the cheapest expected accepted
    // completion. Capability never trades against cost; this runs only over
    // rows that already cleared every gate.
    candidates.sort_by(|left, right| {
        rank_key(left)
            .partial_cmp(&rank_key(right))
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| left.label().cmp(&right.label()))
    });
    let band = candidates
        .iter()
        .find(|c| c.state == CandidateState::Eligible)
        .map(|c| c.standing);
    let mut ranked: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.state == CandidateState::Eligible && Some(c.standing) == band)
        .collect();
    // Everything eligible in a lower band stays eligible for the record, but
    // only the top band can be chosen: a seeded route is not outbid by a row
    // nobody seeded.
    let overflow: Vec<&Candidate> = candidates
        .iter()
        .filter(|c| c.state == CandidateState::Eligible && Some(c.standing) != band)
        .collect();
    ranked.extend(overflow);

    let Some(chosen) = ranked.first().copied().cloned() else {
        return Err(Box::new(no_eligible(
            &request.class,
            tier_name,
            &minimums,
            &candidates,
            registry,
        )));
    };
    let runner_up = ranked.get(1).copied().cloned();

    let review_reasons = review_reasons(request);
    let uncomparable: Vec<String> = ranked
        .iter()
        .filter(|candidate| candidate.cost_prior.is_none())
        .map(|candidate| candidate.label())
        .collect();
    let reason = decision_sentence(
        &chosen,
        runner_up.as_ref(),
        &request.class,
        tier_name,
        effort,
        &minimums,
        sampling,
        cross_provider_applied,
        class.note.as_deref(),
        &uncomparable,
    );

    let record = Routing {
        class: request.class.clone(),
        tier: tier_name.clone(),
        risk: RoutingRisk {
            impact: request.risk.impact,
            uncertainty: request.risk.uncertainty,
            irreversibility: request.risk.irreversibility,
            score,
        },
        profile: if request.profile.is_empty() {
            None
        } else {
            Some(request.profile.clone())
        },
        chosen: Some(RoutingTarget {
            provider: chosen.provider.clone(),
            model: chosen
                .catalog_model
                .clone()
                .unwrap_or_else(|| chosen.registry_model.clone()),
            effort: effort.as_str().to_owned(),
        }),
        runner_up: runner_up.as_ref().map(|candidate| RoutingTarget {
            provider: candidate.provider.clone(),
            model: candidate
                .catalog_model
                .clone()
                .unwrap_or_else(|| candidate.registry_model.clone()),
            effort: effort.as_str().to_owned(),
        }),
        reason: Some(reason),
        review_required: Some(!review_reasons.is_empty()),
        review_reasons,
        challenger_sample: sampling,
        r#override: None,
        registry_version: Some(registry.version),
        catalog_revision,
    };

    let ranked_order: Vec<String> = ranked.iter().map(|c| c.label()).collect();
    let mut ordered: Vec<Candidate> = Vec::with_capacity(candidates.len());
    for label in &ranked_order {
        if let Some(candidate) = candidates.iter().find(|c| c.label() == *label) {
            ordered.push(candidate.clone());
        }
    }
    for candidate in &candidates {
        if !ranked_order.contains(&candidate.label()) {
            ordered.push(candidate.clone());
        }
    }

    Ok(RoutingDecision {
        candidates: ordered,
        record,
        minimums,
        class_note: class.note.clone(),
        class_drafted_by: class.drafted_by.clone(),
        facts_used,
    })
}

/// Sort key: standing band, then whether a cost prior exists, then the
/// expected cost.
fn rank_key(candidate: &Candidate) -> (u8, u8, u8, f64) {
    let state = match candidate.state {
        CandidateState::Eligible => 0,
        CandidateState::Rejected(_) => 1,
        CandidateState::Dormant => 2,
    };
    let band = match candidate.standing {
        Standing::IncumbentAtTier => 0,
        Standing::Incumbent => 1,
        Standing::Unranked => 2,
        Standing::Challenger => 3,
    };
    // A target with no cost prior is ranked after every target that has one.
    // It is never guessed cheap; see `expected_cost`.
    let priced = u8::from(candidate.cost_prior.is_none());
    (
        state,
        band,
        priced,
        candidate.expected_cost.unwrap_or(f64::MAX),
    )
}

/// The standing this row holds for one class at one tier.
fn standing_for(target: &RegistryTarget, registry: &Registry, class: &str, tier: &str) -> Standing {
    let mut untiered: Option<Standing> = None;
    for (key, value) in &target.status {
        let (key_class, key_tier) = split_class_tier(key, registry);
        if key_class != class {
            continue;
        }
        // `builder: incumbent-deep` and `builder-deep: incumbent` are the same
        // fact written two ways; both normalize here.
        let (standing_word, value_tier) = match value.split_once('-') {
            Some((word, tail)) if registry.tiers.contains_key(tail) => (word, Some(tail)),
            _ => (value.as_str(), None),
        };
        let effective_tier = key_tier.or(value_tier);
        let standing = match (standing_word, effective_tier) {
            ("challenger", _) => Standing::Challenger,
            ("incumbent", Some(at)) if at == tier => Standing::IncumbentAtTier,
            ("incumbent", Some(_)) => continue, // seeded at a different tier only
            ("incumbent", None) => Standing::Incumbent,
            _ => continue,
        };
        if standing == Standing::IncumbentAtTier {
            return standing;
        }
        untiered = Some(match untiered {
            Some(held) => held.min(standing),
            None => standing,
        });
    }
    untiered.unwrap_or(Standing::Unranked)
}

/// The catalog id this row would be hired as at one effort, if anything offers
/// it: the effort variant when the catalog publishes one, else the row's own
/// id, else the lowest-sorting variant of its base.
fn resolve_offer(
    target: &RegistryTarget,
    offered: &[OfferedTarget],
    effort: Effort,
) -> Option<(String, bool, Option<u64>)> {
    let here: Vec<&OfferedTarget> = offered
        .iter()
        .filter(|offer| {
            offer.provider == target.provider
                && !offer.model.eq_ignore_ascii_case(DEFAULT_ALIAS)
                && base_id(&offer.model) == base_id(&target.model)
        })
        .collect();
    if here.is_empty() {
        return None;
    }
    let with_effort = format!("{}[{}]", base_id(&target.model), effort.as_str());
    if let Some(offer) = here.iter().find(|offer| offer.model == with_effort) {
        return Some((offer.model.clone(), true, offer.context_window));
    }
    if let Some(offer) = here.iter().find(|offer| offer.model == target.model) {
        return Some((offer.model.clone(), false, offer.context_window));
    }
    let mut sorted = here;
    sorted.sort_by(|left, right| left.model.cmp(&right.model));
    sorted
        .first()
        .map(|offer| (offer.model.clone(), false, offer.context_window))
}

/// The first hard requirement this target fails, if any.
fn hard_requirement_failure(
    target: &RegistryTarget,
    class: &ClassGate,
    request: &RouteRequest,
    catalog_window: Option<u64>,
    note_fact: &mut dyn FnMut(&str),
) -> Option<String> {
    let requires = class.requires.clone().unwrap_or_default();
    if requires.multimodal == Some(true) || request.needs.multimodal == Some(true) {
        note_fact("multimodal");
        if target.facts.multimodal != Some(true) {
            return Some(match target.facts.multimodal {
                Some(false) => {
                    "not multimodal, and this class cannot be done without image input".to_owned()
                }
                _ => "nobody has recorded whether this target is multimodal, so it cannot satisfy \
                      a modality requirement"
                    .to_owned(),
            });
        }
    }
    let mut wanted: Vec<&str> = requires.tools.iter().map(String::as_str).collect();
    wanted.extend(request.needs.tools.iter().map(String::as_str));
    if !wanted.is_empty() {
        note_fact("tools");
        for tool in wanted {
            if !target.facts.tools.iter().any(|held| held == tool) {
                return Some(format!(
                    "no {tool} tool: the registry records tools {:?} for this target",
                    target.facts.tools
                ));
            }
        }
    }
    if let Some(needed) = request.needs.context_window {
        note_fact("contextWindow");
        let window = target.facts.context_window.or(catalog_window);
        match window {
            Some(window) if window >= needed => {}
            Some(window) => {
                return Some(format!(
                    "context window {window} is below the {needed} this task needs"
                ))
            }
            None => {
                return Some(format!(
                    "no context window recorded here or in the catalog, so it cannot be shown to \
                     hold the {needed} tokens this task needs"
                ))
            }
        }
    }
    if !request.needs.incompatible_failure_modes.is_empty() {
        note_fact("knownFailureModes");
        for mode in &request.needs.incompatible_failure_modes {
            if target.facts.known_failure_modes.contains(mode) {
                return Some(format!(
                    "known failure mode {mode:?} is incompatible with this task"
                ));
            }
        }
    }
    if let Some(constraints) = &target.facts.constraints {
        note_fact("constraints");
        if let Some(max) = constraints.ambiguity_max {
            if request.risk.uncertainty > max {
                return Some(format!(
                    "this target is constrained to ambiguity <= {max}; the task's uncertainty is {}",
                    request.risk.uncertainty
                ));
            }
        }
        if let Some(max) = constraints.irreversibility_max {
            if request.risk.irreversibility > max {
                return Some(format!(
                    "this target is constrained to irreversibility <= {max}; the task's is {}",
                    request.risk.irreversibility
                ));
            }
        }
        if let Some(scope) = &constraints.scope {
            match request.needs.scope.as_deref() {
                Some(stated) if stated == scope => {}
                Some(stated) => {
                    return Some(format!(
                        "this target is constrained to scope {scope:?}; the task's scope is \
                         {stated:?}"
                    ))
                }
                None => {
                    return Some(format!(
                        "this target is constrained to scope {scope:?} and nobody stated this \
                         task's scope — unstated is not bounded"
                    ))
                }
            }
        }
    }
    None
}

/// The first class minimum this target fails, if any.
fn minimum_failure(target: &RegistryTarget, minimums: &BTreeMap<String, f64>) -> Option<String> {
    for (trait_name, minimum) in minimums {
        match target.score(trait_name) {
            Some(score) if score >= *minimum => {}
            Some(score) => {
                return Some(format!(
                    "{trait_name} {score} is below the {minimum} this class needs"
                ))
            }
            None => {
                return Some(format!(
                    "no {trait_name} score recorded, so it cannot be shown to clear the {minimum} \
                     this class needs"
                ))
            }
        }
    }
    None
}

/// The spec §6 triggers that fired, in spec order.
fn review_reasons(request: &RouteRequest) -> Vec<String> {
    let mut reasons = Vec::new();
    if request.risk.score() >= 40 {
        reasons.push(format!("risk {} >= 40", request.risk.score()));
    }
    if request.risk.irreversibility >= 4 {
        reasons.push(format!(
            "irreversibility {} >= 4",
            request.risk.irreversibility
        ));
    }
    let flags = request.review_flags;
    for (fired, name) in [
        (flags.security_boundary, "securityBoundary"),
        (flags.contract_change, "contractChange"),
        (flags.outside_plan, "outsidePlan"),
        (flags.builder_uncertain, "builderUncertain"),
        (flags.tests_insufficient, "testsInsufficient"),
        (flags.lead_requests, "leadRequests"),
    ] {
        if fired {
            reasons.push(name.to_owned());
        }
    }
    reasons
}

/// The one-sentence `reason` the routing record carries.
#[allow(clippy::too_many_arguments)]
fn decision_sentence(
    chosen: &Candidate,
    runner_up: Option<&Candidate>,
    class: &str,
    tier: &str,
    effort: Effort,
    minimums: &BTreeMap<String, f64>,
    sampling: bool,
    cross_provider: bool,
    class_note: Option<&str>,
    uncomparable: &[String],
) -> String {
    use std::fmt::Write as _;

    let gates: Vec<String> = minimums
        .iter()
        .map(|(name, minimum)| format!("{name}>={minimum}"))
        .collect();
    let mut sentence = format!(
        "{} cleared the {class} gate ({})",
        chosen.label(),
        gates.join(", ")
    );

    // The load-bearing honesty: "cheapest" is only true within the standing
    // band that was actually selected from. A seeded incumbent is regularly
    // dearer than an unranked row that is eligible but holds no route, and a
    // sentence that said "cheapest" over a visibly cheaper runner-up would be
    // a claim the numbers beside it disprove.
    let outbid = match (
        chosen.expected_cost,
        runner_up.and_then(|c| c.expected_cost),
    ) {
        (Some(mine), Some(theirs)) => theirs < mine,
        _ => false,
    };
    if outbid {
        let runner_up = runner_up.expect("outbid implies a runner-up");
        let _ = write!(
            sentence,
            " and is the cheapest {} for {class}/{tier} at {effort} ({:.1}); {} is cheaper at {:.1} but is {} for this class and tier, so it holds no route here",
            chosen.standing.as_str(),
            chosen.expected_cost.unwrap_or_default(),
            runner_up.label(),
            runner_up.expected_cost.unwrap_or_default(),
            runner_up.standing.as_str()
        );
    } else {
        let _ = write!(
            sentence,
            " and is the cheapest expected accepted completion at {tier}/{effort}"
        );
        match (
            chosen.expected_cost,
            runner_up.and_then(|c| c.expected_cost),
        ) {
            (Some(mine), Some(theirs)) => {
                let _ = write!(
                    sentence,
                    " ({mine:.1} vs {theirs:.1} for {})",
                    runner_up.map_or_else(String::new, Candidate::label)
                );
            }
            (Some(mine), None) => {
                let _ = write!(sentence, " ({mine:.1})");
            }
            _ => sentence.push_str(" (no cost prior is recorded for it)"),
        }
    }

    if !chosen.effort_on_the_wire {
        let _ = write!(
            sentence,
            "; the catalog offers no effort variant of {}, so {effort} is the tier's policy \
             rather than a setting on the wire",
            chosen
                .catalog_model
                .clone()
                .unwrap_or_else(|| chosen.registry_model.clone())
        );
    }
    if let (Some(mine), Some(theirs)) = (
        chosen.quota_class.as_deref(),
        runner_up.and_then(|c| c.quota_class.as_deref()),
    ) {
        if mine != theirs {
            let _ = write!(
                sentence,
                "; the runner-up draws on {theirs} and this on {mine}, so that comparison is \
                 across two quota lanes"
            );
        }
    }
    // A target that cleared every gate and has no cost prior would otherwise
    // vanish from the sentence while sitting in the table with a low-looking
    // number beside it. Named, with the reason it was not compared.
    if !uncomparable.is_empty() {
        let _ = write!(
            sentence,
            "; {} also cleared every gate but {} no cost prior recorded, so {} cannot be \
             compared on cost and {} not chosen on it",
            uncomparable.join(", "),
            if uncomparable.len() == 1 {
                "has"
            } else {
                "have"
            },
            if uncomparable.len() == 1 {
                "it"
            } else {
                "they"
            },
            if uncomparable.len() == 1 {
                "was"
            } else {
                "were"
            }
        );
    }
    if sampling {
        sentence.push_str("; this is a deliberate challenger sample (spec §8)");
    }
    if cross_provider {
        sentence.push_str("; same-provider targets were removed for failure-mode diversity");
    }
    if let Some(note) = class_note {
        let _ = write!(sentence, "; {note}");
    }
    sentence
}

/// Build the honest empty result: which trait bound it, and what the best
/// available score for that trait actually is.
fn no_eligible(
    class: &str,
    tier: &str,
    minimums: &BTreeMap<String, f64>,
    candidates: &[Candidate],
    registry: &Registry,
) -> RouteError {
    let live: BTreeSet<String> = candidates
        .iter()
        .filter(|candidate| candidate.state != CandidateState::Dormant)
        .map(Candidate::label)
        .collect();
    let rows: Vec<&RegistryTarget> = registry
        .targets
        .iter()
        .filter(|target| live.contains(&target.label()))
        .collect();
    // The binding trait is the one that rejected the most live rows — not
    // merely the first alphabetically, and not only a trait nothing clears.
    // Ties break on the name so the sentence is stable run to run.
    let mut binding: Option<(String, f64)> = None;
    let mut worst = 0usize;
    for (trait_name, minimum) in minimums {
        let failures = rows
            .iter()
            .filter(|target| {
                target
                    .score(trait_name)
                    .is_none_or(|score| score < *minimum)
            })
            .count();
        if failures > worst {
            worst = failures;
            binding = Some((trait_name.clone(), *minimum));
        }
    }
    let best_available = binding.as_ref().and_then(|(trait_name, _)| {
        rows.iter()
            .filter_map(|target| {
                target
                    .score(trait_name)
                    .map(|score| (target.label(), score))
            })
            .max_by(|left, right| {
                left.1
                    .partial_cmp(&right.1)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
    });
    let mut rejections: Vec<String> = candidates
        .iter()
        .filter_map(|candidate| match &candidate.state {
            CandidateState::Rejected(reason) => Some(format!("{}: {reason}", candidate.label())),
            CandidateState::Dormant => Some(format!(
                "{}: not offered by today's catalog (dormant registry row, which is not staleness)",
                candidate.label()
            )),
            CandidateState::Eligible => None,
        })
        .collect();
    rejections.sort();
    RouteError::NoEligibleTarget {
        class: class.to_owned(),
        tier: tier.to_owned(),
        binding_trait: binding,
        best_available,
        rejections,
    }
}

// ── registry ↔ catalog coverage ──────────────────────────────────────────────

/// What comparing the registry to the live catalog found.
///
/// The two directions are **not** symmetric, and the asymmetry is the ruling:
///
/// * [`Self::dormant`] — a registry row today's catalog does not offer. Legal.
///   The registry is allowed to know a model this host is not serving.
/// * [`Self::stale`] — a live offered execution target no registry row covers.
///   *That* is staleness, and it is what makes the check exit non-zero: a
///   target the router cannot reason about is a target the lead will reach for
///   with nothing to go on.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RegistryCoverage {
    /// `provider/model` rows the catalog does not offer, sorted.
    pub dormant: Vec<String>,
    /// Offered ids no row covers — one entry per uncovered base, named with an
    /// id the catalog really offers, sorted.
    pub stale: Vec<String>,
    /// Offered ids a row covers by the base rule but does not name literally.
    /// Informational: a variant is a setting of a model already decided about,
    /// so this never makes the registry stale.
    pub variants: Vec<String>,
}

impl RegistryCoverage {
    /// `true` when every offered target has a row.
    ///
    /// [`Self::dormant`] is deliberately not consulted — see the type docs.
    pub fn is_fresh(&self) -> bool {
        self.stale.is_empty()
    }
}

/// Compare the registry's rows to the live offer.
pub fn check_coverage(registry: &Registry, offered: &[OfferedTarget]) -> RegistryCoverage {
    let mut dormant: BTreeSet<String> = BTreeSet::new();
    for target in &registry.targets {
        let offered_here = offered.iter().any(|offer| {
            offer.provider == target.provider && base_id(&offer.model) == base_id(&target.model)
        });
        if !offered_here {
            dormant.insert(target.label());
        }
    }

    let mut uncovered: BTreeMap<(String, String), BTreeSet<String>> = BTreeMap::new();
    let mut variants: BTreeSet<String> = BTreeSet::new();
    for offer in offered {
        if offer.model.eq_ignore_ascii_case(DEFAULT_ALIAS) {
            continue;
        }
        let covered = registry.targets.iter().any(|target| {
            target.provider == offer.provider && base_id(&target.model) == base_id(&offer.model)
        });
        if !covered {
            uncovered
                .entry((offer.provider.clone(), base_id(&offer.model).to_owned()))
                .or_default()
                .insert(offer.model.clone());
        }
        let named_exactly = registry
            .targets
            .iter()
            .any(|target| target.provider == offer.provider && target.model == offer.model);
        if !named_exactly {
            variants.insert(format!("{}/{}", offer.provider, offer.model));
        }
    }
    // One gap per base, named with an id the catalog really offers: a bare
    // base nobody serves would send a reader to write a row this same check
    // would then call dormant.
    let stale: BTreeSet<String> = uncovered
        .into_iter()
        .map(|((provider, base), ids)| {
            let representative = if ids.contains(&base) {
                base
            } else {
                ids.iter().next().cloned().unwrap_or(base)
            };
            format!("{provider}/{representative}")
        })
        .collect();
    for label in &stale {
        variants.remove(label);
    }
    RegistryCoverage {
        dormant: dormant.into_iter().collect(),
        stale: stale.into_iter().collect(),
        variants: variants.into_iter().collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn repo_root() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .canonicalize()
            .expect("repo root")
    }

    /// The registry this repository actually ships.
    fn shipped() -> Registry {
        let path = repo_root().join(DEFAULT_REGISTRY_RELATIVE_PATH);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        parse_registry(&text)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", path.display()))
    }

    /// The recorded live catalog, as offers.
    fn live_offer() -> Vec<OfferedTarget> {
        let path = repo_root().join("testdata/routing/live-catalog-665076ce.json");
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        let fixture: serde_json::Value = serde_json::from_str(&text).expect("fixture is JSON");
        fixture["offered"]
            .as_array()
            .expect("offered")
            .iter()
            .map(|pair| OfferedTarget {
                provider: pair["providerInstanceRef"]
                    .as_str()
                    .expect("provider")
                    .to_owned(),
                model: pair["model"].as_str().expect("model").to_owned(),
                context_window: pair["contextWindow"].as_u64(),
            })
            .collect()
    }

    fn request(class: &str, risk: (u8, u8, u8)) -> RouteRequest {
        RouteRequest {
            class: class.to_owned(),
            risk: Risk {
                impact: risk.0,
                uncertainty: risk.1,
                irreversibility: risk.2,
            },
            ..RouteRequest::default()
        }
    }

    fn chosen(decision: &RoutingDecision) -> String {
        let target = decision.record.chosen.as_ref().expect("a chosen target");
        format!("{}/{}", target.provider, target.model)
    }

    fn runner_up(decision: &RoutingDecision) -> Option<String> {
        decision
            .record
            .runner_up
            .as_ref()
            .map(|target| format!("{}/{}", target.provider, target.model))
    }

    // ── the file ─────────────────────────────────────────────────────────────

    /// The shipped registry parses, carries eleven rows, and every one of them
    /// says out loud that it is an opinion.
    #[test]
    fn the_shipped_registry_parses_and_every_row_admits_it_is_an_opinion() {
        let registry = shipped();
        assert_eq!(registry.version, 1);
        assert_eq!(registry.targets.len(), 11, "eleven seed rows");
        assert_eq!(registry.traits.len(), 10);
        for target in &registry.targets {
            assert_eq!(
                target.rating.status,
                "operational_opinion",
                "{} claims more than an opinion",
                target.label()
            );
            assert_eq!(target.rating.confidence, "low", "{}", target.label());
            assert!(!target.rating.author.trim().is_empty());
            assert!(!target.rating.date.trim().is_empty());
        }
    }

    /// The autonomous ceiling is High. `xhigh`, `max` and `ultra` are not
    /// values this enum can hold, so the router cannot reach them by accident.
    #[test]
    fn no_tier_buys_more_than_high_effort() {
        let registry = shipped();
        assert_eq!(registry.tiers["fast"].effort, Effort::Low);
        assert_eq!(registry.tiers["standard"].effort, Effort::Medium);
        assert_eq!(registry.tiers["deep"].effort, Effort::High);
        for (name, tier) in &registry.tiers {
            assert!(tier.seed_policy, "tiers.{name} must be marked seed_policy");
        }
        for token in ["xhigh", "max", "ultra"] {
            assert!(
                serde_yaml::from_str::<Effort>(token).is_err(),
                "{token} must not be an effort the router can hold"
            );
        }
    }

    /// The `default` alias is a pointer at whatever the host is set to, not a
    /// model, so it can never become a registry row.
    #[test]
    fn the_default_alias_cannot_be_a_registry_row() {
        let text = std::fs::read_to_string(repo_root().join(DEFAULT_REGISTRY_RELATIVE_PATH))
            .expect("read")
            .replace("model: sonnet\n", "model: default\n");
        let error = parse_registry(&text).expect_err("must refuse");
        assert!(
            matches!(&error, RegistryParseError::NotCanonical(detail) if detail.contains("default")),
            "unexpected: {error}"
        );
    }

    /// A misspelled trait in a class gate would silently stop gating. Refused.
    #[test]
    fn a_class_gate_naming_an_unknown_trait_is_refused() {
        let text = std::fs::read_to_string(repo_root().join(DEFAULT_REGISTRY_RELATIVE_PATH))
            .expect("read")
            .replace(
                "minimums: { agency: 4.0, discipline: 3.7, velocity: 4.5 }",
                "minimums: { agency: 4.0, discipline: 3.7, velicity: 4.5 }",
            );
        assert!(matches!(
            parse_registry(&text),
            Err(RegistryParseError::NotCanonical(_))
        ));
    }

    // ── the seeds reproduce ──────────────────────────────────────────────────

    /// Brian's §4 seeds, reproduced by the rules rather than hard-coded.
    /// This is the load-bearing test of the whole module: if the gates, the
    /// standing bands and the cost formula are right, these five answers fall
    /// out; if any of them is wrong, at least one of these moves.
    #[test]
    fn the_recorded_decisions_hold_for_the_live_catalog() {
        let registry = shipped();
        let offer = live_offer();

        // BUILDER / STANDARD — spec §4: "STANDARD: Sonnet 5, Terra".
        let builder = route(&registry, &offer, &request("builder", (3, 3, 2)), Some(7))
            .expect("a builder route");
        assert_eq!(builder.record.tier, "standard");
        assert_eq!(builder.record.risk.score, 18);
        assert_eq!(chosen(&builder), "claude-primary/sonnet");
        assert_eq!(
            builder
                .record
                .chosen
                .as_ref()
                .expect("chosen")
                .effort
                .as_str(),
            "medium"
        );
        assert_eq!(builder.record.registry_version, Some(1));
        assert_eq!(builder.record.catalog_revision, Some(7));

        // ARCHITECT / DEEP — spec §4: "Seed: Sol, Opus 5, Fable".
        let architect = route(&registry, &offer, &request("architect", (5, 4, 4)), None)
            .expect("an architect route");
        assert_eq!(architect.record.tier, "deep");
        assert_eq!(chosen(&architect), "codex-primary/gpt-5.6-sol[high]");
        assert_eq!(
            runner_up(&architect).as_deref(),
            Some("claude-primary/opus[1m]")
        );

        // RUNNER / FAST.
        let runner =
            route(&registry, &offer, &request("runner", (1, 1, 1)), None).expect("a runner route");
        assert_eq!(runner.record.tier, "fast");
        assert_eq!(chosen(&runner), "codex-primary/gpt-5.6-luna[low]");

        // UI_DESIGNER / DEEP — multimodal is a hard requirement, not a score.
        let designer = route(&registry, &offer, &request("ui_designer", (5, 4, 4)), None)
            .expect("a designer route");
        assert_eq!(designer.record.tier, "deep");
        assert_eq!(chosen(&designer), "claude-primary/sonnet");

        // VERIFIER / STANDARD when the builder ran on codex — spec §4's
        // "Sol builder → Opus verifier".
        let mut verifier = request("verifier", (3, 3, 2));
        verifier.counterpart_provider = Some("codex-primary".to_owned());
        let verifier = route(&registry, &offer, &verifier, None).expect("a verifier route");
        assert_eq!(chosen(&verifier), "claude-primary/opus[1m]");
        for candidate in verifier.eligible() {
            assert_ne!(
                candidate.provider, "codex-primary",
                "a cross-provider verifier must not sit on the builder's provider"
            );
        }
    }

    /// A challenger holds no route (spec §8) — until the lead samples one, and
    /// then the record says so. This is the Terra question, answered.
    #[test]
    fn a_challenger_is_routed_only_on_a_deliberate_sample() {
        let registry = shipped();
        let offer = live_offer();

        let routine = route(&registry, &offer, &request("builder", (3, 3, 2)), None)
            .unwrap_or_else(|error| panic!("{error}"));
        assert!(!routine.record.challenger_sample);
        assert!(
            routine
                .eligible()
                .all(|candidate| candidate.standing != Standing::Challenger),
            "a challenger must not be eligible on a routine run"
        );

        let mut sampled = request("builder", (3, 3, 2));
        sampled.challenger_sample = true;
        let sampled = route(&registry, &offer, &sampled, None).expect("a sampled route");
        assert!(sampled.record.challenger_sample);
        assert_eq!(chosen(&sampled), "codex-primary/gpt-5.6-terra[medium]");
        assert!(
            sampled
                .record
                .reason
                .as_deref()
                .expect("reason")
                .contains("challenger sample"),
            "the record must say it sampled: {:?}",
            sampled.record.reason
        );
    }

    /// Spark is text-only and bounded-only. A task whose scope nobody stated
    /// does not satisfy "scope == bounded" — unstated is not bounded.
    #[test]
    fn spark_is_refused_until_the_task_is_stated_bounded() {
        let registry = shipped();
        let offer = live_offer();
        let unstated =
            route(&registry, &offer, &request("runner", (1, 1, 1)), None).expect("a route");
        let spark = unstated
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "gpt-5.3-codex-spark")
            .expect("spark is a candidate");
        assert!(
            matches!(&spark.state, CandidateState::Rejected(reason) if reason.contains("scope")),
            "unexpected: {:?}",
            spark.state
        );

        let mut bounded = request("runner", (1, 1, 1));
        bounded.needs.scope = Some("bounded".to_owned());
        let bounded = route(&registry, &offer, &bounded, None).expect("a route");
        let spark = bounded
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "gpt-5.3-codex-spark")
            .expect("spark is a candidate");
        assert_eq!(spark.state, CandidateState::Eligible);
        // …but Brian recorded no cost prior for it, so it cannot win a
        // cheapest-completion comparison against anything that has one. That
        // is disclosed, never smoothed over into a guess.
        assert_eq!(spark.cost_prior, None);
        assert_ne!(chosen(&bounded), "codex-primary/gpt-5.3-codex-spark[low]");

        // Ambiguity above 2 refuses it even when the scope is bounded.
        let mut ambiguous = request("runner", (1, 3, 1));
        ambiguous.needs.scope = Some("bounded".to_owned());
        let ambiguous = route(&registry, &offer, &ambiguous, None).expect("a route");
        let spark = ambiguous
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "gpt-5.3-codex-spark")
            .expect("spark");
        assert!(
            matches!(&spark.state, CandidateState::Rejected(reason) if reason.contains("ambiguity")),
            "unexpected: {:?}",
            spark.state
        );
    }

    /// Cost never rescues a capability deficit. Haiku is the cheapest thing in
    /// the registry and it is not eligible to lead anything.
    #[test]
    fn the_cheapest_target_cannot_buy_its_way_past_a_class_gate() {
        let registry = shipped();
        let offer = live_offer();
        let lead = route(&registry, &offer, &request("lead", (4, 4, 3)), None).expect("a route");
        let haiku = lead
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "haiku")
            .expect("haiku");
        assert!(matches!(haiku.state, CandidateState::Rejected(_)));
        assert!(
            haiku.expected_cost.expect("a cost")
                < lead
                    .candidates
                    .iter()
                    .find(|c| c.registry_model == "opus[1m]")
                    .expect("opus")
                    .expected_cost
                    .expect("a cost"),
            "haiku must really be cheaper, or this test proves nothing"
        );
        // And the eligible set is exactly Brian's §4 seed for lead.
        let eligible: Vec<String> = lead.eligible().map(Candidate::label).collect();
        assert_eq!(eligible.len(), 3, "unexpected: {eligible:?}");
        for label in [
            "codex-primary/gpt-5.6-sol",
            "claude-primary/opus[1m]",
            "claude-primary/claude-fable-5[1m]",
        ] {
            assert!(
                eligible.contains(&label.to_owned()),
                "missing {label} from {eligible:?}"
            );
        }
    }

    /// The honest empty result: it names the trait, the number, and the best
    /// score anything available actually has. It never falls back to the
    /// smartest model.
    #[test]
    fn an_impossible_gate_names_the_trait_and_the_best_available_score() {
        let registry = shipped();
        let offer = live_offer();
        let mut impossible = request("runner", (1, 1, 1));
        impossible.profile.insert("reasoning".to_owned(), 4.9);
        impossible.profile.insert("velocity".to_owned(), 4.9);
        let error = route(&registry, &offer, &impossible, None).expect_err("must refuse");
        let RouteError::NoEligibleTarget {
            binding_trait,
            best_available,
            ..
        } = error.as_ref()
        else {
            panic!("unexpected: {error}")
        };
        // Reasoning 4.9 rejects nine of the eleven rows; velocity 4.9 rejects
        // seven. The most-binding one is named.
        assert_eq!(binding_trait.as_ref().expect("trait").0, "reasoning");
        let (label, score) = best_available.as_ref().expect("a best available");
        assert!(*score >= 4.9, "{label} scores {score}");
        let text = error.to_string();
        assert!(text.starts_with("no eligible model: runner/fast"), "{text}");
        assert!(text.contains("reasoning>=4.9"), "{text}");
        assert!(
            text.contains("best available") && text.contains("scores 5"),
            "{text}"
        );
        // Sol clears reasoning on its own, so the honest fact is that the
        // *combination* is impossible — never "nothing is good enough".
        assert!(
            text.contains("no single target clears every minimum at once"),
            "{text}"
        );

        // And a gate genuinely nothing scores high enough for says exactly
        // that, with the best score anything really has.
        let mut unreachable = request("runner", (1, 1, 1));
        unreachable.profile.insert("taste".to_owned(), 5.0);
        let error = route(&registry, &offer, &unreachable, None).expect_err("must refuse");
        let text = error.to_string();
        assert!(text.contains("taste>=5"), "{text}");
        assert!(text.contains("scores 4.9"), "{text}");
        assert!(
            !text.contains("no single target clears every minimum at once"),
            "{text}"
        );
    }

    /// A dormant row is not staleness; an offered target with no row is.
    #[test]
    fn dormant_is_not_stale_and_stale_is_the_other_direction() {
        let registry = shipped();
        let offer = live_offer();
        let coverage = check_coverage(&registry, &offer);
        assert!(
            coverage.stale.is_empty(),
            "the shipped registry does not cover {:?}",
            coverage.stale
        );
        assert!(
            coverage.dormant.is_empty(),
            "unexpected: {:?}",
            coverage.dormant
        );
        assert!(coverage.is_fresh());

        // Drop a live model from the offer: the row goes dormant, and dormant
        // is still fresh.
        let narrowed: Vec<OfferedTarget> = offer
            .iter()
            .filter(|offer| base_id(&offer.model) != "haiku")
            .cloned()
            .collect();
        let coverage = check_coverage(&registry, &narrowed);
        assert_eq!(coverage.dormant, vec!["claude-primary/haiku"]);
        assert!(coverage.stale.is_empty());
        assert!(
            coverage.is_fresh(),
            "a dormant row must not be called stale"
        );

        // Add a model nothing has a row for: that is staleness, and it bites.
        let mut widened = offer.clone();
        widened.push(OfferedTarget::new("claude-primary", "claude-mythos-9"));
        let coverage = check_coverage(&registry, &widened);
        assert_eq!(coverage.stale, vec!["claude-primary/claude-mythos-9"]);
        assert!(!coverage.is_fresh());
    }

    /// A registry row a provider does not offer today drops out of routing as
    /// dormant, and the refusal says which.
    #[test]
    fn a_dormant_row_drops_out_of_routing_by_name() {
        let registry = shipped();
        let offer: Vec<OfferedTarget> = live_offer()
            .into_iter()
            .filter(|offer| offer.provider != "claude-primary")
            .collect();
        let decision =
            route(&registry, &offer, &request("builder", (3, 3, 2)), None).expect("a route");
        let sonnet = decision
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "sonnet")
            .expect("sonnet");
        assert_eq!(sonnet.state, CandidateState::Dormant);
        // With Claude gone the standard builder falls to the next band, not to
        // a challenger.
        assert_ne!(chosen(&decision), "codex-primary/gpt-5.6-terra[medium]");
    }

    // ── review ───────────────────────────────────────────────────────────────

    /// Review is the §6 trigger list, not a synonym for DEEP.
    #[test]
    fn review_follows_the_trigger_list_and_not_the_tier() {
        let registry = shipped();
        let offer = live_offer();

        // STANDARD, but the boundary is security: review is required.
        let mut standard = request("builder", (3, 3, 2));
        standard.review_flags.security_boundary = true;
        let decision = route(&registry, &offer, &standard, None).expect("a route");
        assert_eq!(decision.record.tier, "standard");
        assert_eq!(decision.record.review_required, Some(true));
        assert_eq!(decision.record.review_reasons, vec!["securityBoundary"]);

        // DEEP by risk alone still fires, on the risk trigger.
        let deep = route(&registry, &offer, &request("architect", (5, 4, 4)), None).expect("route");
        assert_eq!(decision.record.tier, "standard");
        assert_eq!(deep.record.review_required, Some(true));
        assert_eq!(
            deep.record.review_reasons,
            vec!["risk 80 >= 40", "irreversibility 4 >= 4"]
        );

        // A cheap reversible job fires nothing.
        let quiet = route(&registry, &offer, &request("runner", (1, 1, 1)), None).expect("route");
        assert_eq!(quiet.record.review_required, Some(false));
        assert!(quiet.record.review_reasons.is_empty());

        // Irreversibility alone, well under risk 40.
        let mut fragile = request("runner", (1, 1, 4));
        fragile.review_flags = ReviewFlags::default();
        let fragile = route(&registry, &offer, &fragile, None).expect("route");
        assert_eq!(fragile.record.risk.score, 4);
        assert_eq!(fragile.record.tier, "fast");
        assert_eq!(fragile.record.review_required, Some(true));
        assert_eq!(
            fragile.record.review_reasons,
            vec!["irreversibility 4 >= 4"]
        );
    }

    #[test]
    fn every_spec_review_flag_has_a_name_the_cli_accepts() {
        let mut flags = ReviewFlags::default();
        for name in REVIEW_FLAG_NAMES {
            flags.set(name).unwrap_or_else(|error| panic!("{error}"));
        }
        assert!(flags.security_boundary && flags.lead_requests);
        let error = flags.set("looksHard").expect_err("must refuse");
        assert!(error.contains("securityBoundary"), "{error}");
    }

    // ── the cost formula ─────────────────────────────────────────────────────

    /// The formula, spelled out, and the rule for an unknown cost prior.
    #[test]
    fn the_cost_formula_is_the_documented_one() {
        // sonnet: costEfficiency 4.5 → 1.5; velocity 4.6 → 1.4; retry 1.0.
        assert!((expected_cost(Some(1.5), Some(1.4), 1.0).expect("cost") - 2.9).abs() < 1e-9);
        // Retry multiplies because it counts attempts to acceptance.
        assert!((expected_cost(Some(1.5), Some(1.4), 2.0).expect("cost") - 5.8).abs() < 1e-9);
        // No cost prior: ranked on latency alone, never guessed cheap.
        assert!((expected_cost(None, Some(1.0), 1.0).expect("cost") - 1.0).abs() < 1e-9);
        assert_eq!(expected_cost(None, None, 1.0), None);
        assert_eq!(DEFAULT_RETRY_PRIOR, 1.0);
    }

    /// Price is recorded and printed; it is not in the formula.
    #[test]
    fn list_price_is_a_fact_and_not_a_term() {
        let registry = shipped();
        let luna = registry
            .targets
            .iter()
            .find(|target| target.model == "gpt-5.6-luna")
            .expect("luna");
        let price = luna.facts.price_usd_per_m.expect("a price");
        assert!((price.input - 0.20).abs() < 1e-9);
        // Luna is the cheapest listed price AND the cheapest prior, so the two
        // agreeing proves nothing on its own; what proves it is that changing
        // the price cannot change the ranking, because `expected_cost` takes
        // no price argument at all. That is a fact about the signature.
        assert!((expected_cost(Some(1.0), Some(1.0), 1.0).expect("cost") - 2.0).abs() < 1e-9);
    }

    // ── the wire record ──────────────────────────────────────────────────────

    /// One key set, always: a strict observer has exactly one shape to accept.
    #[test]
    fn the_routing_record_writes_every_key_even_when_the_answer_is_null() {
        let registry = shipped();
        let decision = route(
            &registry,
            &live_offer(),
            &request("builder", (3, 3, 2)),
            Some(7),
        )
        .expect("a route");
        let json = serde_json::to_string(&decision.record).expect("serialize");
        for key in [
            "class",
            "tier",
            "risk",
            "profile",
            "chosen",
            "runnerUp",
            "reason",
            "reviewRequired",
            "reviewReasons",
            "challengerSample",
            "override",
            "registryVersion",
            "catalogRevision",
        ] {
            assert!(
                json.contains(&format!("\"{key}\":")),
                "missing {key}: {json}"
            );
        }
        let value: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert_eq!(value.as_object().expect("object").len(), 13);
        decision.record.validate().expect("valid");
        assert!(decision.record.is_complete());
        // Round-trips byte for byte.
        let again: Routing = serde_json::from_str(&json).expect("decode");
        assert_eq!(again, decision.record);
    }

    /// A hire may carry only the question; the router fills the rest.
    #[test]
    fn a_hire_shaped_record_is_valid_but_not_complete() {
        let question = Routing {
            class: "builder".into(),
            tier: "standard".into(),
            risk: RoutingRisk {
                impact: 3,
                uncertainty: 3,
                irreversibility: 2,
                score: 18,
            },
            profile: None,
            chosen: None,
            runner_up: None,
            reason: None,
            review_required: None,
            review_reasons: Vec::new(),
            challenger_sample: false,
            r#override: None,
            registry_version: None,
            catalog_revision: None,
        };
        question.validate().expect("valid");
        assert!(!question.is_complete());
    }

    /// The record cannot carry an effort the router is forbidden to purchase.
    #[test]
    fn an_effort_above_high_is_refused_on_the_wire() {
        let registry = shipped();
        let mut decision = route(
            &registry,
            &live_offer(),
            &request("architect", (5, 4, 4)),
            None,
        )
        .expect("a route")
        .record;
        for forbidden in ["xhigh", "max", "ultra"] {
            decision
                .chosen
                .as_mut()
                .expect("chosen")
                .effort
                .clone_from(&forbidden.to_owned());
            let error = decision.validate().expect_err("must refuse");
            assert!(error.contains("human override only"), "{error}");
        }
    }

    /// `reviewRequired: false` beside a list of triggers that fired is a lie
    /// about the same object, and the validator refuses it.
    #[test]
    fn a_record_cannot_claim_no_review_while_listing_triggers() {
        let mut record = Routing {
            class: "builder".into(),
            tier: "standard".into(),
            risk: RoutingRisk {
                impact: 3,
                uncertainty: 3,
                irreversibility: 2,
                score: 18,
            },
            profile: None,
            chosen: None,
            runner_up: None,
            reason: None,
            review_required: Some(false),
            review_reasons: vec!["securityBoundary".into()],
            challenger_sample: false,
            r#override: None,
            registry_version: None,
            catalog_revision: None,
        };
        assert!(record.validate().is_err());
        record.review_required = Some(true);
        record.validate().expect("valid");
    }

    /// An override with no explanation is indistinguishable from a bug.
    #[test]
    fn an_override_must_say_why() {
        let mut record = Routing {
            class: "builder".into(),
            tier: "standard".into(),
            risk: RoutingRisk {
                impact: 3,
                uncertainty: 3,
                irreversibility: 2,
                score: 18,
            },
            profile: None,
            chosen: None,
            runner_up: None,
            reason: None,
            review_required: None,
            review_reasons: Vec::new(),
            challenger_sample: false,
            r#override: Some(RoutingOverride {
                model: "opus[1m]".into(),
                effort: None,
                because: "  ".into(),
            }),
            registry_version: None,
            catalog_revision: None,
        };
        let error = record.validate().expect_err("must refuse");
        assert!(error.contains("indistinguishable from a bug"), "{error}");
        record
            .r#override
            .as_mut()
            .expect("override")
            .because
            .clone_from(&"Brian asked for Opus on this one".to_owned());
        record.validate().expect("valid");
    }

    #[test]
    fn an_unknown_key_in_a_routing_record_is_refused_rather_than_ignored() {
        let json = r#"{"class":"builder","tier":"standard","risk":{"impact":1,"uncertainty":1,"irreversibility":1,"score":1},"profile":null,"chosen":null,"runnerUp":null,"reason":null,"reviewRequired":null,"reviewReasons":[],"challengerSample":false,"override":null,"registryVersion":null,"catalogRevision":null,"surprise":true}"#;
        assert!(serde_json::from_str::<Routing>(json).is_err());
    }

    #[test]
    fn a_risk_component_outside_one_to_five_is_refused_rather_than_clamped() {
        let registry = shipped();
        let offer = live_offer();
        let error = route(&registry, &offer, &request("builder", (0, 3, 2)), None)
            .expect_err("must refuse");
        assert!(matches!(error.as_ref(), RouteError::BadRisk(detail) if detail.contains("impact")));
        let error = route(&registry, &offer, &request("builder", (3, 9, 2)), None)
            .expect_err("must refuse");
        assert!(
            matches!(error.as_ref(), RouteError::BadRisk(detail) if detail.contains("uncertainty"))
        );
    }

    #[test]
    fn an_unknown_class_lists_the_ones_the_registry_knows() {
        let registry = shipped();
        let error = route(
            &registry,
            &live_offer(),
            &request("wizard", (1, 1, 1)),
            None,
        )
        .expect_err("must refuse");
        let text = error.to_string();
        assert!(
            text.contains("builder") && text.contains("verifier"),
            "{text}"
        );
    }

    /// The `poker` class is this lane's draft, not Brian's ruling, and the
    /// decision says so rather than presenting it as ruled.
    #[test]
    fn a_lane_drafted_class_is_labelled_on_every_decision_it_makes() {
        let registry = shipped();
        let decision =
            route(&registry, &live_offer(), &request("poker", (2, 2, 2)), None).expect("a route");
        let drafted = decision
            .class_drafted_by
            .expect("poker is labelled drafted");
        assert!(drafted.contains("not ruled on by Brian"), "{drafted}");
        for class in ["lead", "architect", "builder", "runner", "verifier"] {
            assert!(
                registry.classes[class].drafted_by.is_none(),
                "{class} is Brian's and must not be labelled drafted"
            );
        }
    }

    /// Where an effort is not expressible in a catalog id, the record says so
    /// instead of implying the wire carries it.
    #[test]
    fn an_effort_the_catalog_cannot_express_is_disclosed_rather_than_implied() {
        let registry = shipped();
        let offer = live_offer();
        let sonnet =
            route(&registry, &offer, &request("builder", (3, 3, 2)), None).expect("a route");
        assert_eq!(chosen(&sonnet), "claude-primary/sonnet");
        let reason = sonnet.record.reason.as_deref().expect("reason");
        assert!(
            reason.contains("the tier's policy rather than a setting on the wire"),
            "{reason}"
        );

        // Codex publishes an effort variant, so there is nothing to disclose.
        let sol =
            route(&registry, &offer, &request("architect", (5, 4, 4)), None).expect("a route");
        assert_eq!(chosen(&sol), "codex-primary/gpt-5.6-sol[high]");
        let reason = sol.record.reason.as_deref().expect("reason");
        assert!(
            !reason.contains("rather than a setting on the wire"),
            "{reason}"
        );
    }

    /// A target with no cost prior has no expected cost — not a low one.
    ///
    /// Spark is the case: Brian recorded `costEfficiency: null` for it, so it
    /// can clear every gate and still never win a cheapest-completion
    /// comparison. Reporting its latency figure as an expected cost would put
    /// a smaller number beside a target that was not chosen, which reads as a
    /// bug in the router rather than as the missing measurement it is.
    #[test]
    fn a_target_with_no_cost_prior_reports_no_expected_cost_and_is_named() {
        let registry = shipped();
        let offer = live_offer();
        let mut bounded = request("runner", (1, 1, 1));
        bounded.needs.scope = Some("bounded".to_owned());
        let decision = route(&registry, &offer, &bounded, None).expect("a route");
        let spark = decision
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "gpt-5.3-codex-spark")
            .expect("spark");
        assert_eq!(spark.state, CandidateState::Eligible);
        assert_eq!(spark.cost_prior, None);
        assert_eq!(
            spark.expected_cost, None,
            "a latency figure must not be published as an expected cost"
        );
        // It still ranks — after every target in its band that does have a
        // cost prior, by the documented rule.
        let band: Vec<String> = decision
            .eligible()
            .filter(|candidate| candidate.standing == spark.standing)
            .map(Candidate::label)
            .collect();
        assert_eq!(
            band.last().map(String::as_str),
            Some("codex-primary/gpt-5.3-codex-spark"),
            "band order: {band:?}"
        );
        assert!(band.len() > 1, "band order: {band:?}");
        let reason = decision.record.reason.as_deref().expect("reason");
        assert!(
            reason.contains(
                "codex-primary/gpt-5.3-codex-spark also cleared every gate but has \
                             no cost prior recorded"
            ) || reason.contains(
                "gpt-5.3-codex-spark also cleared every gate but has no cost \
                                    prior recorded"
            ),
            "{reason}"
        );
        assert!(reason.contains("cannot be compared on cost"), "{reason}");
    }

    /// The sentence must never claim "cheapest" over a visibly cheaper
    /// runner-up. A seeded incumbent is regularly dearer than an eligible row
    /// that holds no route, and saying "cheapest" beside numbers that disprove
    /// it is the exact class of bug this module exists to prevent.
    #[test]
    fn the_reason_never_claims_cheapest_over_a_cheaper_runner_up() {
        let registry = shipped();
        let offer = live_offer();

        // builder/standard: sonnet (2.9) is the incumbent; luna (2.0) is only
        // a seeded FAST builder, so it is cheaper and still not routable here.
        let decision =
            route(&registry, &offer, &request("builder", (3, 3, 2)), None).expect("a route");
        let chosen = decision.candidates[0].clone();
        let runner_up = decision.candidates[1].clone();
        assert!(
            runner_up.expected_cost < chosen.expected_cost,
            "this test only bites when the runner-up really is cheaper"
        );
        let reason = decision.record.reason.as_deref().expect("reason");
        assert!(
            !reason.contains("cheapest expected accepted completion"),
            "claimed cheapest over a cheaper runner-up: {reason}"
        );
        assert!(
            reason.contains("is the cheapest incumbent for builder/standard"),
            "{reason}"
        );
        assert!(
            reason.contains(
                "is cheaper at 2.0 but is unranked for this class and tier, so it \
                             holds no route here"
            ) || reason.contains("is cheaper at 2.0 but is unranked"),
            "{reason}"
        );

        // architect/deep: sol really is the cheapest of the eligible set, so
        // the plain claim is the true one and is still made.
        let decision =
            route(&registry, &offer, &request("architect", (5, 4, 4)), None).expect("a route");
        let reason = decision.record.reason.as_deref().expect("reason");
        assert!(
            reason.contains("cheapest expected accepted completion at deep/high"),
            "{reason}"
        );
        assert!(!reason.contains("holds no route here"), "{reason}");
    }

    /// A profile may tighten a class gate; it may never loosen one.
    #[test]
    fn a_profile_tightens_a_class_gate_and_never_loosens_it() {
        let registry = shipped();
        let offer = live_offer();
        let mut loosened = request("lead", (4, 4, 3));
        loosened.profile.insert("reasoning".to_owned(), 1.0);
        let decision = route(&registry, &offer, &loosened, None).expect("a route");
        assert_eq!(
            decision.minimums["reasoning"], 4.8,
            "the class gate must survive a lower profile figure"
        );
        let eligible: Vec<String> = decision.eligible().map(Candidate::label).collect();
        assert_eq!(eligible.len(), 3, "unexpected: {eligible:?}");

        let mut tightened = request("builder", (3, 3, 2));
        tightened.profile.insert("taste".to_owned(), 4.8);
        let decision = route(&registry, &offer, &tightened, None).expect("a route");
        assert_eq!(decision.minimums["taste"], 4.8);
        assert_ne!(chosen(&decision), "claude-primary/sonnet");
    }

    /// A seat's own kind:44223 row can answer "why this model", and a row
    /// that never routed says nothing rather than implying a decision.
    #[test]
    fn session_metadata_carries_the_routing_record_only_when_one_exists() {
        use crate::coding_session_command::CodingSessionTarget;
        use crate::coding_session_payload::{Capabilities, SessionMetadata, SessionStatus};

        let decision = route(
            &shipped(),
            &live_offer(),
            &request("builder", (3, 3, 2)),
            Some(7),
        )
        .expect("a route");
        let mut metadata = SessionMetadata {
            schema: crate::coding_session_payload::METADATA_SCHEMA.to_owned(),
            session: CodingSessionTarget {
                driver: "claude-agent-acp".into(),
                instance_id: "claude-primary".into(),
                session_id: "s-1".into(),
                generation: 1,
            },
            project_ref: None,
            repo_ref: None,
            title: None,
            agent_ref: None,
            role: None,
            provider: Some("claude-primary".into()),
            runtime: Some("claude".into()),
            model: Some("sonnet".into()),
            status: SessionStatus::Running,
            branch: None,
            capabilities: Capabilities::v1_claude(),
            session_ref: None,
            observed_commit: None,
            dirty: None,
            relay_reachable: None,
            verified_at: None,
            turn_budget: None,
            routing: None,
        };
        let json = serde_json::to_string(&metadata).expect("json");
        assert!(
            !json.contains("routing"),
            "an unrouted seat must not carry an empty routing key: {json}"
        );

        metadata.routing = Some(decision.record.clone());
        let json = serde_json::to_string(&metadata).expect("json");
        let decoded: SessionMetadata = serde_json::from_str(&json).expect("decode");
        assert_eq!(decoded.routing.as_ref(), Some(&decision.record));
        assert_eq!(
            decoded
                .routing
                .as_ref()
                .and_then(|routing| routing.chosen.as_ref())
                .map(|target| target.model.as_str()),
            Some("sonnet")
        );
    }

    /// The provenance of any fact that gated something is carried with the
    /// decision, so a refusal never rests on an unattributable number.
    #[test]
    fn a_fact_that_gated_a_candidate_carries_its_provenance() {
        let registry = shipped();
        let designer = route(
            &registry,
            &live_offer(),
            &request("ui_designer", (5, 4, 4)),
            None,
        )
        .expect("a route");
        let note = designer
            .facts_used
            .get("multimodal")
            .expect("multimodal gated the designer class, so its provenance travels");
        assert!(note.contains("2026-08-30"), "{note}");

        let researcher = route(
            &registry,
            &live_offer(),
            &request("researcher", (3, 3, 2)),
            None,
        )
        .expect("a route");
        let note = researcher.facts_used.get("tools").expect("tools gated it");
        assert!(
            note.contains("LANE-DRAFTED"),
            "the tools fact is this lane's reading and must say so: {note}"
        );
    }
    /// The cross-implementation contract, on the catalog this relay really
    /// served: every decision in `testdata/routing/live-catalog-665076ce.json`
    /// is recorded once and asserted here. Lane B's TypeScript router pins to
    /// the same file, so the two cannot silently disagree.
    #[test]
    fn every_recorded_decision_in_the_fixture_still_holds() {
        let registry = shipped();
        let offer = live_offer();
        let path = repo_root().join("testdata/routing/live-catalog-665076ce.json");
        let fixture: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");

        let coverage = check_coverage(&registry, &offer);
        let expected = &fixture["expectedCoverage"];
        assert_eq!(
            coverage.dormant,
            expected["dormant"]
                .as_array()
                .expect("dormant")
                .iter()
                .map(|value| value.as_str().expect("string").to_owned())
                .collect::<Vec<String>>()
        );
        assert_eq!(
            coverage.stale,
            expected["stale"]
                .as_array()
                .expect("stale")
                .iter()
                .map(|value| value.as_str().expect("string").to_owned())
                .collect::<Vec<String>>()
        );

        let cases = fixture["expectedDecisions"]
            .as_array()
            .expect("expectedDecisions");
        assert_eq!(cases.len(), 6, "the fixture records six decisions");
        for case in cases {
            let class = case["class"].as_str().expect("class");
            let risk = &case["risk"];
            let mut wanted = RouteRequest {
                class: class.to_owned(),
                risk: Risk {
                    impact: risk["impact"].as_u64().expect("impact") as u8,
                    uncertainty: risk["uncertainty"].as_u64().expect("uncertainty") as u8,
                    irreversibility: risk["irreversibility"].as_u64().expect("irreversibility")
                        as u8,
                },
                ..RouteRequest::default()
            };
            wanted.counterpart_provider = case["counterpartProvider"].as_str().map(str::to_owned);
            wanted.challenger_sample = case["challengerSample"].as_bool().unwrap_or(false);
            let decision = route(&registry, &offer, &wanted, None)
                .unwrap_or_else(|error| panic!("{class}: {error}"));
            let label = format!("{class} {risk}");
            assert_eq!(
                decision.record.tier,
                case["tier"].as_str().expect("tier"),
                "{label} tier"
            );
            assert_eq!(
                chosen(&decision),
                case["chosen"].as_str().expect("chosen"),
                "{label}"
            );
            assert_eq!(
                runner_up(&decision).as_deref(),
                case["runnerUp"].as_str(),
                "{label} runner-up"
            );
            assert_eq!(
                decision
                    .record
                    .chosen
                    .as_ref()
                    .expect("chosen")
                    .effort
                    .as_str(),
                case["effort"].as_str().expect("effort"),
                "{label} effort"
            );
            assert_eq!(
                decision.record.review_required,
                case["reviewRequired"].as_bool(),
                "{label} reviewRequired"
            );
            assert_eq!(
                decision.record.review_reasons,
                case["reviewReasons"]
                    .as_array()
                    .expect("reviewReasons")
                    .iter()
                    .map(|value| value.as_str().expect("string").to_owned())
                    .collect::<Vec<String>>(),
                "{label} reviewReasons"
            );
            assert_eq!(
                decision.record.challenger_sample,
                case["challengerSample"].as_bool().unwrap_or(false),
                "{label} challengerSample"
            );
        }
    }
}
