/**
 * The pure half of the model picker: rails, rows, search, and favorites.
 *
 * The flat `<select>` this replaces listed every model of every provider in
 * one column — thirty rows for Codex alone once live discovery landed (§2 item
 * 45), with no way to pin the two models a person actually uses. The shape
 * borrowed from t3code separates the three questions a picker answers: *whose*
 * model (the rail), *which* one (the searchable list), and *how hard it
 * thinks* (a separate control, because that is a separate decision).
 *
 * Everything here is data-in/data-out so the rules are testable without a DOM.
 */
import {
  codingSessionModelChoices,
  splitCodingSessionModelId,
} from "./codingSessionModelChoice";

/** The favourites rail entry, which belongs to no single provider. */
export const CODING_SESSION_MODEL_FAVORITES_RAIL = "favorites" as const;

export type CodingSessionModelPickerProvider = {
  /** Stable identity of one provider option — the target's selection key. */
  selectionKey: string;
  /** Runtime slug ("claude", "codex", "goose"). */
  runtime: string;
  /** Human provider label ("Claude Code · Primary"). */
  label: string;
  /** Base models this provider offers, adapter order. */
  models: string[];
  /** Whether a session may actually be created against it right now. */
  ready: boolean;
  /** Why it is not ready, when it is not. */
  unavailableNote: string | null;
};

/** One row of the model list. */
export type CodingSessionModelPickerRow = {
  /** `selectionKey` + model — unique across providers. */
  rowKey: string;
  selectionKey: string;
  runtime: string;
  providerLabel: string;
  model: string;
  favorite: boolean;
  ready: boolean;
  unavailableNote: string | null;
  /** 1-based position among favourites, or null — the ⌘N hint. */
  shortcut: number | null;
};

/** A favourite is a provider-scoped model, never a bare model name. */
export function codingSessionModelFavoriteKey(
  selectionKey: string,
  model: string,
): string {
  return `${selectionKey.length}:${selectionKey}${model}`;
}

/**
 * Normalize for search: casefold, and treat separators as spaces so "gpt 5.6"
 * finds `gpt-5.6-terra` and "claude code" finds the provider label.
 */
function normalize(value: string): string {
  return value
    .toLowerCase()
    .replace(/[_\-./]+/g, " ")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Rank one row against a query, or `null` when it does not match.
 *
 * Every whitespace-separated token must match something (name, provider, or
 * runtime), so "codex spark" narrows rather than widens. Lower is better:
 * a prefix beats a mid-string hit, and a favourite beats an equal
 * non-favourite — the pinned models are the ones a person means.
 */
export function scoreCodingSessionModelRow(
  row: Pick<
    CodingSessionModelPickerRow,
    "model" | "providerLabel" | "runtime" | "favorite"
  >,
  query: string,
): number | null {
  const tokens = normalize(query).split(" ").filter(Boolean);
  if (tokens.length === 0) return row.favorite ? -1 : 0;
  const fields = [
    normalize(row.model),
    normalize(row.providerLabel),
    normalize(row.runtime),
  ];
  let total = 0;
  for (const token of tokens) {
    let best: number | null = null;
    for (const [index, field] of fields.entries()) {
      const at = field.indexOf(token);
      if (at < 0) continue;
      const score = index * 10 + (at === 0 ? 0 : 5);
      best = best === null ? score : Math.min(best, score);
    }
    if (best === null) return null;
    total += best;
  }
  return row.favorite ? total - 100 : total;
}

/**
 * Build the rows for one rail selection.
 *
 * The favourites rail crosses providers; a provider rail shows that provider's
 * models only. Shortcut hints are assigned over the *favourites* order and
 * follow the model everywhere it appears, so ⌘2 means the same thing in both
 * rails.
 */
export function codingSessionModelPickerRows(input: {
  providers: readonly CodingSessionModelPickerProvider[];
  favorites: ReadonlySet<string>;
  rail: string;
  query: string;
}): CodingSessionModelPickerRow[] {
  const all: CodingSessionModelPickerRow[] = [];
  for (const provider of input.providers) {
    // A provider whose catalog has no models is still a provider: a signed-out
    // runtime usually publishes none, and dropping it would hide the very row
    // that explains why it cannot be used. The empty model id means "let the
    // adapter choose", which is what a create with no model already sends.
    const models = provider.models.length > 0 ? provider.models : [""];
    for (const model of models) {
      all.push({
        rowKey: codingSessionModelFavoriteKey(provider.selectionKey, model),
        selectionKey: provider.selectionKey,
        runtime: provider.runtime,
        providerLabel: provider.label,
        model,
        favorite: input.favorites.has(
          codingSessionModelFavoriteKey(provider.selectionKey, model),
        ),
        ready: provider.ready,
        unavailableNote: provider.unavailableNote,
        shortcut: null,
      });
    }
  }
  // Shortcuts are a property of the favourites list, assigned in provider
  // order so they are stable across renders and rails.
  let next = 1;
  for (const row of all) {
    if (row.favorite && next <= 9) {
      row.shortcut = next;
      next += 1;
    }
  }

  const scoped =
    input.rail === CODING_SESSION_MODEL_FAVORITES_RAIL
      ? all.filter((row) => row.favorite)
      : all.filter((row) => row.selectionKey === input.rail);

  return scoped
    .map((row) => ({
      row,
      score: scoreCodingSessionModelRow(row, input.query),
    }))
    .filter(
      (entry): entry is { row: CodingSessionModelPickerRow; score: number } =>
        entry.score !== null,
    )
    .sort((left, right) => left.score - right.score)
    .map((entry) => entry.row);
}

/**
 * Which rail to open on.
 *
 * The selected model's own provider, so the picker opens where the person
 * already is. Favourites win only when the selection is itself pinned —
 * opening on an empty favourites rail would hide every model behind a click.
 */
export function codingSessionModelPickerInitialRail(input: {
  providers: readonly CodingSessionModelPickerProvider[];
  favorites: ReadonlySet<string>;
  selectionKey: string | null;
  model: string | null;
}): string {
  const base =
    input.model === null ? null : splitCodingSessionModelId(input.model).model;
  if (
    input.selectionKey !== null &&
    base !== null &&
    input.favorites.has(codingSessionModelFavoriteKey(input.selectionKey, base))
  ) {
    return CODING_SESSION_MODEL_FAVORITES_RAIL;
  }
  if (input.selectionKey !== null) return input.selectionKey;
  return (
    input.providers[0]?.selectionKey ?? CODING_SESSION_MODEL_FAVORITES_RAIL
  );
}

/** The base models one provider offers, folded out of its flat id list. */
export function codingSessionProviderBaseModels(
  allowedModels: readonly string[],
): string[] {
  return codingSessionModelChoices(allowedModels).models;
}
