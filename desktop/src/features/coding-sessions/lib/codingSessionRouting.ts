/**
 * The router: which execution target runs a seat, and why.
 *
 * Brian's ruling of 2026-08-30 (`review-2026-08-30/routing-spec.md`) replaced
 * the flat rubric of item 94 with a pipeline, and the two sentences it turns
 * on are quoted here because they are the thing the code has to keep true:
 *
 * > "The lead chooses the capability required. The router chooses the
 * > execution target."
 *
 * > "Select the least expensive execution target whose expected failure mode
 * > is acceptable for the task."
 *
 * So: **the lead never names a model.** It names a class, a risk, and — when
 * it knows something the class does not — extra trait minimums. This module
 * turns that into an execution target:
 *
 * 1. live catalog — the kind:44222 offer, which is the only model list;
 * 2. hard requirements — modality, tools, context window, known failure modes;
 * 3. class gates — the numeric trait minimums of spec §4;
 * 4. risk tier — `impact × uncertainty × irreversibility`, banded FAST /
 *    STANDARD / DEEP, which fixes the effort at low / medium / high;
 * 5. eligible execution targets;
 * 6. the **cheapest expected accepted completion** among them.
 *
 * The order is load-bearing and the spec says so twice: cost and speed choose
 * only among targets that have *already* cleared every gate. There is no
 * weighted product anywhere in this file — nothing multiplies a capability by
 * a price, and a velocity of 5.0 can never buy back a coding score of 3.7.
 *
 * Three further rules the spec states and this file implements literally:
 *
 * - **Effort is bought three ways and no more.** FAST → low, STANDARD →
 *   medium, DEEP → high. `xhigh`, `max` and `ultra` are human-override only,
 *   and a failed attempt never escalates effort on the same target — that is
 *   a different target's job, or a reviewer's.
 * - **Dormant is not stale.** A registry row the live catalog does not offer
 *   today is simply not a candidate today. Staleness is the other direction:
 *   an offered target with no registry row (see `registryStaleness.ts`).
 * - **Nothing is ever quietly weakened.** A class minimum a target has no
 *   score for excludes it; a required tool nobody has recorded excludes it; a
 *   catalog that offers nothing eligible produces `HIRE_NO_ROUTE` with the
 *   reason, never a "closest match".
 *
 * Pure. It reads a registry, a catalog and a request, and returns a decision
 * plus the exclusions that produced it, so every routing choice can be
 * explained from the wire rather than asserted.
 */
import {
  MODEL_REGISTRY_VERSION,
  ROUTING_EFFORT_FOR_TIER,
  ROUTING_TRAITS,
  codingSessionRiskScore,
  codingSessionRiskTier,
  parseModelRegistry,
  type ModelRegistry,
  type RegistryClass,
  type RegistryTarget,
  type RoutingEffort,
  type RoutingRisk,
  type RoutingTier,
  type RoutingTrait,
} from "./codingSessionModelRegistry";

// Re-exported so the router stays the one import a caller needs: the registry
// module is a reader, not a second public surface.
export {
  MODEL_REGISTRY_VERSION,
  ROUTING_EFFORT_FOR_TIER,
  ROUTING_TRAITS,
  codingSessionRiskScore,
  codingSessionRiskTier,
  parseModelRegistry,
};
export type {
  ModelRegistry,
  RegistryClass,
  RegistryTarget,
  RoutingEffort,
  RoutingRisk,
  RoutingTier,
  RoutingTrait,
};

/** One `(providerInstanceRef, model)` pair the live catalog offers. */
export type RoutingCatalogEntry = {
  providerInstanceRef: string;
  model: string;
  /** The catalog's own per-model figure, when it published one. */
  contextWindow?: number | null;
};

/** The routing record's `chosen` / `runnerUp` shape — the execution target. */
export type RoutedExecutionTarget = {
  provider: string;
  /** The catalog id, exactly as published. Never an alias, never a family. */
  model: string;
  effort: RoutingEffort;
};

/** A human's override of the router, recorded rather than hidden. */
export type RoutingOverride = {
  model: string;
  effort?: RoutingEffort;
  because: string;
};

/** The `routing` object carried on the hire, the create, and the metadata. */
export type CodingSessionRoutingRecord = {
  class: string;
  tier: RoutingTier;
  risk: RoutingRisk & { score: number };
  /** Extra trait minimums the lead asked for, when it asked for any. */
  profile?: Partial<Record<RoutingTrait, number>>;
  chosen: RoutedExecutionTarget;
  runnerUp: RoutedExecutionTarget | null;
  /** One sentence: the gates cleared, and why this was the cheapest. */
  reason: string;
  reviewRequired: boolean;
  reviewReasons: readonly string[];
  challengerSample: boolean;
  override: RoutingOverride | null;
  registryVersion: number;
  /** The 44222 revision the choice was made against, or null when unknown. */
  catalogRevision: number | null;
};

/** Spec §6's trigger list, spelled once. */
export const ROUTING_REVIEW_TRIGGERS = [
  "risk>=40",
  "irreversibility>=4",
  "security-auth-data-boundary",
  "architecture-schema-public-contract",
  "builder-outside-plan",
  "builder-reports-uncertainty",
  "tests-cannot-verify",
  "lead-requested",
] as const;

export type RoutingReviewTrigger = (typeof ROUTING_REVIEW_TRIGGERS)[number];

/** The §6 triggers a caller can assert; the two numeric ones are computed. */
export type RoutingReviewFlags = {
  securityBoundary?: boolean;
  publicContract?: boolean;
  builderOutsidePlan?: boolean;
  builderReportsUncertainty?: boolean;
  testsCannotVerify?: boolean;
  leadRequested?: boolean;
};

/** What the task itself requires, beyond what its class requires. */
export type RoutingRequirements = {
  multimodal?: boolean;
  tools?: readonly string[];
  minContextWindow?: number;
  /** Failure modes this task cannot tolerate; matched against the row's. */
  incompatibleFailureModes?: readonly string[];
  /** 1–5. Compared against a target's own `ambiguityMax`. */
  ambiguity?: number;
  /** `bounded` or anything else; compared against a target's `scope`. */
  scope?: string;
};

export type RouteCodingSessionInput = {
  registry: ModelRegistry;
  catalog: readonly RoutingCatalogEntry[];
  /** The 44222 revision behind `catalog`, or null when the host read none. */
  catalogRevision: number | null;
  className: string;
  risk: RoutingRisk;
  /** Extra trait minimums from the lead. Applied on top of the class gates. */
  profile?: Partial<Record<RoutingTrait, number>>;
  requirements?: RoutingRequirements;
  /** The seat this one is a cross-provider counterpart of, when there is one. */
  peer?: { className: string; provider: string } | null;
  /** The lead's by-rule challenger sample. Marked on the record when it fires. */
  sampleChallenger?: boolean;
  /** A human naming a target by hand. Validated, never trusted blindly. */
  override?: RoutingOverride | null;
  review?: RoutingReviewFlags;
};

/** Why one registry row is not a candidate. */
export type RoutingExclusion = {
  provider: string;
  model: string;
  why: string;
};

/** A candidate that cleared every gate, with the cost that ranked it. */
export type RoutingCandidate = {
  target: RoutedExecutionTarget;
  incumbent: boolean;
  /** Expected cost of an accepted completion. Lower is chosen. */
  expectedCost: number | null;
};

export type RouteCodingSessionDecision =
  | {
      ok: true;
      record: CodingSessionRoutingRecord;
      candidates: readonly RoutingCandidate[];
      excluded: readonly RoutingExclusion[];
    }
  | {
      ok: false;
      code: "HIRE_NO_ROUTE";
      reason: string;
      excluded: readonly RoutingExclusion[];
    };

/** The refusal code a hire with no eligible execution target earns. */
export const HIRE_NO_ROUTE = "HIRE_NO_ROUTE" as const;

/**
 * Choose the execution target, or refuse.
 *
 * Follows spec §7 step for step. Every exclusion is returned alongside the
 * decision, so the surface can say *why* a target the operator expected was
 * not chosen without the router having to guess what they expected.
 */
export function routeCodingSession(
  input: RouteCodingSessionInput,
): RouteCodingSessionDecision {
  const { registry, className } = input;
  const score = codingSessionRiskScore(input.risk);
  const tier = codingSessionRiskTier(registry, score);
  if (tier === null) {
    return {
      ok: false,
      code: HIRE_NO_ROUTE,
      reason:
        `risk ${score} (impact ${input.risk.impact} × uncertainty ` +
        `${input.risk.uncertainty} × irreversibility ` +
        `${input.risk.irreversibility}) falls outside every tier band; each ` +
        "factor must be 1–5.",
      excluded: [],
    };
  }
  const effort = ROUTING_EFFORT_FOR_TIER[tier];
  const classGate = registry.classes[className];
  if (classGate === undefined) {
    return {
      ok: false,
      code: HIRE_NO_ROUTE,
      reason: `the registry defines no ${className} class, so nothing can clear its gates.`,
      excluded: [],
    };
  }

  const excluded: RoutingExclusion[] = [];
  const candidates: RoutingCandidate[] = [];
  const tierNamesItsOwn = tierHasQualifiedIncumbent(registry, className, tier);
  for (const target of registry.targets) {
    const model = catalogModelId(input.catalog, target, effort);
    if (model === null) {
      excluded.push({
        provider: target.provider,
        model: target.model,
        why: `not offered by ${target.provider} in the live catalog (dormant, not stale)`,
      });
      continue;
    }
    const refusal = disqualify({
      target,
      classGate,
      className,
      profile: input.profile,
      requirements: input.requirements,
      risk: input.risk,
      catalogContextWindow: contextWindowOf(input.catalog, target, model),
    });
    if (refusal !== null) {
      excluded.push({
        provider: target.provider,
        model: target.model,
        why: refusal,
      });
      continue;
    }
    candidates.push({
      target: { provider: target.provider, model, effort },
      incumbent: isIncumbent(target, className, tier, tierNamesItsOwn),
      expectedCost: expectedCostOf(target, effort),
    });
  }

  // Cross-provider diversity (§4 VERIFIER): a hard filter, but only when it
  // leaves somebody standing. "Prefer a different failure mode" must never
  // become "refuse the work".
  const crossProviderOf = classGate.crossProviderOf;
  const peerProvider = input.peer?.provider ?? null;
  let pool = candidates;
  if (crossProviderOf !== undefined && peerProvider !== null) {
    const peerVendor = vendorOf(peerProvider);
    const diverse = candidates.filter(
      (candidate) => vendorOf(candidate.target.provider) !== peerVendor,
    );
    if (diverse.length > 0) {
      for (const candidate of candidates) {
        if (diverse.includes(candidate)) continue;
        excluded.push({
          provider: candidate.target.provider,
          model: candidate.target.model,
          why:
            `shares a vendor with the ${crossProviderOf} it reviews ` +
            `(${peerProvider}); an eligible cross-provider target exists`,
        });
      }
      pool = diverse;
    }
  }

  if (pool.length === 0) {
    return {
      ok: false,
      code: HIRE_NO_ROUTE,
      reason: describeNoRoute(className, tier, excluded),
      excluded,
    };
  }

  const sampling = input.sampleChallenger === true;
  const ranked = [...pool].sort((left, right) =>
    compareCandidates(left, right, sampling),
  );
  const first = ranked[0];
  const second = ranked[1] ?? null;
  if (first === undefined) {
    return {
      ok: false,
      code: HIRE_NO_ROUTE,
      reason: describeNoRoute(className, tier, excluded),
      excluded,
    };
  }

  const override = input.override ?? null;
  let chosen = first.target;
  let runnerUp = second?.target ?? null;
  let reason = describeChoice({
    className,
    tier,
    classGate,
    chosen: first,
    runnerUp: second,
    sampling,
  });
  if (override !== null) {
    const applied = applyOverride(override, input.catalog, effort);
    if (!applied.ok) {
      return { ok: false, code: HIRE_NO_ROUTE, reason: applied.why, excluded };
    }
    // The router's own answer is kept as the runner-up, so a reader can see
    // what was overridden rather than only what was asked for.
    runnerUp = chosen;
    chosen = applied.target;
    reason =
      `human override: ${override.because.trim()} — the router would have ` +
      `chosen ${first.target.provider}/${first.target.model}.`;
  }

  const reviewReasons = reviewReasonsFor(score, input.risk, input.review);
  const record: CodingSessionRoutingRecord = {
    class: className,
    tier,
    risk: { ...input.risk, score },
    ...(input.profile && Object.keys(input.profile).length > 0
      ? { profile: input.profile }
      : {}),
    chosen,
    runnerUp,
    reason,
    reviewRequired: reviewReasons.length > 0,
    reviewReasons,
    challengerSample: sampling && !first.incumbent && override === null,
    override:
      override === null
        ? null
        : {
            model: override.model.trim(),
            ...(override.effort ? { effort: override.effort } : {}),
            because: override.because.trim(),
          },
    registryVersion: registry.version,
    catalogRevision: input.catalogRevision,
  };
  return { ok: true, record, candidates: ranked, excluded };
}

/**
 * The one line a seat's provenance row shows.
 *
 * `routed: builder/standard → claude-primary/sonnet (medium) — <reason>`.
 * One line by contract: a routing decision that needs a panel to be readable
 * is a decision nobody reads.
 */
export function describeCodingSessionRouting(
  record: CodingSessionRoutingRecord,
): string {
  return (
    `routed: ${record.class}/${record.tier} → ${record.chosen.provider}/` +
    `${record.chosen.model} (${record.chosen.effort}) — ${record.reason}`
  );
}

const ROUTING_RECORD_KEYS = [
  "class",
  "tier",
  "risk",
  "chosen",
  "runnerUp",
  "reason",
  "reviewRequired",
  "reviewReasons",
  "challengerSample",
  "override",
  "registryVersion",
  "catalogRevision",
] as const;

/**
 * The closed-shape check every strict observer applies to a `routing` object.
 *
 * Mirrored — not imported — by `sessionCoordinationStrictJson.ts`, the web
 * observer and the mobile decoder, each of which has to stay dependency-free.
 * The rule is the same in all four: exactly these keys, `profile` optional,
 * and an effort the router is allowed to buy.
 */
export function isStrictCodingSessionRoutingRecord(value: unknown): boolean {
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  const allowed = new Set<string>([...ROUTING_RECORD_KEYS, "profile"]);
  if (keys.some((key) => !allowed.has(key))) return false;
  if (ROUTING_RECORD_KEYS.some((key) => !Object.hasOwn(value, key))) {
    return false;
  }
  if (
    typeof value.class !== "string" ||
    !/^[a-z0-9_-]{1,64}$/.test(value.class)
  ) {
    return false;
  }
  if (!["fast", "standard", "deep"].includes(value.tier as string))
    return false;
  if (!isStrictRisk(value.risk)) return false;
  if (!isStrictTarget(value.chosen)) return false;
  if (value.runnerUp !== null && !isStrictTarget(value.runnerUp)) return false;
  if (typeof value.reason !== "string" || value.reason.trim().length === 0) {
    return false;
  }
  if (typeof value.reviewRequired !== "boolean") return false;
  if (
    !Array.isArray(value.reviewReasons) ||
    value.reviewReasons.some(
      (entry) =>
        typeof entry !== "string" ||
        !(ROUTING_REVIEW_TRIGGERS as readonly string[]).includes(entry),
    )
  ) {
    return false;
  }
  if (value.reviewRequired !== value.reviewReasons.length > 0) return false;
  if (typeof value.challengerSample !== "boolean") return false;
  if (value.override !== null && !isStrictOverride(value.override))
    return false;
  if (!Number.isSafeInteger(value.registryVersion)) return false;
  if (
    value.catalogRevision !== null &&
    !(
      Number.isSafeInteger(value.catalogRevision) &&
      (value.catalogRevision as number) > 0
    )
  ) {
    return false;
  }
  if (Object.hasOwn(value, "profile") && !isStrictProfile(value.profile)) {
    return false;
  }
  return true;
}

function isStrictRisk(value: unknown): boolean {
  if (!isRecord(value)) return false;
  const keys = ["impact", "uncertainty", "irreversibility", "score"];
  if (Object.keys(value).length !== keys.length) return false;
  if (keys.some((key) => !Number.isSafeInteger(value[key]))) return false;
  for (const key of ["impact", "uncertainty", "irreversibility"]) {
    const factor = value[key] as number;
    if (factor < 1 || factor > 5) return false;
  }
  return (
    value.score ===
    (value.impact as number) *
      (value.uncertainty as number) *
      (value.irreversibility as number)
  );
}

function isStrictTarget(value: unknown): boolean {
  return (
    isRecord(value) &&
    Object.keys(value).length === 3 &&
    typeof value.provider === "string" &&
    value.provider.trim().length > 0 &&
    typeof value.model === "string" &&
    value.model.trim().length > 0 &&
    ["low", "medium", "high"].includes(value.effort as string)
  );
}

function isStrictOverride(value: unknown): boolean {
  if (!isRecord(value)) return false;
  const keys = Object.keys(value);
  if (keys.some((key) => !["model", "effort", "because"].includes(key))) {
    return false;
  }
  if (typeof value.model !== "string" || value.model.trim().length === 0) {
    return false;
  }
  if (typeof value.because !== "string" || value.because.trim().length === 0) {
    return false;
  }
  return (
    !Object.hasOwn(value, "effort") ||
    ["low", "medium", "high"].includes(value.effort as string)
  );
}

function isStrictProfile(value: unknown): boolean {
  if (!isRecord(value)) return false;
  const traits = new Set<string>(ROUTING_TRAITS);
  return Object.entries(value).every(
    ([trait, minimum]) =>
      traits.has(trait) &&
      typeof minimum === "number" &&
      Number.isFinite(minimum) &&
      minimum >= 1 &&
      minimum <= 5,
  );
}

/**
 * The catalog id this target runs under at this effort, or null when the
 * catalog offers neither.
 *
 * `gpt-5.6-sol` at `high` is `gpt-5.6-sol[high]` when the catalog publishes
 * that variant, and the bare id when it does not. `claude-primary` publishes
 * no effort variants at all, so a Claude target's id is always the bare one
 * and the effort in the record is what the router asked the harness for
 * rather than an id it bought — which is why the effort is recorded
 * separately from the model instead of being read back off it.
 */
function catalogModelId(
  catalog: readonly RoutingCatalogEntry[],
  target: RegistryTarget,
  effort: RoutingEffort,
): string | null {
  const variant = `${target.model}[${effort}]`;
  if (offers(catalog, target.provider, variant)) return variant;
  if (offers(catalog, target.provider, target.model)) return target.model;
  return null;
}

function offers(
  catalog: readonly RoutingCatalogEntry[],
  provider: string,
  model: string,
): boolean {
  return catalog.some(
    (entry) => entry.providerInstanceRef === provider && entry.model === model,
  );
}

function contextWindowOf(
  catalog: readonly RoutingCatalogEntry[],
  target: RegistryTarget,
  model: string,
): number | null {
  const entry = catalog.find(
    (candidate) =>
      candidate.providerInstanceRef === target.provider &&
      candidate.model === model,
  );
  return entry?.contextWindow ?? target.facts.contextWindow ?? null;
}

/**
 * Why this target cannot take this job, or null when it can.
 *
 * Hard requirements before class gates, exactly as §7 orders them, so the
 * sentence a reader gets names the first thing that was actually wrong.
 */
function disqualify(input: {
  target: RegistryTarget;
  classGate: RegistryClass;
  className: string;
  profile: Partial<Record<RoutingTrait, number>> | undefined;
  requirements: RoutingRequirements | undefined;
  risk: RoutingRisk;
  catalogContextWindow: number | null;
}): string | null {
  const { target, classGate, requirements } = input;
  const facts = target.facts;

  const multimodalRequired =
    requirements?.multimodal === true ||
    classGate.requires?.multimodal === true;
  if (multimodalRequired && facts.multimodal !== true) {
    return facts.multimodal === false
      ? "not multimodal, and this class requires it"
      : "no recorded modality, and this class requires multimodal";
  }

  const tools = [
    ...(classGate.requires?.tools ?? []),
    ...(requirements?.tools ?? []),
  ];
  for (const tool of tools) {
    const known = facts.tools;
    if (known === null || known === undefined) {
      return `no recorded tool support, so ${tool} is not established for it`;
    }
    if (!known.includes(tool))
      return `does not offer the required ${tool} tool`;
  }

  const minWindow = requirements?.minContextWindow;
  if (minWindow !== undefined) {
    const window = input.catalogContextWindow;
    if (window === null) {
      return `no recorded context window, so ${minWindow} tokens is not established for it`;
    }
    if (window < minWindow) {
      return `holds ${window} tokens, and the task needs ${minWindow}`;
    }
  }

  for (const mode of requirements?.incompatibleFailureModes ?? []) {
    if (facts.knownFailureModes.includes(mode)) {
      return `has the known failure mode this task cannot tolerate: ${mode}`;
    }
  }

  const constraints = facts.constraints;
  if (constraints !== undefined) {
    if (
      constraints.ambiguityMax !== undefined &&
      requirements?.ambiguity !== undefined &&
      requirements.ambiguity > constraints.ambiguityMax
    ) {
      return `takes work at ambiguity ${constraints.ambiguityMax} or below; this is ${requirements.ambiguity}`;
    }
    if (
      constraints.irreversibilityMax !== undefined &&
      input.risk.irreversibility > constraints.irreversibilityMax
    ) {
      return `takes work at irreversibility ${constraints.irreversibilityMax} or below; this is ${input.risk.irreversibility}`;
    }
    if (
      constraints.scope !== undefined &&
      requirements?.scope !== undefined &&
      requirements.scope !== constraints.scope
    ) {
      return `takes ${constraints.scope} scope only; this is ${requirements.scope}`;
    }
  }

  // Class gates last, and the task's own minimums on top of them. A trait the
  // row has no number for fails: an unmeasured capability is not a cleared one.
  const minimums: Partial<Record<RoutingTrait, number>> = {
    ...classGate.minimums,
  };
  for (const [trait, minimum] of Object.entries(input.profile ?? {})) {
    const key = trait as RoutingTrait;
    const existing = minimums[key];
    if (
      typeof minimum === "number" &&
      (existing === undefined || minimum > existing)
    ) {
      minimums[key] = minimum;
    }
  }
  for (const [trait, minimum] of Object.entries(minimums)) {
    const score = target.scores[trait as RoutingTrait];
    if (typeof score !== "number") {
      return `has no ${trait} prior, and ${input.className} requires ${minimum}`;
    }
    if (score < (minimum as number)) {
      return `${trait} ${score} misses the ${input.className} minimum of ${minimum}`;
    }
  }
  return null;
}

/** Effort as a multiplier on what a run spends and how long it takes. */
function effortWeight(effort: RoutingEffort): number {
  return effort === "low" ? 1 : effort === "medium" ? 2 : 3;
}

/**
 * Expected cost of an **accepted** completion — the only thing cost is
 * allowed to decide, and only among targets that already cleared every gate.
 *
 * Three named terms, summed, exactly as §7 describes the pre-telemetry
 * estimate: quota spend, a latency prior, and a retry prior.
 *
 * - **spend** uses `costEfficiency`, deliberately *not* `priceUsdPerM`. Spec
 *   §1 forbids equating the two: our real cost is a subscription quota lane,
 *   and the published token price says nothing about how much of a Claude
 *   subscription or a Codex plan a run burns. The price stays in the registry
 *   as a fact and is never read here.
 * - **latency** is the velocity prior, scaled by effort for the same reason
 *   spend is: a high-effort run of a slow model is the slowest thing there is.
 * - **retry** is discipline plus verification — the rework a target is
 *   expected to cause — and it does *not* scale with effort, because a
 *   rework round is a whole extra run either way.
 *
 * `null` when the row has no cost prior. That is not "free": a target whose
 * spend nobody has estimated is ranked behind every target whose spend is
 * known, because an unpriced run is not evidence of a cheap one.
 */
function expectedCostOf(
  target: RegistryTarget,
  effort: RoutingEffort,
): number | null {
  const costEfficiency = target.scores.costEfficiency;
  const velocity = target.scores.velocity;
  const discipline = target.scores.discipline;
  const verification = target.scores.verification;
  if (
    typeof costEfficiency !== "number" ||
    typeof velocity !== "number" ||
    typeof discipline !== "number" ||
    typeof verification !== "number"
  ) {
    return null;
  }
  const weight = effortWeight(effort);
  const spend = weight * (6 - costEfficiency);
  const latency = weight * (6 - velocity);
  const retry = 6 - discipline + (6 - verification);
  return Number((spend + latency + retry).toFixed(4));
}

/**
 * Rank: incumbency first, then cost, then a stable id.
 *
 * Incumbency before cost is §8: a challenger does not earn a route by being
 * cheap, it earns one through measured results, and until then it is
 * *sampled* by rule rather than routed to by default. When the lead is
 * sampling, the same key runs the other way — which is the whole of what
 * sampling means here.
 */
function compareCandidates(
  left: RoutingCandidate,
  right: RoutingCandidate,
  sampling: boolean,
): number {
  const leftFirst = sampling ? !left.incumbent : left.incumbent;
  const rightFirst = sampling ? !right.incumbent : right.incumbent;
  if (leftFirst !== rightFirst) return leftFirst ? -1 : 1;
  const leftCost = left.expectedCost;
  const rightCost = right.expectedCost;
  if (leftCost === null || rightCost === null) {
    if (leftCost !== rightCost) return leftCost === null ? 1 : -1;
  } else if (leftCost !== rightCost) {
    return leftCost - rightCost;
  }
  return `${left.target.provider}/${left.target.model}`.localeCompare(
    `${right.target.provider}/${right.target.model}`,
  );
}

/**
 * Is this row the incumbent for this class at this tier?
 *
 * The ruling writes incumbency two ways — a tier-qualified key
 * (`builder-fast: incumbent`) and a tier-qualified value
 * (`builder: incumbent-deep`) — and both appear in Brian's own table, so both
 * are read.
 *
 * **A tier that names its own incumbents does not inherit the class-wide
 * one**, which is what `tierNamesItsOwn` carries in. Without that rule
 * Sonnet's unqualified `builder: incumbent` would make it the incumbent at
 * every tier, and — being the cheapest of them — it would take DEEP builder
 * work away from Sol and Opus, which is precisely the assignment spec §4
 * writes out by hand ("FAST: Luna, 5.4-mini. STANDARD: Sonnet 5, Terra …
 * DEEP: Sol, Opus 5"). Tier-qualified is the more specific claim, so where a
 * tier has one it is the whole answer for that tier.
 *
 * Anything else, including a class the row says nothing about, is a
 * challenger: unproven for this class is exactly what challenger means.
 */
function isIncumbent(
  target: RegistryTarget,
  className: string,
  tier: RoutingTier,
  tierNamesItsOwn: boolean,
): boolean {
  const qualified = target.status[`${className}-${tier}`];
  if (typeof qualified === "string") return qualified === "incumbent";
  const plain = target.status[className];
  if (typeof plain !== "string") return false;
  if (plain === `incumbent-${tier}`) return true;
  return plain === "incumbent" && !tierNamesItsOwn;
}

/** Does any row claim incumbency for this class *at this tier specifically*? */
function tierHasQualifiedIncumbent(
  registry: ModelRegistry,
  className: string,
  tier: RoutingTier,
): boolean {
  return registry.targets.some(
    (target) =>
      target.status[`${className}-${tier}`] === "incumbent" ||
      target.status[className] === `incumbent-${tier}`,
  );
}

/** `claude-primary` and `claude-secondary` are one vendor; `codex-*` another. */
function vendorOf(providerInstanceRef: string): string {
  const separator = providerInstanceRef.lastIndexOf("-");
  return separator <= 0
    ? providerInstanceRef
    : providerInstanceRef.slice(0, separator);
}

function describeChoice(input: {
  className: string;
  tier: RoutingTier;
  classGate: RegistryClass;
  chosen: RoutingCandidate;
  runnerUp: RoutingCandidate | null;
  sampling: boolean;
}): string {
  const gates = Object.entries(input.classGate.minimums)
    .map(([trait, minimum]) => `${trait}≥${minimum}`)
    .join(", ");
  const standing = input.chosen.incumbent
    ? "incumbent"
    : input.sampling
      ? "sampled challenger"
      : "no incumbent was eligible";
  const against =
    input.runnerUp === null
      ? "nothing else cleared them"
      : `cheaper than ${input.runnerUp.target.provider}/${input.runnerUp.target.model}`;
  return (
    `cleared the ${input.className} gates (${gates || "none"}) and the ` +
    `${input.tier} tier's ${input.chosen.target.effort} effort; ${standing}, ` +
    `${against}.`
  );
}

function describeNoRoute(
  className: string,
  tier: RoutingTier,
  excluded: readonly RoutingExclusion[],
): string {
  const said = excluded
    .slice(0, 4)
    .map((entry) => `${entry.provider}/${entry.model}: ${entry.why}`)
    .join("; ");
  return (
    `no execution target clears the ${className} gates at the ${tier} tier ` +
    `on this host's live catalog. ${said}${
      excluded.length > 4 ? `; and ${excluded.length - 4} more` : ""
    }.`
  );
}

function applyOverride(
  override: RoutingOverride,
  catalog: readonly RoutingCatalogEntry[],
  effort: RoutingEffort,
): { ok: true; target: RoutedExecutionTarget } | { ok: false; why: string } {
  const because = override.because.trim();
  if (because.length === 0) {
    return {
      ok: false,
      why:
        `the hire names a model by hand but says nothing about why. An ` +
        "override needs a `because`: the router's own choice is the thing " +
        "being set aside.",
    };
  }
  const model = override.model.trim();
  const providers = [
    ...new Set(
      catalog
        .filter((entry) => entry.model === model)
        .map((entry) => entry.providerInstanceRef),
    ),
  ];
  if (providers.length === 0) {
    return {
      ok: false,
      why:
        `no runtime on this computer offers the model ${model}. The catalog ` +
        "is the model list, and nothing is translated onto a neighbouring id.",
    };
  }
  if (providers.length > 1) {
    return {
      ok: false,
      why:
        `${model} is offered by ${providers.join(" and ")}; name the runtime ` +
        "as well, because the override does not pick between them.",
    };
  }
  const provider = providers[0];
  if (provider === undefined) {
    return { ok: false, why: `no runtime on this computer offers ${model}.` };
  }
  return {
    ok: true,
    target: { provider, model, effort: override.effort ?? effort },
  };
}

function reviewReasonsFor(
  score: number,
  risk: RoutingRisk,
  flags: RoutingReviewFlags | undefined,
): RoutingReviewTrigger[] {
  const reasons: RoutingReviewTrigger[] = [];
  if (score >= 40) reasons.push("risk>=40");
  if (risk.irreversibility >= 4) reasons.push("irreversibility>=4");
  if (flags?.securityBoundary === true) {
    reasons.push("security-auth-data-boundary");
  }
  if (flags?.publicContract === true) {
    reasons.push("architecture-schema-public-contract");
  }
  if (flags?.builderOutsidePlan === true) reasons.push("builder-outside-plan");
  if (flags?.builderReportsUncertainty === true) {
    reasons.push("builder-reports-uncertainty");
  }
  if (flags?.testsCannotVerify === true) reasons.push("tests-cannot-verify");
  if (flags?.leadRequested === true) reasons.push("lead-requested");
  return reasons;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
