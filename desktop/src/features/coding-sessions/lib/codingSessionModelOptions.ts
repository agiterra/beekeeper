/**
 * A model's options — reasoning effort and fast mode — as the runtime reports
 * them, and the one string a session is created with.
 *
 * The catalog's per-model rows (NIP-CSPC § Per-model rows) carry what the
 * adapter itself says about each id: its display name, the effort values it
 * accepts with that id selected, and whether a fast-mode switch exists. Before
 * those rows existed, effort was only ever inferred from bracketed variant ids
 * (`gpt-5.6-sol[high]`), and that inference still governs any provider whose
 * catalog carries no rows — {@link codingSessionModelChoices} is unchanged.
 *
 * The selection contract, shared with the provider:
 *
 *     <id>[<effort>][fast]
 *
 * where `<id>` is an `allowedModels` entry (it may itself carry a context
 * token, `opus[1m]`), `<effort>` is one of that id's row `efforts` other than
 * `default` (omitted for the default), and `[fast]` appears only when the row
 * says `fastMode: true`. Nothing here extends the offer: a composed string is
 * valid only when every token is one the row for its id names.
 */
import type { CodingSessionProviderCatalogModel } from "./codingSessionProviderCatalog";
import {
  type CodingSessionModelChoices,
  codingSessionModelChoices,
  joinCodingSessionModelId,
  resolveCodingSessionContext,
  resolveCodingSessionThinking,
  splitCodingSessionModelId,
} from "./codingSessionModelChoice";

/** The effort value a runtime uses for "its own default" — never a token. */
export const CODING_SESSION_DEFAULT_EFFORT = "default";

/** The bracket token that turns fast mode on. */
export const CODING_SESSION_FAST_TOKEN = "fast";
const FAST_SUFFIX = `[${CODING_SESSION_FAST_TOKEN}]`;

/** The parts of a catalog row the picker reads. */
export type CodingSessionModelRow = Pick<
  CodingSessionProviderCatalogModel,
  "id" | "name" | "description" | "efforts" | "fastMode" | "rank"
>;

/** What the picker needs from one provider: its offer and its rows. */
export type CodingSessionModelOffer = {
  allowedModels: readonly string[];
  models?: readonly CodingSessionModelRow[];
};

/** One selection, split into the decisions it encodes. */
export type CodingSessionModelSelection = {
  /** Base model, context window and effort folded out. */
  model: string;
  context: string | null;
  /** Effort token, or null for the runtime's default. */
  effort: string | null;
  fast: boolean;
};

export function codingSessionModelRowsById(
  offer: CodingSessionModelOffer,
): Map<string, CodingSessionModelRow> {
  return new Map((offer.models ?? []).map((row) => [row.id, row]));
}

/** Every effort token any row names — the brackets splitting must recognise. */
export function codingSessionRowEfforts(
  offer: CodingSessionModelOffer,
): Set<string> {
  const efforts = new Set<string>();
  for (const row of offer.models ?? []) {
    for (const effort of row.efforts ?? []) {
      if (
        effort !== CODING_SESSION_DEFAULT_EFFORT &&
        effort !== CODING_SESSION_FAST_TOKEN
      ) {
        efforts.add(effort);
      }
    }
  }
  return efforts;
}

/** Folded choices that also recognise the runtime's own effort values. */
export function codingSessionOfferChoices(
  offer: CodingSessionModelOffer,
): CodingSessionModelChoices {
  return codingSessionModelChoices(
    offer.allowedModels,
    codingSessionRowEfforts(offer),
  );
}

function anyRowHasFastMode(offer: CodingSessionModelOffer): boolean {
  return (offer.models ?? []).some((row) => row.fastMode === true);
}

/** Split a selection string: `[fast]` last, then effort, then context. */
export function splitCodingSessionModelSelection(
  selection: string,
  offer: CodingSessionModelOffer,
): CodingSessionModelSelection {
  let rest = selection;
  let fast = false;
  // Only a provider whose rows offer fast mode can mean it; anywhere else a
  // `[fast]` suffix stays part of the id, byte for byte.
  if (
    anyRowHasFastMode(offer) &&
    rest.length > FAST_SUFFIX.length &&
    rest.endsWith(FAST_SUFFIX)
  ) {
    fast = true;
    rest = rest.slice(0, -FAST_SUFFIX.length);
  }
  const split = splitCodingSessionModelId(rest, codingSessionRowEfforts(offer));
  return {
    model: split.model,
    context: split.context,
    effort: split.thinking,
    fast,
  };
}

/** The inverse: `<base>[<context>][<effort>][fast]`. */
export function joinCodingSessionModelSelection(
  selection: CodingSessionModelSelection,
): string {
  const effort =
    selection.effort === CODING_SESSION_DEFAULT_EFFORT
      ? null
      : selection.effort;
  const id = joinCodingSessionModelId(
    selection.model,
    effort,
    selection.context,
  );
  return selection.fast ? `${id}${FAST_SUFFIX}` : id;
}

/**
 * The row whose efforts and fast mode apply to a base model and window.
 *
 * Exact id only: `opus`'s efforts are not assumed to hold for `opus[1m]`,
 * because the provider validates against the row for the id it is given.
 */
export function codingSessionModelOptionsRow(
  offer: CodingSessionModelOffer,
  model: string,
  context: string | null,
): CodingSessionModelRow | null {
  const id = joinCodingSessionModelId(model, null, context);
  return codingSessionModelRowsById(offer).get(id) ?? null;
}

/** What the options control offers for the selected model. */
export type CodingSessionModelOptions = {
  /**
   * `runtime` when the row reports efforts (the runtime's own list);
   * `variants` when levels are inferred from bracketed ids only.
   */
  source: "runtime" | "variants";
  /**
   * Selectable efforts, adapter order. For a `runtime` source this keeps the
   * runtime's own `default` value where the runtime placed it: selecting it
   * sends no token.
   */
  levels: string[];
  /**
   * How "no token" is offered: `named` when the runtime lists `default` among
   * its efforts (its own word, shown as "Default" in its own position),
   * `unnamed` when omitting the token is valid but the runtime named no
   * default, `none` when a level must be chosen.
   */
  defaultOption: "named" | "unnamed" | "none";
  /** The row reports a fast-mode switch for this id. */
  fastMode: boolean;
};

export function codingSessionModelOptions(
  offer: CodingSessionModelOffer,
  model: string,
  context: string | null,
): CodingSessionModelOptions {
  const choices = codingSessionOfferChoices(offer);
  const variantLevels = choices.thinkingByModel.get(model) ?? [];
  const row = codingSessionModelOptionsRow(offer, model, context);
  const fastMode = row?.fastMode === true;
  if (row?.efforts !== undefined) {
    const levels = [...row.efforts];
    // A published variant the row does not list stays reachable here: the
    // model list no longer shows it, so this is the only way to pick it.
    for (const level of variantLevels) {
      if (!levels.includes(level)) levels.push(level);
    }
    return {
      source: "runtime",
      levels,
      // The row's id is itself in `allowedModels`, so the bare form is valid.
      defaultOption: row.efforts.includes(CODING_SESSION_DEFAULT_EFFORT)
        ? "named"
        : "unnamed",
      fastMode,
    };
  }
  return {
    source: "variants",
    levels: [...variantLevels],
    defaultOption: choices.bareModels.has(model) ? "unnamed" : "none",
    fastMode,
  };
}

/**
 * The selection after the person picks a (possibly different) model.
 *
 * Carries the previous effort, window and fast mode over only where the new
 * model offers them; anything it does not offer is reset, never smuggled
 * through into an id the provider would refuse.
 */
export function resolveCodingSessionModelPick(input: {
  offer: CodingSessionModelOffer;
  model: string;
  previous: Pick<CodingSessionModelSelection, "context" | "effort" | "fast">;
}): string {
  const choices = codingSessionOfferChoices(input.offer);
  const context = resolveCodingSessionContext(
    choices,
    input.model,
    input.previous.context,
  );
  const options = codingSessionModelOptions(input.offer, input.model, context);
  const effort =
    options.source === "runtime"
      ? input.previous.effort !== null &&
        input.previous.effort !== CODING_SESSION_DEFAULT_EFFORT &&
        options.levels.includes(input.previous.effort)
        ? input.previous.effort
        : null
      : resolveCodingSessionThinking(
          choices,
          input.model,
          input.previous.effort,
        );
  return joinCodingSessionModelSelection({
    model: input.model,
    context,
    effort,
    fast: input.previous.fast && options.fastMode,
  });
}

/**
 * Whether a selection string is one this offer accepts.
 *
 * Exactly an `allowedModels` id, or `<id>[<effort>][fast]` where `<id>` is
 * offered, the effort is one its row names (or `<id>[<effort>]` is itself a
 * published variant), and `[fast]` appears only when its row has fast mode.
 */
export function isCodingSessionModelSelectionOffered(
  offer: CodingSessionModelOffer,
  selection: string,
): boolean {
  if (offer.allowedModels.includes(selection)) return true;
  const rows = codingSessionModelRowsById(offer);
  let rest = selection;
  let fast = false;
  if (rest.endsWith(FAST_SUFFIX)) {
    fast = true;
    rest = rest.slice(0, -FAST_SUFFIX.length);
  }
  const accepts = (id: string, effort: string | null): boolean => {
    if (!offer.allowedModels.includes(id)) return false;
    const row = rows.get(id);
    if (fast && row?.fastMode !== true) return false;
    if (effort === null) return true;
    if (
      effort === CODING_SESSION_DEFAULT_EFFORT ||
      effort === CODING_SESSION_FAST_TOKEN
    ) {
      return false;
    }
    return (
      (row?.efforts?.includes(effort) ?? false) ||
      offer.allowedModels.includes(`${id}[${effort}]`)
    );
  };
  if (fast && accepts(rest, null)) return true;
  const open = rest.lastIndexOf("[");
  if (open <= 0 || !rest.endsWith("]")) return false;
  return accepts(rest.slice(0, open), rest.slice(open + 1, -1));
}

/** What the model list shows for one base model: name, line, and order. */
export type CodingSessionModelDetail = {
  name: string | null;
  description: string | null;
  rank: number | null;
};

/**
 * The base models a provider's list shows, in the runtime's own order, with
 * the runtime's name for each.
 *
 * Ranked models sort by rank; unranked ones follow in today's order (sorted
 * `allowedModels`, the `default` alias last). A base model borrows the facts
 * of its own row, or of the first variant row that folds into it.
 */
export function codingSessionProviderPickerModels(
  offer: CodingSessionModelOffer,
): { models: string[]; details: Map<string, CodingSessionModelDetail> } {
  const choices = codingSessionOfferChoices(offer);
  const extras = codingSessionRowEfforts(offer);
  const details = new Map<string, CodingSessionModelDetail>();
  for (const row of offer.models ?? []) {
    const base = splitCodingSessionModelId(row.id, extras).model;
    // The base's own row wins; otherwise the first variant row that folds in.
    if (details.has(base) && row.id !== base) continue;
    details.set(base, {
      name: row.name ?? null,
      description: row.description ?? null,
      rank: row.rank ?? null,
    });
  }
  const indexed = choices.models.map((model, index) => ({
    model,
    index,
    rank: details.get(model)?.rank ?? null,
  }));
  indexed.sort((left, right) => {
    if (left.rank !== null && right.rank !== null) {
      return left.rank - right.rank || left.index - right.index;
    }
    if (left.rank !== null) return -1;
    if (right.rank !== null) return 1;
    return left.index - right.index;
  });
  return { models: indexed.map((entry) => entry.model), details };
}
