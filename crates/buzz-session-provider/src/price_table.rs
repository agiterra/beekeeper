//! Static USD price table for turns whose adapter reported token counts but
//! no dollar figure of its own.
//!
//! [`crate::turn_cost`] tries this table only after a turn's adapter estimate
//! is unavailable. Every dollar figure it produces is this project's own
//! estimate — the caller marks it `CostBasis::TableEstimate` and stamps
//! [`PRICE_TABLE_ID`] / [`PRICE_TABLE_VERSION`] alongside it so a reader
//! never mistakes it for the adapter's figure, let alone an invoice (ledger
//! 266, 272(d)).
//!
//! Rates are USD per one million tokens, split the way the provider's
//! cache-aware billing is: a fresh input token, a cache write, and a cache
//! read are priced differently.
//!
//! **Keyed by the exact model id the adapter reported answering**
//! (`claude-opus-5-5`), never by a family or a picker label. `opus`,
//! `sonnet`, `default` are requests the SDK resolves to an id this table
//! cannot see; a family row would price whichever generation the label
//! happened to mean, which is how run 11's lead turn was priced at the 2024
//! Opus rate (ledger 272(d)). An id not listed is unpriced.
//!
//! **Source:** Anthropic's published first-party API rates, as cached in the
//! claude-api reference on 2026-06-24 (input/output per model; cache reads at
//! 0.1× input — except Fable 5.1, published at $0.25). **Cache writes** are
//! priced at 2× input, the one-hour-TTL rate: the ACP prompt usage reports a
//! single cache-write count with no TTL split, and the adapter's own figure
//! for every run-11 turn it priced (ten turns, two models) equals this table
//! at 2× to six decimals, where 1.25× (the five-minute rate) is 9–12% low.
//! That is evidence of what this runtime writes, not a published guarantee;
//! a turn that wrote five-minute cache entries is over-estimated here.

/// Identifies this price table on the wire (`TurnCost::price_table_id`).
pub(crate) const PRICE_TABLE_ID: &str = "buzz-static";

/// This table's revision (`TurnCost::price_table_version`). Bump whenever a
/// rate changes — comparing an estimate across versions without knowing this
/// changed would be misleading.
pub(crate) const PRICE_TABLE_VERSION: &str = "2026-09-28.1";

/// One model's per-million-token USD rates.
struct Rate {
    model: &'static str,
    input_per_million: f64,
    output_per_million: f64,
    cache_read_per_million: f64,
}

impl Rate {
    /// Cache writes at the one-hour-TTL rate, 2× input (see the module doc).
    fn cache_write_per_million(&self) -> f64 {
        self.input_per_million * 2.0
    }
}

/// Published Anthropic rates for the exact ids this table recognizes
/// (claude-api reference, cached 2026-06-24). An id not listed here is
/// unpriced by this table — [`estimate_cost_usd`] returns `None` rather than
/// guessing.
const RATES: &[Rate] = &[
    Rate {
        model: "claude-opus-5-5",
        input_per_million: 4.0,
        output_per_million: 20.0,
        cache_read_per_million: 0.2,
    },
    Rate {
        model: "claude-opus-5",
        input_per_million: 5.0,
        output_per_million: 25.0,
        cache_read_per_million: 0.5,
    },
    Rate {
        model: "claude-opus-4-8",
        input_per_million: 5.0,
        output_per_million: 25.0,
        cache_read_per_million: 0.5,
    },
    Rate {
        model: "claude-sonnet-5",
        input_per_million: 2.0,
        output_per_million: 10.0,
        cache_read_per_million: 0.2,
    },
    Rate {
        model: "claude-sonnet-4-6",
        input_per_million: 3.0,
        output_per_million: 15.0,
        cache_read_per_million: 0.3,
    },
    Rate {
        model: "claude-haiku-4-5",
        input_per_million: 1.0,
        output_per_million: 5.0,
        cache_read_per_million: 0.1,
    },
    Rate {
        model: "claude-fable-5-1",
        input_per_million: 10.0,
        output_per_million: 50.0,
        cache_read_per_million: 0.25,
    },
];

/// Resolved-id prefixes that name a family's members, as the adapter reports
/// them (`claude-opus-5-5`, `claude-sonnet-5`), and the picker aliases that
/// request one. Used only to say whether a request and an answer are the
/// same model ([`same_model`]); never to price.
const FAMILY_PREFIXES: &[(&str, &str)] = &[
    ("claude-opus-", "opus"),
    ("claude-sonnet-", "sonnet"),
    ("claude-haiku-", "haiku"),
];

/// The long-context decoration both the picker (`opus[1m]`) and the SDK's
/// resolved ids (`claude-sonnet-5[1m]`) carry.
const LONG_CONTEXT_SUFFIX: &str = "[1m]";

/// The family an id or alias belongs to, ignoring the long-context
/// decoration, or `None` for one no row names — `default` included.
fn family(model: &str) -> Option<&'static str> {
    let model = model.trim();
    let model = model.strip_suffix(LONG_CONTEXT_SUFFIX).unwrap_or(model);
    if let Some((_, family)) = FAMILY_PREFIXES.iter().find(|(_, family)| *family == model) {
        return Some(family);
    }
    FAMILY_PREFIXES
        .iter()
        .find(|(prefix, _)| model.starts_with(prefix))
        .map(|(_, family)| *family)
}

/// Whether `model` is one of the adapter's picker labels (`default`, `opus`,
/// `sonnet`, `haiku`, optionally `[1m]`) rather than a resolved model id. A
/// label is a request; it names no model that answered (ledger 272(d)).
pub(crate) fn is_picker_label(model: &str) -> bool {
    let model = model.trim();
    let model = model.strip_suffix(LONG_CONTEXT_SUFFIX).unwrap_or(model);
    model == "default" || FAMILY_PREFIXES.iter().any(|(_, family)| *family == model)
}

/// Whether a requested label and the model the adapter reported are the same
/// model: identical ids, or an alias and a resolved id of one family
/// (`opus` / `claude-opus-5-5`). `default` names no family, so any model
/// answering a `default` request is a stand-in and is disclosed as one.
pub(crate) fn same_model(requested: &str, effective: &str) -> bool {
    let (requested, effective) = (requested.trim(), effective.trim());
    requested == effective || family(requested).is_some_and(|f| family(effective) == Some(f))
}

/// The row for an exact reported id. A long-context id (`…[1m]`) is
/// unpriced: this table does not carry long-context rates.
fn rate_for_model(model: &str) -> Option<&'static Rate> {
    let model = model.trim();
    RATES.iter().find(|rate| rate.model == model)
}

/// Estimate a turn's USD cost from this table, or `None` when the exact
/// model id is not in it.
///
/// `fresh_input`/`output` are the turn's non-cache token counts; `cache_read`
/// and `cache_write` are priced at their own rates rather than folded into
/// `fresh_input`, matching how the provider itself bills a cache hit or
/// write. Any absent count is treated as zero contribution, not as an unknown
/// that blocks the whole estimate — the caller only reaches this function
/// once it has confirmed at least one count is present.
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
            + million(cache_write) * rate.cache_write_per_million(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Ledger 272(d), RED-first: the published per-model rates (claude-api
    /// reference, cached 2026-06-24), keyed by the exact id the adapter
    /// reports. A label (`opus`, `sonnet`, `default`) is a request and never
    /// priced.
    #[test]
    fn published_rates_price_exact_ids_and_labels_are_unpriced() {
        let per_million = |model: &str| {
            [
                estimate_cost_usd(model, Some(1_000_000), None, None, None),
                estimate_cost_usd(model, None, Some(1_000_000), None, None),
                estimate_cost_usd(model, None, None, Some(1_000_000), None),
            ]
        };
        assert_eq!(
            per_million("claude-opus-5-5"),
            [Some(4.0), Some(20.0), Some(0.2)]
        );
        assert_eq!(
            per_million("claude-sonnet-5"),
            [Some(2.0), Some(10.0), Some(0.2)]
        );
        assert_eq!(
            per_million("claude-haiku-4-5"),
            [Some(1.0), Some(5.0), Some(0.1)]
        );
        for label in ["opus", "sonnet", "haiku", "default", "opus[1m]"] {
            assert_eq!(per_million(label), [None, None, None], "{label}");
        }
        assert_eq!(
            estimate_cost_usd("claude-opus-9-9", Some(1), None, None, None),
            None,
            "an id this table does not list is unpriced, not priced as its family"
        );
    }

    #[test]
    fn picker_labels_are_told_apart_from_resolved_ids() {
        for label in ["default", "opus", "sonnet", "haiku", "opus[1m]", " sonnet "] {
            assert!(is_picker_label(label), "{label}");
        }
        for id in ["claude-opus-5-5", "claude-sonnet-5[1m]", "gpt-5.5", ""] {
            assert!(!is_picker_label(id), "{id}");
        }
    }

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
            "claude-sonnet-5",
            Some(1_000_000),
            Some(1_000_000),
            Some(1_000_000),
            Some(1_000_000),
        )
        .expect("claude-sonnet-5 is in the table");
        // 2.0 + 10.0 + 0.2 + 4.0 (cache write at 2x input)
        assert!((cost - 16.2).abs() < 1e-9, "got {cost}");
    }

    #[test]
    fn a_long_context_id_and_the_default_label_are_unpriced() {
        assert_eq!(
            estimate_cost_usd("default", Some(1_000_000), None, None, None),
            None
        );
        assert_eq!(
            estimate_cost_usd("claude-opus-5-5[1m]", Some(1_000_000), None, None, None),
            None,
            "long-context rates are not the base rates"
        );
    }

    /// Ledger 272(d): run 11's lead turn seq 62 (`claude-opus-5-5` per the
    /// seat's native transcript) — 14 fresh input, 4,184 output, 388,927
    /// cache-read, 8,237 cache-write tokens. The adapter published
    /// $0.2274174; the old family table said $1.0518 (2024 Opus rates). This
    /// table reproduces the adapter's figure; the five-minute cache-write
    /// rate would give $0.2027.
    #[test]
    fn run_11_lead_seq_62_prices_to_the_adapters_own_figure() {
        let cost = estimate_cost_usd(
            "claude-opus-5-5",
            Some(14),
            Some(4_184),
            Some(388_927),
            Some(8_237),
        )
        .expect("priced");
        assert!((cost - 0.2274174).abs() < 1e-7, "got {cost}");
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
        let cost = estimate_cost_usd("claude-haiku-4-5", None, None, None, None)
            .expect("claude-haiku-4-5 is priced");
        assert_eq!(cost, 0.0);
    }
}
