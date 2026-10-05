import * as React from "react";

import {
  CODING_SESSION_NAME_SUGGEST_INTERVAL_MS,
  codingSessionNameSuggestionEnabled,
  shouldRequestCodingSessionName,
} from "./lib/codingSessionNameSuggestion";
import {
  generateCodingSessionName,
  getCodingSessionNamingSettings,
  type CodingSessionNamingSettings,
} from "@/shared/api/tauriCodingSessionNaming";

/**
 * Name a session from its first message, on a cadence and on blur.
 *
 * The cadence is a timer rather than a debounce because the interesting
 * moment is "this text has settled into something different", not "a
 * keystroke happened": a five-second tick that finds unchanged text sends
 * nothing, so a person typing steadily for a minute produces a handful of
 * requests, not one per word.
 *
 * The hook holds no key and builds no request. It asks the host, which knows
 * whether a namer is configured and holds the credential — so a desktop not
 * on "Use my naming model" (the agent mode, the default, or Off) makes
 * exactly one call here (the settings read) and never touches the network
 * (D9, SV-56).
 */
export function useCodingSessionNameSuggestion({
  firstMessage,
  onSuggestion,
}: {
  firstMessage: string;
  /** Called with a cleaned one-to-four-word name. */
  onSuggestion: (name: string) => void;
}): {
  /** The configured namer, or null until the settings read resolves. */
  settings: CodingSessionNamingSettings | null;
  enabled: boolean;
  error: string | null;
  isGenerating: boolean;
  /** Ask now — what the first message's blur handler calls. */
  requestNow: () => void;
} {
  const [settings, setSettings] =
    React.useState<CodingSessionNamingSettings | null>(null);
  const [isGenerating, setIsGenerating] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  React.useEffect(() => {
    let cancelled = false;
    void getCodingSessionNamingSettings()
      .then((next) => {
        if (!cancelled) setSettings(next);
      })
      .catch(() => {
        // No host (browser preview, E2E mock) means no namer. That is the
        // same as "off", and it is not an error worth showing.
        if (!cancelled) setSettings(null);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const enabled = codingSessionNameSuggestionEnabled(settings);

  // Read inside callbacks so neither the interval nor `requestNow` has to be
  // rebuilt on every keystroke.
  const textRef = React.useRef(firstMessage);
  textRef.current = firstMessage;
  const onSuggestionRef = React.useRef(onSuggestion);
  onSuggestionRef.current = onSuggestion;
  const inFlightRef = React.useRef(false);
  const lastRequestedRef = React.useRef<string | null>(null);
  // Survives unmount: the dialog can close while a request is in the air, and
  // a resolved promise must not set state on a gone component.
  const mountedRef = React.useRef(true);
  React.useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const request = React.useCallback(() => {
    const text = textRef.current.trim();
    if (
      !shouldRequestCodingSessionName({
        enabled,
        inFlight: inFlightRef.current,
        lastRequestedText: lastRequestedRef.current,
        text,
      })
    ) {
      return;
    }
    inFlightRef.current = true;
    lastRequestedRef.current = text;
    setIsGenerating(true);
    void generateCodingSessionName(text)
      .then((name) => {
        if (!mountedRef.current) return;
        setError(null);
        const cleaned = name.trim();
        if (cleaned.length > 0) onSuggestionRef.current(cleaned);
      })
      .catch((cause: unknown) => {
        if (!mountedRef.current) return;
        setError(cause instanceof Error ? cause.message : String(cause));
        // Let the next tick retry the same text: the failure may have been
        // a dropped connection rather than a refusal.
        lastRequestedRef.current = null;
      })
      .finally(() => {
        inFlightRef.current = false;
        if (mountedRef.current) setIsGenerating(false);
      });
  }, [enabled]);

  React.useEffect(() => {
    if (!enabled) return;
    const handle = window.setInterval(
      request,
      CODING_SESSION_NAME_SUGGEST_INTERVAL_MS,
    );
    return () => window.clearInterval(handle);
  }, [enabled, request]);

  return { enabled, error, isGenerating, requestNow: request, settings };
}
