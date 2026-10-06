//! `bee sessions registry check` — is the model registry still true about the
//! live offer?
//!
//! Brian's ruling of 2026-08-30 replaced the flat rubric of item 94 with a
//! registry of execution targets, and the same argument carries over: a
//! registry is a good instrument and a stale one is a quiet lie. This command
//! is the check. It never edits the registry and it never translates an id —
//! it prints three lists and exits non-zero on exactly one of them.
//!
//! # The two directions are deliberately not symmetric
//!
//! * **Dormant** — a registry row today's catalog does not offer. **Legal.**
//!   The registry is allowed to hold an opinion about a model this host is not
//!   serving right now; that is a model coming back, or a second host's
//!   inventory, not a defect. A dormant row never fails this check.
//! * **Stale** — a live offered execution target no registry row covers.
//!   *That* is staleness, and it is what exits 4: a target the router cannot
//!   reason about is a target that is on offer with nothing behind it, and the
//!   lead will reach for it anyway.
//! * **Variants** — offered ids a row covers by the base rule without naming
//!   literally. Informational; never staleness.
//!
//! # A bracket suffix is a variant of its base
//!
//! `gpt-5.6-sol[high]`, `[low]`, `[max]`, `[ultra]` are one model at four
//! effort levels; `opus[1m]` and `opus` are one model at two context windows.
//! The bracket is a knob on a model, not a different model, so the registry
//! decides at the base: a row naming `gpt-5.6-terra` has decided about every
//! `gpt-5.6-terra` id in the catalog. Without that rule this check would be
//! permanently red on a catalog that publishes six ids per model, and a check
//! that is always red trains its reader to ignore it.
//!
//! The `default` alias is never a row and never a gap — it is a provider's
//! pointer at whatever the host is set to, not a model.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use beekeeper_core::coding_session_routing::{
    check_coverage, parse_registry, Registry, DEFAULT_REGISTRY_RELATIVE_PATH,
};
use beekeeper_core::kind::KIND_CODING_SESSION_OBSERVATION;
use beekeeper_core::model_registry_source::{
    ancestor_model_registry_candidates, resolve_model_registry, ModelRegistryCandidate,
    ResolvedModelRegistry, AGENTS_REPO_REGISTRY_FILE,
};

use crate::client::BeekeeperClient;
use crate::error::CliError;

use super::catalog::{load_catalogs, CatalogSnapshot};

/// Find the registry: the explicit path, or — walking up from `start` — the
/// project's agents repository, a seat's sibling agents clone, then a code
/// checkout's `team/model-registry.yaml`.
///
/// The order is `beekeeper_core::model_registry_source`'s, the same one the
/// desktop host uses, because a `bee sessions route` that printed a decision
/// from one file while the host seated a decision from another would be
/// worse than no command at all. Under the agents-repository pivot the
/// registry lives at `<agents repo>/model-registry.yaml` (spec § 4.11); the
/// checkout rung is kept for this repository, which still holds
/// `team/model-registry.yaml`.
///
/// Walking up is what lets a seat run this from a worktree subdirectory. The
/// error names every place that was tried, because "registry not found" with
/// no path is the kind of message that costs an hour.
///
/// # Errors
///
/// [`CliError::NotFound`] naming the path, or every path that was tried.
pub fn resolve_registry_source(
    explicit: Option<&str>,
    start: &Path,
) -> Result<ResolvedModelRegistry, CliError> {
    let candidates = match explicit {
        Some(path) => vec![ModelRegistryCandidate::explicit(Path::new(path))],
        None => ancestor_model_registry_candidates(start),
    };
    resolve_model_registry(&candidates).map_err(|missing| {
        CliError::NotFound(match explicit {
            Some(path) => format!("no model registry at {path}"),
            None => format!(
                "{missing} — pass --registry <path>, or run from a project's agents repository \
                 (holding {AGENTS_REPO_REGISTRY_FILE}) or a checkout holding \
                 {DEFAULT_REGISTRY_RELATIVE_PATH}"
            ),
        })
    })
}

/// Read and parse the registry, naming the file in any failure, and say which
/// copy answered.
///
/// # Errors
///
/// [`CliError::NotFound`] when there is no file, [`CliError::Usage`] when the
/// file is not a registry.
pub fn load_registry_source(
    explicit: Option<&str>,
) -> Result<(ResolvedModelRegistry, Registry), CliError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let resolved = resolve_registry_source(explicit, &cwd)?;
    let registry = parse_registry(&resolved.text)
        .map_err(|error| CliError::Usage(format!("{}: {error}", resolved.path.display())))?;
    Ok((resolved, registry))
}

/// [`load_registry_source`], for callers that only need the path it came from.
///
/// # Errors
///
/// [`CliError::NotFound`] when there is no file, [`CliError::Usage`] when the
/// file is not a registry.
pub fn load_registry(explicit: Option<&str>) -> Result<(PathBuf, Registry), CliError> {
    let (resolved, registry) = load_registry_source(explicit)?;
    Ok((resolved.path, registry))
}

/// The single catalog revision, or `null` when more than one signer published.
///
/// With two hosts there is no shared clock and therefore no single number;
/// printing one anyway would name a revision nothing has.
pub fn catalog_revision(snapshot: &CatalogSnapshot) -> Option<u64> {
    match snapshot.records.as_slice() {
        [only] => Some(only.catalog.revision),
        _ => None,
    }
}

/// The word each measured row has earned, and the confidence it may claim.
///
/// A `measured` block names the 44246 gate rows behind it. Until somebody
/// resolves those ids the block is a claim about evidence nobody has looked
/// for, so this asks the relay for them by id and reports what came back.
/// Never fails the check — a relay that cannot be reached is not a stale
/// registry — but a row whose runs do not resolve says so, and never reads
/// `confidence: high`.
pub async fn resolve_measured_runs(client: &BeekeeperClient, registry: &Registry) -> Vec<Value> {
    let mut rows = Vec::new();
    for target in &registry.targets {
        let Some(measured) = &target.measured else {
            continue;
        };
        let found: BTreeSet<String> = if measured.runs.is_empty() {
            BTreeSet::new()
        } else {
            client
                .query_all(json!({
                    "kinds": [KIND_CODING_SESSION_OBSERVATION],
                    "ids": measured.runs,
                }))
                .await
                .unwrap_or_default()
                .iter()
                .filter_map(|event| event.get("id").and_then(Value::as_str).map(str::to_owned))
                .collect()
        };
        let missing: Vec<String> = measured
            .runs
            .iter()
            .filter(|id| !found.contains(*id))
            .cloned()
            .collect();
        let resolved = !measured.runs.is_empty() && missing.is_empty();
        rows.push(json!({
            "target": target.label(),
            "role": measured.role,
            "benchVersion": measured.bench_version,
            // The word a reader sees. Two words, and the second is not a
            // softening of the first — it says the evidence was looked for.
            "scores": if resolved { "measured" } else { "measured (unresolved runs)" },
            "runs": measured.runs.len(),
            "runsResolved": found.len(),
            "runsMissing": missing,
            "claimedConfidence": measured.confidence(),
            "confidence": measured.resolved_confidence(resolved),
        }));
    }
    rows
}

/// `bee sessions registry check --channel <uuid> [--registry <path>]`.
///
/// # Errors
///
/// Exit 4 ([`CliError::Other`]) when the registry is stale — an offered target
/// has no row. A dormant row is reported and does **not** fail.
pub async fn cmd_registry_check(
    client: &BeekeeperClient,
    channel_id: &str,
    registry_path: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let (resolved, registry) = load_registry_source(registry_path)?;
    let path = resolved.path.clone();
    let snapshot = load_catalogs(client, channel_id).await?;
    let offered = super::route::offers(&snapshot);
    let coverage = check_coverage(&registry, &offered);
    let measured_rows = resolve_measured_runs(client, &registry).await;

    let catalogs: Vec<Value> = snapshot
        .records
        .iter()
        .map(|record| {
            json!({
                "signer": record.signer,
                "revision": record.catalog.revision,
                "eventId": record.event_id,
            })
        })
        .collect();
    // What each provider's `default` alias points at. The check never counts
    // the alias as a gap, so this is where a reader sees which id it is.
    let default_resolves_to: Value = snapshot
        .default_models()
        .into_iter()
        .map(|(provider, model)| (provider, model.map_or(Value::Null, Value::String)))
        .collect::<serde_json::Map<String, Value>>()
        .into();

    let report = json!({
        "registry": path.display().to_string(),
        // Which copy answered, and every place that was tried: a check that
        // said only "fresh" without naming the file's provenance could be
        // green about a registry no seat on this machine routes against
        // (ledger 178(a)).
        "registrySource": resolved.origin.as_str(),
        "registrySourceLabel": resolved.origin.describe(),
        "registryLookedIn": resolved.looked_in,
        "registryVersion": registry.version,
        "registryUpdatedAt": registry.updated_at,
        "rows": registry.targets.len(),
        "channel": channel_id,
        "catalogRevision": catalog_revision(&snapshot),
        "catalogs": catalogs,
        "offered": offered
            .iter()
            .map(|offer| format!("{}/{}", offer.provider, offer.model))
            .collect::<Vec<String>>(),
        "defaultResolvesTo": default_resolves_to,
        // Legal: the registry may hold an opinion about a model nothing serves
        // today. Reported, never counted against freshness.
        "dormant": coverage.dormant,
        // The one list that fails this command.
        "stale": coverage.stale,
        "variants": coverage.variants,
        // Its own word: offered, covered by a row, and that row has never been
        // measured. Like `dormant` it NEVER fails the check — refusing here
        // would stop the team — but eleven opinions must not read as eleven
        // measurements just because nothing complained.
        "unmeasured": coverage.unmeasured,
        "unmeasuredCount": coverage.unmeasured.len(),
        // F4b: `runs` is a list of event ids and nothing used to resolve them,
        // so a hand-edited block with a fabricated id and `n: 99` read as
        // `confidence: high`. Each row's word is `measured` only when its runs
        // are on the relay; otherwise `measured (unresolved runs)`, and its
        // confidence is capped at `medium` however large its `n`.
        "measuredRows": measured_rows,
        "isStale": !coverage.is_fresh(),
        "malformedCatalogs": snapshot
            .malformed
            .iter()
            .map(|(event_id, reason)| json!({ "eventId": event_id, "reason": reason }))
            .collect::<Vec<Value>>(),
    });

    match format {
        crate::OutputFormat::Compact => println!(
            "{}",
            json!({
                "registryVersion": registry.version,
                "defaultResolvesTo": default_resolves_to,
                "dormant": coverage.dormant,
                "stale": coverage.stale,
                "variants": coverage.variants,
                "unmeasured": coverage.unmeasured,
                "measuredRows": measured_rows,
                "isStale": !coverage.is_fresh(),
            })
        ),
        crate::OutputFormat::Json => println!("{report}"),
    }

    if coverage.is_fresh() {
        if !coverage.unmeasured.is_empty() {
            // Printed, not failed, and printed even on the clean path: a
            // reader who only ever sees "exit 0" learns nothing about how many
            // of these rows are somebody's guess.
            eprintln!(
                "{} offered target(s) route on unmeasured rows ({}). Not staleness, and not a \
                 failure — run bee sessions registry measure to replace an opinion with a \
                 number.",
                coverage.unmeasured.len(),
                coverage.unmeasured.join(", ")
            );
        }
        let unresolved: Vec<&Value> = measured_rows
            .iter()
            .filter(|row| row["scores"] != "measured")
            .collect();
        if !unresolved.is_empty() {
            eprintln!(
                "{} measured row(s) name gate events this relay does not have; they read \
                 `measured (unresolved runs)` and are capped at confidence medium.",
                unresolved.len()
            );
        }
        return Ok(());
    }
    Err(CliError::Other(format!(
        "registry is stale: {} offered target(s) no row covers ({}). {} dormant row(s) are not \
         staleness and did not affect this result. {} covered target(s) are unmeasured, which is \
         also not staleness and also did not affect this result.",
        coverage.stale.len(),
        coverage.stale.join(", "),
        coverage.dormant.len(),
        coverage.unmeasured.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use beekeeper_core::coding_session_routing::OfferedTarget;

    fn shipped_registry() -> Registry {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join(DEFAULT_REGISTRY_RELATIVE_PATH);
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()));
        parse_registry(&text)
            .unwrap_or_else(|error| panic!("{} does not parse: {error}", path.display()))
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

    /// The registry this repository ships is fresh against the catalog this
    /// relay really served. This is the clean result, and the only one that
    /// exits 0.
    #[test]
    fn the_shipped_registry_covers_the_live_catalog() {
        let coverage = check_coverage(&shipped_registry(), &live_offer());
        assert!(coverage.is_fresh(), "stale: {:?}", coverage.stale);
        assert!(coverage.dormant.is_empty(), "{:?}", coverage.dormant);
        // Everything the catalog offers beyond the eleven named rows is a
        // bracket variant, and every one of them is listed rather than hidden
        // by the collapse to the base.
        assert_eq!(coverage.variants.len(), 33, "{:?}", coverage.variants);
    }

    /// A dormant row does not fail the check; an uncovered offer does. This is
    /// the whole asymmetry, in one test.
    #[test]
    fn dormant_passes_and_uncovered_fails() {
        let registry = shipped_registry();

        let narrowed: Vec<OfferedTarget> = live_offer()
            .into_iter()
            .filter(|offer| offer.provider != "codex-primary")
            .collect();
        let coverage = check_coverage(&registry, &narrowed);
        assert_eq!(coverage.dormant.len(), 7, "{:?}", coverage.dormant);
        assert!(coverage.stale.is_empty());
        assert!(
            coverage.is_fresh(),
            "seven dormant rows must not make the registry stale"
        );

        let mut widened = live_offer();
        widened.push(OfferedTarget::new("codex-primary", "gpt-6-nova[high]"));
        let coverage = check_coverage(&registry, &widened);
        assert_eq!(coverage.stale, vec!["codex-primary/gpt-6-nova[high]"]);
        assert!(!coverage.is_fresh());
    }

    /// The gap is named with an id the catalog really offers, so pasting it
    /// into a new row closes the gap rather than creating a dormant one.
    #[test]
    fn a_gap_is_named_with_an_id_the_catalog_actually_offers() {
        let mut registry = shipped_registry();
        registry.targets.retain(|target| target.model != "haiku");
        let coverage = check_coverage(&registry, &live_offer());
        assert_eq!(coverage.stale, vec!["claude-primary/haiku"]);
        assert!(live_offer()
            .iter()
            .any(|offer| offer.provider == "claude-primary" && offer.model == "haiku"));
    }

    /// Eleven rows, eleven opinions, and `check` says the word and exits 0.
    /// Refusing here would stop the team; saying nothing would let eleven
    /// guesses read as eleven measurements.
    #[test]
    fn check_lists_eleven_unmeasured_rows_and_still_exits_zero() {
        let coverage = check_coverage(&shipped_registry(), &live_offer());
        assert_eq!(coverage.unmeasured.len(), 11, "{:?}", coverage.unmeasured);
        assert!(coverage.is_fresh());
        assert!(coverage
            .unmeasured
            .contains(&"claude-primary/opus[1m]".to_owned()));
    }

    /// The three words never overlap: a target with no row is `stale`, a row
    /// nothing offers is `dormant`, and a row that routes on priors is
    /// `unmeasured`.
    #[test]
    fn the_three_words_name_three_different_things() {
        let mut registry = shipped_registry();
        registry.targets.retain(|target| target.model != "haiku");
        let mut offered = live_offer();
        offered.retain(|offer| offer.provider != "codex-primary");
        let coverage = check_coverage(&registry, &offered);
        assert_eq!(coverage.stale, vec!["claude-primary/haiku"]);
        assert_eq!(coverage.dormant.len(), 7);
        // Four claude rows minus the removed haiku row.
        assert_eq!(coverage.unmeasured.len(), 3, "{:?}", coverage.unmeasured);
        for label in &coverage.unmeasured {
            assert!(!coverage.stale.contains(label));
            assert!(!coverage.dormant.contains(label));
        }
    }

    #[test]
    fn an_explicit_path_that_does_not_exist_is_named() {
        let error = resolve_registry_source(Some("/nope/registry.yaml"), Path::new("/tmp"))
            .expect_err("must fail");
        assert!(
            error.to_string().contains("/nope/registry.yaml"),
            "unexpected: {error}"
        );
    }

    #[test]
    fn the_default_path_is_found_by_walking_up_from_the_working_directory() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let registry = root.join(DEFAULT_REGISTRY_RELATIVE_PATH);
        std::fs::create_dir_all(registry.parent().expect("parent")).expect("mkdir");
        std::fs::write(&registry, "version: 1\n").expect("write");
        let nested = root.join("crates").join("buzz-cli");
        std::fs::create_dir_all(&nested).expect("mkdir");
        assert_eq!(
            resolve_registry_source(None, &nested)
                .expect("resolve")
                .path,
            registry
        );
    }

    #[test]
    fn a_missing_registry_names_the_paths_it_tried() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = resolve_registry_source(None, dir.path()).expect_err("must fail");
        assert!(
            error.to_string().contains(DEFAULT_REGISTRY_RELATIVE_PATH),
            "unexpected: {error}"
        );
    }

    /// Ledger 178(a): a project's registry lives in its agents repository,
    /// and that is the copy `bee` routes against.
    #[test]
    fn an_agents_repository_beside_the_seat_answers_before_the_checkout() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        let checkout = root.join("pivot-test");
        let agents = root.join("pivot-test-agents");
        std::fs::create_dir_all(checkout.join("team")).expect("mkdir");
        std::fs::create_dir_all(&agents).expect("mkdir");
        std::fs::write(
            checkout.join(DEFAULT_REGISTRY_RELATIVE_PATH),
            "version: 1\n# checkout\n",
        )
        .expect("write");
        std::fs::write(
            agents.join(AGENTS_REPO_REGISTRY_FILE),
            "version: 1\n# agents\n",
        )
        .expect("write");

        // Standing in the seat's worktree, the sibling agents clone wins.
        let resolved = resolve_registry_source(None, &checkout).expect("resolve");
        assert_eq!(resolved.origin.as_str(), "agents-repo");
        assert!(resolved.text.contains("# agents"), "{}", resolved.text);

        // Standing inside the agents repository itself, its own file wins.
        let inside = resolve_registry_source(None, &agents).expect("resolve");
        assert_eq!(inside.path, agents.join(AGENTS_REPO_REGISTRY_FILE));

        // `--registry` still overrides both, and says it was told.
        let named = resolve_registry_source(
            Some(
                checkout
                    .join(DEFAULT_REGISTRY_RELATIVE_PATH)
                    .to_str()
                    .expect("utf8"),
            ),
            &agents,
        )
        .expect("resolve");
        assert_eq!(named.origin.as_str(), "explicit");
        assert!(named.text.contains("# checkout"), "{}", named.text);
    }

    /// The refusal names the places, and never borrows the sentence about a
    /// class nothing clears (ledger 178(a)).
    #[test]
    fn a_registry_nowhere_to_be_found_names_both_kinds_of_place() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = resolve_registry_source(None, dir.path()).expect_err("must fail");
        let sentence = error.to_string();
        assert!(
            sentence.contains("no model registry: looked in "),
            "{sentence}"
        );
        assert!(sentence.contains(AGENTS_REPO_REGISTRY_FILE), "{sentence}");
        assert!(
            sentence.contains(DEFAULT_REGISTRY_RELATIVE_PATH),
            "{sentence}"
        );
        assert!(!sentence.contains("risk tier"), "{sentence}");
    }

    /// With one signer the revision is a number; with two there is no shared
    /// clock, so it is `null` rather than one of the two.
    #[test]
    fn the_catalog_revision_is_null_when_two_signers_published() {
        let snapshot = CatalogSnapshot::default();
        assert_eq!(catalog_revision(&snapshot), None);
    }
}
