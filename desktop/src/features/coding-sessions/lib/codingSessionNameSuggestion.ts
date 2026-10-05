import {
  type CodingSessionNamingSettings,
  namingModelConsulted,
} from "@/shared/api/tauriCodingSessionNaming";

/**
 * When to ask a model for a session name, and when to accept its answer.
 *
 * The naming request goes to whatever endpoint the person configured — which
 * may be someone else's — so it is not fired on every keystroke. It fires on
 * a cadence, and again the moment the first-message field loses focus, which
 * is the point at which someone has stopped writing and is looking at the
 * rest of the form.
 *
 * Two rules keep it honest: never re-send text that has already been named,
 * and never overwrite a name a person typed.
 */

/**
 * Whether the Name field may ask the person's naming model at all.
 *
 * Only in "Use my naming model" with an endpoint chosen (D9, SV-56). In the
 * agent mode — the default — the session's own agent titles it after Start,
 * on the computer it runs on, and Off titles nothing: in both, no draft
 * text leaves this computer for a name. A configured endpoint left behind
 * from before a mode switch is not consent to use it. `null` is no host, or
 * the read still in flight: no naming model, not an error.
 */
export function codingSessionNameSuggestionEnabled(
  settings: CodingSessionNamingSettings | null,
): boolean {
  return namingModelConsulted(settings);
}

/** How often the cadence may fire while the first message keeps changing. */
export const CODING_SESSION_NAME_SUGGEST_INTERVAL_MS = 5_000;

/**
 * Below this, there is nothing to name. A model handed three characters
 * invents a title rather than describing one.
 */
export const MIN_CODING_SESSION_NAME_SUGGEST_CHARS = 12;

/**
 * Whether this first message is worth sending to the namer right now.
 *
 * `lastRequestedText` is the exact text of the previous request, not a hash
 * or a timestamp: re-typing a word and deleting it again leaves the message
 * identical, and identical text would produce an identical name at the cost
 * of one more outbound request.
 */
export function shouldRequestCodingSessionName(input: {
  enabled: boolean;
  inFlight: boolean;
  lastRequestedText: string | null;
  text: string;
}): boolean {
  if (!input.enabled || input.inFlight) return false;
  const text = input.text.trim();
  if (text.length < MIN_CODING_SESSION_NAME_SUGGEST_CHARS) return false;
  return text !== input.lastRequestedText;
}

/**
 * Whether a generated value may replace what is in a field.
 *
 * Tracking the last value written automatically — rather than a bare
 * "touched" flag — lets a *newer* suggestion replace an older one while a
 * hand-typed value is never disturbed.
 */
export function shouldAdoptSuggestion(input: {
  current: string;
  lastAutoFilled: string | null;
  suggestion: string;
}): boolean {
  if (input.suggestion.length === 0) return false;
  if (input.suggestion === input.current) return false;
  return (
    input.current.trim().length === 0 || input.current === input.lastAutoFilled
  );
}

/** The sentence under the name field for one state of the namer. */
export type CodingSessionNameSuggestStatus =
  | { state: "off"; message: null }
  | { state: "idle"; message: null }
  | { state: "generating"; message: string }
  | { state: "failed"; message: string };

/**
 * What to say about the namer, if anything.
 *
 * A configured namer that is failing says so. An unconfigured one says
 * nothing at all: someone who never turned this on does not need a standing
 * notice about a feature they declined.
 */
export function codingSessionNameSuggestStatus(input: {
  enabled: boolean;
  error: string | null;
  isGenerating: boolean;
}): CodingSessionNameSuggestStatus {
  if (!input.enabled) return { state: "off", message: null };
  if (input.isGenerating) {
    return { state: "generating", message: "Naming this session…" };
  }
  if (input.error) {
    return { state: "failed", message: `Could not name it: ${input.error}` };
  }
  return { state: "idle", message: null };
}
