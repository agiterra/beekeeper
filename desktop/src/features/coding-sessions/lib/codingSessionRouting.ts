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
  LEGACY_ROW_GRACE_DAYS,
  ROUTING_TRAITS,
  codingSessionGraceDaysLeft,
  codingSessionProvenanceClause,
  codingSessionRiskScore,
  codingSessionRiskTier,
  codingSessionScoreProvenance,
  parseModelRegistry,
  type ModelRegistry,
  type RegistryClass,
  type RegistryScoreProvenance,
  type RegistryTarget,
  type RoutingEffort,
  type RoutingRisk,
  type RoutingTier,
  type RoutingTrait,
} from "./codingSessionModelRegistry";

import {
  describeCodingSessionRouting,
  isStrictCodingSessionRoutingRecord,
  MAX_ROUTING_REVIEW_REASONS,
  MAX_ROUTING_TOKEN_BYTES,
  ROUTING_REVIEW_FLAG_TRIGGERS,
  type CodingSessionRoutingRecord,
  type RoutedExecutionTarget,
  type RoutingOverride,
  type RoutingReviewFlagTrigger,
} from "./codingSessionRoutingRecord";

import { fallbackCatalogTarget } from "./codingSessionRoutingFallback";

// Re-exported so the router stays the one import a caller needs: the registry
// module is a reader and the record module is a shape; neither is a second
// public surface.
export {
  MODEL_REGISTRY_VERSION,
  ROUTING_EFFORT_FOR_TIER,
  ROUTING_TRAITS,
  codingSessionRiskScore,
  codingSessionRiskTier,
  parseModelRegistry,
  describeCodingSessionRouting,
  isStrictCodingSessionRoutingRecord,
  MAX_ROUTING_REVIEW_REASONS,
  MAX_ROUTING_TOKEN_BYTES,
  ROUTING_REVIEW_FLAG_TRIGGERS,
};
export type {
  ModelRegistry,
  RegistryClass,
  RegistryTarget,
  RoutingEffort,
  RoutingRisk,
  RoutingTier,
  RoutingTrait,
  CodingSessionRoutingRecord,
  RoutedExecutionTarget,
  RoutingOverride,
  RoutingReviewFlagTrigger,
};

/** One `(providerInstanceRef, model)` pair the live catalog offers. */
export type RoutingCatalogEntry = {
  providerInstanceRef: string;
  model: string;
  /** The catalog's own per-model figure, when it published one. */
  contextWindow?: number | null;
};

/** The §6 triggers a caller can assert; the two numeric ones are computed. */
export type RoutingReviewFlags = {
  securityBoundary?: boolean;
  contractChange?: boolean;
  outsidePlan?: boolean;
  builderUncertain?: boolean;
  testsInsufficient?: boolean;
  leadRequests?: boolean;
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
  /**
   * Today, `YYYY-MM-DD`, for the legacy-row grace clock only.
   *
   * Absent means no clock — a legacy row routes and is disclosed as legacy,
   * exactly as before. Supplying it opts the class's thirty-day deadline in,
   * matching `RouteRequest.today` in the Rust router so the app and the CLI
   * cannot give opposite answers about the same row.
   */
  today?: string | null;
};

/** Why one registry row is not a candidate. */
export type RoutingExclusion = {
  provider: string;
  model: string;
  why: string;
};

/**
 * The standing one registry row holds for one class at one tier, as four
 * ordered bands. Lower is preferred, and the band is consulted **before**
 * cost — spec §8: a challenger does not earn a route by being cheap.
 *
 * Mirrors `Standing` in `crates/beekeeper-core/src/coding_session_routing.rs:635`.
 * `unranked` and `challenger` are different facts: a row that says nothing
 * about a class has not been ruled a challenger for it, and only a row the
 * registry actually calls `challenger` is what a sample samples.
 */
export const ROUTING_STANDINGS = [
  "incumbent-at-tier",
  "incumbent",
  "unranked",
  "challenger",
] as const;

export type RoutingStanding = (typeof ROUTING_STANDINGS)[number];

/** A candidate that cleared every gate, with the cost that ranked it. */
export type RoutingCandidate = {
  target: RoutedExecutionTarget;
  standing: RoutingStanding;
  /** `true` for either incumbent band. Kept for readers that only ask that. */
  incumbent: boolean;
  /**
   * Does the row carry a `costEfficiency` prior at all? A row without one is
   * ranked behind every row that has one, rather than being folded in at an
   * invented middle value.
   */
  priced: boolean;
  /** Expected cost of an accepted completion. Lower is chosen. */
  expectedCost: number | null;
  /**
   * `measured` or `legacy` — where this row's ten numbers came from.
   *
   * Live run 3, finding 25: this host disclosed that a target "cleared the
   * verifier gates (reasoning≥4.5, judgment≥4.5, verification≥4.7) …
   * incumbent, nothing else cleared them" while every number in that sentence
   * was an operational prior nobody had sampled. The word goes on the
   * candidate and into the sentence, in the Rust router's own vocabulary.
   */
  scores: RegistryScoreProvenance;
  /** The clause the sentence appends. One of exactly two. */
  provenanceClause: string;
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
    const graceLeft = codingSessionGraceDaysLeft(
      classGate,
      input.today ?? null,
    );
    if (target.measured === undefined && graceLeft !== null && graceLeft <= 0) {
      excluded.push({
        provider: target.provider,
        model,
        why:
          `unmeasured: a bench has existed for ${className} since ` +
          `${classGate.benchAvailableSince ?? "?"} and this row still carries ` +
          `no measured scores, so its ${String(LEGACY_ROW_GRACE_DAYS)}-day ` +
          "grace is spent — run bee sessions registry measure --role " +
          className,
      });
      continue;
    }
    const standing = standingOf(registry, target, className, tier);
    candidates.push({
      target: { provider: target.provider, model, effort },
      standing,
      incumbent: standing === "incumbent-at-tier" || standing === "incumbent",
      priced: hasCostPrior(target),
      expectedCost: expectedCostOf(target),
      scores: codingSessionScoreProvenance(target),
      provenanceClause: codingSessionProvenanceClause(
        target,
        classGate,
        input.today ?? null,
      ),
    });
  }

  // Standing (spec §8, and step 4 of the canonical router). A challenger holds
  // no route: it is sampled by rule, and on a sampling run it is the *only*
  // thing eligible. A row the registry is silent about is `unranked`, not a
  // challenger — being unproven is not the same claim as being a contender,
  // and a sample that swept up every silent row would attribute nothing.
  const sampling =
    input.sampleChallenger === true &&
    candidates.some((candidate) => candidate.standing === "challenger");
  const standingPool: RoutingCandidate[] = [];
  for (const candidate of candidates) {
    const isChallenger = candidate.standing === "challenger";
    if (sampling && !isChallenger) {
      excluded.push({
        provider: candidate.target.provider,
        model: candidate.target.model,
        why: "not sampled: this run deliberately samples a challenger for this class",
      });
      continue;
    }
    if (!sampling && isChallenger) {
      excluded.push({
        provider: candidate.target.provider,
        model: candidate.target.model,
        why:
          `challenger for ${className}: a challenger earns an incumbent route ` +
          "only through measured results (spec §8), so it is routed only on a " +
          "deliberate sample",
      });
      continue;
    }
    standingPool.push(candidate);
  }

  // Cross-provider diversity (§4 VERIFIER): a hard filter, but only when it
  // leaves somebody standing. "Prefer a different failure mode" must never
  // become "refuse the work".
  const crossProviderOf = classGate.crossProviderOf;
  const peerProvider =
    crossProviderOf !== undefined && input.peer?.className === crossProviderOf
      ? input.peer.provider
      : null;
  let pool = standingPool;
  if (crossProviderOf !== undefined && peerProvider !== null) {
    const peerVendor = vendorOf(peerProvider);
    const diverse = standingPool.filter(
      (candidate) => vendorOf(candidate.target.provider) !== peerVendor,
    );
    if (diverse.length > 0) {
      for (const candidate of standingPool) {
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

  const ranked = [...pool].sort(compareCandidates);
  const first = ranked[0] ?? null;
  const second = ranked[1] ?? null;

  // Ledger 267 (control run 8): the override is considered *before* an empty
  // ranked pool is rejected. A registry with no row for a live catalog offer
  // is a fact about the registry's coverage, not a veto over an operator who
  // named a target by hand and said why — the override still has to survive
  // `applyOverride`'s own checks (the catalog actually offers it, and offers
  // it unambiguously).
  const override = input.override ?? null;
  let chosen: RoutedExecutionTarget;
  let runnerUp: RoutedExecutionTarget | null;
  let reason: string;
  if (override !== null) {
    const applied = applyOverride(override, input.catalog, effort);
    if (!applied.ok) {
      return { ok: false, code: HIRE_NO_ROUTE, reason: applied.why, excluded };
    }
    // The router's own answer (when it found one) is kept as the runner-up,
    // so a reader can see what was overridden rather than only what was asked
    // for.
    runnerUp = first?.target ?? null;
    chosen = applied.target;
    reason =
      `human override: ${override.because.trim()} — ` +
      (first === null
        ? "no registry-ranked candidate cleared the gates to compare it to."
        : `the router would have chosen ${first.target.provider}/${first.target.model}.`);
  } else if (first === null) {
    // No override, and nothing the registry ranks clears the gates. Spec §7's
    // "never quietly weakened" is about class minimums and requirements, not
    // about whether a row exists at all: a live catalog offer with no
    // registry row is DORMANT knowledge, not an unauthorized target (finding
    // ledger 266 — the registry's own comment says so of the other
    // direction, STALE). For an ordinary session with no hard requirement
    // this host cannot vouch for, fall back to the catalog's own order among
    // targets no registry row covers at all; a hard requirement (this class's
    // `requires`, or the task's `requirements`) still refuses, named.
    const fallback = fallbackCatalogTarget({
      registry,
      catalog: input.catalog,
      classGate,
      requirements: input.requirements,
      effort,
      excluded,
    });
    if (!fallback.ok) {
      return {
        ok: false,
        code: HIRE_NO_ROUTE,
        reason:
          fallback.why !== null
            ? `no execution target clears the ${className} gates at the ` +
              `${tier} tier: ${fallback.why}. No registry-ranked or catalog ` +
              "fallback target is eligible."
            : describeNoRoute(className, tier, excluded),
        excluded,
      };
    }
    chosen = fallback.target;
    runnerUp = null;
    reason =
      `catalog fallback: no registry row ranks a target for ${className} at ` +
      `the ${tier} tier, so the least-preferred but authorized, available ` +
      `catalog offer ${fallback.target.provider}/${fallback.target.model} ` +
      "was used; registry ranking is a preference here, not a gate, and no " +
      "hard requirement was recorded against it.";
  } else {
    chosen = first.target;
    runnerUp = second?.target ?? null;
    reason = describeChoice({
      className,
      tier,
      classGate,
      chosen: first,
      runnerUp: second,
      sampling,
    });
  }
  if (crossProviderOf !== undefined && peerProvider !== null) {
    reason +=
      vendorOf(chosen.provider) === vendorOf(peerProvider)
        ? ` no eligible cross-provider target was available on this host, so ` +
          `the ${className} shares ${vendorOf(peerProvider)} with the ` +
          `${crossProviderOf} it reviews.`
        : ` failure-mode diversity: the ${className} runs on ` +
          `${vendorOf(chosen.provider)}, not the ${crossProviderOf}'s ` +
          `${vendorOf(peerProvider)} vendor.`;
  }

  const reviewReasons = reviewReasonsFor(score, input.risk, input.review);
  const record: CodingSessionRoutingRecord = {
    class: className,
    tier,
    risk: { ...input.risk, score },
    // RoutingRecord::profile is an Option without skip_serializing_if, so the
    // provider's Rust-signed 44223 writes `null` when no profile was asked for.
    // Keep the create byte-identical instead of letting JSON.stringify omit it.
    profile:
      input.profile && Object.keys(input.profile).length > 0
        ? input.profile
        : null,
    chosen,
    runnerUp,
    reason,
    reviewRequired: reviewReasons.length > 0,
    reviewReasons,
    challengerSample: sampling,
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

/**
 * The retry prior every target carries until telemetry replaces it: the
 * expected number of attempts to an *accepted* completion. One flat value for
 * everything is an admission that we have measured nothing yet, and is
 * deliberately not a guess that varies by model.
 */
const DEFAULT_RETRY_PRIOR = 1;

/**
 * Expected cost of an **accepted** completion — the only thing cost is
 * allowed to decide, and only among targets that already cleared every gate.
 * Lower is better.
 *
 * ```text
 * cost_prior    = 6 − costEfficiency     (1.0 cheapest … 5.0 dearest)
 * latency_prior = 6 − velocity           (1.0 fastest  … 5.0 slowest)
 * retry_prior   = expected attempts to acceptance (1.0 until telemetry)
 *
 * expected_cost = retry_prior × (cost_prior + latency_prior)
 * ```
 *
 * This is `expected_cost` in `crates/beekeeper-core/src/coding_session_routing.rs`
 * (:1048), character for character in its arithmetic, because the two routers
 * have to reach the same answer on the same registry or one of them is lying.
 * The two priors add because they are two costs paid on the same attempt; the
 * retry prior multiplies because it counts attempts.
 *
 * `priceUsdPerM` is **not** in here. Spec §1 forbids equating cost with API
 * $/MTok: our real cost is a subscription quota lane. Price stays a recorded,
 * printed fact and is never scored.
 *
 * A row with no `costEfficiency` is ranked on latency alone and, by
 * `compareCandidates`, only after every target that has a cost prior. It is
 * never guessed cheap and never guessed dear.
 */
function expectedCostOf(target: RegistryTarget): number | null {
  const costEfficiency = target.scores.costEfficiency;
  const velocity = target.scores.velocity;
  const cost = typeof costEfficiency === "number" ? 6 - costEfficiency : null;
  const latency = typeof velocity === "number" ? 6 - velocity : null;
  if (cost === null && latency === null) return null;
  return Number(
    (DEFAULT_RETRY_PRIOR * ((cost ?? 0) + (latency ?? 0))).toFixed(4),
  );
}

/** Does this row carry a cost prior at all? Ranked ahead of rows that do not. */
function hasCostPrior(target: RegistryTarget): boolean {
  return typeof target.scores.costEfficiency === "number";
}

/**
 * Sort key, mirroring `rank_key` (`coding_session_routing.rs:1341`): standing
 * band, then whether a cost prior exists at all, then the expected cost.
 *
 * Standing before cost is §8. The priced flag before the number is §7's "never
 * silently weaken": an unpriced run is not evidence of a cheap one, so it
 * sorts behind everything priced rather than being folded in at some invented
 * middle value. Ties return 0 and the sort is stable, so registry order breaks
 * them — the same order the canonical router's stable sort produces.
 */
function compareCandidates(
  left: RoutingCandidate,
  right: RoutingCandidate,
): number {
  const band =
    ROUTING_STANDINGS.indexOf(left.standing) -
    ROUTING_STANDINGS.indexOf(right.standing);
  if (band !== 0) return band;
  const leftPriced = left.expectedCost !== null && left.priced;
  const rightPriced = right.expectedCost !== null && right.priced;
  if (leftPriced !== rightPriced) return leftPriced ? -1 : 1;
  const leftCost = left.expectedCost ?? Number.MAX_VALUE;
  const rightCost = right.expectedCost ?? Number.MAX_VALUE;
  if (leftCost !== rightCost) return leftCost - rightCost;
  return 0;
}

/**
 * The standing this row holds for one class at one tier.
 *
 * Mirrors `standing_for` (`coding_session_routing.rs:1366`). The ruling writes
 * incumbency two ways — a tier-qualified key (`builder-fast: incumbent`) and a
 * tier-qualified value (`builder: incumbent-deep`) — and both appear in
 * Brian's own table, so both are read and they mean the same thing.
 *
 * A tier-qualified incumbency is the *more specific* claim, so it wins its
 * tier outright (`incumbent-at-tier`), and a row seeded as incumbent at some
 * *other* tier contributes nothing here. That is how §4's hand-written
 * assignment ("FAST: Luna, 5.4-mini … DEEP: Sol, Opus 5") falls out of the
 * bands rather than out of a special case.
 *
 * A class this row says nothing about is `unranked`, not `challenger`.
 */
function standingOf(
  registry: ModelRegistry,
  target: RegistryTarget,
  className: string,
  tier: RoutingTier,
): RoutingStanding {
  let untiered: RoutingStanding | null = null;
  for (const [key, value] of Object.entries(target.status)) {
    const split = splitClassTier(registry, key);
    if (split.className !== className) continue;
    const separator = value.indexOf("-");
    const tail = separator < 0 ? null : value.slice(separator + 1);
    const word =
      tail !== null && Object.hasOwn(registry.tiers, tail)
        ? value.slice(0, separator)
        : value;
    const valueTier =
      tail !== null && Object.hasOwn(registry.tiers, tail) ? tail : null;
    const effectiveTier = split.tier ?? valueTier;
    let standing: RoutingStanding;
    if (word === "challenger") {
      standing = "challenger";
    } else if (word === "incumbent") {
      if (effectiveTier === null) standing = "incumbent";
      else if (effectiveTier === tier) return "incumbent-at-tier";
      else continue; // seeded at a different tier only
    } else {
      continue;
    }
    untiered =
      untiered === null
        ? standing
        : ROUTING_STANDINGS.indexOf(standing) <
            ROUTING_STANDINGS.indexOf(untiered)
          ? standing
          : untiered;
  }
  return untiered ?? "unranked";
}

/**
 * Split a status key into its class and, when it names one, its tier.
 *
 * `builder-fast` is the builder class at the fast tier; `ui_designer` is a
 * class whose own name contains no tier. Only a trailing segment that is
 * actually one of the registry's tier names is read as a tier.
 */
function splitClassTier(
  registry: ModelRegistry,
  key: string,
): { className: string; tier: RoutingTier | null } {
  const separator = key.lastIndexOf("-");
  if (separator <= 0) return { className: key, tier: null };
  const tail = key.slice(separator + 1);
  if (!Object.hasOwn(registry.tiers, tail)) {
    return { className: key, tier: null };
  }
  return { className: key.slice(0, separator), tier: tail as RoutingTier };
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
  // Last, and always: where the numbers in the gate above came from. Finding
  // 25 is a sentence exactly like this one that could not say it.
  return (
    `cleared the ${input.className} gates (${gates || "none"}) and the ` +
    `${input.tier} tier's ${input.chosen.target.effort} effort; ${standing}, ` +
    `${against}; ${input.chosen.provenanceClause}.`
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

/**
 * The §6 triggers that fired, in spec order and in the canonical router's own
 * words (`coding_session_routing.rs:1555-1579`).
 *
 * The two numeric triggers carry the value that fired them. `risk 80 >= 40`
 * is a fact a reader can check; `risk>=40` is a rule they have to re-apply.
 */
function reviewReasonsFor(
  score: number,
  risk: RoutingRisk,
  flags: RoutingReviewFlags | undefined,
): string[] {
  const reasons: string[] = [];
  if (score >= 40) reasons.push(`risk ${score} >= 40`);
  if (risk.irreversibility >= 4) {
    reasons.push(`irreversibility ${risk.irreversibility} >= 4`);
  }
  for (const [fired, name] of [
    [flags?.securityBoundary, "securityBoundary"],
    [flags?.contractChange, "contractChange"],
    [flags?.outsidePlan, "outsidePlan"],
    [flags?.builderUncertain, "builderUncertain"],
    [flags?.testsInsufficient, "testsInsufficient"],
    [flags?.leadRequests, "leadRequests"],
  ] as const) {
    if (fired === true) reasons.push(name);
  }
  return reasons;
}
