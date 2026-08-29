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

    let model = model.trim();
    if model.is_empty() || model == "default" {
        return None;
    }
    EXACT
        .iter()
        .find(|(id, _)| *id == model)
        .or_else(|| PREFIX.iter().find(|(id, _)| model.starts_with(id)))
        .map(|(_, window)| *window)
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
}
