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

use crate::registry_bench::{MeasuredBlock, LEGACY_ROW_GRACE_DAYS};

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
    /// The day a bench first existed for this class, `YYYY-MM-DD`.
    ///
    /// Brian's addendum of 2026-09-01: an unmeasured row keeps routing until a
    /// bench exists for its class; from that day it has
    /// [`LEGACY_ROW_GRACE_DAYS`] days, disclosed in every routing record, and
    /// then [`route`] refuses it with the word `unmeasured`. Absent means no
    /// bench exists yet and the clock has not started.
    #[serde(default)]
    pub bench_available_since: Option<String>,
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
    /// What a bench actually measured about this target, when one has.
    ///
    /// Absent on every row this repository has ever shipped, and a row without
    /// it **routes exactly as it did before this key existed** — same
    /// candidates, same order, same choice. The only thing that changes is
    /// that the routing record now says the word `legacy` out loud instead of
    /// letting ten priors read as numbers somebody sampled.
    ///
    /// Traits the bench did not evidence are absent from
    /// [`MeasuredBlock::traits`] and stay opinions in [`Self::scores`]; the
    /// disclosure says which are which, because a half-measured row reading as
    /// measured is the same lie as a badge with no event behind it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub measured: Option<MeasuredBlock>,
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

    /// `true` when a bench has measured this row.
    ///
    /// The one word that separates a number from an opinion, and the only
    /// thing `registry check`'s `unmeasured` list counts.
    pub fn is_measured(&self) -> bool {
        self.measured.is_some()
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

    /// The flags that are set, by wire name, in spec §6 order.
    ///
    /// The inverse of [`set`](Self::set). Order is the spec's, not the
    /// caller's, so two hires that assert the same triggers put the same
    /// bytes on the wire.
    pub fn names(&self) -> Vec<String> {
        let set = [
            self.security_boundary,
            self.contract_change,
            self.outside_plan,
            self.builder_uncertain,
            self.tests_insufficient,
            self.lead_requests,
        ];
        REVIEW_FLAG_NAMES
            .iter()
            .zip(set)
            .filter(|(_, on)| *on)
            .map(|(name, _)| (*name).to_owned())
            .collect()
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
    /// Today, `YYYY-MM-DD`, for the legacy-row grace clock only.
    ///
    /// `None` — the default, and every call site that existed before this
    /// field — means no clock: a legacy row routes and is disclosed as legacy,
    /// exactly as before. A caller that passes a date opts the class's
    /// thirty-day deadline in, and a date the router cannot parse is treated
    /// as no date rather than as an expiry.
    pub today: Option<String>,
}

impl RouteRequest {
    /// The routing question a hire asked, as the router's own request.
    ///
    /// The host calls this: it takes the lead's [`HireRoutingRequest`] and
    /// routes it against *its* catalog. Nothing about the requester's
    /// [`ProposedRouting`] crosses over — a proposal is disclosure, never
    /// input, or the host would be re-deriving the requester's answer instead
    /// of making its own.
    ///
    /// # Errors
    ///
    /// A sentence naming the field that was wrong, from
    /// [`HireRoutingRequest::validate`].
    pub fn from_hire_routing(request: &HireRoutingRequest) -> Result<Self, String> {
        request.validate()?;
        Ok(Self {
            class: request.class.clone(),
            risk: request.risk,
            profile: request.profile.clone().unwrap_or_default(),
            needs: TaskNeeds::default(),
            review_flags: request.review_flag_set()?,
            challenger_sample: request.challenger_sample,
            counterpart_provider: None,
            today: None,
        })
    }
}

/// One sentence for [`RoutingRecord::proposed_disagreement`], or `None` when
/// the host landed exactly where the requester proposed.
///
/// The host calls this after routing. Saying nothing when the two differ is
/// the failure this exists to prevent: a lead that proposed Sonnet and reads
/// the create back without a word has no way to learn its host disagreed.
pub fn describe_proposed_disagreement(
    record: &RoutingRecord,
    proposed: &ProposedRouting,
) -> Option<String> {
    let chosen = record.chosen.as_ref()?;
    if *chosen == proposed.chosen {
        return None;
    }
    // An override is the requester's own instruction, not the host's
    // judgment. Reporting "this host routed elsewhere" for a target the
    // requester itself named would be a disagreement nobody had.
    if record
        .r#override
        .as_ref()
        .is_some_and(|over| over.model == chosen.model)
    {
        return None;
    }
    Some(format!(
        "the requester proposed {}/{} ({}); this host routed {}/{} ({}) against its own live \
         catalog and registry, and the host's decision is the one that ran",
        proposed.chosen.provider,
        proposed.chosen.model,
        proposed.chosen.effort,
        chosen.provider,
        chosen.model,
        chosen.effort,
    ))
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
    /// The catalog offers it and the registry has **no row** for it at all.
    ///
    /// Live run 3, finding 25: a Codex target sat on the bench while the
    /// router disclosed that *"nothing else cleared"* the verifier gates. It
    /// was not rejected — it was never a candidate, because
    /// [`check_coverage`] had one word for this (`stale`) and [`route`] built
    /// nothing at all. Three words where there was one: `dormant` is a row
    /// nothing offers, `no row` is an offer nothing has decided about, and
    /// `rejected` is a gate that said no. They never overlap.
    NoRow,
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
    /// Eligible, dormant, no row, or rejected with a reason.
    pub state: CandidateState,
    /// `measured` or `legacy` — where this row's ten numbers came from.
    /// `legacy` on a [`CandidateState::NoRow`] candidate too: there is nothing
    /// there to have measured.
    pub scores: &'static str,
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
    pub record: RoutingRecord,
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
        /// How many candidates were refused **only** because their legacy row's
        /// grace is spent, and the day that grace started.
        ///
        /// F6: with every eligible row expired, the headline blamed a gate the
        /// named target clears (`needs judgment>=4.5; best available … scores
        /// 5`) because `binding_trait` compares scores to minimums and knows
        /// nothing about this refusal class. The true reason reached a reader
        /// only through the CLI's `rejections`; any consumer rendering
        /// `error.to_string()` — Desktop included — saw the false sentence.
        /// `Some` here means the expiry is the whole story and
        /// [`std::fmt::Display`] says so instead.
        unmeasured_expiry: Option<(usize, String)>,
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
                unmeasured_expiry,
                ..
            } => {
                write!(formatter, "no eligible model: {class}/{tier}")?;
                // The true reason, on every path a reader can reach — not only
                // through `rejections`.
                if let Some((count, since)) = unmeasured_expiry {
                    return write!(
                        formatter,
                        "; {count} row(s) cleared every gate and are unmeasured, and the \
                         {LEGACY_ROW_GRACE_DAYS}-day grace that began {since} is spent — run bee \
                         sessions registry measure --role {class}"
                    );
                }
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

/// The routing **record**: the answer, carried on `session.create` and echoed
/// onto the seat's kind:44223 metadata.
///
/// A hire never carries this. A hire carries a
/// [`HireRoutingRequest`] — the *question* — and the founder's host is the
/// only thing that routes, because only the host can see its own live
/// kind:44222 catalog. The requester's own local decision travels beside the
/// question as [`ProposedRouting`], clearly labelled as informational, and the
/// host discloses any disagreement with it in
/// [`proposed_disagreement`](Self::proposed_disagreement).
///
/// That split is not decoration. Until 2026-08-30 both halves were this one
/// type, the CLI emitted the record on a hire, and the desktop host accepted
/// only the request — so every routed hire was classified malformed and
/// dropped in silence (ledger draft 97). Two types is how that stops being
/// possible to write.
///
/// **One key set, always.** Every field below is written, `null` where the
/// answer is not known, so a strict observer has exactly one shape to accept.
/// The single exception is `proposedDisagreement`, which is omitted when the
/// host agreed with the requester — a record with nothing to disclose is then
/// byte-identical to the thirteen-key form consumers already accept.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RoutingRecord {
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
    /// One sentence, written by the host, when its own choice differs from the
    /// [`ProposedRouting`] the requester attached to the hire.
    ///
    /// Omitted — never written as an explicit `null` — when they agree or when
    /// nothing was proposed, so a record with nothing to disclose keeps the
    /// exact thirteen-key shape every consumer already accepts. Present, it is
    /// the host saying out loud that it overruled the requester and why; a
    /// silent divergence would leave a lead reading its own proposal back as
    /// though it had been honoured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed_disagreement: Option<String>,
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

// ── the wire request ─────────────────────────────────────────────────────────

/// `false` — the serde predicate that keeps an unset `challengerSample` off the
/// wire, so an ordinary hire carries the smallest honest key set.
fn is_false(value: &bool) -> bool {
    !*value
}

/// The routing **request**: the question, carried on `session.hire`.
///
/// > "The lead chooses the capability required. The router chooses the
/// > execution target."
///
/// This type is that sentence as a struct. A lead names a class and a risk
/// triple; it does not name a model, a provider or an effort. The founder's
/// host — the only party that can see its own live kind:44222 catalog — routes
/// and writes the [`RoutingRecord`] onto the create it publishes.
///
/// The requester may still run the router locally (`bee sessions route`) and
/// attach what it got as [`proposed`](Self::proposed). That is *informational*:
/// it lets a lead see the decision it expected beside the one the host made,
/// and it obliges the host to disclose any disagreement
/// ([`RoutingRecord::proposed_disagreement`]). It never binds the host, whose
/// catalog may legitimately differ from the requester's.
///
/// The only way a hire dictates an execution target is
/// [`override`](Self::override) — and an override must say `because`, because
/// an unexplained override is indistinguishable from a bug.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HireRoutingRequest {
    /// The capability class the lead named: `builder`, `architect`, `lead`, …
    pub class: String,
    /// Impact × uncertainty × irreversibility, each 1–5.
    ///
    /// Deliberately [`Risk`] and not [`RoutingRisk`]: the request carries the
    /// three factors and **no** `score`. The product is arithmetic the host
    /// does, and a `score` a requester could set is a number that can disagree
    /// with its own factors.
    pub risk: Risk,
    /// Extra per-trait minimums on top of the class gate, or omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profile: Option<BTreeMap<String, f64>>,
    /// A deliberate human override of the router, or omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#override: Option<RoutingOverride>,
    /// `true` to spend this job on a challenger (spec §8). Omitted when false.
    #[serde(default, skip_serializing_if = "is_false")]
    pub challenger_sample: bool,
    /// The spec §6 review triggers the lead asserts, by name.
    ///
    /// Tokens from [`REVIEW_FLAG_NAMES`]; omitted when empty. Names rather
    /// than a struct of six booleans, because the wire should not have to be
    /// re-cut every time the spec grows a trigger.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub review_flags: Vec<String>,
    /// The requester's own local routing decision, or omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proposed: Option<ProposedRouting>,
}

/// What the requester's own router chose, attached to a hire for disclosure.
///
/// Informational, always. The host routes for itself and may land somewhere
/// else; when it does it says so in
/// [`RoutingRecord::proposed_disagreement`] rather than quietly substituting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProposedRouting {
    /// The execution target the requester's router chose.
    pub chosen: RoutingTarget,
    /// Its runner-up, or omitted when there was no second.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_up: Option<RoutingTarget>,
    /// One sentence naming the gates cleared and why this was cheapest.
    pub reason: String,
    /// `version` of the registry the requester read.
    pub registry_version: u32,
    /// The catalog revision it was intersected with, or `null` when more than
    /// one signer published and there is therefore no single number.
    pub catalog_revision: Option<u64>,
}

/// `profile` carries `f64` minimums, so `Eq` cannot be derived. Sound for the
/// same reason [`RoutingRecord`]'s is: JSON has no `NaN` literal and
/// [`HireRoutingRequest::validate`] refuses a minimum outside `1..=5`.
impl Eq for HireRoutingRequest {}

impl HireRoutingRequest {
    /// Check every bound and token.
    ///
    /// # Errors
    ///
    /// A sentence naming the field that was wrong.
    pub fn validate(&self) -> Result<(), String> {
        bounded_token("routing.class", &self.class)?;
        self.risk
            .validate()
            .map_err(|detail| format!("routing.{detail}"))?;
        if let Some(profile) = &self.profile {
            validate_profile(profile)?;
        }
        if self.review_flags.len() > MAX_ROUTING_REVIEW_REASONS {
            return Err(format!(
                "routing.reviewFlags holds {} entries; at most {MAX_ROUTING_REVIEW_REASONS}",
                self.review_flags.len()
            ));
        }
        // Refused by name rather than ignored: a trigger the host silently
        // drops is a review the lead believes it asked for and did not get.
        let mut flags = ReviewFlags::default();
        for flag in &self.review_flags {
            flags
                .set(flag)
                .map_err(|detail| format!("routing.reviewFlags: {detail}"))?;
        }
        if let Some(over) = &self.r#override {
            validate_override(over)?;
        }
        if let Some(proposed) = &self.proposed {
            validate_routing_target("routing.proposed.chosen", &proposed.chosen)?;
            if let Some(runner_up) = &proposed.runner_up {
                validate_routing_target("routing.proposed.runnerUp", runner_up)?;
            }
            validate_reason("routing.proposed.reason", &proposed.reason)?;
        }
        Ok(())
    }

    /// The §6 triggers as the router's own struct.
    ///
    /// # Errors
    ///
    /// The unknown token, with the accepted list.
    pub fn review_flag_set(&self) -> Result<ReviewFlags, String> {
        let mut flags = ReviewFlags::default();
        for flag in &self.review_flags {
            flags.set(flag)?;
        }
        Ok(flags)
    }
}

/// Shared bounds for the extra trait minimums a lead may ask for.
fn validate_profile(profile: &BTreeMap<String, f64>) -> Result<(), String> {
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
    Ok(())
}

/// Shared bounds for one execution target on the wire.
fn validate_routing_target(field: &str, target: &RoutingTarget) -> Result<(), String> {
    bounded_token(&format!("{field}.provider"), &target.provider)?;
    bounded_token(&format!("{field}.model"), &target.model)?;
    if !matches!(target.effort.as_str(), "low" | "medium" | "high") {
        return Err(format!(
            "{field}.effort is {:?}; the router only ever purchases low, medium or high — \
             xhigh, max and ultra are human override only",
            target.effort
        ));
    }
    Ok(())
}

/// Shared bounds for a one-sentence explanation.
fn validate_reason(field: &str, reason: &str) -> Result<(), String> {
    if reason.trim().is_empty() || reason.len() > MAX_ROUTING_REASON_BYTES {
        return Err(format!(
            "{field} must be 1..={MAX_ROUTING_REASON_BYTES} non-blank bytes"
        ));
    }
    Ok(())
}

/// Shared bounds for a human's override of the router.
fn validate_override(over: &RoutingOverride) -> Result<(), String> {
    bounded_token("routing.override.model", &over.model)?;
    if let Some(effort) = &over.effort {
        bounded_token("routing.override.effort", effort)?;
    }
    if over.because.trim().is_empty() || over.because.len() > MAX_ROUTING_REASON_BYTES {
        return Err(format!(
            "routing.override.because must be 1..={MAX_ROUTING_REASON_BYTES} non-blank bytes: \
             an unexplained override is indistinguishable from a bug"
        ));
    }
    Ok(())
}

/// `profile` carries `f64` minimums, so `Eq` cannot be derived — but the
/// enclosing lifecycle action is `Eq`, and this record must ride inside it.
///
/// The implementation is sound because a non-reflexive float can never get
/// here: JSON has no `NaN` literal, so a decoded record's minimums are always
/// finite, and [`RoutingRecord::validate`] refuses a hand-built record whose minimum
/// is outside `1..=5` — a range check `NaN` also fails.
impl Eq for RoutingRecord {}

impl RoutingRecord {
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
            validate_profile(profile)?;
        }
        for (field, target) in [("chosen", &self.chosen), ("runnerUp", &self.runner_up)] {
            if let Some(target) = target {
                validate_routing_target(&format!("routing.{field}"), target)?;
            }
        }
        if let Some(reason) = &self.reason {
            validate_reason("routing.reason", reason)?;
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
            validate_override(over)?;
        }
        if let Some(disagreement) = &self.proposed_disagreement {
            validate_reason("routing.proposedDisagreement", disagreement)?;
        }
        Ok(())
    }

    /// This record as the [`ProposedRouting`] a hire attaches, or `None` when
    /// the decision is not complete enough to propose anything.
    ///
    /// Used by `bee sessions hire`: the CLI routes locally, then attaches the
    /// answer to the *question* it sends, so the host can disclose a
    /// disagreement instead of the lead silently reading its own guess back.
    pub fn as_proposed(&self) -> Option<ProposedRouting> {
        Some(ProposedRouting {
            chosen: self.chosen.clone()?,
            runner_up: self.runner_up.clone(),
            reason: self.reason.clone()?,
            registry_version: self.registry_version?,
            catalog_revision: self.catalog_revision,
        })
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

    // Where every row's numbers came from, computed once: the chosen row's
    // clause is appended to the decision sentence, and every row's word rides
    // in the candidate table.
    let provenance: BTreeMap<String, ScoreProvenance> = registry
        .targets
        .iter()
        .map(|target| {
            (
                target.label(),
                score_provenance(target, class, &minimums, request.today.as_deref()),
            )
        })
        .collect();
    let provenance_words: BTreeMap<String, &'static str> = provenance
        .iter()
        .map(|(label, value)| (label.clone(), value.word))
        .collect();

    let mut candidates: Vec<Candidate> = Vec::with_capacity(registry.targets.len());
    for target in &registry.targets {
        let mut candidate = Candidate {
            provider: target.provider.clone(),
            registry_model: target.model.clone(),
            catalog_model: None,
            effort_on_the_wire: false,
            standing: standing_for(target, registry, &request.class, tier_name),
            state: CandidateState::Eligible,
            // F17 — `.get(...)` rather than an index. Safe by construction
            // (both maps are built from this same iteration), but a panicking
            // index is the class §0.4 bans for `unwrap`.
            scores: provenance_words
                .get(&target.label())
                .copied()
                .unwrap_or("legacy"),
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

        // 3b — the legacy row's grace, when a bench exists for this class and
        // the caller supplied a date. With no bench and no date this is inert,
        // which is why a row with no `measured` block routes exactly as it did
        // before this key existed.
        if provenance
            .get(&target.label())
            .is_some_and(|value| value.expired)
        {
            candidate.state = CandidateState::Rejected(unmeasured_refusal(
                &request.class,
                class.bench_available_since.as_deref().unwrap_or("?"),
            ));
            candidates.push(candidate);
            continue;
        }

        candidates.push(candidate);
    }

    // 3c — finding 25's missing candidate. An offered execution target with no
    // registry row was not rejected and was not dormant: it was never built,
    // so a disclosure could say "nothing else cleared them" with a benched
    // model sitting right there. It is a candidate now, and it says why it is
    // not one.
    for label in &check_coverage(registry, offered).stale {
        let Some((provider, model)) = label.split_once('/') else {
            continue;
        };
        candidates.push(Candidate {
            provider: provider.to_owned(),
            registry_model: model.to_owned(),
            catalog_model: Some(model.to_owned()),
            effort_on_the_wire: false,
            standing: Standing::Unranked,
            state: CandidateState::NoRow,
            scores: "legacy",
            cost_prior: None,
            latency_prior: None,
            retry_prior: DEFAULT_RETRY_PRIOR,
            expected_cost: None,
            rank_score: f64::MAX,
            quota_class: None,
            price: None,
        });
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
            class.bench_available_since.as_deref(),
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
        provenance
            .get(&format!("{}/{}", chosen.provider, chosen.registry_model))
            .map_or("", |value| value.clause.as_str()),
    );

    let record = RoutingRecord {
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
        // Only the host that compared its own answer with a requester's
        // `proposed` can fill this, and it is never the router's to guess.
        proposed_disagreement: None,
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

/// How a row's scores came to exist — the one thing a routing record says that
/// finding 25 could not.
///
/// Live run 3 disclosed *"cleared the verifier gates (reasoning≥4.5,
/// judgment≥4.5, verification≥4.7) … incumbent, nothing else cleared them"*.
/// Every one of those numbers was an opinion, and the sentence gave a reader
/// no way to know it. This type is the fix: exactly one clause, in exactly one
/// place ([`RoutingRecord::reason`]), saying `measured` or `legacy` out loud.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScoreProvenance {
    /// `measured` or `legacy` — the word `route`'s candidate table prints.
    pub word: &'static str,
    /// The clause appended to the decision sentence, beginning with `"; "`.
    pub clause: String,
    /// `true` when this legacy row is past its class's thirty-day grace and
    /// must be refused with the word `unmeasured`.
    pub expired: bool,
    /// Days left on the grace clock, when a bench exists for this class and a
    /// date was supplied. `None` means no clock is running.
    pub days_left: Option<i64>,
}

/// Days between two `YYYY-MM-DD` dates, or `None` when either will not parse.
///
/// A date the router cannot read is treated as no date rather than as an
/// expiry: a typo in the registry must never silently retire a row.
fn days_between(from: &str, to: &str) -> Option<i64> {
    let from = chrono::NaiveDate::parse_from_str(from, "%Y-%m-%d").ok()?;
    let to = chrono::NaiveDate::parse_from_str(to, "%Y-%m-%d").ok()?;
    Some((to - from).num_days())
}

/// The trait with the least room above its class minimum — the one the gate
/// actually turns on, and therefore the one whose spread a reader needs.
fn binding_trait(
    measured: &MeasuredBlock,
    minimums: &BTreeMap<String, f64>,
) -> Option<(String, crate::registry_bench::MeasuredTrait)> {
    let mut best: Option<(String, crate::registry_bench::MeasuredTrait, f64)> = None;
    for (name, minimum) in minimums {
        let Some(trait_value) = measured.traits.get(name) else {
            continue;
        };
        let margin = trait_value.score - minimum;
        if best.as_ref().is_none_or(|(_, _, seen)| margin < *seen) {
            best = Some((name.clone(), *trait_value, margin));
        }
    }
    best.map(|(name, value, _)| (name, value))
}

/// Build the one clause a routing record appends about where its scores came
/// from.
///
/// **Exactly two shapes**, plus the grace sentence Brian's addendum of
/// 2026-09-01 added to the legacy one. Nothing else is ever appended, and the
/// provenance rides here and **nowhere else**: [`RoutingRecord`] is a
/// thirteen-key `deny_unknown_fields` shape carried on hires, and a fourteenth
/// key is priced elsewhere.
pub fn score_provenance(
    target: &RegistryTarget,
    class: &ClassGate,
    minimums: &BTreeMap<String, f64>,
    today: Option<&str>,
) -> ScoreProvenance {
    if let Some(measured) = &target.measured {
        let binding = binding_trait(measured, minimums);
        let detail = binding.map_or_else(
            || format!("n={}, no gated trait was measured", measured.samples()),
            |(name, value)| {
                format!(
                    "n={}, spread {}–{} on the binding trait {name}",
                    value.n, value.min, value.max
                )
            },
        );
        return ScoreProvenance {
            word: "measured",
            clause: format!(
                "; scores measured by {}/{} v{} on {} ({detail})",
                crate::registry_bench::REGISTRY_BENCH_RELATIVE_PATH
                    .rsplit('/')
                    .next()
                    .unwrap_or("registry-bench"),
                measured.role,
                measured.bench_version,
                measured.measured_at
            ),
            expired: false,
            days_left: None,
        };
    }

    let mut clause = format!(
        "; scores are operational priors, not measurements (rating: {}, confidence {}, {} {}) — this row is legacy",
        target.rating.status, target.rating.confidence, target.rating.author, target.rating.date
    );
    // Brian's addendum, ruling 2: a legacy row routes until a bench exists for
    // its class; from that day it has thirty days, disclosed on every record,
    // and then it is refused with the word `unmeasured`.
    let mut expired = false;
    let mut days_left = None;
    if let (Some(since), Some(today)) = (class.bench_available_since.as_deref(), today) {
        if let Some(elapsed) = days_between(since, today) {
            let left = LEGACY_ROW_GRACE_DAYS - elapsed;
            days_left = Some(left);
            expired = left <= 0;
            clause = format!(
                "; scores are operational priors, not measurements (rating: {}, confidence {}, {} {}) — legacy row · bench available · {left} days left",
                target.rating.status,
                target.rating.confidence,
                target.rating.author,
                target.rating.date
            );
        }
    }
    ScoreProvenance {
        word: "legacy",
        clause,
        expired,
        days_left,
    }
}

/// The refusal an expired legacy row gets. Carries the word `unmeasured`,
/// which is the word `registry check` prints and the word the Desktop hire
/// host shows.
/// The prefix every grace-expiry refusal carries, so [`no_eligible`] can tell
/// this refusal class from a gate refusal without matching on prose.
pub const UNMEASURED_REFUSAL_PREFIX: &str = "unmeasured:";

fn unmeasured_refusal(class: &str, since: &str) -> String {
    format!(
        "unmeasured: a bench has existed for {class} since {since} and this row still carries no measured scores, so its {LEGACY_ROW_GRACE_DAYS}-day grace is spent — run bee sessions registry measure --role {class}"
    )
}

/// Sort key: standing band, then whether a cost prior exists, then the
/// expected cost.
fn rank_key(candidate: &Candidate) -> (u8, u8, u8, f64) {
    let state = match candidate.state {
        CandidateState::Eligible => 0,
        CandidateState::Rejected(_) => 1,
        CandidateState::Dormant => 2,
        // Last, and never chosen: nothing has decided about it.
        CandidateState::NoRow => 3,
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
    provenance_clause: &str,
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
    // Last, and always: where the numbers in the gate above came from. One of
    // exactly two clauses, and the only place provenance ever rides.
    sentence.push_str(provenance_clause);
    sentence
}

/// Build the honest empty result: which trait bound it, and what the best
/// available score for that trait actually is.
fn no_eligible(
    bench_available_since: Option<&str>,
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
            CandidateState::NoRow => Some(format!(
                "{}: offered by the catalog, no registry row: it was never considered — write \
                 one with bee sessions registry measure",
                candidate.label()
            )),
            CandidateState::Eligible => None,
        })
        .collect();
    rejections.sort();
    // F6 — the expiry refusal runs AFTER every gate, so a row refused for it
    // is a row that cleared everything and would have routed. One such row is
    // therefore the whole answer, however many other rows failed a minimum:
    // those would have been refused anyway, and naming one of their traits
    // sends a reader hunting for a model that is sitting right there. A `NoRow`
    // candidate never had a row to expire and is not counted.
    let expired = candidates
        .iter()
        .filter(|candidate| match &candidate.state {
            CandidateState::Rejected(reason) => reason.starts_with(UNMEASURED_REFUSAL_PREFIX),
            _ => false,
        })
        .count();
    let unmeasured_expiry =
        (expired > 0).then(|| (expired, bench_available_since.unwrap_or("?").to_owned()));
    RouteError::NoEligibleTarget {
        class: class.to_owned(),
        tier: tier.to_owned(),
        binding_trait: binding,
        best_available,
        rejections,
        unmeasured_expiry,
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
    /// Offered targets that *have* a row and whose row carries no `measured`
    /// block — the rows deciding routes on priors nobody sampled.
    ///
    /// Reported as its own word, and — like [`Self::dormant`] — it **never**
    /// fails the check: the team would stop. It is the count that makes the
    /// gap visible instead of leaving eleven opinions reading as eleven
    /// measurements.
    pub unmeasured: Vec<String>,
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
    // Offered, covered by a row, and that row has never been measured. Its own
    // word, because "covered" and "decided on evidence" are different claims.
    let mut unmeasured: BTreeSet<String> = BTreeSet::new();
    for offer in offered {
        if offer.model.eq_ignore_ascii_case(DEFAULT_ALIAS) {
            continue;
        }
        let row = registry.targets.iter().find(|target| {
            target.provider == offer.provider && base_id(&target.model) == base_id(&offer.model)
        });
        if let Some(row) = row {
            if !row.is_measured() {
                unmeasured.insert(row.label());
            }
        }
        let covered = row.is_some();
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
        unmeasured: unmeasured.into_iter().collect(),
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
        let again: RoutingRecord = serde_json::from_str(&json).expect("decode");
        assert_eq!(again, decision.record);
    }

    /// A hire may carry only the question; the router fills the rest.
    #[test]
    fn a_hire_shaped_record_is_valid_but_not_complete() {
        let question = RoutingRecord {
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
            proposed_disagreement: None,
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
        let mut record = RoutingRecord {
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
            proposed_disagreement: None,
        };
        assert!(record.validate().is_err());
        record.review_required = Some(true);
        record.validate().expect("valid");
    }

    /// An override with no explanation is indistinguishable from a bug.
    #[test]
    fn an_override_must_say_why() {
        let mut record = RoutingRecord {
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
            proposed_disagreement: None,
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
        assert!(serde_json::from_str::<RoutingRecord>(json).is_err());
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
            provider: Some("claude-primary".try_into().expect("alias")),
            runtime: Some("claude".try_into().expect("runtime")),
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
    /// The other half of the cross-implementation contract: a record the
    /// **desktop** router produced, deserialized here.
    ///
    /// The literal below is stdout from `routeCodingSession` in
    /// `desktop/src/features/coding-sessions/lib/codingSessionRouting.ts`, run
    /// against this repository's own `team/model-registry.yaml` and the live
    /// catalog fixture for `architect` at risk 5×4×4 with the security-boundary
    /// flag set. It is pasted rather than generated because the point is that
    /// buzz-core accepts bytes it did not write.
    ///
    /// It caught a real break: every strict observer used to police
    /// `reviewReasons` against a closed set of slugs, so this router's own
    /// `"risk 80 >= 40"` was refused as malformed by desktop, web and mobile
    /// alike. The vocabulary is open and bounded on both sides now, and this
    /// test is what says so.
    #[test]
    fn a_record_the_desktop_router_wrote_is_one_this_crate_accepts() {
        const FROM_DESKTOP: &str = r#"{"class":"architect","tier":"deep","risk":{"impact":5,"uncertainty":4,"irreversibility":4,"score":80},"chosen":{"provider":"codex-primary","model":"gpt-5.6-sol[high]","effort":"high"},"runnerUp":{"provider":"claude-primary","model":"opus[1m]","effort":"high"},"reason":"cleared the architect gates (reasoning≥4.7, judgment≥4.7, discipline≥4.5, context≥4.5, verification≥4.5) and the deep tier's high effort; incumbent, cheaper than claude-primary/opus[1m].","reviewRequired":true,"reviewReasons":["risk 80 >= 40","irreversibility 4 >= 4","securityBoundary"],"challengerSample":false,"override":null,"registryVersion":1,"catalogRevision":7}"#;

        let record: RoutingRecord = serde_json::from_str(FROM_DESKTOP)
            .expect("buzz-core deserializes the desktop router's own record");
        record
            .validate()
            .expect("and every bound and token in it holds");
        assert!(
            record.is_complete(),
            "a routed create must carry every router-filled field"
        );

        // And the two routers agree about it, field for field.
        let mut wanted = request("architect", (5, 4, 4));
        wanted.review_flags.security_boundary = true;
        let ours = route(&shipped(), &live_offer(), &wanted, Some(7)).expect("a route");
        assert_eq!(chosen(&ours), "codex-primary/gpt-5.6-sol[high]");
        assert_eq!(runner_up(&ours).as_deref(), Some("claude-primary/opus[1m]"));
        assert_eq!(ours.record.tier, record.tier);
        assert_eq!(ours.record.review_required, record.review_required);
        assert_eq!(ours.record.review_reasons, record.review_reasons);
        assert_eq!(ours.record.challenger_sample, record.challenger_sample);
        assert_eq!(ours.record.registry_version, record.registry_version);
        assert_eq!(ours.record.catalog_revision, record.catalog_revision);
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
    // ── the request / record split (ledger draft 97) ──────────────────────

    fn a_request() -> HireRoutingRequest {
        HireRoutingRequest {
            class: "builder".to_owned(),
            risk: Risk {
                impact: 3,
                uncertainty: 3,
                irreversibility: 2,
            },
            profile: None,
            r#override: None,
            challenger_sample: false,
            review_flags: Vec::new(),
            proposed: None,
        }
    }

    /// The smallest request writes two keys and no more. Everything optional
    /// is omitted rather than written as an explicit `null`, so a hire that
    /// asks a plain question looks like a plain question on the wire.
    #[test]
    fn the_smallest_request_writes_only_class_and_risk() {
        assert_eq!(
            serde_json::to_string(&a_request()).expect("serialize"),
            r#"{"class":"builder","risk":{"impact":3,"uncertainty":3,"irreversibility":2}}"#
        );
    }

    /// An override with no `because` is refused. An unexplained override is
    /// indistinguishable from a bug — including on the request, where until
    /// now only the record checked it.
    #[test]
    fn a_request_override_must_say_because() {
        let mut request = a_request();
        request.r#override = Some(RoutingOverride {
            model: "gpt-5.6-terra[medium]".to_owned(),
            effort: None,
            because: "   ".to_owned(),
        });
        let error = request.validate().expect_err("a blank because is refused");
        assert!(error.contains("because"), "{error}");
        assert!(error.contains("indistinguishable from a bug"), "{error}");
    }

    /// A review flag nobody defined is refused by name. A trigger the wire
    /// swallows is a review the lead believes it asked for and did not get.
    #[test]
    fn a_request_review_flag_is_refused_by_name_not_dropped() {
        let mut request = a_request();
        request.review_flags = vec!["contractChange".to_owned(), "looksHard".to_owned()];
        let error = request.validate().expect_err("an unknown flag is refused");
        assert!(error.contains("looksHard"), "{error}");
        assert!(
            error.contains("contractChange"),
            "the list is printed: {error}"
        );
    }

    /// The host routes the question, and nothing about the requester's own
    /// answer crosses into the request it routes: a proposal is disclosure,
    /// never input.
    #[test]
    fn a_hire_request_becomes_a_route_request_without_its_proposal() {
        let mut request = a_request();
        request.profile = Some([("taste".to_owned(), 4.5)].into_iter().collect());
        request.challenger_sample = true;
        request.review_flags = vec!["contractChange".to_owned()];
        request.proposed = Some(ProposedRouting {
            chosen: RoutingTarget {
                provider: "claude-primary".to_owned(),
                model: "sonnet".to_owned(),
                effort: "medium".to_owned(),
            },
            runner_up: None,
            reason: "the requester's own local decision".to_owned(),
            registry_version: 1,
            catalog_revision: Some(7),
        });
        let routed = RouteRequest::from_hire_routing(&request).expect("a route request");
        assert_eq!(routed.class, "builder");
        assert_eq!(routed.risk.score(), 18);
        assert_eq!(routed.profile.get("taste"), Some(&4.5));
        assert!(routed.challenger_sample);
        assert!(routed.review_flags.contract_change);
        // No field of `RouteRequest` can carry the proposal, and that is the
        // point: the host re-derives its own answer.
        assert_eq!(routed.counterpart_provider, None);
        assert_eq!(routed.needs, TaskNeeds::default());
    }

    /// A host that lands somewhere else says so. A silent divergence would
    /// leave a lead reading its own proposal back as though it had been
    /// honoured — which is the whole reason `proposed` is on the wire.
    #[test]
    fn a_host_that_overrules_a_proposal_discloses_it() {
        let proposed = ProposedRouting {
            chosen: RoutingTarget {
                provider: "claude-primary".to_owned(),
                model: "opus[1m]".to_owned(),
                effort: "high".to_owned(),
            },
            runner_up: None,
            reason: "the requester's own local decision".to_owned(),
            registry_version: 1,
            catalog_revision: Some(7),
        };
        let mut record = RoutingRecord {
            class: "architect".to_owned(),
            tier: "deep".to_owned(),
            risk: RoutingRisk {
                impact: 5,
                uncertainty: 4,
                irreversibility: 4,
                score: 80,
            },
            profile: None,
            chosen: Some(RoutingTarget {
                provider: "codex-primary".to_owned(),
                model: "gpt-5.6-sol[high]".to_owned(),
                effort: "high".to_owned(),
            }),
            runner_up: None,
            reason: Some("cheapest that cleared the architect gate".to_owned()),
            review_required: Some(true),
            review_reasons: vec!["risk 80 >= 40".to_owned()],
            challenger_sample: false,
            r#override: None,
            registry_version: Some(1),
            catalog_revision: Some(7),
            proposed_disagreement: None,
        };
        let sentence =
            describe_proposed_disagreement(&record, &proposed).expect("a disagreement to disclose");
        assert!(sentence.contains("claude-primary/opus[1m]"), "{sentence}");
        assert!(
            sentence.contains("codex-primary/gpt-5.6-sol[high]"),
            "{sentence}"
        );
        record.proposed_disagreement = Some(sentence);
        record.validate().expect("a valid record");

        // Agreement discloses nothing, so the record keeps its thirteen keys.
        let agreed = RoutingRecord {
            chosen: Some(proposed.chosen.clone()),
            proposed_disagreement: None,
            ..record.clone()
        };
        assert_eq!(describe_proposed_disagreement(&agreed, &proposed), None);
        assert!(
            !serde_json::to_string(&agreed)
                .expect("serialize")
                .contains("proposedDisagreement"),
            "an agreed record must not write the fourteenth key"
        );
    }

    /// An override is the requester's own instruction, so honouring it is not
    /// a disagreement. Reporting one would be the host claiming a judgment it
    /// never made.
    #[test]
    fn honouring_an_override_is_not_a_disagreement() {
        let proposed = ProposedRouting {
            chosen: RoutingTarget {
                provider: "claude-primary".to_owned(),
                model: "sonnet".to_owned(),
                effort: "medium".to_owned(),
            },
            runner_up: None,
            reason: "the requester's own local decision".to_owned(),
            registry_version: 1,
            catalog_revision: Some(7),
        };
        let record = RoutingRecord {
            class: "builder".to_owned(),
            tier: "standard".to_owned(),
            risk: RoutingRisk {
                impact: 3,
                uncertainty: 3,
                irreversibility: 2,
                score: 18,
            },
            profile: None,
            chosen: Some(RoutingTarget {
                provider: "codex-primary".to_owned(),
                model: "gpt-5.6-terra[medium]".to_owned(),
                effort: "medium".to_owned(),
            }),
            runner_up: Some(proposed.chosen.clone()),
            reason: Some("human override: buying a measurement on a challenger".to_owned()),
            review_required: Some(false),
            review_reasons: Vec::new(),
            challenger_sample: false,
            r#override: Some(RoutingOverride {
                model: "gpt-5.6-terra[medium]".to_owned(),
                effort: None,
                because: "buying a measurement on a challenger".to_owned(),
            }),
            registry_version: Some(1),
            catalog_revision: Some(7),
            proposed_disagreement: None,
        };
        assert_eq!(describe_proposed_disagreement(&record, &proposed), None);
    }

    /// A record only proposes something once it has an answer to propose.
    #[test]
    fn an_unanswered_record_proposes_nothing() {
        let mut record = RoutingRecord {
            class: "builder".to_owned(),
            tier: "standard".to_owned(),
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
            proposed_disagreement: None,
        };
        assert_eq!(record.as_proposed(), None);
        record.chosen = Some(RoutingTarget {
            provider: "claude-primary".to_owned(),
            model: "sonnet".to_owned(),
            effort: "medium".to_owned(),
        });
        record.reason = Some("cleared the builder gate".to_owned());
        record.registry_version = Some(1);
        let proposed = record.as_proposed().expect("a proposal");
        assert_eq!(proposed.chosen.model, "sonnet");
        assert_eq!(proposed.registry_version, 1);
    }
    // ── L10.4 — three words where the router had one ─────────────────────────

    use crate::registry_bench::{MeasuredBlock, MeasuredTrait};

    fn measured_block(role: &str) -> MeasuredBlock {
        MeasuredBlock {
            role: role.to_owned(),
            bench_version: 2,
            bench_hash: "0123456789abcdef".to_owned(),
            measured_at: "2026-09-02".to_owned(),
            measured_by: "a".repeat(64),
            runs: vec!["e1".to_owned(), "e2".to_owned(), "e3".to_owned()],
            traits: BTreeMap::from([
                (
                    "reasoning".to_owned(),
                    MeasuredTrait {
                        score: 4.6,
                        n: 3,
                        min: 4.4,
                        max: 4.8,
                    },
                ),
                (
                    "judgment".to_owned(),
                    MeasuredTrait {
                        score: 4.8,
                        n: 3,
                        min: 4.7,
                        max: 4.9,
                    },
                ),
                (
                    "verification".to_owned(),
                    MeasuredTrait {
                        score: 4.9,
                        n: 3,
                        min: 4.8,
                        max: 5.0,
                    },
                ),
            ]),
        }
    }

    /// Every class at every tier decides **identically** with and without the
    /// new key. Back-compat is not a claim about one route; it is a claim
    /// about the whole table, so the whole table is what is asserted.
    #[test]
    fn the_measured_key_changes_no_decision_for_any_class_at_any_tier() {
        let plain = shipped();
        let mut annotated = plain.clone();
        for target in &mut annotated.targets {
            target.measured = Some(measured_block("verifier"));
        }
        let offer = live_offer();

        let mut compared = 0;
        for class in plain.classes.keys() {
            for risk in [(1_u8, 1_u8, 1_u8), (3, 3, 3), (5, 5, 5)] {
                let before = route(&plain, &offer, &request(class, risk), Some(7));
                let after = route(&annotated, &offer, &request(class, risk), Some(7));
                compared += 1;
                match (before, after) {
                    (Ok(before), Ok(after)) => {
                        assert_eq!(
                            before.record.chosen, after.record.chosen,
                            "{class} at {risk:?} chose differently"
                        );
                        assert_eq!(before.record.runner_up, after.record.runner_up);
                        assert_eq!(
                            before
                                .candidates
                                .iter()
                                .map(Candidate::label)
                                .collect::<Vec<String>>(),
                            after
                                .candidates
                                .iter()
                                .map(Candidate::label)
                                .collect::<Vec<String>>(),
                            "{class} at {risk:?} ordered candidates differently"
                        );
                        // The only difference is the appended clause.
                        let before_reason = before.record.reason.clone().unwrap_or_default();
                        let after_reason = after.record.reason.clone().unwrap_or_default();
                        let before_head = before_reason
                            .split_once("; scores ")
                            .map(|(head, _)| head.to_owned())
                            .unwrap_or(before_reason);
                        let after_head = after_reason
                            .split_once("; scores ")
                            .map(|(head, _)| head.to_owned())
                            .unwrap_or(after_reason);
                        assert_eq!(before_head, after_head, "{class} at {risk:?}");
                    }
                    (Err(before), Err(after)) => {
                        assert_eq!(
                            before.to_string(),
                            after.to_string(),
                            "{class} at {risk:?} refused differently"
                        );
                    }
                    (before, after) => {
                        panic!("{class} at {risk:?} disagreed: {before:?} vs {after:?}")
                    }
                }
            }
        }
        assert_eq!(compared, plain.classes.len() * 3);
    }

    /// The shipped registry is eleven opinions, and `check` says so in its own
    /// word without failing: refusing here would stop the team.
    #[test]
    fn the_shipped_registry_is_eleven_unmeasured_rows_and_that_is_not_staleness() {
        let coverage = check_coverage(&shipped(), &live_offer());
        assert_eq!(coverage.unmeasured.len(), 11, "{:?}", coverage.unmeasured);
        assert!(coverage.stale.is_empty());
        assert!(
            coverage.is_fresh(),
            "unmeasured rows are reported, never counted against freshness"
        );
    }

    /// Finding 25, as a test: an offered target with no row is a candidate now,
    /// and it says it was never considered.
    #[test]
    fn an_offered_target_with_no_row_is_a_candidate_that_says_it_was_never_considered() {
        let registry = shipped();
        let mut offer = live_offer();
        offer.push(OfferedTarget::new("codex-primary", "gpt-6-nova[high]"));
        let decision =
            route(&registry, &offer, &request("verifier", (3, 3, 3)), None).expect("a route");
        let missing = decision
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "gpt-6-nova[high]")
            .expect("the offered target with no row is a candidate");
        assert_eq!(missing.state, CandidateState::NoRow);
        assert_eq!(missing.scores, "legacy");
        // The three words never overlap.
        assert!(decision.candidates.iter().all(|candidate| !matches!(
            candidate.state,
            CandidateState::Rejected(_)
        ) || candidate.state
            != CandidateState::NoRow));
    }

    /// The disclosure names the bench version, the date and the binding trait's
    /// spread — the sentence finding 25 could not produce.
    #[test]
    fn a_measured_row_names_its_bench_in_the_routing_record() {
        let mut registry = shipped();
        for target in &mut registry.targets {
            // The verifier/deep route lands on gpt-5.6-sol; the disclosure is
            // about the row that was CHOSEN, so that is the row measured here.
            if target.model == "gpt-5.6-sol" {
                target.measured = Some(measured_block("verifier"));
            }
        }
        let decision = route(
            &registry,
            &live_offer(),
            &request("verifier", (5, 5, 5)),
            None,
        )
        .expect("a route");
        let reason = decision.record.reason.clone().expect("a reason");
        assert!(
            reason.contains("scores measured by registry-bench/verifier v2 on 2026-09-02"),
            "unexpected: {reason}"
        );
        assert!(
            reason.contains("spread 4.4–4.8 on the binding trait reasoning"),
            "the binding trait is the one with the least room above its minimum: {reason}"
        );
        let chosen = decision
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "gpt-5.6-sol")
            .expect("the measured row");
        assert_eq!(chosen.scores, "measured");
    }

    /// A legacy row's disclosure names it legacy, and quotes the rating block
    /// that makes it one.
    #[test]
    fn a_legacy_row_says_the_word_legacy_and_names_its_rating() {
        let decision = route(
            &shipped(),
            &live_offer(),
            &request("verifier", (5, 5, 5)),
            None,
        )
        .expect("a route");
        let reason = decision.record.reason.expect("a reason");
        assert!(
            reason.ends_with(
                "; scores are operational priors, not measurements (rating: operational_opinion, \
                 confidence low, brian 2026-08-30) — this row is legacy"
            ),
            "unexpected: {reason}"
        );
    }

    /// Brian's addendum, ruling 2: the grace runs, is disclosed with the days
    /// left, and then the row is refused with the word `unmeasured`.
    #[test]
    fn a_legacy_row_routes_during_its_grace_and_is_refused_with_the_word_unmeasured_after_it() {
        let mut registry = shipped();
        registry
            .classes
            .get_mut("verifier")
            .expect("verifier")
            .bench_available_since = Some("2026-09-02".to_owned());

        let mut during = request("verifier", (5, 5, 5));
        during.today = Some("2026-09-20".to_owned());
        let decision =
            route(&registry, &live_offer(), &during, None).expect("routes during the grace");
        let reason = decision.record.reason.expect("a reason");
        assert!(
            reason.ends_with("— legacy row · bench available · 12 days left"),
            "unexpected: {reason}"
        );

        let mut after = request("verifier", (5, 5, 5));
        after.today = Some("2026-10-03".to_owned());
        let error = route(&registry, &live_offer(), &after, None).expect_err("the grace is spent");
        // F6 — the HEADLINE names the true reason. It used to blame a gate the
        // named target clears ("needs judgment>=4.5; best available …
        // scores 5"), and the real reason reached a reader only through
        // `rejections`, which `Display` never carries.
        let headline = error.to_string();
        assert!(
            headline.contains("row(s) cleared every gate and are unmeasured"),
            "unexpected: {headline}"
        );
        assert!(
            headline.contains("grace that began 2026-09-02 is spent"),
            "{headline}"
        );
        assert!(
            !headline.contains("needs judgment>="),
            "must not blame a gate the target clears: {headline}"
        );
        let RouteError::NoEligibleTarget { rejections, .. } = error.as_ref() else {
            panic!("expected no eligible target, got {error:?}");
        };
        // A row that failed the class gate is refused for THAT reason; only a
        // row that cleared it and still has no measurement is refused with the
        // word. The three words never overlap.
        let unmeasured: Vec<&String> = rejections
            .iter()
            .filter(|refusal| refusal.contains("unmeasured:"))
            .collect();
        assert_eq!(unmeasured.len(), 4, "{rejections:?}");
        assert!(
            unmeasured
                .iter()
                .any(|refusal| refusal.starts_with("claude-primary/opus[1m]:")),
            "the incumbent is refused too — the bar is the bar: {unmeasured:?}"
        );
        assert!(
            rejections
                .iter()
                .any(|refusal| refusal.contains("judgment 4.4 is below")),
            "a gate refusal is still a gate refusal: {rejections:?}"
        );
        assert!(
            rejections
                .iter()
                .any(|refusal| refusal.contains("registry measure --role verifier")),
            "{rejections:?}"
        );
    }

    /// A gate refusal is still a gate refusal: the honest expiry headline fires
    /// only when the expiry is the WHOLE story, never when something also
    /// failed a minimum.
    #[test]
    fn a_mixed_refusal_keeps_the_binding_trait_headline() {
        let mut registry = shipped();
        registry
            .classes
            .get_mut("verifier")
            .expect("verifier")
            .bench_available_since = Some("2026-09-02".to_owned());
        // Ask for something no row clears, so gate refusals sit beside the
        // expiry ones.
        let mut when = request("verifier", (5, 5, 5));
        when.today = Some("2026-10-03".to_owned());
        when.profile = BTreeMap::from([("taste".to_owned(), 4.95)]);
        let error = route(&registry, &live_offer(), &when, None).expect_err("nothing clears");
        let headline = error.to_string();
        assert!(
            !headline.contains("cleared every gate and are unmeasured"),
            "the expiry is not the whole story here: {headline}"
        );
        assert!(headline.contains("needs taste>=4.95"), "{headline}");
    }

    /// A date the router cannot read retires nothing.
    #[test]
    fn an_unreadable_bench_date_is_no_date_rather_than_an_expiry() {
        let mut registry = shipped();
        registry
            .classes
            .get_mut("verifier")
            .expect("verifier")
            .bench_available_since = Some("last tuesday".to_owned());
        let mut when = request("verifier", (5, 5, 5));
        when.today = Some("2030-01-01".to_owned());
        let decision = route(&registry, &live_offer(), &when, None).expect("routes");
        assert!(decision
            .record
            .reason
            .expect("a reason")
            .ends_with("this row is legacy"));
    }
}
