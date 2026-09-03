/**
 * Closed-shape and duplicate-key checks for the signed coding-session facts.
 *
 * Shared, not Pulse's: every surface that reads a 44221/44223/44224/24223/44230
 * decodes it through these, so "this event is well-formed" has exactly one
 * answer in the app. Deliberately dependency-free and erasable-syntax-only —
 * the conformance binder loads this file under plain `node --test`.
 */

function isPlainObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

const encoder = new TextEncoder();
const MAX_IDENTIFIER_BYTES = 256;
const MAX_REFERENCE_BYTES = 2 * 1024;
// Closed and kept closed: this is `SessionStatus`, a `#[serde(rename_all =
// "snake_case")]` Rust enum (crates/buzz-core/src/coding_session_payload.rs),
// not a config-driven string — the ten variants here are exactly its ten, and
// a wire value can only widen this set by a Rust change on the other side,
// which moves this list too. It is also the state machine every consumer
// keys UI off of, so an unrecognized value must surface as "this build does
// not know this status" rather than be silently accepted as some default.
const SESSION_STATUSES = new Set([
  "starting",
  "idle",
  "running",
  "waiting_for_input",
  "completed",
  "stopped",
  "failed",
  "interrupted",
  "disconnected",
  "unknown",
]);

function boundedString(value: unknown, maxBytes: number): value is string {
  return (
    typeof value === "string" &&
    value.trim().length > 0 &&
    encoder.encode(value).length <= maxBytes
  );
}

function boundedNullable(value: unknown, maxBytes: number): boolean {
  return value === null || boundedString(value, maxBytes);
}

function isCanonicalSessionRef(value: unknown): value is string {
  return (
    typeof value === "string" &&
    /^[0-9a-f]{8}-[0-9a-f]{4}-[1-5][0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      value,
    )
  );
}

/** Values of an exact target, including Rust's bounds and control rejection. */
export function hasStrictSessionTargetValues(
  value: unknown,
): value is Record<string, unknown> {
  return (
    hasExactSessionTargetFields(value) &&
    boundedString(value.driver, MAX_IDENTIFIER_BYTES) &&
    boundedString(value.instanceId, MAX_IDENTIFIER_BYTES) &&
    boundedString(value.sessionId, MAX_IDENTIFIER_BYTES) &&
    ![value.driver, value.instanceId, value.sessionId].some((identifier) =>
      [...(identifier as string)].some((character) =>
        /\p{Cc}/u.test(character),
      ),
    ) &&
    Number.isSafeInteger(value.generation) &&
    (value.generation as number) > 0
  );
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

// Closed and kept closed: mirrors `CodingSessionTarget`
// (crates/buzz-core/src/coding_session_command.rs), which is itself
// `#[serde(deny_unknown_fields)]` — the four fields are the exact identity of
// one live provider execution a signed command addresses, so a fifth field
// here would be routing surface the relay does not itself recognize either.
/** Exact four-field shape shared by command, receipt, and lease targets. */
export function hasExactSessionTargetFields(
  value: unknown,
): value is Record<string, unknown> {
  return hasExactFields(value, [
    ["driver", "instanceId", "sessionId", "generation"],
  ]);
}

/** Strict lifecycle-command object shapes, including additive create forms. */
export function hasStrictLifecycleCommandJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  if (
    hasDuplicateJsonKeys(source) ||
    !hasExactFields(content, [["schema", "commandId", "action"]]) ||
    !isPlainObject(content.action)
  ) {
    return false;
  }
  const action = content.action;
  if (action.type === "session.create") {
    const base = [
      "type",
      "projectRef",
      "repoRef",
      "providerInstanceRef",
      "providerAuthorityPubkey",
      "model",
      "title",
      "initialTurn",
    ];
    const unseated = [
      base,
      [...base.slice(0, 3), "sessionRef", ...base.slice(3)],
      [...base.slice(0, 3), "sessionRef", "genesisRef", ...base.slice(3)],
    ];
    // The seated forms buzz-core accepts: every unseated form plus the
    // `actor`/`role` pair, which travels together or not at all.
    const seatForms = [
      ...unseated,
      ...unseated.map((form) => [...form, "actor", "role"]),
    ];
    // The 2026-09-01 attribution amendment: `hireRef` names the 44221 hire a
    // seated create answers. Trailing and independent of the seat pair and of
    // routing, exactly as buzz-core enumerates it — 3 bases x seated x
    // attributed x routed = 24 accepted shapes
    // (`coding_session_lifecycle_command.rs`, matrix at REPORT-B1 2.3).
    const hireForms = [
      ...seatForms,
      ...seatForms.map((form) => [...form, "hireRef"]),
    ];
    // The 2026-08-30 routing amendment, trailing and independent — so every
    // seat form doubles rather than being replaced. Enumerating fewer would
    // drop every routed create on the floor, which on this surface is not a
    // strictness nuance but a blank session list.
    return hasExactFields(action, [
      ...hireForms,
      ...hireForms.map((form) => [...form, "routing"]),
    ]);
  }
  return (
    (action.type === "session.resume" || action.type === "session.stop") &&
    hasExactFields(action, [["type", "session", "providerAuthorityPubkey"]])
  );
}

/** Validate lifecycle values after the closed JSON shape has been established. */
export function hasStrictLifecycleCommandValues(
  content: Record<string, unknown>,
): boolean {
  if (
    content.schema !== "buzz-coding-session-lifecycle-command/v1" ||
    !boundedString(content.commandId, MAX_IDENTIFIER_BYTES) ||
    !isPlainObject(content.action) ||
    typeof content.action.providerAuthorityPubkey !== "string" ||
    !/^[0-9a-f]{64}$/.test(content.action.providerAuthorityPubkey)
  ) {
    return false;
  }
  const action = content.action;
  if (action.type === "session.create") {
    const sessionRefValid =
      !Object.hasOwn(action, "sessionRef") ||
      action.sessionRef === null ||
      isCanonicalSessionRef(action.sessionRef);
    const genesisValid =
      !Object.hasOwn(action, "genesisRef") ||
      (isCanonicalSessionRef(action.sessionRef) &&
        typeof action.genesisRef === "string" &&
        /^[0-9a-f]{64}$/.test(action.genesisRef));
    const seatValid =
      !Object.hasOwn(action, "actor") ||
      (typeof action.actor === "string" &&
        /^[0-9a-f]{64}$/.test(action.actor) &&
        typeof action.role === "string" &&
        isRoleSlug(action.role));
    // Absent is not null. A key-set check sees an explicit null as *present*,
    // so without this line the same bytes would mean "no hire" to this reader
    // and "malformed" to buzz-core's strict decoder. Uppercase is rejected,
    // never coerced: these ids are compared byte-for-byte against signed
    // facts.
    const hireRefValid =
      !Object.hasOwn(action, "hireRef") ||
      (typeof action.hireRef === "string" &&
        /^[0-9a-f]{64}$/.test(action.hireRef));
    return (
      boundedNullable(action.projectRef, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.repoRef, MAX_REFERENCE_BYTES) &&
      boundedString(action.providerInstanceRef, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.model, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.title, MAX_REFERENCE_BYTES) &&
      boundedNullable(action.initialTurn, 12 * 1024) &&
      sessionRefValid &&
      genesisValid &&
      seatValid &&
      hireRefValid &&
      (!Object.hasOwn(action, "routing") ||
        hasStrictRoutingRecord(action.routing))
    );
  }
  return hasStrictSessionTargetValues(action.session);
}

/** Strict lifecycle-receipt top-level and nested object shapes. */
export function hasStrictLifecycleReceiptJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  return (
    !hasDuplicateJsonKeys(source) &&
    hasExactFields(content, [
      ["schema", "commandId", "status", "session", "error"],
    ]) &&
    (content.session === null ||
      hasExactSessionTargetFields(content.session)) &&
    (content.error === null ||
      hasExactFields(content.error, [["code", "message"]]))
  );
}

/** Validate receipt values and the status/session/error coupling. */
export function hasStrictLifecycleReceiptValues(
  content: Record<string, unknown>,
): boolean {
  if (
    content.schema !== "buzz-coding-session-lifecycle-receipt/v1" ||
    !boundedString(content.commandId, MAX_IDENTIFIER_BYTES) ||
    typeof content.status !== "string"
  ) {
    return false;
  }
  // A create that never reached a session names none: `failed` is the one
  // lifecycle status whose `session` is `None`
  // (`LifecycleReceipt::failed`, crates/buzz-core/src/coding_session_payload.rs:338).
  // Every other status this reader accepts always carries a target
  // (`validate_lifecycle_receipt`, coding_session_payload.rs:629-636), so the
  // session-target check runs only past this branch. `code` here is a bounded
  // identifier, not a closed set — the same callers that produce it
  // (`PROVIDER_UNAVAILABLE`, `SESSION_LIMIT`, …) show it is open, and Rust's
  // own general error check only bounds its length.
  if (content.status === "failed") {
    return (
      content.session === null &&
      isPlainObject(content.error) &&
      boundedString(content.error.code, MAX_IDENTIFIER_BYTES) &&
      boundedString(content.error.message, 1024 + 3)
    );
  }
  if (!hasStrictSessionTargetValues(content.session)) return false;
  // `stopped` is a plain terminal receipt exactly like `created`/`resumed` —
  // a session, no error (`LifecycleReceipt::stopped`, coding_session_payload.rs:384).
  if (
    content.status === "created" ||
    content.status === "resumed" ||
    content.status === "stopped"
  ) {
    return content.error === null;
  }
  if (
    content.status === "created_with_failed_initial_turn" ||
    content.status === "resumed_without_context"
  ) {
    if (!isPlainObject(content.error)) return false;
    const expectedCode =
      content.status === "created_with_failed_initial_turn"
        ? "INITIAL_TURN_FAILED"
        : "CONTEXT_NOT_RECOVERED";
    return (
      content.error.code === expectedCode &&
      boundedString(content.error.message, 1024 + 3)
    );
  }
  // Every turn-stage status (`turn_queued`, `turn_started`, …) is rejected
  // here on purpose: this reader is scoped to the lifecycle vocabulary the
  // coordination fold cares about, and the caller (`readLifecycleReceipt`,
  // `sessionCoordinationFold.ts`) further discriminates by the `csl-command`/
  // `cslr-v` tag pair a turn receipt never carries. Turn-stage receipts
  // decode through `codingSessionIngressPayloads.ts` instead.
  return false;
}

/** Strict lease top-level and nested target shapes. */
export function hasStrictLeaseJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  return (
    !hasDuplicateJsonKeys(source) &&
    hasExactFields(content, [["schema", "target", "state", "leaseSequence"]]) &&
    hasExactSessionTargetFields(content.target)
  );
}

/** Validate lease values after its closed shape and duplicate-key checks. */
export function hasStrictLeaseValues(
  content: Record<string, unknown>,
): boolean {
  // `state` closed and kept closed: `CodingSessionLeaseState`
  // (crates/buzz-core/src/coding_session_lease.rs) is a two-variant Rust enum,
  // and the value is the authority fact this lease exists to assert — whether
  // a provider currently owns a reachable live actor for this generation.
  return (
    content.schema === "buzz-coding-session-lease/v1" &&
    hasStrictSessionTargetValues(content.target) &&
    (content.state === "live" || content.state === "released") &&
    Number.isSafeInteger(content.leaseSequence) &&
    (content.leaseSequence as number) > 0
  );
}

/**
 * Every field set `buzz-core`'s `decode_coding_session_metadata` accepts.
 *
 * Six independent additive amendments have landed on the metadata payload —
 * the `sessionRef` echo, the agent seat's `role`, D9's `turnBudget`, B1's four
 * coordinate facts (which travel all-four-or-none), the 2026-08-30 `routing`
 * record, and `beeStamp` (`crates/buzz-core/src/coding_session_payload.rs:989`,
 * documented there in Rust's own words as "the sixth independent additive
 * key") — and each is present or absent on its own, so the base key set has
 * **sixty-four** valid shapes, not five (this list drifted to five amendments
 * / thirty-two shapes when `beeStamp` shipped without a matching bit here,
 * rejecting every 44223 that carried one). Enumerating fewer silently drops
 * every event carrying an amendment this list forgot, which is a
 * whole-surface outage rather than a strictness nuance: the reader sees no
 * sessions at all.
 */
function metadataFieldForms(): string[][] {
  const base = [
    "schema",
    "session",
    "projectRef",
    "repoRef",
    "title",
    "agentRef",
    "provider",
    "runtime",
    "model",
    "status",
    "branch",
    "capabilities",
  ];
  const amendments = [
    ["sessionRef"],
    ["role"],
    ["turnBudget"],
    METADATA_FACT_FIELDS,
    ["routing"],
    ["beeStamp"],
  ];
  const forms: string[][] = [];
  for (let mask = 0; mask < 1 << amendments.length; mask += 1) {
    const form = [...base];
    for (const [index, keys] of amendments.entries()) {
      if (mask & (1 << index)) form.push(...keys);
    }
    forms.push(form);
  }
  return forms;
}

const METADATA_FACT_FIELDS = [
  "observedCommit",
  "dirty",
  "relayReachable",
  "verifiedAt",
];
const METADATA_FIELD_FORMS = metadataFieldForms();
const MAX_ROLE_SLUG_BYTES = 64;

/** A seat's role slug: `[a-z0-9-]`, 1..=64 bytes, exactly as Rust reads it. */
function isRoleSlug(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    encoder.encode(value).length <= MAX_ROLE_SLUG_BYTES &&
    /^[a-z0-9-]+$/.test(value)
  );
}

/**
 * A `turnBudget` object: exactly `{used, limit}`, and only beside an umbrella.
 *
 * A budget is a fact about an umbrella, so it cannot describe an execution
 * that claimed none, and a `limit` of zero would read as "no turn may ever
 * pass" rather than "unbudgeted" — the producer omits the key instead. Both
 * are the rejections `validate_session_metadata` makes.
 */
function isTurnBudget(value: unknown, sessionRef: unknown): boolean {
  return (
    typeof sessionRef === "string" &&
    hasExactFields(value, [["used", "limit"]]) &&
    Number.isSafeInteger(value.used) &&
    (value.used as number) >= 0 &&
    Number.isSafeInteger(value.limit) &&
    (value.limit as number) > 0
  );
}

const BEE_STAMP_SOURCES = new Set(["bundled", "path"]);
const SHORT_SHA = /^[0-9a-f]{7,40}$/;

/**
 * A `beeStamp` object: exactly the five keys `BeeStamp`
 * (`crates/buzz-core/src/coding_session_payload.rs:1030`, `deny_unknown_fields`)
 * carries, each on its own terms. `source` is a genuine two-variant Rust enum
 * (`BeeStampSource`) — closed and kept closed, unlike `capabilities` or
 * `routing`'s open tokens, because a third resolution outcome is a Rust change
 * this file would need to grow with. `sha`'s hex-and-length check mirrors the
 * desktop's own `readSeatBeeStamp` (`codingSessionSeatBee.ts`) rather than
 * Rust's plain `Option<String>`, so the two decoders agree exactly rather than
 * this one being looser.
 */
function isBeeStamp(value: unknown): boolean {
  return (
    isPlainObject(value) &&
    hasExactFields(value, [["path", "source", "version", "sha", "dirty"]]) &&
    typeof value.path === "string" &&
    value.path.length > 0 &&
    BEE_STAMP_SOURCES.has(value.source as string) &&
    (value.version === null || typeof value.version === "string") &&
    (value.sha === null ||
      (typeof value.sha === "string" && SHORT_SHA.test(value.sha))) &&
    (value.dirty === null || typeof value.dirty === "boolean")
  );
}

/** Strict metadata shape/value check matching buzz-core's sixteen forms. */
export function hasStrictMetadataJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  if (
    encoder.encode(source).length > 32 * 1024 ||
    hasDuplicateJsonKeys(source) ||
    !hasExactFields(content, METADATA_FIELD_FORMS) ||
    content.schema !== "buzz-coding-session-metadata/v1" ||
    !hasStrictSessionTargetValues(content.session) ||
    !boundedNullable(content.projectRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.repoRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.title, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.agentRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.provider, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.runtime, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.model, MAX_REFERENCE_BYTES) ||
    !boundedNullable(content.branch, MAX_REFERENCE_BYTES) ||
    !SESSION_STATUSES.has(content.status as string)
  ) {
    return false;
  }
  if (
    Object.hasOwn(content, "sessionRef") &&
    !isCanonicalSessionRef(content.sessionRef)
  ) {
    return false;
  }
  // An explicit `null` is how serde's `Option` reads an absent value, and the
  // Rust decoder validates neither key when it holds one — so neither does
  // this. A non-null `role` labels a seat, so it needs the actor that holds
  // it; a non-null `turnBudget` needs the umbrella it bounds.
  if (
    Object.hasOwn(content, "role") &&
    content.role !== null &&
    (typeof content.agentRef !== "string" || !isRoleSlug(content.role))
  ) {
    return false;
  }
  if (
    Object.hasOwn(content, "turnBudget") &&
    content.turnBudget !== null &&
    !isTurnBudget(content.turnBudget, content.sessionRef)
  ) {
    return false;
  }
  if (
    Object.hasOwn(content, "routing") &&
    content.routing !== null &&
    !hasStrictRoutingRecord(content.routing)
  ) {
    return false;
  }
  // Same omit-when-absent contract as `routing`: `beeStamp`'s producer never
  // writes an explicit `null` ("coding-session metadata beeStamp must not be
  // null", `decode_coding_session_metadata`), so a null here is a shape this
  // build has never seen a real host emit, not "no stamp".
  if (
    Object.hasOwn(content, "beeStamp") &&
    (content.beeStamp === null || !isBeeStamp(content.beeStamp))
  ) {
    return false;
  }
  // `capabilities` is an open map of booleans, not a closed set. It carries
  // no authority and decides no protocol enum the relay enforces — it is a
  // provider's self-report of what its own execution can do, read straight
  // off `Capabilities` (`crates/buzz-core/src/coding_session_payload.rs`),
  // whose fields grow by plain Rust struct addition (`prompt_image` in
  // 15bbe6158, `#[serde(default)]`, no `deny_unknown_fields`). A closed key
  // set here silently drops every event from a host newer than this build —
  // exactly finding 34: 209 of 1,500 live 44223s rejected the day
  // `promptImage` shipped. So the six named keys stay required (a producer
  // that dropped one would be a different regression), a boolean-valued key
  // this build has never heard of is ignored, and a non-boolean value on any
  // key, named or not, is still refused — that is a shape violation
  // regardless of what the key is called.
  const capabilityKeys = [
    "threadTurnStart",
    "threadTurnInterrupt",
    "threadSteer",
    "context",
    "diff",
    "plan",
  ];
  const capabilities = content.capabilities;
  if (
    !isPlainObject(capabilities) ||
    !capabilityKeys.every((key) => Object.hasOwn(capabilities, key)) ||
    !Object.values(capabilities).every(
      (entryValue) => typeof entryValue === "boolean",
    )
  ) {
    return false;
  }
  if (!Object.hasOwn(content, "observedCommit")) return true;
  return (
    boundedNullable(content.observedCommit, MAX_REFERENCE_BYTES) &&
    (content.dirty === null || typeof content.dirty === "boolean") &&
    (content.relayReachable === null ||
      typeof content.relayReachable === "boolean") &&
    (content.verifiedAt === null || Number.isSafeInteger(content.verifiedAt)) &&
    (content.relayReachable === null) === (content.verifiedAt === null)
  );
}

/** Parse and strictly validate one metadata content string without throwing. */
export function isStrictMetadataContent(source: string): boolean {
  try {
    return hasStrictMetadataJson(source, JSON.parse(source));
  } catch {
    return false;
  }
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
 * the fact rather than the rule (`crates/buzz-core/src/coding_session_routing.rs:1555`).
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
  // (crates/buzz-core/src/coding_session_routing.rs:1005), so an override on
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

// `action` closed and kept closed: `CodingSessionClosureAction`
// (crates/buzz-core/src/coding_session_closure.rs) is a three-variant Rust
// enum, and the value decides the umbrella's shared ownership/settled state —
// the exact authority fact `CodingSessionClosurePayload` exists to carry.
/** Strict closure content check matching buzz-core's decoder. */
export function hasStrictClosureJson(
  source: string,
  content: unknown,
): content is Record<string, unknown> {
  return (
    encoder.encode(source).length <= 512 &&
    !hasDuplicateJsonKeys(source) &&
    hasExactFields(content, [["action", "genesisRef", "sessionRef", "v"]]) &&
    (content.action === "closed" ||
      content.action === "open" ||
      content.action === "archived") &&
    typeof content.genesisRef === "string" &&
    /^[0-9a-f]{64}$/.test(content.genesisRef) &&
    isCanonicalSessionRef(content.sessionRef) &&
    content.v === 1
  );
}

/** Detect duplicate object keys at any depth before `JSON.parse` overwrites. */
export function hasDuplicateJsonKeys(source: string): boolean {
  const stack: Array<{ keys: Set<string> | null; wantsKey: boolean }> = [];
  for (let index = 0; index < source.length; index += 1) {
    const token = source[index];
    if (token === '"') {
      let end = index + 1;
      while (end < source.length && source[end] !== '"') {
        end += source[end] === "\\" ? 2 : 1;
      }
      const frame = stack.at(-1);
      if (frame?.keys && frame.wantsKey) {
        let decoded: unknown;
        try {
          decoded = JSON.parse(source.slice(index, end + 1));
        } catch {
          return true;
        }
        if (typeof decoded !== "string" || frame.keys.has(decoded)) return true;
        frame.keys.add(decoded);
        frame.wantsKey = false;
      }
      index = end;
    } else if (token === "{") {
      stack.push({ keys: new Set(), wantsKey: true });
    } else if (token === "[") {
      stack.push({ keys: null, wantsKey: false });
    } else if (token === "}" || token === "]") {
      stack.pop();
    } else if (token === "," && stack.at(-1)?.keys) {
      const frame = stack.at(-1);
      if (frame) frame.wantsKey = true;
    }
  }
  return false;
}
