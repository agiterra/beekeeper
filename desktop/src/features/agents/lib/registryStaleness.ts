/**
 * Does this team's model registry still cover the models on offer?
 *
 * Renamed from `rubricStaleness.ts` on 2026-08-30, when Brian's routing ruling
 * replaced the flat rubric with a registry plus a router. The badge's question
 * changed with it, and the change runs in one direction only:
 *
 * > **Stale means a live offered execution target has no registry row.** A
 * > registry row the catalog does not offer today is *dormant*, and dormant is
 * > not stale (spec §10). The old rubric check counted both directions; this
 * > one counts one, and says how many dormant rows it saw without letting them
 * > colour the badge.
 *
 * The rubric comparison below is kept as-is, because it is still the desktop
 * mirror of `bee sessions rubric check` and the two are pinned to one fixture
 * (`testdata/routing/live-catalog-665076ce.json`). Deleting it here would
 * silently end that agreement.
 *
 * ---
 *
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

/**
 * A registry row, reduced to what the coverage check needs.
 *
 * `provider` is the catalog's `providerInstanceRef`; `model` is the base id
 * the registry names. Effort variants are settings of a base, not different
 * models, so a row for `gpt-5.6-sol` covers `gpt-5.6-sol[high]`.
 */
export type RegistryCoverageRow = {
  provider: string;
  model: string;
};

/** Why the registry badge could say nothing. */
export type RegistryStalenessUnknownReason =
  /** No command on this host can read the registry file. */
  | "registry-unreadable"
  /** A file was read and it is not a registry this build understands. */
  | "registry-unparseable"
  /** No provider has published a catalog here yet. */
  | "no-catalog";

export type RegistryStaleness =
  | {
      state: "unknown";
      reason: RegistryStalenessUnknownReason;
      label: string;
      detail: string;
    }
  | {
      state: "fresh";
      /** Offered ids with a row, and rows with no live offer. */
      covered: number;
      dormant: string[];
      label: string;
      detail: string;
    }
  | {
      state: "stale";
      /** `provider/model` the catalog offers and no registry row covers. */
      unregistered: string[];
      dormant: string[];
      label: string;
      detail: string;
    };

/**
 * Which offered targets have no registry row, and which rows are dormant.
 *
 * Asymmetric on purpose, and the asymmetry *is* spec §10:
 *
 * - **Unregistered is what makes the badge stale.** A live offered target no
 *   row covers is a target the router cannot reason about at all — it will
 *   never be chosen, and nobody is told why.
 * - **Dormant never makes the badge stale.** A row for a model this host does
 *   not offer today is the registry remembering something, which is the
 *   behaviour the ruling asked for. It is counted and named, never scored.
 *
 * The `default` alias is not a model and is skipped, exactly as the rubric
 * check skips it.
 */
export function compareRegistryToCatalog(
  rows: readonly RegistryCoverageRow[],
  offered: readonly RubricOfferedPair[],
): { unregistered: string[]; dormant: string[]; covered: number } {
  const unregistered = new Set<string>();
  let covered = 0;
  for (const pair of offered) {
    if (pair.model.toLowerCase() === DEFAULT_ALIAS) continue;
    const hit = rows.some(
      (row) =>
        providerMatches(row.provider, pair.providerInstanceRef) &&
        baseId(row.model) === baseId(pair.model),
    );
    if (hit) {
      covered += 1;
      continue;
    }
    unregistered.add(pairLabel(pair.providerInstanceRef, baseId(pair.model)));
  }
  const dormant = new Set<string>();
  for (const row of rows) {
    const live = offered.some(
      (pair) =>
        providerMatches(row.provider, pair.providerInstanceRef) &&
        baseId(pair.model) === baseId(row.model),
    );
    if (!live) dormant.add(pairLabel(row.provider, row.model));
  }
  return {
    unregistered: [...unregistered].sort(),
    dormant: [...dormant].sort(),
    covered,
  };
}

/**
 * The badge's whole state.
 *
 * `rows: null` means this host could not read the registry, and
 * `unreadableBecause` carries the reader's own sentence — a missing project,
 * a missing file, a file this build cannot parse. Since ledger 97(C) the
 * desktop *can* read `team/model-registry.yaml`
 * (`features/coding-sessions/lib/codingSessionRegistrySource.ts`), so this is
 * a real failure to report rather than a permanent gap to describe.
 *
 * Every state that read a file names how many rows it read. A badge that said
 * only "no provider catalog" would read identically whether the registry had
 * been opened or not, and a badge that rendered "fresh" on the strength of
 * having compared nothing is the exact defect this whole surface exists to
 * prevent.
 */
export function resolveRegistryStaleness(input: {
  /** Registry rows, or null when the registry could not be read at all. */
  rows: readonly RegistryCoverageRow[] | null;
  /** Why it could not be read. Rendered verbatim when `rows` is null. */
  unreadableBecause?: string | null;
  /** The registry's own version, when one was read. */
  version?: number | null;
  offered: readonly RubricOfferedPair[] | null;
}): RegistryStaleness {
  if (input.rows === null) {
    return {
      state: "unknown",
      reason: "registry-unreadable",
      label: "Registry: unknown (not readable)",
      detail:
        input.unreadableBecause?.trim() ||
        "This app cannot read team/model-registry.yaml from here, so the registry cannot be checked against the catalog.",
    };
  }
  if (input.rows.length === 0) {
    return {
      state: "unknown",
      reason: "registry-unparseable",
      label: "Registry: unknown (no rows)",
      detail:
        "A registry was read and it lists no execution targets, so there is nothing to check the catalog against.",
    };
  }
  const named =
    typeof input.version === "number"
      ? `Registry v${input.version}`
      : "Registry";
  const read = `${named} \u00b7 ${input.rows.length} rows`;
  if (input.offered === null) {
    return {
      state: "unknown",
      reason: "no-catalog",
      label: `${read} \u00b7 no provider catalog yet`,
      detail: `${input.rows.length} registry rows were read here, but no provider has published a kind:44222 catalog yet, so there is nothing to check them against.`,
    };
  }
  const { unregistered, dormant, covered } = compareRegistryToCatalog(
    input.rows,
    input.offered,
  );
  if (unregistered.length === 0) {
    return {
      state: "fresh",
      covered,
      dormant,
      label: `${read} \u00b7 covers the catalog`,
      detail:
        `Every offered execution target has a registry row (${covered} ids).` +
        (dormant.length > 0
          ? ` ${dormant.length} rows are dormant — the registry knows them and this host does not offer them today, which is not staleness: ${dormant.join(", ")}.`
          : ""),
    };
  }
  return {
    state: "stale",
    unregistered,
    dormant,
    label: `${read} \u00b7 ${unregistered.length} offered with no row`,
    detail:
      `Offered here and in no registry row: ${unregistered.join(", ")}.` +
      (dormant.length > 0
        ? ` ${dormant.length} rows are dormant and do not count: ${dormant.join(", ")}.`
        : ""),
  };
}
