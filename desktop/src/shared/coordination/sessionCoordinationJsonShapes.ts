/**
 * The dependency-free JSON primitives every strict coding-session reader uses,
 * and the routing record — the largest single shape among them.
 *
 * Split out of `sessionCoordinationStrictJson.ts` by lane 216 so that file
 * stays under the repository file-size ratchet while it grows the amendments
 * `buzz-core` already accepts. The behaviour is unchanged and every symbol the
 * app imported from there is still re-exported from there; this module is
 * where the shapes live, not a new contract.
 *
 * Erasable-syntax-only and import-free, exactly like its parent: the
 * conformance binder loads both under plain `node --test`.
 */

/** Whether a value is a non-array JSON object. */
export function isPlainObject(
  value: unknown,
): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export const encoder = new TextEncoder();

/** The generic bound on a wire reference string, 2 KiB. */
export const MAX_REFERENCE_BYTES = 2 * 1024;

/** A non-blank string within `maxBytes` UTF-8 bytes. */
export function boundedString(
  value: unknown,
  maxBytes: number,
): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    encoder.encode(value).length <= maxBytes
  );
}

/** [`boundedString`] or an explicit `null`. */
export function boundedNullable(value: unknown, maxBytes: number): boolean {
  return value === null || boundedString(value, maxBytes);
}

/** Whether a decoded JSON object has exactly one of the accepted field sets. */
export function hasExactFields(
  value: unknown,
  forms: readonly (readonly string[])[],
): value is Record<string, unknown> {
  if (!isPlainObject(value)) return false;
  const keys = Object.keys(value);
  return forms.some(
    (form) =>
      keys.length === form.length &&
      form.every((key) => Object.hasOwn(value, key)),
  );
}

// Only the router's own purchase — `chosen`/`runnerUp` — is a closed effort
// set: `validate_routing_target` (coding_session_routing.rs:1252-1262) is the
// protocol boundary the relay itself enforces, the autonomous ceiling `xhigh`/
// `max`/`ultra` are deliberately excluded from. `RoutingOverride.effort` is
// the opposite case: `validate_override` (:1276-1283) only `bounded_token`s
// it, because a human override is exactly where those three values belong
// (:118-121) — closing it here would refuse every legitimate xhigh/max/ultra
// override the wire ever carries.
const ROUTING_EFFORTS = new Set(["low", "medium", "high"]);
/**
 * A review reason is a bounded token, not a member of a closed set.
 *
 * The canonical router renders the two numeric §6 triggers with the value that
 * fired them — `risk 80 >= 40`, `irreversibility 4 >= 4` — so a reader is told
 * the fact rather than the rule (`crates/beekeeper-core/src/coding_session_routing.rs:1555`).
 * A closed vocabulary here would make this observer reject the router's own
 * record as malformed, which is exactly the kind of lie that shows up as
 * "the seat says nothing about why it is that model".
 */
const MAX_ROUTING_REVIEW_REASONS = 16;
const MAX_ROUTING_TOKEN_BYTES = 256;
/** One sentence, bounded. Mirrors `MAX_ROUTING_DISAGREEMENT_BYTES`. */
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
 * The `routing` record, closed and checked.
 *
 * Mirrors `isStrictCodingSessionRoutingRecord` in
 * `features/coding-sessions/lib/codingSessionRouting.ts`, and is written out
 * again here rather than imported because this module is loaded by the
 * conformance binder under plain `node --test` and stays dependency-free.
 *
 * Two checks are worth naming, because both are the honesty of the record
 * rather than its shape:
 *
 * - `risk.score` must equal `impact × uncertainty × irreversibility`. A record
 *   whose score disagrees with its own factors is not a rounding difference,
 *   it is a claim nobody can reproduce.
 * - `chosen.effort` must be one the router is allowed to buy. `xhigh`, `max`
 *   and `ultra` are human-override only (spec §2); a record asserting one as
 *   a routed effort is refused rather than displayed.
 *
 * `class` and `tier` are bounded tokens, not closed sets: `bounded_token`
 * checks both (`RoutingRecord::validate`, coding_session_routing.rs:1314-1316)
 * against the registry's own config-driven names
 * (`Registry::tiers: BTreeMap<String, TierPolicy>`, `::classes`), not a Rust
 * enum — a registry that adds a tier or a class does not recompile this
 * reader, so pinning either here would silently drop it.
 */
export function hasStrictRoutingRecord(value: unknown): boolean {
  if (!isPlainObject(value)) return false;
  const allowed = new Set([
    ...ROUTING_RECORD_FIELDS,
    "profile",
    "proposedDisagreement",
  ]);
  if (Object.keys(value).some((key) => !allowed.has(key))) return false;
  if (ROUTING_RECORD_FIELDS.some((key) => !Object.hasOwn(value, key))) {
    return false;
  }
  if (!boundedString(value.class, MAX_ROUTING_TOKEN_BYTES)) return false;
  if (!boundedString(value.tier, MAX_ROUTING_TOKEN_BYTES)) return false;
  if (!isRoutingRisk(value.risk)) return false;
  if (!isRoutingTarget(value.chosen)) return false;
  if (value.runnerUp !== null && !isRoutingTarget(value.runnerUp)) return false;
  if (!boundedString(value.reason, MAX_REFERENCE_BYTES)) return false;
  if (typeof value.reviewRequired !== "boolean") return false;
  if (
    !Array.isArray(value.reviewReasons) ||
    value.reviewReasons.length > MAX_ROUTING_REVIEW_REASONS ||
    value.reviewReasons.some(
      (entry) => !boundedString(entry, MAX_ROUTING_TOKEN_BYTES),
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
    Object.hasOwn(value, "proposedDisagreement") &&
    !boundedString(value.proposedDisagreement, MAX_ROUTING_DISAGREEMENT_BYTES)
  ) {
    return false;
  }
  // `profile: null` is the answered-with-nothing shape the canonical producer
  // writes (`Routing::profile` has no `skip_serializing_if`), so an observer
  // that accepted only an object refused every record the CLI ever wrote.
  if (!Object.hasOwn(value, "profile") || value.profile === null) return true;
  // A trait name is `bounded_token`-checked (`validate_profile`,
  // coding_session_routing.rs:1233-1250), not a member of a closed
  // vocabulary — a lead may ask for a minimum on any trait it names, and a
  // registry can score a trait this build has never heard of.
  return (
    isPlainObject(value.profile) &&
    Object.entries(value.profile).every(
      ([trait, minimum]) =>
        boundedString(trait, MAX_ROUTING_TOKEN_BYTES) &&
        typeof minimum === "number" &&
        Number.isFinite(minimum) &&
        minimum >= 1 &&
        minimum <= 5,
    )
  );
}

function isRoutingRisk(value: unknown): boolean {
  if (!isPlainObject(value)) return false;
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
    isPlainObject(value) &&
    Object.keys(value).length === 3 &&
    boundedString(value.provider, MAX_REFERENCE_BYTES) &&
    boundedString(value.model, MAX_REFERENCE_BYTES) &&
    ROUTING_EFFORTS.has(value.effort as string)
  );
}

function isRoutingOverride(value: unknown): boolean {
  if (!isPlainObject(value)) return false;
  if (
    Object.keys(value).some(
      (key) => !["model", "effort", "because"].includes(key),
    )
  ) {
    return false;
  }
  if (!boundedString(value.model, MAX_REFERENCE_BYTES)) return false;
  if (!boundedString(value.because, MAX_REFERENCE_BYTES)) return false;
  // `null` is the canonical "take the tier's effort": buzz-core writes
  // `RoutingOverride.effort` unconditionally
  // (crates/beekeeper-core/src/coding_session_routing.rs:1005), so an override on
  // the wire always carries the key, explicitly null when unstated. A
  // non-null value is a `bounded_token`, not the router's closed
  // `low`/`medium`/`high` set (`validate_override`, :1276-1283) — `xhigh`,
  // `max` and `ultra` are exactly what a human override is for.
  return (
    !Object.hasOwn(value, "effort") ||
    value.effort === null ||
    boundedString(value.effort, MAX_ROUTING_TOKEN_BYTES)
  );
}
