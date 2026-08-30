/**
 * The `routing` record as it rides on the wire, and the strict check every
 * observer applies to it.
 *
 * Split out of `codingSessionRouting.ts` because producing a decision and
 * validating the object that carries one are two jobs: the router has an
 * opinion, this module only has a shape. Keeping them apart also keeps the
 * router readable — the shape is mirrored by three other implementations
 * (`sessionCoordinationStrictJson.ts`, the web decoder, the mobile decoder)
 * and by `Routing` in `crates/buzz-core/src/coding_session_routing.rs`, which
 * is the canonical one. Where they disagree, buzz-core is right.
 */
import {
  ROUTING_TRAITS,
  type RoutingEffort,
  type RoutingRisk,
  type RoutingTier,
  type RoutingTrait,
} from "./codingSessionModelRegistry";

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

/**
 * Spec §6's six *flag* triggers, spelled the way the canonical router spells
 * them (`crates/buzz-core/src/coding_session_routing.rs:1565-1577`).
 *
 * The other two triggers are **not** in this list on purpose. Risk and
 * irreversibility are numeric, and the record carries the number that fired
 * — `risk 80 >= 40`, `irreversibility 4 >= 4` — because a reader who is told
 * only `risk>=40` has to go and recompute the risk to know what happened.
 * That makes `reviewReasons` an open vocabulary of bounded tokens, exactly as
 * `RoutingRecord::validate` treats it, and no observer may close it.
 */
export const ROUTING_REVIEW_FLAG_TRIGGERS = [
  "securityBoundary",
  "contractChange",
  "outsidePlan",
  "builderUncertain",
  "testsInsufficient",
  "leadRequests",
] as const;

export type RoutingReviewFlagTrigger =
  (typeof ROUTING_REVIEW_FLAG_TRIGGERS)[number];

/** At most this many triggers ride on one record (buzz-core's own ceiling). */
export const MAX_ROUTING_REVIEW_REASONS = 16;

/** A review reason is a bounded, non-blank token — never a closed set. */
export const MAX_ROUTING_TOKEN_BYTES = 256;

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
  // Open vocabulary, bounded shape. `reviewReasons` carries rendered facts
  // (`risk 80 >= 40`) as well as flag names, so a closed set here would make
  // every observer reject the canonical router's own record.
  if (
    !Array.isArray(value.reviewReasons) ||
    value.reviewReasons.length > MAX_ROUTING_REVIEW_REASONS ||
    value.reviewReasons.some(
      (entry) =>
        typeof entry !== "string" ||
        entry.trim().length === 0 ||
        entry.length > MAX_ROUTING_TOKEN_BYTES,
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

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}
