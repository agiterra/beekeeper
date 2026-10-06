//! Reading the per-model controls an ACP adapter publishes beside its model
//! option, and splitting a picker selection into a model and its modifiers.
//!
//! An adapter's `configOptions` carry more than the `model` select: a
//! reasoning-effort select (category `thought_level`) and a fast-mode switch
//! (category `model_config`, id `fast` in claude-agent-acp, `fast-mode` in
//! codex-acp). Both depend on the model selected, so they are read from the
//! response to a model switch, never assumed from another model's answer.
//!
//! A picker selection is `<model>` optionally followed by bracket groups:
//! `opus[1m][high][fast]`. Some brackets are part of an advertised id (`[1m]`,
//! Codex's `gpt-5.6-sol[high]`); the rest are modifiers this crate applies as
//! separate config options. [`resolve_model_selection`] decides which is which
//! against the adapter's own model option values, never by vocabulary.

use crate::acp::{extract_model_config_options, resolve_model_switch_method};

/// ACP config-option category for reasoning effort.
pub const THOUGHT_LEVEL_CATEGORY: &str = "thought_level";
/// ACP config-option category the fast-mode switch is published under.
pub const MODEL_CONFIG_CATEGORY: &str = "model_config";
/// The selection modifier that asks for the adapter's fast mode.
pub const FAST_MODIFIER: &str = "fast";
/// Fast-mode option ids the known adapters publish.
const FAST_MODE_IDS: &[&str] = &["fast", "fast-mode"];

/// A config option's id: the ACP spec says `configId`, claude-agent-acp and
/// codex-acp send `id`. Accept both, as [`resolve_model_switch_method`] does.
pub fn config_option_id(option: &serde_json::Value) -> Option<&str> {
    option
        .get("configId")
        .or_else(|| option.get("id"))
        .and_then(serde_json::Value::as_str)
}

/// A select option's values in the adapter's order, flattening grouped
/// options (`options: [{ options: [...] }]`) the way the adapters do.
pub fn config_option_values(option: &serde_json::Value) -> Vec<String> {
    let mut values = Vec::new();
    for entry in option
        .get("options")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
    {
        let nested = entry.get("options").and_then(serde_json::Value::as_array);
        for leaf in nested.map_or_else(|| vec![entry], |group| group.iter().collect()) {
            if let Some(value) = leaf
                .get("value")
                .and_then(serde_json::Value::as_str)
                .map(str::trim)
                .filter(|value| !value.is_empty())
            {
                if !values.iter().any(|seen| seen == value) {
                    values.push(value.to_owned());
                }
            }
        }
    }
    values
}

fn config_options(result: &serde_json::Value) -> impl Iterator<Item = &serde_json::Value> {
    result
        .get("configOptions")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
}

/// Whether a response carries a `configOptions` array at all. Absent means
/// the adapter said nothing about the controls — which is not the same as
/// saying the model has none.
pub fn has_config_options(result: &serde_json::Value) -> bool {
    result
        .get("configOptions")
        .is_some_and(serde_json::Value::is_array)
}

/// The reasoning-effort option in a `session/new` or `set_config_option`
/// response, if the adapter offers one for the selected model.
pub fn extract_thought_level_option(result: &serde_json::Value) -> Option<&serde_json::Value> {
    config_options(result).find(|option| {
        option.get("category").and_then(serde_json::Value::as_str) == Some(THOUGHT_LEVEL_CATEGORY)
    })
}

/// Whether one config option is the adapter's fast-mode switch.
///
/// Category `model_config` and either one of the known ids or a native
/// boolean whose id or name says fast. A client that has not opted into
/// boolean config options receives the switch as an `on`/`off` select.
pub fn is_fast_mode_option(option: &serde_json::Value) -> bool {
    if option.get("category").and_then(serde_json::Value::as_str) != Some(MODEL_CONFIG_CATEGORY) {
        return false;
    }
    let id = config_option_id(option).unwrap_or_default();
    if FAST_MODE_IDS.contains(&id) {
        return true;
    }
    let says_fast = |text: &str| text.to_ascii_lowercase().contains("fast");
    option.get("type").and_then(serde_json::Value::as_str) == Some("boolean")
        && (says_fast(id)
            || option
                .get("name")
                .and_then(serde_json::Value::as_str)
                .is_some_and(says_fast))
}

/// The fast-mode switch in a response, if the selected model offers one.
pub fn extract_fast_mode_option(result: &serde_json::Value) -> Option<&serde_json::Value> {
    config_options(result).find(|option| is_fast_mode_option(option))
}

/// The value that turns a fast-mode option on: `true` for a native boolean,
/// `"on"` for the select fallback. `None` when the option offers neither.
pub fn fast_mode_on_value(option: &serde_json::Value) -> Option<serde_json::Value> {
    if option.get("type").and_then(serde_json::Value::as_str) == Some("boolean") {
        return Some(serde_json::Value::Bool(true));
    }
    config_option_values(option)
        .iter()
        .any(|value| value == "on")
        .then(|| serde_json::Value::String("on".into()))
}

/// Every way to read `selection` as `<remainder>` plus trailing bracket
/// groups, peeling one group at a time from the right. Each entry's tokens are
/// in their original left-to-right order, without brackets.
///
/// `a[1m][high]` → `[("a[1m]", ["high"]), ("a", ["1m", "high"])]`. A string
/// that does not end in a well-formed, non-empty group yields nothing.
pub fn peel_bracket_suffixes(selection: &str) -> Vec<(&str, Vec<&str>)> {
    let mut peeled = Vec::new();
    let mut tokens: Vec<&str> = Vec::new();
    let mut rest = selection;
    while let Some(inner) = rest.strip_suffix(']') {
        let Some(open) = inner.rfind('[') else {
            break;
        };
        let token = &inner[open + 1..];
        if token.is_empty() || token.contains(']') {
            break;
        }
        rest = &inner[..open];
        if rest.is_empty() {
            break;
        }
        tokens.insert(0, token);
        peeled.push((rest, tokens.clone()));
    }
    peeled
}

/// A selection that names an advertised model option plus modifiers the
/// adapter applies separately.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSelectionPlan {
    /// The model option's config id.
    pub config_id: String,
    /// The advertised model option value the selection starts with.
    pub base: String,
    /// The stripped bracket tokens, left to right (`high`, `fast`).
    pub modifiers: Vec<String>,
}

/// Split a selection the adapter does not advertise whole into an advertised
/// model option value and the modifiers after it.
///
/// Returns `None` when the whole string *is* advertised — the caller's
/// existing whole-id path owns that case, unchanged — and when no peeled
/// remainder is a model option value. Peeling runs right to left and stops at
/// the first remainder that is, so a bracket that belongs to an advertised id
/// (`opus[1m]`) stays with it.
pub fn resolve_model_selection(
    session_new_result: &serde_json::Value,
    selection: &str,
) -> Option<ModelSelectionPlan> {
    if resolve_model_switch_method(session_new_result, selection).is_some() {
        return None;
    }
    let model_options = extract_model_config_options(session_new_result);
    for (remainder, tokens) in peel_bracket_suffixes(selection) {
        for option in &model_options {
            let Some(config_id) = config_option_id(option) else {
                continue;
            };
            if config_option_values(option)
                .iter()
                .any(|value| value == remainder)
            {
                return Some(ModelSelectionPlan {
                    config_id: config_id.to_owned(),
                    base: remainder.to_owned(),
                    modifiers: tokens.iter().map(|token| (*token).to_owned()).collect(),
                });
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claude_new() -> serde_json::Value {
        serde_json::json!({
            "configOptions": [
                {"id": "mode", "category": "mode", "type": "select", "options": [{"value": "default"}]},
                {"id": "model", "category": "model", "type": "select", "currentValue": "default",
                 "options": [{"value": "default"}, {"value": "opus[1m]"}, {"value": "haiku"}]},
                {"id": "effort", "category": "thought_level", "type": "select",
                 "options": [{"value": "default"}, {"value": "low"}, {"value": "high"}]},
                {"id": "fast", "category": "model_config", "type": "select",
                 "currentValue": "off", "options": [{"value": "on"}, {"value": "off"}]}
            ],
            "models": {"availableModels": [{"modelId": "gpt-5.6-sol[high]"}]}
        })
    }

    #[test]
    fn peeling_goes_right_to_left_and_keeps_token_order() {
        assert_eq!(
            peel_bracket_suffixes("opus[1m][high][fast]"),
            vec![
                ("opus[1m][high]", vec!["fast"]),
                ("opus[1m]", vec!["high", "fast"]),
                ("opus", vec!["1m", "high", "fast"]),
            ]
        );
        assert!(peel_bracket_suffixes("opus").is_empty());
        assert!(peel_bracket_suffixes("opus[]").is_empty());
        assert!(peel_bracket_suffixes("[fast]").is_empty());
    }

    #[test]
    fn an_advertised_whole_id_is_left_to_the_existing_path() {
        let raw = claude_new();
        assert_eq!(resolve_model_selection(&raw, "opus[1m]"), None);
        assert_eq!(resolve_model_selection(&raw, "gpt-5.6-sol[high]"), None);
    }

    #[test]
    fn modifiers_are_stripped_until_an_advertised_option_value_remains() {
        let raw = claude_new();
        assert_eq!(
            resolve_model_selection(&raw, "opus[1m][high][fast]"),
            Some(ModelSelectionPlan {
                config_id: "model".into(),
                base: "opus[1m]".into(),
                modifiers: vec!["high".into(), "fast".into()],
            })
        );
        assert_eq!(
            resolve_model_selection(&raw, "haiku[fast]").map(|plan| plan.base),
            Some("haiku".into())
        );
        // Nothing advertised under the brackets: no plan, no guess.
        assert_eq!(resolve_model_selection(&raw, "sonnet[high]"), None);
        assert_eq!(resolve_model_selection(&raw, "sonnet"), None);
    }

    #[test]
    fn effort_and_fast_options_are_found_by_category() {
        let raw = claude_new();
        let effort = extract_thought_level_option(&raw).expect("effort");
        assert_eq!(config_option_values(effort), vec!["default", "low", "high"]);
        let fast = extract_fast_mode_option(&raw).expect("fast");
        assert_eq!(
            fast_mode_on_value(fast),
            Some(serde_json::Value::String("on".into()))
        );
        let boolean = serde_json::json!({
            "id": "fast-mode", "category": "model_config", "type": "boolean", "currentValue": false
        });
        assert_eq!(
            fast_mode_on_value(&boolean),
            Some(serde_json::Value::Bool(true))
        );
        // Another model_config option is not fast mode.
        let other =
            serde_json::json!({"id": "verbosity", "category": "model_config", "type": "boolean"});
        assert!(!is_fast_mode_option(&other));
        assert!(extract_thought_level_option(&serde_json::json!({"configOptions": []})).is_none());
    }

    #[test]
    fn grouped_select_options_are_flattened_in_order() {
        let option = serde_json::json!({
            "options": [
                {"group": "a", "options": [{"value": "x"}, {"value": "y"}]},
                {"value": "z"},
                {"value": "x"}
            ]
        });
        assert_eq!(config_option_values(&option), vec!["x", "y", "z"]);
    }
}
