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

use std::path::{Path, PathBuf};

use serde_json::{json, Value};

use buzz_core::coding_session_routing::{
    check_coverage, parse_registry, Registry, DEFAULT_REGISTRY_RELATIVE_PATH,
};

use crate::client::BuzzClient;
use crate::error::CliError;

use super::catalog::{load_catalogs, CatalogSnapshot};

/// Find the registry: the explicit path, or the nearest ancestor of the
/// working directory that holds `team/model-registry.yaml`.
///
/// Walking up is what lets a seat run this from a worktree subdirectory. The
/// error names every directory that was tried, because "registry not found"
/// with no path is the kind of message that costs an hour.
///
/// # Errors
///
/// [`CliError::NotFound`] naming the path, or every path that was tried.
pub fn resolve_registry_path(explicit: Option<&str>, start: &Path) -> Result<PathBuf, CliError> {
    if let Some(path) = explicit {
        let path = PathBuf::from(path);
        return if path.is_file() {
            Ok(path)
        } else {
            Err(CliError::NotFound(format!(
                "no model registry at {}",
                path.display()
            )))
        };
    }
    let mut tried = Vec::new();
    for ancestor in start.ancestors() {
        let candidate = ancestor.join(DEFAULT_REGISTRY_RELATIVE_PATH);
        if candidate.is_file() {
            return Ok(candidate);
        }
        tried.push(candidate.display().to_string());
    }
    Err(CliError::NotFound(format!(
        "no model registry found — pass --registry <path>, or run from a checkout holding \
         {DEFAULT_REGISTRY_RELATIVE_PATH}. Tried: {}",
        tried.join(", ")
    )))
}

/// Read and parse the registry, naming the file in any failure.
///
/// # Errors
///
/// [`CliError::NotFound`] when there is no file, [`CliError::Usage`] when the
/// file is not a registry.
pub fn load_registry(explicit: Option<&str>) -> Result<(PathBuf, Registry), CliError> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let path = resolve_registry_path(explicit, &cwd)?;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| CliError::Other(format!("cannot read {}: {error}", path.display())))?;
    let registry = parse_registry(&text)
        .map_err(|error| CliError::Usage(format!("{}: {error}", path.display())))?;
    Ok((path, registry))
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

/// `bee sessions registry check --channel <uuid> [--registry <path>]`.
///
/// # Errors
///
/// Exit 4 ([`CliError::Other`]) when the registry is stale — an offered target
/// has no row. A dormant row is reported and does **not** fail.
pub async fn cmd_registry_check(
    client: &BuzzClient,
    channel_id: &str,
    registry_path: Option<&str>,
    format: &crate::OutputFormat,
) -> Result<(), CliError> {
    let (path, registry) = load_registry(registry_path)?;
    let snapshot = load_catalogs(client, channel_id).await?;
    let offered = super::route::offers(&snapshot);
    let coverage = check_coverage(&registry, &offered);

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
                "isStale": !coverage.is_fresh(),
            })
        ),
        crate::OutputFormat::Json => println!("{report}"),
    }

    if coverage.is_fresh() {
        return Ok(());
    }
    Err(CliError::Other(format!(
        "registry is stale: {} offered target(s) no row covers ({}). {} dormant row(s) are not \
         staleness and did not affect this result.",
        coverage.stale.len(),
        coverage.stale.join(", "),
        coverage.dormant.len()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use buzz_core::coding_session_routing::OfferedTarget;

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

    #[test]
    fn an_explicit_path_that_does_not_exist_is_named() {
        let error = resolve_registry_path(Some("/nope/registry.yaml"), Path::new("/tmp"))
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
            resolve_registry_path(None, &nested).expect("resolve"),
            registry
        );
    }

    #[test]
    fn a_missing_registry_names_the_paths_it_tried() {
        let dir = tempfile::tempdir().expect("tempdir");
        let error = resolve_registry_path(None, dir.path()).expect_err("must fail");
        assert!(
            error.to_string().contains(DEFAULT_REGISTRY_RELATIVE_PATH),
            "unexpected: {error}"
        );
    }

    /// With one signer the revision is a number; with two there is no shared
    /// clock, so it is `null` rather than one of the two.
    #[test]
    fn the_catalog_revision_is_null_when_two_signers_published() {
        let snapshot = CatalogSnapshot::default();
        assert_eq!(catalog_revision(&snapshot), None);
    }
}
