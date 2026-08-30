/**
 * Shared strict decoders for the 442xx wire payloads.
 *
 * Copied from
 * `desktop/src/features/coding-sessions/lib/codingSessionWireDecode.ts`.
 *
 * Every one of these is a rejection primitive, not a coercion: an input that
 * is not exactly the declared shape decodes to `null`/`false` rather than to a
 * best guess. The producer signs canonical JSON with a fixed key set, so
 * anything looser than exact-key matching would admit an envelope this client
 * cannot claim to have verified.
 */
import type { CodingSessionTarget } from "./types.ts";

export const MAX_TARGET_IDENTITY_BYTES = 512;

/** The reference ceiling buzz-core applies to every ref-shaped string. */
export const MAX_REFERENCE_BYTES = 2 * 1024;

export function parseBoundedJson(content: unknown, maxBytes: number): unknown {
  if (
    typeof content !== "string" ||
    new TextEncoder().encode(content).byteLength > maxBytes
  ) {
    return null;
  }
  try {
    return JSON.parse(content);
  } catch {
    return null;
  }
}

export function decodeTarget(
  value: unknown,
  maxIdentityBytes: number = MAX_TARGET_IDENTITY_BYTES,
): CodingSessionTarget | null {
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, ["driver", "instanceId", "sessionId", "generation"]) ||
    !boundedNonempty(value.driver, maxIdentityBytes) ||
    !boundedNonempty(value.instanceId, maxIdentityBytes) ||
    !boundedNonempty(value.sessionId, maxIdentityBytes) ||
    !Number.isSafeInteger(value.generation) ||
    (value.generation as number) <= 0
  ) {
    return null;
  }
  return Object.freeze({
    driver: value.driver,
    instanceId: value.instanceId,
    sessionId: value.sessionId,
    generation: value.generation as number,
  });
}

export function boundedNonempty(
  value: unknown,
  maxBytes: number,
): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    new TextEncoder().encode(value).byteLength <= maxBytes
  );
}

export function boundedNullable(
  value: unknown,
  maxBytes: number,
): value is string | null {
  return value === null || boundedNonempty(value, maxBytes);
}

export function isPlainRecord(
  value: unknown,
): value is Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    return false;
  }
  const prototype = Object.getPrototypeOf(value);
  return prototype === Object.prototype || prototype === null;
}

export function hasExactKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return (
    actual.length === keys.length && actual.every((key) => keys.includes(key))
  );
}

export function hasRequiredAndOptionalKeys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[],
): boolean {
  const actual = Object.keys(value);
  return (
    required.every((key) => hasOwnKey(value, key)) &&
    actual.every((key) => required.includes(key) || optional.includes(key))
  );
}

/**
 * A key group that must appear as a unit or not at all. This is how a payload
 * amendment stays unambiguous: a producer either speaks the amended dialect
 * (every key present, if only as nulls) or the base one — a partial subset is
 * a corrupted or hand-rolled payload, not a version skew.
 */
export function hasAllOrNoneKeys(
  value: Record<string, unknown>,
  keys: readonly string[],
): boolean {
  const present = keys.filter((key) => hasOwnKey(value, key)).length;
  return present === 0 || present === keys.length;
}

/** Depth- and cycle-bounded structural probe for untrusted nested payloads. */
export function isWithinDepth(value: unknown, maxDepth: number): boolean {
  const stack: Array<{ value: unknown; depth: number }> = [{ value, depth: 0 }];
  const seen = new Set<object>();
  while (stack.length > 0) {
    const current = stack.pop();
    if (!current) continue;
    if (current.depth > maxDepth) return false;
    if (typeof current.value !== "object" || current.value === null) continue;
    if (seen.has(current.value)) return false;
    seen.add(current.value);
    const nested = Array.isArray(current.value)
      ? current.value
      : Object.values(current.value);
    for (const item of nested) {
      stack.push({ value: item, depth: current.depth + 1 });
    }
  }
  return true;
}

/**
 * Read the event's tags as an exact, ordered list of single-value tags.
 *
 * Order and count are part of the contract: the producer emits these tags in
 * one fixed order, so an event with the right tags in the wrong order, or with
 * an extra tag appended, is not one this client signed off on.
 */
export function parseExactTags(
  tags: string[][],
  names: readonly string[],
): string[] | null {
  if (tags.length !== names.length) return null;
  const values: string[] = [];
  for (let index = 0; index < names.length; index += 1) {
    const tag = tags[index];
    if (
      !Array.isArray(tag) ||
      tag.length !== 2 ||
      tag[0] !== names[index] ||
      typeof tag[1] !== "string"
    ) {
      return null;
    }
    values.push(tag[1]);
  }
  return values;
}

export function hasTagNamed(tags: string[][], name: string): boolean {
  return tags.some((tag) => Array.isArray(tag) && tag[0] === name);
}

/**
 * Canonical umbrella session reference: a lowercase, hyphenated UUID
 * (36 chars, lowercase hex). This is the only shape a `sessionRef` may take,
 * on the 44221 create payload and in the 44223 metadata echo alike.
 */
export function isCodingSessionSessionRef(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/.test(value)
  );
}

export function normalizePubkey(value: unknown): string {
  if (typeof value !== "string") return "";
  const normalized = value.trim().toLowerCase();
  return isExactProviderAuthorityPubkey(normalized) ? normalized : "";
}

export function isExactProviderAuthorityPubkey(
  value: unknown,
): value is string {
  return typeof value === "string" && /^[0-9a-f]{64}$/.test(value);
}

/**
 * `Object.hasOwn` semantics under this project's ES2020 lib target.
 *
 * Read through a property descriptor rather than calling the object's own
 * `hasOwnProperty`: an untrusted payload may carry a `hasOwnProperty` key of
 * its own, and a descriptor lookup never invokes a getter the payload
 * defined. Exported so the other decoders share one answer.
 */
export function hasOwnKey(value: object, key: string): boolean {
  return Object.getOwnPropertyDescriptor(value, key) !== undefined;
}

const ROUTING_EFFORTS = new Set(["low", "medium", "high"]);
const ROUTING_TIERS = new Set(["fast", "standard", "deep"]);
/**
 * A review reason is a bounded token, not a member of a closed set.
 *
 * The canonical router renders the two numeric §6 triggers with the value that
 * fired them — `risk 80 >= 40`, `irreversibility 4 >= 4` — so a reader is told
 * the fact rather than the rule (`crates/buzz-core/src/coding_session_routing.rs:1555`).
 * A closed vocabulary here would make this observer reject the router's own
 * record as malformed, which is exactly the kind of lie that shows up as
 * "the seat says nothing about why it is that model".
 */
const MAX_ROUTING_REVIEW_REASONS = 16;
const MAX_ROUTING_TOKEN_BYTES = 256;
const ROUTING_TRAITS = new Set([
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
]);
/** One sentence, bounded: the host's disagreement with a carried proposal. */
const MAX_ROUTING_DISAGREEMENT_BYTES = 512;
const ROUTING_RECORD_FIELDS = [
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
];

/**
 * The `routing` record a routed 44221 create and its 44223 metadata carry
 * (Brian's routing ruling, 2026-08-30).
 *
 * Written out here rather than imported from the desktop because this
 * observer shares no code with it; the rule is identical and is asserted
 * against the same shapes. Two of the checks are about honesty rather than
 * shape and are worth naming:
 *
 * - `risk.score` must be `impact × uncertainty × irreversibility`. A record
 *   whose score disagrees with its own factors is a claim nobody can redo.
 * - `chosen.effort` must be one of the three the router may buy. `xhigh`,
 *   `max` and `ultra` are human-override only, so a record presenting one as
 *   a routed effort is refused rather than rendered.
 */
export function isStrictRoutingRecord(value: unknown): boolean {
  if (!isPlainRecord(value)) return false;
  const allowed = new Set([
    ...ROUTING_RECORD_FIELDS,
    "profile",
    "proposedDisagreement",
  ]);
  if (Object.keys(value).some((key) => !allowed.has(key))) return false;
  if (ROUTING_RECORD_FIELDS.some((key) => !hasOwnKey(value, key))) return false;
  if (
    typeof value.class !== "string" ||
    !/^[a-z0-9_-]{1,64}$/.test(value.class)
  ) {
    return false;
  }
  if (!ROUTING_TIERS.has(value.tier as string)) return false;
  if (!isRoutingRisk(value.risk)) return false;
  if (!isRoutingTarget(value.chosen)) return false;
  if (value.runnerUp !== null && !isRoutingTarget(value.runnerUp)) return false;
  if (!boundedNonempty(value.reason, MAX_REFERENCE_BYTES)) return false;
  if (typeof value.reviewRequired !== "boolean") return false;
  if (
    !Array.isArray(value.reviewReasons) ||
    value.reviewReasons.length > MAX_ROUTING_REVIEW_REASONS ||
    value.reviewReasons.some(
      (entry) => !boundedNonempty(entry, MAX_ROUTING_TOKEN_BYTES),
    )
  ) {
    return false;
  }
  if (value.reviewRequired !== value.reviewReasons.length > 0) return false;
  if (typeof value.challengerSample !== "boolean") return false;
  if (value.override !== null && !isRoutingOverride(value.override)) {
    return false;
  }
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
  if (
    hasOwnKey(value, "proposedDisagreement") &&
    !boundedNonempty(value.proposedDisagreement, MAX_ROUTING_DISAGREEMENT_BYTES)
  ) {
    return false;
  }
  // `profile: null` is what the canonical producer writes when the lead asked
  // for no extra minimums (`Routing::profile` has no `skip_serializing_if`),
  // so an observer accepting only an object refused every record the CLI wrote.
  if (!hasOwnKey(value, "profile") || value.profile === null) return true;
  return (
    isPlainRecord(value.profile) &&
    Object.entries(value.profile).every(
      ([trait, minimum]) =>
        ROUTING_TRAITS.has(trait) &&
        typeof minimum === "number" &&
        Number.isFinite(minimum) &&
        minimum >= 1 &&
        minimum <= 5,
    )
  );
}

function isRoutingRisk(value: unknown): boolean {
  if (!isPlainRecord(value)) return false;
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

function isRoutingTarget(value: unknown): boolean {
  return (
    isPlainRecord(value) &&
    Object.keys(value).length === 3 &&
    boundedNonempty(value.provider, MAX_REFERENCE_BYTES) &&
    boundedNonempty(value.model, MAX_REFERENCE_BYTES) &&
    ROUTING_EFFORTS.has(value.effort as string)
  );
}

function isRoutingOverride(value: unknown): boolean {
  if (!isPlainRecord(value)) return false;
  if (
    Object.keys(value).some(
      (key) => !["model", "effort", "because"].includes(key),
    )
  ) {
    return false;
  }
  if (!boundedNonempty(value.model, MAX_REFERENCE_BYTES)) return false;
  if (!boundedNonempty(value.because, MAX_REFERENCE_BYTES)) return false;
  // `null` is "take the tier's effort", and it is the shape the canonical
  // producer writes: buzz-core's `RoutingOverride.effort` is an
  // `Option<String>` with no `skip_serializing_if`
  // (crates/buzz-core/src/coding_session_routing.rs:1005), so every override
  // on the wire carries the key explicitly null. Refusing it would drop the
  // whole routing record of every seat a human overrode.
  return (
    !hasOwnKey(value, "effort") ||
    value.effort === null ||
    ROUTING_EFFORTS.has(value.effort as string)
  );
}
