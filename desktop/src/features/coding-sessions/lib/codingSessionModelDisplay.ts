/**
 * Human names for model ids, derived rather than invented.
 *
 * The picker listed `claude-fable-5[1m]` and `gpt-5.6-terra` — wire tokens,
 * straight from the adapter, as the primary thing a person reads. The fix is
 * *not* a hand-written table of pretty names: a table drifts the moment an
 * adapter ships a model nobody here has heard of, and a name we made up for an
 * id we do not recognise is a small lie in the same family as `Codex ·
 * default`.
 *
 * So the transform is mechanical and total — it works on any id, present or
 * future — and the raw id stays on the row beneath it. Nothing is hidden;
 * something readable is added.
 */
import {
  CODING_SESSION_ADAPTER_DEFAULT_MODEL,
  splitCodingSessionModelId,
} from "./codingSessionModelChoice";

/** Tokens whose conventional casing a title-caser would get wrong. */
const CASED_TOKENS = new Map<string, string>([
  ["gpt", "GPT"],
  ["cli", "CLI"],
  ["acp", "ACP"],
  ["ai", "AI"],
  ["gemini", "Gemini"],
  ["oss", "OSS"],
  ["mini", "mini"],
  ["nano", "nano"],
  ["preview", "preview"],
  ["latest", "latest"],
]);

/** Families whose version follows a hyphen rather than a space. */
const HYPHENATES_VERSION = new Set(["GPT"]);

function titleToken(token: string): string {
  const lowered = token.toLowerCase();
  const known = CASED_TOKENS.get(lowered);
  if (known !== undefined) return known;
  // A version-ish token keeps its shape: `5.6`, `4o`, `20250219`.
  if (/^[\d.]/.test(token)) return token;
  return token.slice(0, 1).toUpperCase() + token.slice(1);
}

/**
 * What a control calls a model the record does not name.
 *
 * Kept identical to `CODING_SESSION_CREATE_UNNAMED_MODEL_LABEL`
 * (`../ui/useNewCodingSessionCreate`), which discloses the same state on the
 * create path. Not imported from there: this is a `lib` module and that is a
 * `ui` hook module, so the string is duplicated deliberately and both copies
 * are pinned by tests.
 */
const UNNAMED_MODEL_LABEL = "Runtime default (not named on the record)";

/**
 * The name shown as the row's title.
 *
 * `default` is the adapter's "you choose" alias rather than a model, and
 * saying so is the whole point — it used to sit in the list looking like a
 * peer of Sonnet. It says it the same way the create path does: "Adapter
 * default" and "Runtime default (not named on the record)" were two voices for
 * one fact in the same One-session dialog.
 */
export function codingSessionModelDisplayName(model: string): string {
  if (model === "") return UNNAMED_MODEL_LABEL;
  if (model === CODING_SESSION_ADAPTER_DEFAULT_MODEL)
    return UNNAMED_MODEL_LABEL;
  const { model: base } = splitCodingSessionModelId(model);
  const tokens = base.split(/[-_\s]+/).filter(Boolean);
  if (tokens.length === 0) return base;
  const named = tokens.map(titleToken);
  return named.reduce((rendered, token, index) => {
    if (index === 0) return token;
    // One conventional exception, and it is the vendor's own spelling: OpenAI
    // writes "GPT-5.6", Anthropic writes "Claude Opus 5". Everything else is a
    // space, so an unrecognised family reads as words rather than a slug.
    const separator =
      HYPHENATES_VERSION.has(named[index - 1]) && /^\d/.test(token) ? "-" : " ";
    return `${rendered}${separator}${token}`;
  }, "");
}

/** `1m` → `1M`, `200k` → `200K`: a window, printed the way people write it. */
export function codingSessionContextLabel(context: string): string {
  return context.toUpperCase();
}

/**
 * The summary a traits control shows when it is closed: `High · 1M`.
 *
 * Null when the model carries neither dimension, so a surface can hide the
 * control instead of showing an empty one.
 */
export function codingSessionTraitsSummary(input: {
  thinking: string | null;
  context: string | null;
}): string | null {
  const parts = [
    input.thinking === null ? null : titleToken(input.thinking),
    input.context === null ? null : codingSessionContextLabel(input.context),
  ].filter((part): part is string => part !== null);
  return parts.length === 0 ? null : parts.join(" · ");
}
