/**
 * What model a create writes, and what a control may print for it.
 *
 * Split out of `useNewCodingSessionCreate.ts` when that file crossed the
 * 1000-line ceiling. Pure functions over the picker's value and the runtime's
 * published catalog; nothing here touches React or the host.
 */
import { CODING_SESSION_ADAPTER_DEFAULT_MODEL } from "./codingSessionModelChoice";

/**
 * What the dialog must admit when the record cannot name the model.
 *
 * Some adapters publish a model whose id is literally `default` — "whatever
 * this CLI is configured for". A create carrying that id records a label, not
 * a model, and nothing downstream can resolve which weights ran. That is
 * allowed, because it is the truth about that runtime; what is not allowed is
 * letting the picker imply the session named a model when it did not.
 */
export const CODING_SESSION_CREATE_UNNAMED_MODEL_DISCLOSURE =
  "Runs the runtime's default model — the record will not name it.";

/** One runtime's published model list, as the create flow reads it. */
export type CodingSessionCreateModelCatalog = {
  defaultModel: string;
  allowedModels: readonly string[];
};

/**
 * The model id a create actually writes.
 *
 * The picker's unselected value is the *label* the adapter default carries,
 * and on 2026-08-28 that label — `default` — went onto the wire as the lead's
 * model (item 88(b)). A record that says `default` names nothing: it is the
 * same honesty class as a badge pointing at a message you cannot find.
 *
 * So `default` is resolved to the catalog's own `defaultModel` whenever that
 * is a concrete id. When the catalog's default is *itself* `default`, that is
 * all this computer knows and `default` is written — paired with
 * {@link codingSessionCreateModelDisclosure}, which says so next to the
 * picker rather than leaving the person to assume otherwise. Every other
 * value is written byte for byte: `opus[1m]` and `opus` are different ids.
 */
export function resolveCodingSessionCreateModel(input: {
  model: string | null;
  catalog?: CodingSessionCreateModelCatalog | null;
}): string | null {
  const model = input.model?.trim() ?? "";
  if (model.length === 0) return null;
  if (model !== CODING_SESSION_ADAPTER_DEFAULT_MODEL) return model;
  const resolved = input.catalog?.defaultModel?.trim() ?? "";
  if (
    resolved.length === 0 ||
    resolved === CODING_SESSION_ADAPTER_DEFAULT_MODEL ||
    !input.catalog?.allowedModels.includes(resolved)
  ) {
    return CODING_SESSION_ADAPTER_DEFAULT_MODEL;
  }
  return resolved;
}

/**
 * What a control may call the model when the record cannot name it.
 *
 * The adapter's own id for that entry is the word `default`, and a row reading
 * `default` next to a record reading `default` looks like a model someone
 * chose. This says which of the two it is.
 */
export const CODING_SESSION_CREATE_UNNAMED_MODEL_LABEL =
  "Runtime default (not named on the record)";

/**
 * The model id a surface should *print*, for the id it will write.
 *
 * Identical to {@link resolveCodingSessionCreateModel} except in the one case
 * that has no id to print: a catalog whose own default is `default`, where the
 * honest label replaces the wire token.
 */
export function codingSessionCreateModelLabel(input: {
  model: string | null;
  catalog?: CodingSessionCreateModelCatalog | null;
}): string | null {
  const resolved = resolveCodingSessionCreateModel(input);
  if (resolved === null) return null;
  return resolved === CODING_SESSION_ADAPTER_DEFAULT_MODEL
    ? CODING_SESSION_CREATE_UNNAMED_MODEL_LABEL
    : resolved;
}

/**
 * The sentence the dialog owes the person next to the model picker, or null.
 *
 * Non-null exactly when the create will carry `default` — the one case where
 * the signed record does not name the model that ran.
 */
export function codingSessionCreateModelDisclosure(input: {
  model: string | null;
  catalog?: CodingSessionCreateModelCatalog | null;
}): string | null {
  return resolveCodingSessionCreateModel(input) ===
    CODING_SESSION_ADAPTER_DEFAULT_MODEL
    ? CODING_SESSION_CREATE_UNNAMED_MODEL_DISCLOSURE
    : null;
}
