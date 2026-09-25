//! Static USD price table for turns whose adapter reported token counts but
//! no billed dollar figure of its own.
//!
//! [`crate::turn_cost`] tries this table only after a turn's billed cost is
//! unavailable. Every dollar figure it produces is this project's own
//! estimate, not the provider's bill — the caller marks it
//! `CostBasis::Estimated` and stamps [`PRICE_TABLE_ID`] / [`PRICE_TABLE_VERSION`]
//! alongside it so a reader never mistakes an estimate for a receipt (ledger
//! 266).
//!
//! Rates are USD per one million tokens, split the same way the provider's
//! own cache-aware billing is: a fresh input token, a cache write, and a
//! cache read are priced differently. A row is keyed by model family
//! (`opus`, `sonnet`, `haiku`); an id reaches a row through [`price_family`],
//! which accepts the family alias itself and the resolved id the adapter
//! reports answering (`claude-opus-4-6`), never the `default` picker label —
//! pricing keys on the model that answered, not the one requested (ledger
//! 268(e)).

/// Identifies this price table on the wire (`TurnCost::price_table_id`).
pub(crate) const PRICE_TABLE_ID: &str = "buzz-static";

/// This table's revision (`TurnCost::price_table_version`). Bump whenever a
/// rate changes — comparing an estimate across versions without knowing this
/// changed would be misleading.
pub(crate) const PRICE_TABLE_VERSION: &str = "2026-09-25.2";

/// One model's per-million-token USD rates.
struct Rate {
    model: &'static str,
    input_per_million: f64,
    output_per_million: f64,
    cache_read_per_million: f64,
    cache_write_per_million: f64,
}

/// Published Anthropic rates for the models this table recognizes. An id not
/// listed here is unpriced by this table — [`estimate_cost_usd`] returns
/// `None` rather than guessing.
const RATES: &[Rate] = &[
    Rate {
        model: "opus",
        input_per_million: 15.0,
        output_per_million: 75.0,
        cache_read_per_million: 1.5,
        cache_write_per_million: 18.75,
    },
    Rate {
        model: "sonnet",
        input_per_million: 3.0,
        output_per_million: 15.0,
        cache_read_per_million: 0.3,
        cache_write_per_million: 3.75,
    },
    Rate {
        model: "haiku",
        input_per_million: 0.8,
        output_per_million: 4.0,
        cache_read_per_million: 0.08,
        cache_write_per_million: 1.0,
    },
];

/// Resolved-id prefixes that name a family's members, as the adapter reports
/// them (`claude-opus-4-6`, `claude-sonnet-5`). A table, not a parse: an id
/// no row names has no family.
const FAMILY_PREFIXES: &[(&str, &str)] = &[
    ("claude-opus-", "opus"),
    ("claude-sonnet-", "sonnet"),
    ("claude-haiku-", "haiku"),
];

/// The long-context decoration both the picker (`opus[1m]`) and the SDK's
/// resolved ids (`claude-sonnet-5[1m]`) carry.
const LONG_CONTEXT_SUFFIX: &str = "[1m]";

/// The family an id belongs to, ignoring the long-context decoration, or
/// `None` for an id this table does not name — `default` included.
fn family(model: &str) -> Option<&'static str> {
    let model = model.trim();
    let model = model.strip_suffix(LONG_CONTEXT_SUFFIX).unwrap_or(model);
    if let Some(rate) = RATES.iter().find(|rate| rate.model == model) {
        return Some(rate.model);
    }
    FAMILY_PREFIXES
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, family)| *family)
}

/// The price-table family an id is billed as, or `None` when this table
/// cannot price it. A long-context id is unpriced: its rates are not the
/// family's base rates, and this table does not carry them.
fn price_family(model: &str) -> Option<&'static str> {
    if model.trim().ends_with(LONG_CONTEXT_SUFFIX) {
        return None;
    }
    family(model)
}

/// Whether a requested label and the model the adapter reported are the same
/// model: identical ids, or an alias and a resolved id of one family
/// (`opus` / `claude-opus-4-6`). `default` names no family, so any model
/// answering a `default` request is a stand-in and is disclosed as one.
pub(crate) fn same_model(requested: &str, effective: &str) -> bool {
    let (requested, effective) = (requested.trim(), effective.trim());
    requested == effective || family(requested).is_some_and(|f| family(effective) == Some(f))
}

fn rate_for_model(model: &str) -> Option<&'static Rate> {
    let family = price_family(model)?;
    RATES.iter().find(|rate| rate.model == family)
}

/// Estimate a turn's USD cost from this table, or `None` when the model is
/// not in it.
///
/// `fresh_input`/`output` are the turn's non-cache token counts; `cache_read`
/// and `cache_write` are priced at their own (cheaper/pricier) rates rather
/// than folded into `fresh_input`, matching how the provider itself bills a
/// cache hit or write. Any absent count is treated as zero contribution, not
/// as an unknown that blocks the whole estimate — the caller only reaches
/// this function once it has confirmed at least one count is present.
pub(crate) fn estimate_cost_usd(
    model: &str,
    fresh_input: Option<u64>,
    output: Option<u64>,
    cache_read: Option<u64>,
    cache_write: Option<u64>,
) -> Option<f64> {
    let rate = rate_for_model(model)?;
    let million = |tokens: Option<u64>| tokens.unwrap_or(0) as f64 / 1_000_000.0;
    Some(
        million(fresh_input) * rate.input_per_million
            + million(output) * rate.output_per_million
            + million(cache_read) * rate.cache_read_per_million
            + million(cache_write) * rate.cache_write_per_million,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_model_is_unpriced() {
        assert_eq!(
            estimate_cost_usd("gpt-5.6-codex", Some(1_000_000), None, None, None),
            None
        );
    }

    #[test]
    fn known_model_prices_each_bucket_at_its_own_rate() {
        let cost = estimate_cost_usd(
            "sonnet",
            Some(1_000_000),
            Some(1_000_000),
            Some(1_000_000),
            Some(1_000_000),
        )
        .expect("sonnet is in the table");
        // 3.0 + 15.0 + 0.3 + 3.75
        assert!((cost - 22.05).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn a_resolved_id_prices_as_its_family_and_the_default_label_does_not() {
        let opus = estimate_cost_usd("claude-opus-4-6", Some(1_000_000), None, None, None);
        assert_eq!(opus, Some(15.0));
        assert_eq!(
            estimate_cost_usd("default", Some(1_000_000), None, None, None),
            None
        );
        assert_eq!(
            estimate_cost_usd("claude-opus-4-6[1m]", Some(1_000_000), None, None, None),
            None,
            "long-context rates are not the base rates"
        );
    }

    #[test]
    fn an_alias_and_its_resolved_id_are_the_same_model_but_default_is_not() {
        assert!(same_model("opus", "claude-opus-4-6"));
        assert!(same_model("opus[1m]", "claude-opus-4-6[1m]"));
        assert!(same_model("claude-fable-5[1m]", "claude-fable-5[1m]"));
        assert!(!same_model("default", "claude-opus-4-6"));
        assert!(!same_model("sonnet", "claude-opus-4-6"));
    }

    #[test]
    fn absent_counts_contribute_nothing() {
        let cost = estimate_cost_usd("haiku", None, None, None, None).expect("haiku is priced");
        assert_eq!(cost, 0.0);
    }
}
