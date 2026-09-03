/**
 * The shared model registry: its shape on disk, and the reader that refuses
 * anything it cannot vouch for.
 *
 * `team/model-registry.yaml` is the team's own artefact — Brian's operational
 * priors for every execution target, the class gates of the routing spec §4,
 * and the risk bands. It is split out of the router
 * (`codingSessionRouting.ts`) because reading a file and choosing a target are
 * two jobs, and only one of them has an opinion.
 *
 * Two rules the reader holds, both from spec §10 and §1:
 *
 * - **Every no-answer is an answer.** A registry that will not parse, or that
 *   is a version this build does not understand, comes back as `why` — never
 *   as a partially-read registry and never as a built-in fallback. A host that
 *   routed on a copy compiled into the app would be making decisions nobody
 *   could check against the file the team edits.
 * - **`null` is "no prior", never "average".** A trait a row has no number for
 *   stays null all the way through, and the router excludes the row from any
 *   class that gates on it rather than inventing a middle score.
 *
 * The registry does not get to redefine the effort ladder: spec §2 fixes
 * FAST/STANDARD/DEEP to low/medium/high, and a file claiming otherwise is
 * refused rather than obeyed.
 */
import { parse as parseYaml } from "yaml";

/** The ten scored traits, in the spec's own order (§1). */
export const ROUTING_TRAITS = [
  "reasoning",
  "coding",
  "taste",
  "judgment",
  "agency",
  "discipline",
  "context",
  "verification",
  "velocity",
  "costEfficiency",
] as const;

export type RoutingTrait = (typeof ROUTING_TRAITS)[number];

/** The three risk bands. Marked `seed_policy` in the spec, not truth. */
export type RoutingTier = "fast" | "standard" | "deep";

/**
 * The three efforts the router may buy.
 *
 * `xhigh`, `max`, `ultra` and every equivalent are human-override only until
 * our own telemetry shows they buy something (spec §2).
 */
export type RoutingEffort = "low" | "medium" | "high";

/** Spec §2, as a table: the only automatic effort purchases there are. */
export const ROUTING_EFFORT_FOR_TIER: Readonly<
  Record<RoutingTier, RoutingEffort>
> = Object.freeze({ fast: "low", standard: "medium", deep: "high" });

/** Risk inputs, 1–5 each. */
export type RoutingRisk = {
  impact: number;
  uncertainty: number;
  irreversibility: number;
};

/** A registry class: its gates, and what it factually requires. */
export type RegistryClass = {
  minimums: Partial<Record<RoutingTrait, number>>;
  requires?: {
    multimodal?: boolean;
    tools?: readonly string[];
  };
  /** This class must not share a vendor with that one, where possible. */
  crossProviderOf?: string;
  /** True for a class this repo drafted rather than one Brian ruled on. */
  laneDrafted?: boolean;
  /**
   * The day a bench first existed for this class, `YYYY-MM-DD`.
   *
   * From that day a legacy row has `LEGACY_ROW_GRACE_DAYS` days, disclosed on
   * every routing line, and then it is refused with the word `unmeasured` —
   * the same rule the Rust router applies, so the two cannot disagree.
   */
  benchAvailableSince?: string;
};

/** Hard constraints a single target carries, beyond its class's. */
export type RegistryTargetConstraints = {
  ambiguityMax?: number;
  irreversibilityMax?: number;
  scope?: string;
};

/** The non-scored, factual half of a row. Kept apart from the scores (§1). */
export type RegistryTargetFacts = {
  multimodal?: boolean | null;
  contextWindow?: number | null;
  tools?: readonly string[] | null;
  priceUsdPerM?: { input: number; output: number } | null;
  quotaClass?: string | null;
  knownFailureModes: readonly string[];
  constraints?: RegistryTargetConstraints;
};

/** Where a row's numbers came from. Every row carries one (§10). */
export type RegistryRating = {
  status: string;
  confidence: string;
  author: string;
  date: string;
};

/** One trait a bench actually measured, with the spread behind it. */
export type RegistryMeasuredTrait = {
  score: number;
  n: number;
  min: number;
  max: number;
};

/**
 * What a bench measured about one row, when one has.
 *
 * Absent on every row this repository has ever shipped, and a row without it
 * routes exactly as it did before this key existed. The only thing that
 * changes is that the app now says the word `legacy` out loud instead of
 * letting ten priors read as numbers somebody sampled — live run 3, finding
 * 25.
 *
 * Traits the bench did not evidence are **absent** from `traits` and stay
 * opinions in `scores`. A half-measured row reading as measured is the same
 * lie as a badge with no event behind it.
 */
export type RegistryMeasured = {
  role: string;
  benchVersion: number;
  benchHash: string;
  measuredAt: string;
  measuredBy: string;
  runs: readonly string[];
  traits: Readonly<Record<string, RegistryMeasuredTrait>>;
};

/** The two words a row's numbers may be described by, and no third. */
export type RegistryScoreProvenance = "measured" | "legacy";

/** One registry row: an execution target minus its effort. */
export type RegistryTarget = {
  provider: string;
  model: string;
  /** 1–5 operational priors. `null` is "no prior", never "average". */
  scores: Partial<Record<RoutingTrait, number | null>>;
  facts: RegistryTargetFacts;
  /**
   * Incumbency per class, in either of the two spellings the ruling uses:
   * a tier-qualified key (`builder-fast: incumbent`) or a tier-qualified
   * value (`builder: incumbent-deep`).
   */
  status: Readonly<Record<string, string>>;
  rating: RegistryRating;
  /** Present only once a bench has measured this row. */
  measured?: RegistryMeasured;
};

/**
 * `measured` or `legacy` — where a row's ten numbers came from.
 *
 * The same two words the Rust router prints, deliberately: a reader who sees
 * `legacy` in the app and something else in `bee sessions route` is reading
 * two different claims about one row.
 */
export function codingSessionScoreProvenance(
  target: Pick<RegistryTarget, "measured">,
): RegistryScoreProvenance {
  return target.measured === undefined ? "legacy" : "measured";
}

/** Days a legacy row keeps routing after a bench exists for its class. */
export const LEGACY_ROW_GRACE_DAYS = 30;

/** Whole days between two `YYYY-MM-DD` dates, or null if either will not parse. */
export function codingSessionDaysBetween(
  from: string,
  to: string,
): number | null {
  const start = Date.parse(`${from}T00:00:00Z`);
  const end = Date.parse(`${to}T00:00:00Z`);
  if (Number.isNaN(start) || Number.isNaN(end)) return null;
  return Math.round((end - start) / 86_400_000);
}

/**
 * Days left on a legacy row's grace, or null when no clock is running.
 *
 * A date this reader cannot parse is treated as no date rather than as an
 * expiry: a typo in the registry must never silently retire a row.
 */
export function codingSessionGraceDaysLeft(
  classGate: RegistryClass,
  today: string | null,
): number | null {
  const since = classGate.benchAvailableSince;
  if (since === undefined || today === null) return null;
  const elapsed = codingSessionDaysBetween(since, today);
  return elapsed === null ? null : LEGACY_ROW_GRACE_DAYS - elapsed;
}

/**
 * The trait with the least room above its class minimum — the one the gate
 * actually turns on, and therefore the one whose spread a reader needs.
 */
function bindingMeasuredTrait(
  measured: RegistryMeasured,
  minimums: Partial<Record<RoutingTrait, number>>,
): { name: string; value: RegistryMeasuredTrait } | null {
  let best: {
    name: string;
    value: RegistryMeasuredTrait;
    margin: number;
  } | null = null;
  for (const [name, minimum] of Object.entries(minimums)) {
    const value = measured.traits[name];
    if (value === undefined || minimum === undefined) continue;
    const margin = value.score - minimum;
    if (best === null || margin < best.margin) best = { name, value, margin };
  }
  return best === null ? null : { name: best.name, value: best.value };
}

/**
 * The clause the hire host appends to its routing line, in the router's own
 * words — including its numbers.
 *
 * The words used to match the Rust router and the *clauses* did not: this
 * dropped `n`, the spread and the binding trait, and knew nothing of the
 * thirty-day grace, so once any class carried `benchAvailableSince` the app
 * and the CLI gave opposite answers about the same row.
 */
export function codingSessionProvenanceClause(
  target: RegistryTarget,
  classGate?: RegistryClass,
  today?: string | null,
): string {
  const measured = target.measured;
  if (measured === undefined) {
    const head =
      `scores are operational priors, not measurements (rating: ` +
      `${target.rating.status}, confidence ${target.rating.confidence}, ` +
      `${target.rating.author} ${target.rating.date})`;
    const left =
      classGate === undefined
        ? null
        : codingSessionGraceDaysLeft(classGate, today ?? null);
    return left === null
      ? `${head} — this row is legacy`
      : `${head} — legacy row · bench available · ${String(left)} days left`;
  }
  const binding =
    classGate === undefined
      ? null
      : bindingMeasuredTrait(measured, classGate.minimums);
  const detail =
    binding === null
      ? `n=${String(measuredSamples(measured))}`
      : `n=${String(binding.value.n)}, spread ${String(binding.value.min)}–` +
        `${String(binding.value.max)} on the binding trait ${binding.name}`;
  return (
    `scores measured by registry-bench/${measured.role} ` +
    `v${String(measured.benchVersion)} on ${measured.measuredAt} (${detail})`
  );
}

/** A block is only as measured as its thinnest trait. */
export function measuredSamples(measured: RegistryMeasured): number {
  const counts = Object.values(measured.traits).map((trait) => trait.n);
  return counts.length === 0 ? 0 : Math.min(...counts);
}

/**
 * The traits on this row that are still opinions, in the registry's own trait
 * order.
 *
 * Never elided: a row measured on three of ten traits is three measurements
 * and seven guesses, and both halves get said.
 */
export function codingSessionOpinionTraits(
  target: RegistryTarget,
): readonly RoutingTrait[] {
  const measured = target.measured;
  if (measured === undefined) return ROUTING_TRAITS;
  return ROUTING_TRAITS.filter(
    (trait) => !Object.hasOwn(measured.traits, trait),
  );
}

/** The whole registry file. */
export type ModelRegistry = {
  version: number;
  updatedAt: string | null;
  traits: readonly RoutingTrait[];
  tiers: Readonly<
    Record<
      RoutingTier,
      { risk: readonly [number, number]; effort: RoutingEffort }
    >
  >;
  classes: Readonly<Record<string, RegistryClass>>;
  targets: readonly RegistryTarget[];
};

/** The version of the registry contract this build understands. */
export const MODEL_REGISTRY_VERSION = 1;

/** `impact × uncertainty × irreversibility`, the spec's §5 product. */
export function codingSessionRiskScore(risk: RoutingRisk): number {
  return risk.impact * risk.uncertainty * risk.irreversibility;
}

/** The band a score falls in, or null when it falls outside every band. */
export function codingSessionRiskTier(
  registry: ModelRegistry,
  score: number,
): RoutingTier | null {
  for (const tier of ["fast", "standard", "deep"] as const) {
    const band = registry.tiers[tier];
    if (band && score >= band.risk[0] && score <= band.risk[1]) return tier;
  }
  return null;
}

/**
 * Read the shared registry file.
 *
 * Total and explicit: anything it cannot read comes back as `why`, because a
 * host that fell back to a built-in copy would be routing on numbers nobody
 * on this machine can see. A version this build does not understand is
 * refused for the same reason.
 */
export function parseModelRegistry(
  text: string,
): { ok: true; registry: ModelRegistry } | { ok: false; why: string } {
  let document: unknown;
  try {
    document = parseYaml(text);
  } catch (error) {
    return {
      ok: false,
      why: `the registry is not valid YAML: ${
        error instanceof Error ? error.message : String(error)
      }`,
    };
  }
  if (!isRecord(document)) {
    return { ok: false, why: "the registry is not a YAML mapping" };
  }
  if (document.version !== MODEL_REGISTRY_VERSION) {
    return {
      ok: false,
      why:
        `the registry is version ${String(document.version)}; this build ` +
        `reads version ${MODEL_REGISTRY_VERSION}`,
    };
  }
  const tiers = readTiers(document.tiers);
  if (tiers === null) return { ok: false, why: "the registry has no tiers" };
  const classes = readClasses(document.classes);
  if (classes === null)
    return { ok: false, why: "the registry has no classes" };
  const targets = readTargets(document.targets);
  if (targets === null || targets.length === 0) {
    return { ok: false, why: "the registry lists no execution targets" };
  }
  return {
    ok: true,
    registry: {
      version: MODEL_REGISTRY_VERSION,
      updatedAt:
        typeof document.updatedAt === "string" ? document.updatedAt : null,
      traits: ROUTING_TRAITS,
      tiers,
      classes,
      targets,
    },
  };
}

function readTiers(value: unknown): ModelRegistry["tiers"] | null {
  if (!isRecord(value)) return null;
  const bands: Partial<
    Record<
      RoutingTier,
      { risk: readonly [number, number]; effort: RoutingEffort }
    >
  > = {};
  for (const tier of ["fast", "standard", "deep"] as const) {
    const row = value[tier];
    if (
      !isRecord(row) ||
      !Array.isArray(row.risk) ||
      row.risk.length !== 2 ||
      typeof row.risk[0] !== "number" ||
      typeof row.risk[1] !== "number" ||
      // The registry does not get to redefine the effort ladder: spec §2 fixes
      // FAST/STANDARD/DEEP to low/medium/high, and a file claiming otherwise
      // is refused rather than obeyed.
      row.effort !== ROUTING_EFFORT_FOR_TIER[tier]
    ) {
      return null;
    }
    bands[tier] = {
      risk: [row.risk[0], row.risk[1]] as const,
      effort: ROUTING_EFFORT_FOR_TIER[tier],
    };
  }
  const { fast, standard, deep } = bands;
  if (fast === undefined || standard === undefined || deep === undefined) {
    return null;
  }
  return { fast, standard, deep };
}

function readClasses(value: unknown): ModelRegistry["classes"] | null {
  if (!isRecord(value)) return null;
  const classes: Record<string, RegistryClass> = {};
  for (const [name, row] of Object.entries(value)) {
    if (!isRecord(row)) continue;
    const requires = isRecord(row.requires) ? row.requires : null;
    classes[name] = {
      minimums: readMinimums(row.minimums),
      ...(requires === null
        ? {}
        : {
            requires: {
              ...(typeof requires.multimodal === "boolean"
                ? { multimodal: requires.multimodal }
                : {}),
              ...(Array.isArray(requires.tools)
                ? { tools: requires.tools.filter(isNonEmptyString) }
                : {}),
            },
          }),
      ...(typeof row.crossProviderOf === "string"
        ? { crossProviderOf: row.crossProviderOf }
        : {}),
      ...(row.laneDrafted === true ? { laneDrafted: true } : {}),
      ...(isNonEmptyString(row.benchAvailableSince)
        ? { benchAvailableSince: row.benchAvailableSince }
        : {}),
    };
  }
  return Object.keys(classes).length > 0 ? classes : null;
}

function readMinimums(value: unknown): Partial<Record<RoutingTrait, number>> {
  const minimums: Partial<Record<RoutingTrait, number>> = {};
  if (!isRecord(value)) return minimums;
  for (const trait of ROUTING_TRAITS) {
    const minimum = value[trait];
    if (typeof minimum === "number") minimums[trait] = minimum;
  }
  return minimums;
}

function readTargets(value: unknown): RegistryTarget[] | null {
  if (!Array.isArray(value)) return null;
  const targets: RegistryTarget[] = [];
  for (const row of value) {
    if (
      !isRecord(row) ||
      !isNonEmptyString(row.provider) ||
      !isNonEmptyString(row.model) ||
      !isRecord(row.rating)
    ) {
      return null;
    }
    const scores: Partial<Record<RoutingTrait, number | null>> = {};
    const rawScores = isRecord(row.scores) ? row.scores : {};
    for (const trait of ROUTING_TRAITS) {
      const score = rawScores[trait];
      scores[trait] = typeof score === "number" ? score : null;
    }
    targets.push({
      provider: row.provider,
      model: row.model,
      scores,
      facts: readFacts(row.facts),
      status: readStatus(row.status),
      rating: {
        status: String(row.rating.status ?? ""),
        confidence: String(row.rating.confidence ?? ""),
        author: String(row.rating.author ?? ""),
        date: String(row.rating.date ?? ""),
      },
      ...readMeasured(row.measured),
    });
  }
  return targets;
}

/**
 * Read a row's `measured` block, or read nothing.
 *
 * Strict on purpose: a block missing any of its six required fields, or whose
 * `traits` is not a mapping of four-number rows, is read as **absent** rather
 * than as a partial measurement. `measured` is a claim that a number came out
 * of a run, and a half-read block would let a row make that claim on the
 * strength of a typo.
 */
function readMeasured(
  value: unknown,
): { measured: RegistryMeasured } | Record<string, never> {
  if (!isRecord(value)) return {};
  if (
    !isNonEmptyString(value.role) ||
    typeof value.benchVersion !== "number" ||
    !isNonEmptyString(value.benchHash) ||
    !isNonEmptyString(value.measuredAt) ||
    !isNonEmptyString(value.measuredBy) ||
    !Array.isArray(value.runs) ||
    !isRecord(value.traits)
  ) {
    return {};
  }
  const traits: Record<string, RegistryMeasuredTrait> = {};
  for (const [name, row] of Object.entries(value.traits)) {
    if (
      !isRecord(row) ||
      typeof row.score !== "number" ||
      typeof row.n !== "number" ||
      typeof row.min !== "number" ||
      typeof row.max !== "number"
    ) {
      // One malformed trait does not become a zero and does not become an
      // absent trait with the rest still claiming to be measured.
      return {};
    }
    traits[name] = { score: row.score, n: row.n, min: row.min, max: row.max };
  }
  if (Object.keys(traits).length === 0) return {};
  return {
    measured: {
      role: value.role,
      benchVersion: value.benchVersion,
      benchHash: value.benchHash,
      measuredAt: value.measuredAt,
      measuredBy: value.measuredBy,
      runs: value.runs.filter(isNonEmptyString),
      traits,
    },
  };
}

function readFacts(value: unknown): RegistryTargetFacts {
  const facts: RegistryTargetFacts = { knownFailureModes: [] };
  if (!isRecord(value)) return facts;
  if (typeof value.multimodal === "boolean")
    facts.multimodal = value.multimodal;
  if (typeof value.contextWindow === "number") {
    facts.contextWindow = value.contextWindow;
  }
  if (Array.isArray(value.tools)) {
    facts.tools = value.tools.filter(isNonEmptyString);
  }
  if (
    isRecord(value.priceUsdPerM) &&
    typeof value.priceUsdPerM.input === "number" &&
    typeof value.priceUsdPerM.output === "number"
  ) {
    facts.priceUsdPerM = {
      input: value.priceUsdPerM.input,
      output: value.priceUsdPerM.output,
    };
  }
  if (isNonEmptyString(value.quotaClass)) facts.quotaClass = value.quotaClass;
  if (Array.isArray(value.knownFailureModes)) {
    facts.knownFailureModes = value.knownFailureModes.filter(isNonEmptyString);
  }
  if (isRecord(value.constraints)) {
    const constraints: RegistryTargetConstraints = {};
    if (typeof value.constraints.ambiguityMax === "number") {
      constraints.ambiguityMax = value.constraints.ambiguityMax;
    }
    if (typeof value.constraints.irreversibilityMax === "number") {
      constraints.irreversibilityMax = value.constraints.irreversibilityMax;
    }
    if (isNonEmptyString(value.constraints.scope)) {
      constraints.scope = value.constraints.scope;
    }
    facts.constraints = constraints;
  }
  return facts;
}

function readStatus(value: unknown): Record<string, string> {
  const status: Record<string, string> = {};
  if (!isRecord(value)) return status;
  for (const [key, entry] of Object.entries(value)) {
    if (isNonEmptyString(entry)) status[key] = entry;
  }
  return status;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.trim().length > 0;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
