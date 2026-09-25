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
//! cache read are priced differently. Model ids match
//! [`crate::context_window::context_window_for_model`]'s table — the
//! resolved id as it appears in session metadata, never the `default` alias.

/// Identifies this price table on the wire (`TurnCost::price_table_id`).
pub(crate) const PRICE_TABLE_ID: &str = "buzz-static";

/// This table's revision (`TurnCost::price_table_version`). Bump whenever a
/// rate changes — comparing an estimate across versions without knowing this
/// changed would be misleading.
pub(crate) const PRICE_TABLE_VERSION: &str = "2026-09-25.1";

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

fn rate_for_model(model: &str) -> Option<&'static Rate> {
    let model = model.trim();
    RATES.iter().find(|rate| rate.model == model)
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
    fn absent_counts_contribute_nothing() {
        let cost = estimate_cost_usd("haiku", None, None, None, None).expect("haiku is priced");
        assert_eq!(cost, 0.0);
    }
}
