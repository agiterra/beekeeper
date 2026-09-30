//! What each model id's context window is, when this provider knows it.
//!
//! The provider stamps [`buzz_core::coding_session_payload::TurnUsageReport::context_window`]
//! from this table so a reader can turn a token count into a percentage
//! without holding its own model list. An id this table does not recognize
//! resolves to `None` and the field is omitted: a percentage computed against
//! a guessed denominator reads exactly like a measured one and is a lie.
//!
//! A driver that states its own window — `claude-agent-acp` sends
//! `{"kind":"context_window_updated","usage":{"size":…,"used":…}}` — outranks
//! this table at every reader, because that number came from the thing doing
//! the work. This is the fallback for drivers that report tokens but never
//! name a window.

/// Context window in tokens for a model id the provider recognizes.
///
/// Matching is on the id as it appears in session metadata (the resolved id,
/// never the `default` alias — `default` deliberately resolves to `None`,
/// because the label names a choice rather than a model and the window behind
/// it changes with the runtime's catalog).
///
/// The Anthropic entries are the published windows for those models. The
/// `gpt-5.6-*` entry is **an assumption**: codex-acp does not report a window
/// and the provider has no catalog call that returns one, so 400 000 is
/// recorded here as this project's working figure and should be replaced the
/// moment the driver states its own.
pub(crate) fn context_window_for_model(model: &str) -> Option<u64> {
    /// Exact ids, longest-lived first. Kept as a table rather than a match so
    /// the list reads as data and a new entry is one line.
    const EXACT: &[(&str, u64)] = &[
        ("claude-fable-5[1m]", 1_000_000),
        ("opus[1m]", 1_000_000),
        ("sonnet", 200_000),
        ("haiku", 200_000),
    ];
    /// Id prefixes, for families whose members share a window.
    const PREFIX: &[(&str, u64)] = &[("gpt-5.6-", 400_000)];

    // Selection modifiers (`[high]`, `[fast]`) do not change the window or
    // the family; a context token (`[1m]`) does and is kept.
    let model = crate::price_table::model_identity(model);
    if model.is_empty() || model == "default" {
        return None;
    }
    EXACT
        .iter()
        .find(|(id, _)| *id == model)
        .or_else(|| PREFIX.iter().find(|(id, _)| model.starts_with(id)))
        .map(|(_, window)| *window)
}

/// Model family for an id this provider recognizes, or `None`.
///
/// A *family* groups ids that are the same model at different sizes or point
/// releases — `opus`, `sonnet`, `gpt-5.6`. It is published in the kind:44222
/// catalog next to the context window so a picker can group an offer without
/// holding its own model list.
///
/// Like [`context_window_for_model`] this is a **table, not a parse**. Nothing
/// here splits an id on a hyphen and calls the head a family: `claude-fable-5`
/// would become `claude`, `gpt-5.6-codex` would become `gpt`, and both would
/// read to a consumer exactly like a fact somebody checked. An id this table
/// does not name has no family, and the catalog omits the field.
pub(crate) fn model_family_for_model(model: &str) -> Option<&'static str> {
    /// Exact ids this project seats agents on.
    const EXACT: &[(&str, &str)] = &[
        ("claude-fable-5[1m]", "fable"),
        ("opus[1m]", "opus"),
        ("opus", "opus"),
        ("sonnet", "sonnet"),
        ("haiku", "haiku"),
    ];
    /// Id prefixes whose members share a family.
    const PREFIX: &[(&str, &str)] = &[("gpt-5.6-", "gpt-5.6")];

    // Selection modifiers (`[high]`, `[fast]`) do not change the window or
    // the family; a context token (`[1m]`) does and is kept.
    let model = crate::price_table::model_identity(model);
    // `default` names a choice rather than a model, exactly as it does for the
    // window: the family behind it changes with the runtime's catalog.
    if model.is_empty() || model == "default" {
        return None;
    }
    EXACT
        .iter()
        .find(|(id, _)| *id == model)
        .or_else(|| PREFIX.iter().find(|(id, _)| model.starts_with(id)))
        .map(|(_, family)| *family)
}

#[cfg(test)]
mod tests {
    use super::context_window_for_model;

    /// The million-token Anthropic ids this project actually seats agents on.
    #[test]
    fn the_one_million_token_ids_resolve() {
        assert_eq!(
            context_window_for_model("claude-fable-5[1m]"),
            Some(1_000_000)
        );
        assert_eq!(context_window_for_model("opus[1m]"), Some(1_000_000));
    }

    /// The two-hundred-thousand ids.
    #[test]
    fn sonnet_and_haiku_resolve_to_two_hundred_thousand() {
        assert_eq!(context_window_for_model("sonnet"), Some(200_000));
        assert_eq!(context_window_for_model("haiku"), Some(200_000));
    }

    /// The codex family matches by prefix, so a new point release needs no
    /// table edit.
    #[test]
    fn the_codex_family_matches_by_prefix() {
        assert_eq!(context_window_for_model("gpt-5.6-sol"), Some(400_000));
        assert_eq!(context_window_for_model("gpt-5.6-codex"), Some(400_000));
    }

    /// A selection's effort and fast-mode modifiers change neither the
    /// window nor the family; its context token does.
    #[test]
    fn selection_modifiers_keep_the_models_window_and_family() {
        use super::model_family_for_model;
        assert_eq!(
            context_window_for_model("opus[1m][high][fast]"),
            Some(1_000_000)
        );
        assert_eq!(context_window_for_model("haiku[fast]"), Some(200_000));
        assert_eq!(context_window_for_model("default[high]"), None);
        assert_eq!(model_family_for_model("opus[1m][max]"), Some("opus"));
    }

    /// `default` names a choice, not a model: the window behind it is whatever
    /// the runtime's catalog resolved, which this table cannot know.
    #[test]
    fn the_default_alias_has_no_window() {
        assert_eq!(context_window_for_model("default"), None);
    }

    /// An unrecognized id omits the window rather than guessing one.
    #[test]
    fn an_unknown_model_has_no_window() {
        assert_eq!(context_window_for_model("some-model-nobody-shipped"), None);
        assert_eq!(context_window_for_model(""), None);
    }

    /// The family table names ids; it never splits one on a hyphen. A parse
    /// would turn `claude-fable-5[1m]` into `claude` and `gpt-5.6-codex` into
    /// `gpt`, and publish both as though somebody had checked.
    #[test]
    fn the_family_table_names_ids_rather_than_parsing_them() {
        use super::model_family_for_model;
        assert_eq!(model_family_for_model("claude-fable-5[1m]"), Some("fable"));
        assert_eq!(model_family_for_model("opus[1m]"), Some("opus"));
        assert_eq!(model_family_for_model("gpt-5.6-codex"), Some("gpt-5.6"));
        assert_eq!(model_family_for_model("claude-3-opus-20240229"), None);
        assert_eq!(model_family_for_model("default"), None);
        assert_eq!(model_family_for_model(""), None);
    }
}
