//! `bee sessions route` — which execution target, and why.
//!
//! > "The lead chooses the capability required. The router chooses the
//! > execution target."
//!
//! > "Select the least expensive execution target whose expected failure mode
//! > is acceptable for the task."
//!
//! Brian's ruling of 2026-08-30. The lead names a **class** and a **risk
//! triple** and never a model; this command intersects the registry with the
//! live kind:44222 catalog, applies the hard requirements and the class gate,
//! derives the tier from the risk, and only then lets cost and speed choose
//! among what is left.
//!
//! Everything it prints is checkable. Each candidate carries the gate it
//! failed in words, the chosen target carries the numbers behind the cost
//! comparison, and any fact that gated something carries the provenance of
//! that fact. A routing decision that cannot be explained from the wire is the
//! bug this command exists to prevent.
//!
//! The tier is **derived**, never passed: `--tier` is refused with an
//! explanation rather than accepted, because a tier a caller can set is a risk
//! assessment nobody made.

use std::collections::BTreeMap;

use serde_json::{json, Value};

use buzz_core::coding_session_routing::{
    route, Candidate, CandidateState, OfferedTarget, ReviewFlags, Risk, RouteError, RouteRequest,
    RoutingDecision,
};

use crate::client::BuzzClient;
use crate::error::CliError;

use super::catalog::CatalogSnapshot;
use super::registry::{catalog_revision, load_registry};

/// Every `(provider, model)` a snapshot offers, with the catalog's own
/// per-model context window where it publishes one.
pub fn offers(snapshot: &CatalogSnapshot) -> Vec<OfferedTarget> {
    let mut offers: Vec<OfferedTarget> = snapshot
        .records
        .iter()
        .flat_map(|record| record.catalog.providers.iter())
        .flat_map(|provider| {
            provider.allowed_models.iter().map(move |model| {
                let described = provider
                    .models
                    .iter()
                    .find(|described| described.id == *model);
                OfferedTarget {
                    provider: provider.provider_instance_ref.clone(),
                    model: model.clone(),
                    context_window: described.and_then(|described| described.context_window),
                }
            })
        })
        .collect();
    offers
        .sort_by(|left, right| (&left.provider, &left.model).cmp(&(&right.provider, &right.model)));
    offers.dedup_by(|left, right| left.provider == right.provider && left.model == right.model);
    offers
}

/// Parse `--risk i,u,i` into a risk triple.
///
/// # Errors
///
/// [`CliError::Usage`] naming what was wrong. Out-of-range components are
/// refused, never clamped: a clamped 9 would silently become a different
/// assessment from the one the lead made.
pub fn parse_risk(text: &str) -> Result<Risk, CliError> {
    let parts: Vec<&str> = text.split(',').map(str::trim).collect();
    let [impact, uncertainty, irreversibility] = parts.as_slice() else {
        return Err(CliError::Usage(format!(
            "--risk takes three comma-separated numbers, impact,uncertainty,irreversibility, \
             each 1-5 (got {text:?})"
        )));
    };
    let mut values = [0u8; 3];
    for (slot, raw) in values
        .iter_mut()
        .zip([impact, uncertainty, irreversibility])
    {
        *slot = raw.parse::<u8>().map_err(|_| {
            CliError::Usage(format!("--risk component {raw:?} is not a number 1-5"))
        })?;
    }
    let risk = Risk {
        impact: values[0],
        uncertainty: values[1],
        irreversibility: values[2],
    };
    risk.validate().map_err(CliError::Usage)?;
    Ok(risk)
}

/// Parse `--profile '{"taste": 4.6}'` into extra trait minimums.
///
/// # Errors
///
/// [`CliError::Usage`] when it is not a flat object of numbers in 1..=5.
pub fn parse_profile(text: &str) -> Result<BTreeMap<String, f64>, CliError> {
    let value: Value = serde_json::from_str(text)
        .map_err(|error| CliError::Usage(format!("--profile is not JSON: {error}")))?;
    let object = value.as_object().ok_or_else(|| {
        CliError::Usage(
            "--profile is a JSON object of trait minimums, e.g. '{\"taste\":4.6}'".into(),
        )
    })?;
    let mut profile = BTreeMap::new();
    for (name, minimum) in object {
        let minimum = minimum
            .as_f64()
            .ok_or_else(|| CliError::Usage(format!("--profile.{name} is not a number 1-5")))?;
        if !(1.0..=5.0).contains(&minimum) {
            return Err(CliError::Usage(format!(
                "--profile.{name} is {minimum}, outside 1..=5"
            )));
        }
        profile.insert(name.clone(), minimum);
    }
    Ok(profile)
}

/// Parse `--review-flags a,b` into the spec §6 triggers.
///
/// # Errors
///
/// [`CliError::Usage`] naming the unknown token and listing the accepted ones.
pub fn parse_review_flags(text: &str) -> Result<ReviewFlags, CliError> {
    let mut flags = ReviewFlags::default();
    for name in text
        .split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
    {
        flags.set(name).map_err(CliError::Usage)?;
    }
    Ok(flags)
}

/// Parse `--review-flags a,b` into the spec §6 trigger names, in the order
/// [`REVIEW_FLAG_NAMES`](buzz_core::coding_session_routing::REVIEW_FLAG_NAMES)
/// lists them.
///
/// The wire carries names rather than six booleans, so the shape does not have
/// to be re-cut every time the spec grows a trigger. Duplicates collapse; an
/// unknown token is refused by name rather than dropped, because a trigger the
/// wire swallows is a review the lead believes it asked for and did not get.
///
/// # Errors
///
/// [`CliError::Usage`] naming the unknown token and listing the accepted ones.
pub fn parse_review_flag_names(text: &str) -> Result<Vec<String>, CliError> {
    Ok(parse_review_flags(text)?.names())
}

/// Build the request every routing entry point shares.
///
/// # Errors
///
/// [`CliError::Usage`] from any of the parsers above.
#[allow(clippy::too_many_arguments)]
pub fn build_request(
    class: &str,
    risk: &str,
    profile: Option<&str>,
    review_flags: Option<&str>,
    challenger_sample: bool,
    scope: Option<&str>,
    context_need: Option<u64>,
    counterpart_provider: Option<&str>,
) -> Result<RouteRequest, CliError> {
    let mut request = RouteRequest {
        class: class.to_owned(),
        risk: parse_risk(risk)?,
        challenger_sample,
        counterpart_provider: counterpart_provider.map(str::to_owned),
        ..RouteRequest::default()
    };
    if let Some(profile) = profile {
        request.profile = parse_profile(profile)?;
    }
    if let Some(flags) = review_flags {
        request.review_flags = parse_review_flags(flags)?;
    }
    request.needs.scope = scope.map(str::to_owned);
    request.needs.context_window = context_need;
    Ok(request)
}

/// Three decimal places — enough to separate any two of these priors, few
/// enough not to imply a precision an opinion does not have.
fn round_three(value: f64) -> f64 {
    (value * 1000.0).round() / 1000.0
}

/// One candidate as a report row: the gate it cleared or the reason it did not,
/// plus every number behind the cost comparison.
fn candidate_row(candidate: &Candidate) -> Value {
    let (state, detail) = match &candidate.state {
        CandidateState::Eligible => ("eligible", Value::Null),
        // Legal, and explicitly not staleness — see `registry check`.
        CandidateState::Dormant => (
            "dormant",
            json!("the registry knows this target; today's catalog does not offer it"),
        ),
        CandidateState::Rejected(reason) => ("rejected", json!(reason)),
        // Finding 25: an offered target the registry has never decided about.
        // It was not rejected and it is not dormant — it was never a candidate
        // at all, which is how a disclosure could say "nothing else cleared
        // them" with a benched model sitting right there.
        CandidateState::NoRow => (
            "no-row",
            json!(
                "offered by the catalog, no registry row: it was never considered — write one \
                 with bee sessions registry measure"
            ),
        ),
    };
    json!({
        "target": candidate.label(),
        "provider": candidate.provider,
        "registryModel": candidate.registry_model,
        "catalogModel": candidate.catalog_model,
        // false = the catalog publishes no effort variant of this id, so the
        // effort is the tier's policy rather than a setting on the wire.
        "effortOnTheWire": candidate.effort_on_the_wire,
        "standing": candidate.standing.as_str(),
        "state": state,
        // `measured` or `legacy` — where this row's ten numbers came from. The
        // one fact a reader of finding 25's disclosure could not get.
        "scores": candidate.scores,
        "detail": detail,
        "costPrior": candidate.cost_prior.map(round_three),
        "latencyPrior": candidate.latency_prior.map(round_three),
        "retryPrior": candidate.retry_prior,
        // Rounded for display only. `null` here is not "cheap" and not
        // "expensive": it is a target with no cost prior, which cannot be
        // compared on cost at all. See `costPrior` on the same row.
        "expectedCost": candidate.expected_cost.map(round_three),
        "quotaClass": candidate.quota_class,
        "listPriceUsdPerM": candidate.price.map(|price| json!({
            "input": price.input,
            "output": price.output,
        })),
    })
}

/// The full decision document.
pub fn decision_report(decision: &RoutingDecision, registry_path: &str) -> Value {
    let record = serde_json::to_value(&decision.record).unwrap_or(Value::Null);
    json!({
        "registry": registry_path,
        // One line a human can read without decoding the record.
        "summary": summary(decision),
        "routing": record,
        // The same decision in the shape a hire actually carries it: a hire
        // sends the routing REQUEST, and this is the `proposed` block inside
        // it. Printed beside the record so nobody has to translate one into
        // the other by hand — copying the record onto a hire is exactly the
        // mistake that dropped a routed hire in silence on 2026-08-30
        // (ledger draft 97).
        "proposed": decision
            .record
            .as_proposed()
            .and_then(|proposed| serde_json::to_value(proposed).ok()),
        "gate": decision.minimums,
        "classNote": decision.class_note,
        // Set when the class is a lane's draft rather than Brian's ruling.
        "classDraftedBy": decision.class_drafted_by,
        "costFormula": "expected_cost = retry_prior x (cost_prior + latency_prior), where \
                        cost_prior = 6 - costEfficiency and latency_prior = 6 - velocity. Lower \
                        is better. retry_prior is the expected number of attempts to an accepted \
                        completion and is 1.0 for everything until telemetry exists. List price \
                        is recorded but is not a term: our real cost is quota lanes. A target \
                        with no cost prior is ranked on latency alone, after every target that \
                        has one, and its expectedCost is reported as null rather than as a \
                        latency figure that would read as a cheaper option nobody chose.",
        "factsUsed": decision.facts_used,
        "candidates": decision
            .candidates
            .iter()
            .map(candidate_row)
            .collect::<Vec<Value>>(),
    })
}

/// Turn a [`RouteError`] into the CLI's honest refusal.
///
/// Exit 4, and the message names the binding trait, the minimum it wanted, and
/// the best score anything available actually has. There is no fallback.
pub fn route_error(error: &RouteError) -> CliError {
    match error {
        RouteError::UnknownClass { .. } | RouteError::BadRisk(_) => {
            CliError::Usage(error.to_string())
        }
        RouteError::NoTier { .. } => CliError::Other(error.to_string()),
        RouteError::NoEligibleTarget { rejections, .. } => {
            CliError::Other(format!("{error}\n  {}", rejections.join("\n  ")))
        }
    }
}

/// `bee sessions route --class <c> --risk i,u,i …`.
///
/// # Errors
///
/// [`CliError::Usage`] for a malformed request, [`CliError::Other`] (exit 4)
/// when nothing clears the bar.
#[allow(clippy::too_many_arguments)]
pub async fn cmd_route(
    client: &BuzzClient,
    channel_id: &str,
    registry_path: Option<&str>,
    request: &RouteRequest,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let (path, registry) = load_registry(registry_path)?;
    let snapshot = super::catalog::load_catalogs(client, channel_id).await?;
    let offered = offers(&snapshot);
    let decision = route(&registry, &offered, request, catalog_revision(&snapshot))
        .map_err(|error| route_error(&error))?;

    match format {
        crate::OutputFormat::Compact => {
            println!(
                "{}",
                serde_json::to_value(&decision.record).unwrap_or(Value::Null)
            );
        }
        crate::OutputFormat::Json => {
            println!(
                "{}",
                decision_report(&decision, &path.display().to_string())
            );
        }
    }
    Ok(())
}

/// The one-line summary a human reads: risk, tier, effort, choice, runner-up.
pub fn summary(decision: &RoutingDecision) -> String {
    let record = &decision.record;
    let chosen = record.chosen.as_ref().map_or_else(
        || "none".to_owned(),
        |target| format!("{}/{} ({})", target.provider, target.model, target.effort),
    );
    let runner_up = record.runner_up.as_ref().map_or_else(
        || "no second".to_owned(),
        |target| format!("{}/{}", target.provider, target.model),
    );
    format!(
        "risk {} = {} -> {} -> {} (runner-up {}); review {}",
        record.risk.score,
        format_args!(
            "{}x{}x{}",
            record.risk.impact, record.risk.uncertainty, record.risk.irreversibility
        ),
        record.tier,
        chosen,
        runner_up,
        if record.review_required == Some(true) {
            format!("required: {}", record.review_reasons.join(", "))
        } else {
            "not required".to_owned()
        }
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::coding_session_routing::{parse_registry, Registry, Standing};
    use std::path::Path;

    fn shipped() -> Registry {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("team/model-registry.yaml");
        parse_registry(&std::fs::read_to_string(&path).expect("read")).expect("parse")
    }

    fn live_offer() -> Vec<OfferedTarget> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("testdata/routing/live-catalog-665076ce.json");
        let fixture: Value =
            serde_json::from_str(&std::fs::read_to_string(&path).expect("read")).expect("json");
        fixture["offered"]
            .as_array()
            .expect("offered")
            .iter()
            .map(|pair| {
                OfferedTarget::new(
                    pair["providerInstanceRef"].as_str().expect("provider"),
                    pair["model"].as_str().expect("model"),
                )
            })
            .collect()
    }

    #[test]
    fn a_risk_triple_is_parsed_and_out_of_range_components_are_refused() {
        let risk = parse_risk("3,3,2").expect("parse");
        assert_eq!(risk.score(), 18);
        assert_eq!(parse_risk(" 5 , 4 , 4 ").expect("parse").score(), 80);
        for bad in ["3,3", "3,3,2,1", "0,3,2", "3,9,2", "a,3,2", ""] {
            assert!(parse_risk(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn a_profile_is_a_flat_object_of_minimums_in_range() {
        let profile = parse_profile(r#"{"taste":4.6,"coding":4.4}"#).expect("parse");
        assert_eq!(profile.len(), 2);
        assert!((profile["taste"] - 4.6).abs() < 1e-9);
        for bad in [r#"{"taste":9}"#, r#"{"taste":"high"}"#, "[1,2]", "not json"] {
            assert!(parse_profile(bad).is_err(), "{bad:?} was accepted");
        }
    }

    #[test]
    fn review_flags_are_parsed_and_an_unknown_one_lists_the_accepted_set() {
        let flags = parse_review_flags("securityBoundary, contractChange").expect("parse");
        assert!(flags.security_boundary && flags.contract_change);
        assert!(!flags.lead_requests);
        let error = parse_review_flags("looksHard").expect_err("must refuse");
        assert!(error.to_string().contains("securityBoundary"), "{error}");
    }

    /// Every candidate row says where its numbers came from, and an offered
    /// target the registry never decided about is a row in the table rather
    /// than an absence — live run 3, finding 25.
    #[test]
    fn the_candidate_table_says_measured_or_legacy_and_shows_the_missing_target() {
        let request = build_request("builder", "3,3,2", None, None, false, None, None, None)
            .expect("request");
        let mut offered = live_offer();
        offered.push(OfferedTarget::new("codex-primary", "gpt-6-nova[high]"));
        let decision = route(&shipped(), &offered, &request, Some(7)).expect("route");
        let report = decision_report(&decision, "team/model-registry.yaml");
        let rows = report["candidates"].as_array().expect("candidates").clone();

        // Today every shipped row is an opinion, and every row says so.
        for row in &rows {
            if row["state"] == "no-row" {
                continue;
            }
            assert_eq!(row["scores"], "legacy", "{row}");
        }

        let missing = rows
            .iter()
            .find(|row| row["registryModel"] == "gpt-6-nova[high]")
            .expect("the offered target with no row is a candidate");
        assert_eq!(missing["state"], "no-row");
        assert!(
            missing["detail"]
                .as_str()
                .expect("detail")
                .contains("it was never considered"),
            "{missing}"
        );
        // And the sentence itself now names the provenance.
        assert!(
            report["routing"]["reason"]
                .as_str()
                .expect("reason")
                .ends_with("this row is legacy"),
            "{}",
            report["routing"]["reason"]
        );
    }

    /// The report carries the numbers behind the comparison, not just the
    /// verdict — a score presented without its inputs is an opinion dressed as
    /// a measurement.
    #[test]
    fn the_report_shows_the_cost_numbers_behind_the_choice() {
        let request = build_request("builder", "3,3,2", None, None, false, None, None, None)
            .expect("request");
        let decision = route(&shipped(), &live_offer(), &request, Some(7)).expect("route");
        let report = decision_report(&decision, "team/model-registry.yaml");
        let chosen = report["candidates"][0].clone();
        assert_eq!(chosen["target"], "claude-primary/sonnet");
        assert_eq!(chosen["state"], "eligible");
        assert!((chosen["costPrior"].as_f64().expect("cost") - 1.5).abs() < 1e-9);
        assert!((chosen["latencyPrior"].as_f64().expect("latency") - 1.4).abs() < 1e-9);
        assert!((chosen["expectedCost"].as_f64().expect("expected") - 2.9).abs() < 1e-9);
        assert_eq!(chosen["retryPrior"], 1.0);
        assert_eq!(chosen["quotaClass"], "claude-subscription");
        assert_eq!(chosen["effortOnTheWire"], false);
        assert!(report["costFormula"]
            .as_str()
            .expect("formula")
            .contains("retry_prior x (cost_prior + latency_prior)"));
        // Every rejected row says which gate refused it, in words.
        for candidate in report["candidates"].as_array().expect("candidates") {
            if candidate["state"] == "rejected" {
                assert!(
                    candidate["detail"].as_str().is_some_and(|d| !d.is_empty()),
                    "a rejection with no reason: {candidate}"
                );
            }
        }
    }

    /// A challenger appears in the table with its standing, so a reader can
    /// see what was deliberately not routed rather than wondering.
    #[test]
    fn a_challenger_is_shown_as_rejected_with_its_standing() {
        let request = build_request("builder", "3,3,2", None, None, false, None, None, None)
            .expect("request");
        let decision = route(&shipped(), &live_offer(), &request, None).expect("route");
        let terra = decision
            .candidates
            .iter()
            .find(|candidate| candidate.registry_model == "gpt-5.6-terra")
            .expect("terra");
        assert_eq!(terra.standing, Standing::Challenger);
        let row = candidate_row(terra);
        assert_eq!(row["state"], "rejected");
        assert!(row["detail"]
            .as_str()
            .expect("detail")
            .contains("measured results"));
    }

    /// The honest empty result reaches the shell as exit 4 with the binding
    /// trait and the best available score, and never as a fallback choice.
    #[test]
    fn no_eligible_target_becomes_an_exit_four_that_names_the_gate() {
        let request = build_request(
            "runner",
            "1,1,1",
            Some(r#"{"taste":5.0}"#),
            None,
            false,
            None,
            None,
            None,
        )
        .expect("request");
        let error = route(&shipped(), &live_offer(), &request, None).expect_err("must refuse");
        let cli = route_error(&error);
        assert_eq!(crate::error::exit_code(&cli), 4, "{cli}");
        let text = cli.to_string();
        assert!(text.contains("no eligible model: runner/fast"), "{text}");
        assert!(text.contains("taste>=5"), "{text}");
        // …and every row it dropped says why.
        assert!(text.contains("claude-primary/haiku:"), "{text}");
    }

    /// An unknown class is the caller's mistake (exit 1), not the registry's.
    #[test]
    fn an_unknown_class_is_a_usage_error_listing_the_real_ones() {
        let request =
            build_request("wizard", "1,1,1", None, None, false, None, None, None).expect("request");
        let error = route(&shipped(), &live_offer(), &request, None).expect_err("must refuse");
        let cli = route_error(&error);
        assert_eq!(crate::error::exit_code(&cli), 1, "{cli}");
        assert!(cli.to_string().contains("builder"), "{cli}");
    }

    #[test]
    fn the_summary_names_the_risk_the_tier_the_choice_and_the_review() {
        let request = build_request("architect", "5,4,4", None, None, false, None, None, None)
            .expect("request");
        let decision = route(&shipped(), &live_offer(), &request, None).expect("route");
        let line = summary(&decision);
        assert!(line.contains("risk 80 = 5x4x4"), "{line}");
        assert!(line.contains("-> deep ->"), "{line}");
        assert!(
            line.contains("codex-primary/gpt-5.6-sol[high] (high)"),
            "{line}"
        );
        assert!(line.contains("runner-up claude-primary/opus[1m]"), "{line}");
        assert!(line.contains("review required: risk 80 >= 40"), "{line}");
    }

    /// The catalog's own per-model context window reaches the router, so a
    /// context requirement is checked against a published figure rather than a
    /// figure copied into the registry.
    #[test]
    fn offers_carry_the_catalogs_own_context_window() {
        use super::super::catalog::CatalogRecord;
        use buzz_core::coding_session_catalog::{Catalog, CatalogModel, CatalogProvider};
        use buzz_core::coding_session_payload::Capabilities;

        let snapshot = CatalogSnapshot {
            records: vec![CatalogRecord {
                event_id: "e1".into(),
                signer: "s1".into(),
                created_at: 1,
                catalog: Catalog {
                    schema: buzz_core::coding_session_catalog::CATALOG_SCHEMA.into(),
                    revision: 3,
                    providers: vec![CatalogProvider {
                        provider_instance_ref: "claude-primary".into(),
                        driver: "claude-agent-acp".into(),
                        runtime: "claude".into(),
                        default_model: "opus[1m]".into(),
                        allowed_models: vec!["opus[1m]".into(), "sonnet".into()],
                        capabilities: Capabilities::v1_claude(),
                        models: vec![CatalogModel {
                            context_window: Some(1_000_000),
                            ..CatalogModel::new("opus[1m]")
                        }],
                    }],
                    projects: Vec::new(),
                },
            }],
            malformed: Vec::new(),
        };
        let offers = offers(&snapshot);
        assert_eq!(offers.len(), 2);
        assert_eq!(offers[0].model, "opus[1m]");
        assert_eq!(offers[0].context_window, Some(1_000_000));
        // Nobody said, for sonnet — and `None` is that fact, never a default.
        assert_eq!(offers[1].context_window, None);
    }

    /// `bee sessions route --format json` prints the decision twice: as the
    /// record, and as the `proposed` block a hire actually carries.
    ///
    /// A lead that copies `routing` onto a hire is emitting the record, and
    /// the record on a hire is what the host dropped in silence on 2026-08-30
    /// (ledger draft 97). The shape it should copy is printed beside it.
    #[test]
    fn the_route_report_prints_the_shape_a_hire_carries() {
        use buzz_core::coding_session_routing::{route, ProposedRouting, RouteRequest};

        let decision = route(
            &shipped(),
            &live_offer(),
            &RouteRequest {
                class: "builder".to_owned(),
                risk: parse_risk("3,3,2").expect("risk"),
                ..RouteRequest::default()
            },
            Some(7),
        )
        .expect("a route");
        let report = decision_report(&decision, "team/model-registry.yaml");

        let proposed: ProposedRouting =
            serde_json::from_value(report["proposed"].clone()).expect("a proposal");
        assert_eq!(proposed.chosen.provider, "claude-primary");
        assert_eq!(proposed.chosen.model, "sonnet");
        assert_eq!(proposed.registry_version, 1);
        assert_eq!(proposed.catalog_revision, Some(7));

        // It is the proposal, not the record: none of the record's own keys
        // survive into it.
        let object = report["proposed"].as_object().expect("object");
        for key in ["tier", "risk", "class", "reviewRequired", "reviewReasons"] {
            assert!(!object.contains_key(key), "proposed carries {key:?}");
        }
        // And the record is still printed in full beside it.
        assert_eq!(report["routing"]["tier"], serde_json::json!("standard"));
    }
}
