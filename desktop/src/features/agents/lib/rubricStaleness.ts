/**
 * Is the lead's model rubric still true about the models on offer?
 *
 * Brian's ruling: the rubric stays — it works — but it has to be checked, and
 * the check has to be visible. A rubric that can go stale without saying so is
 * the same class of bug as a picker that offers a model nobody serves: both
 * hand a reader a comfortable answer that nothing verified.
 *
 * This module is the desktop half of `bee sessions rubric check`. It applies
 * the identical rule — parse the `rubric` block, compare every row against the
 * kind:44222 catalog, report `not offered` and `unassigned` — and it never
 * repairs, never guesses, and never translates a model id onto a neighbouring
 * one that happens to be offered.
 *
 * A bracket suffix is a variant of its base — `gpt-5.6-sol[high]` and
 * `gpt-5.6-sol[max]` are one model at two effort levels — so a row that names
 * any one of them has decided about the model, and the badge does not count the
 * siblings as gaps. Nothing is hidden by that: the ids no row names literally
 * are returned as `variants`, which never sets the badge stale.
 *
 * **Every no-answer is an answer here.** If the rubric cannot be read, or the
 * catalog has not arrived, the badge says which — it never renders nothing and
 * it never renders "fresh" on the strength of having compared nothing.
 */

/** The provider column value meaning "whichever provider offers it". */
const ANY_PROVIDER = "*";

/**
 * The runtime alias every provider publishes for "whatever this host is set
 * to". It is not a model, so no rubric row can assign it and it is never
 * reported as a gap.
 */
const DEFAULT_ALIAS = "default";

/**
 * A model id with its bracket suffix removed: the base model the variant is a
 * setting of.
 *
 * `gpt-5.6-sol[high]` and `gpt-5.6-sol[max]` are one model at two effort
 * levels; `opus[1m]` and `opus` are one model at two context windows. The
 * bracket is a knob, not a different model, so the rubric decides at the base.
 */
function baseId(model: string): string {
  const bracket = model.indexOf("[");
  return bracket < 0 ? model : model.slice(0, bracket);
}

function providerMatches(rowProvider: string, provider: string): boolean {
  return rowProvider === ANY_PROVIDER || rowProvider === provider;
}

/** One `(providerInstanceRef, model)` pair the catalog offers. */
export type RubricOfferedPair = {
  providerInstanceRef: string;
  model: string;
};

/** One parsed rubric row. */
export type RubricRow = {
  tier: string;
  roles: string[];
  /** `providerInstanceRef`, or `*`. */
  provider: string;
  model: string;
  reason: string;
};

/** A parsed rubric block. */
export type ParsedRubric = {
  /** The token after `rubric` on the fence line, when the author wrote one. */
  version: string | null;
  rows: RubricRow[];
};

/**
 * Why nothing could be compared. Each maps to its own badge sentence, because
 * "we could not read the rubric" and "no provider has published a catalog"
 * send an operator to different places.
 */
export type RubricStalenessUnknownReason =
  | "pack-unreadable"
  | "no-rubric-block"
  | "no-catalog";

export type RubricStaleness =
  | {
      state: "unknown";
      reason: RubricStalenessUnknownReason;
      /** Badge text. */
      label: string;
      /** The longer sentence, for a title attribute. */
      detail: string;
    }
  | {
      state: "fresh";
      version: string | null;
      label: string;
      detail: string;
    }
  | {
      state: "stale";
      version: string | null;
      /** `provider/model` the rubric names and the catalog does not offer. */
      notOffered: string[];
      /** `provider/model` the catalog offers and no row names. */
      unassigned: string[];
      label: string;
      detail: string;
    };

/**
 * Parse the fenced `rubric` block out of a skill document.
 *
 * Returns `null` when the document holds no such block — which is a fact about
 * the pack, reported as its own badge state rather than as an empty rubric
 * that would then look perfectly fresh against an empty catalog.
 */
export function parseRubricBlock(document: string): ParsedRubric | null {
  const lines = document.split("\n");
  let index = 0;
  let fence: string | null = null;
  let version: string | null = null;
  for (; index < lines.length; index += 1) {
    const trimmed = lines[index].trimStart();
    const fenceChar = trimmed.startsWith("`")
      ? "`"
      : trimmed.startsWith("~")
        ? "~"
        : null;
    if (!fenceChar) continue;
    let run = 0;
    while (trimmed[run] === fenceChar) run += 1;
    if (run < 3) continue;
    const info = trimmed.slice(run).trim().split(/\s+/);
    if (info[0] !== "rubric") continue;
    fence = fenceChar.repeat(run);
    version = info[1] ?? null;
    index += 1;
    break;
  }
  if (fence === null) return null;

  const rows: RubricRow[] = [];
  let headerSeen = false;
  for (; index < lines.length; index += 1) {
    const trimmed = lines[index].trim();
    if (trimmed.startsWith(fence)) break;
    if (!trimmed.startsWith("|")) continue;
    const cells = splitRow(trimmed);
    if (isSeparator(cells)) continue;
    if (!headerSeen) {
      headerSeen = true;
      continue;
    }
    // A malformed row is skipped rather than thrown: one broken line must not
    // blank the whole badge, and the rows that did parse are still checkable.
    if (cells.length !== 5) continue;
    const provider = unwrapCode(cells[2]);
    const model = unwrapCode(cells[3]);
    if (provider.length === 0 || model.length === 0) continue;
    rows.push({
      tier: cells[0],
      roles: cells[1]
        .split(",")
        .map((role) => unwrapCode(role))
        .filter((role) => role.length > 0),
      provider,
      model,
      reason: cells[4],
    });
  }
  return rows.length > 0 ? { version, rows } : null;
}

function splitRow(line: string): string[] {
  return line
    .replace(/^\|/, "")
    .replace(/\|$/, "")
    .split("|")
    .map((cell) => cell.trim());
}

function isSeparator(cells: readonly string[]): boolean {
  return (
    cells.length > 0 &&
    cells.every((cell) => cell.length > 0 && /^[-: ]+$/.test(cell))
  );
}

function unwrapCode(cell: string): string {
  return cell
    .trim()
    .replace(/^`+|`+$/g, "")
    .trim();
}

/** How the fresh badge admits the ids it covered by base rather than by name. */
function variantNote(variants: readonly string[]): string {
  return variants.length > 0
    ? `, ${variants.length} of the offered ids covered as variants of a named base`
    : "";
}

/** `provider/model`, the label both lists use. */
function pairLabel(providerInstanceRef: string, model: string): string {
  return `${providerInstanceRef}/${model}`;
}

/** What one comparison found. Mirrors the CLI's `RubricCheck`. */
export type RubricComparison = {
  /** `provider/model` the rubric names and the catalog does not offer. */
  notOffered: string[];
  /** One entry per uncovered base, named with an id the catalog offers. */
  unassigned: string[];
  /**
   * Offered ids no row names literally, absorbed by the base rule.
   * Informational — this list never makes a rubric stale.
   */
  variants: string[];
};

/**
 * Compare rubric rows to the catalog's offered pairs.
 *
 * A `*` row matches its id on any provider and covers it on every provider
 * that offers it: "haiku, wherever you find it" has assigned haiku everywhere
 * and is not stale for it.
 *
 * The two directions are deliberately not symmetric, and this must stay
 * identical to `check_rubric` in
 * `crates/buzz-cli/src/commands/sessions/rubric.rs`:
 *
 * - **Not offered is exact.** A create names one id and the relay refuses
 *   anything else, so a row naming an id nothing serves is stale even when a
 *   sibling variant is on offer.
 * - **Unassigned is by base.** A catalog id is covered when any row names its
 *   base or any variant of that base, and the gap is reported once per base.
 *   Without this the badge was permanently stale on a real host — 41 gaps, all
 *   of them effort levels of models the rubric had already ruled on.
 */
export function compareRubricToCatalog(
  rows: readonly RubricRow[],
  offered: readonly RubricOfferedPair[],
): RubricComparison {
  const notOffered = new Set<string>();
  for (const row of rows) {
    const offeredHere = offered.some(
      (pair) =>
        pair.model === row.model &&
        providerMatches(row.provider, pair.providerInstanceRef),
    );
    if (!offeredHere) notOffered.add(pairLabel(row.provider, row.model));
  }

  const uncovered = new Map<string, { provider: string; ids: string[] }>();
  const variants = new Set<string>();
  for (const pair of offered) {
    if (pair.model.toLowerCase() === DEFAULT_ALIAS) continue;
    const covered = rows.some(
      (row) =>
        providerMatches(row.provider, pair.providerInstanceRef) &&
        baseId(row.model) === baseId(pair.model),
    );
    if (!covered) {
      const key = pairLabel(pair.providerInstanceRef, baseId(pair.model));
      const group = uncovered.get(key) ?? {
        provider: pair.providerInstanceRef,
        ids: [],
      };
      if (!group.ids.includes(pair.model)) group.ids.push(pair.model);
      uncovered.set(key, group);
    }
    const namedExactly = rows.some(
      (row) =>
        providerMatches(row.provider, pair.providerInstanceRef) &&
        row.model === pair.model,
    );
    if (!namedExactly)
      variants.add(pairLabel(pair.providerInstanceRef, pair.model));
  }

  // One gap per base, named with an id the catalog really offers: the bare base
  // when it is on offer, otherwise the first variant of it. A bare base nobody
  // serves would send a reader to add a row this same check calls not offered.
  const unassigned = new Set<string>();
  for (const [key, group] of uncovered) {
    const base = key.slice(group.provider.length + 1);
    const sorted = [...group.ids].sort();
    const representative = sorted.includes(base) ? base : (sorted[0] ?? base);
    unassigned.add(pairLabel(group.provider, representative));
  }
  for (const label of unassigned) variants.delete(label);

  return {
    notOffered: [...notOffered].sort(),
    unassigned: [...unassigned].sort(),
    variants: [...variants].sort(),
  };
}

/**
 * The badge's whole state, from the rubric text and the catalog the app holds.
 *
 * `rubricText: null` means the lead pack could not be read at all. `offered:
 * null` means no catalog has arrived yet. Both are reported as themselves —
 * neither is allowed to pass for a fresh rubric, because "nothing to compare"
 * and "compared and matched" are different facts and only one of them is good
 * news.
 */
export function resolveRubricStaleness(input: {
  rubricText: string | null;
  offered: readonly RubricOfferedPair[] | null;
}): RubricStaleness {
  if (input.rubricText === null) {
    return {
      state: "unknown",
      reason: "pack-unreadable",
      label: "Rubric: unknown (pack not readable)",
      detail:
        "This app cannot read the lead role pack from here, so the model rubric cannot be checked against the catalog. Run `bee sessions rubric check --channel <uuid>` in the checkout to see it.",
    };
  }
  const rubric = parseRubricBlock(input.rubricText);
  if (!rubric) {
    return {
      state: "unknown",
      reason: "no-rubric-block",
      label: "Rubric: unknown (no rubric block)",
      detail:
        "The lead role pack holds no fenced `rubric` block, so there are no rows to check against the catalog.",
    };
  }
  if (input.offered === null) {
    return {
      state: "unknown",
      reason: "no-catalog",
      label: "Rubric: unknown (no provider catalog)",
      detail:
        "No provider has published a kind:44222 catalog here yet, so there is nothing to check the rubric against.",
    };
  }
  const { notOffered, unassigned, variants } = compareRubricToCatalog(
    rubric.rows,
    input.offered,
  );
  const version = rubric.version;
  if (notOffered.length === 0 && unassigned.length === 0) {
    return {
      state: "fresh",
      version,
      label: version
        ? `Rubric ${version} matches the catalog`
        : "Rubric matches the catalog",
      detail: `Every rubric row names a model the catalog offers, and every offered model is covered by a row (${rubric.rows.length} rows${variantNote(variants)}).`,
    };
  }
  return {
    state: "stale",
    version,
    notOffered,
    unassigned,
    label: `Rubric stale — ${notOffered.length} not offered · ${unassigned.length} unassigned`,
    detail: [
      notOffered.length > 0
        ? `Not offered: ${notOffered.join(", ")}.`
        : "Not offered: none.",
      unassigned.length > 0
        ? `Unassigned: ${unassigned.join(", ")}.`
        : "Unassigned: none.",
      variants.length > 0
        ? `${variants.length} further ids are variants of a model a row already names, and are not counted here.`
        : "",
    ]
      .filter((sentence) => sentence.length > 0)
      .join(" "),
  };
}
