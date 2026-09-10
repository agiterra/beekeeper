import * as React from "react";

import { useCodingSessionNameSuggestion } from "../useCodingSessionNameSuggestion";
import { codingSessionNameSuggestStatus } from "../lib/codingSessionNameSuggestion";

/**
 * The name field's relationship with the namer.
 *
 * A generated name may replace an earlier generated name — the goal grows, and
 * so should its title — but never one a person typed.
 */
export function useNewCodingSessionTitleSuggestion({
  firstMessage,
  setTitle,
  title,
}: {
  firstMessage: string;
  setTitle: (title: string) => void;
  title: string;
}) {
  const autoFilledRef = React.useRef<string | null>(null);
  const titleRef = React.useRef(title);
  titleRef.current = title;

  const setTitleByHand = React.useCallback(
    (next: string) => {
      autoFilledRef.current = null;
      setTitle(next);
    },
    [setTitle],
  );

  const handleSuggestion = React.useCallback(
    (suggestion: string) => {
      const current = titleRef.current;
      if (current.trim().length > 0 && current !== autoFilledRef.current) {
        return;
      }
      autoFilledRef.current = suggestion;
      setTitle(suggestion);
    },
    [setTitle],
  );

  const { enabled, error, isGenerating, requestNow, settings } =
    useCodingSessionNameSuggestion({
      firstMessage,
      onSuggestion: handleSuggestion,
    });

  return {
    requestNow,
    setTitleByHand,
    /** The configured namer, or null until the settings read resolves. */
    settings,
    status: codingSessionNameSuggestStatus({ enabled, error, isGenerating }),
  };
}
