/**
 * Closed-shape and duplicate-key checks for the signed coding-session facts.
 *
 * Shared, not Pulse's: every surface that reads a 44221/44223/44224/24223/44230
 * decodes it through these, so "this event is well-formed" has exactly one
 * answer in the app. Deliberately dependency-free and erasable-syntax-only —
 * the conformance binder loads this file under plain `node --test`.
 */

import {
  boundedNullable,
  boundedString,
  encoder,
  MAX_REFERENCE_BYTES,
  hasExactFields,
  hasStrictRoutingRecord,
  isPlainObject,
} from "./sessionCoordinationJsonShapes";

// Re-exported so every existing importer of this module keeps working: the
// lane-216 split moved where these live, not what they mean.
export {
  hasExactFields,
  hasStrictRoutingRecord,
} from "./sessionCoordinationJsonShapes";

const MAX_IDENTIFIER_BYTES = 256;
/** `1024 + '…'.len_utf8()` — the exact bound `validate_lifecycle_receipt`
 * sets on a receipt error message (`coding_session_payload.rs:829`). */
const MAX_RECEIPT_ERROR_MESSAGE_BYTES = 1024 + 3;
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
  // `session.restart` carries the identical three-key target form
  // (`decode_coding_session_lifecycle_command` matches
  // `"session.resume" | "session.restart" | "session.stop"` in one arm), and
  // this app **writes** one — `buildCodingSessionRestartEvent`,
  // `features/coding-sessions/lib/codingSessionLifecycleCommand.ts:411`. Until
  // lane 216 its only strict reader refused to read its own signed command.
  //
  // `session.hire` is deliberately absent and is not a divergence: a hire
  // names no `providerAuthorityPubkey` at all — it asks a host to choose one —
  // so it is read by the sibling decoder `readLifecycleHire`
  // (`sessionCoordinationCommissioning.ts`), which says so in its own words.
  return (
    (action.type === "session.resume" ||
      action.type === "session.restart" ||
      action.type === "session.stop") &&
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
      boundedString(content.error.message, MAX_RECEIPT_ERROR_MESSAGE_BYTES)
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
      boundedString(content.error.message, MAX_RECEIPT_ERROR_MESSAGE_BYTES)
    );
  }
  // Every turn-stage status (`turn_queued`, `turn_started`,
  // `continuation_registered`, …) is rejected here on purpose: this reader is
  // scoped to the lifecycle vocabulary the coordination fold cares about, and
  // the caller (`readLifecycleReceipt`, `sessionCoordinationFold.ts`) further
  // discriminates by the `csl-command`/`cslr-v` tag pair a turn receipt never
  // carries. Turn-stage receipts decode through
  // `codingSessionIngressPayloads.ts` instead.
  //
  // `continuation_registered` is named because it is the newest and the
  // easiest to mistake for a lifecycle fact: it is a stage of one 44220 that
  // says a CI continuation was stored, so it must not create, confirm, or end
  // a generation here any more than a `turn_queued` does.
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
 * Seven independent additive amendments have landed on the metadata payload —
 * the `sessionRef` echo, the agent seat's `role`, D9's `turnBudget`, B1's four
 * coordinate facts (which travel all-four-or-none), the 2026-08-30 `routing`
 * record, `beeStamp` (`crates/buzz-core/src/coding_session_payload.rs:989`,
 * documented there in Rust's own words as "the sixth independent additive
 * key"), and `packRef` (LANE-L23, the seventh) — and each is present or
 * absent on its own, so the base key set has **128** valid shapes, not sixty-
 * four (this list drifted to five amendments / thirty-two shapes when
 * `beeStamp` shipped without a matching bit here, rejecting every 44223 that
 * carried one — finding 31's own class, guarded against here by adding
 * `packRef`'s bit in the same lane that adds the key to the decoder). The
 * eighth is `handover` (§3.1): a fenced provider advertises its execution as
 * `disconnected` **and** says who fenced it, so the desktop reads the fence
 * from the coordination fold rather than from a second query — 256 shapes.
 * The ninth is `composeRef` (spec § 4.6), independent on the wire — 512
 * shapes — though `validate_session_metadata` refuses it without `packRef`, so
 * this reader refuses it alone too. It was missing here until lane 216, and
 * the provider emits it for every seat staged from a composed pack
 * (`seat_compose_ref`, `crates/buzz-session-provider/src/lib.rs`), so this
 * gate refused live sessions the session decoder beside it accepted — the
 * one direction the parity rule forbids (ledger 216).
 * Enumerating fewer silently drops every event carrying an amendment this
 * list forgot, which is a whole-surface outage rather than a strictness
 * nuance: the reader sees no sessions at all.
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
    ["packRef"],
    ["handover"],
    ["composeRef"],
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

const EXACT_SHA = /^[0-9a-f]{40}$/;
const PACK_REF_ROLE_SLUG = /^[a-z0-9-]{1,64}$/;
/** `30617:<64-hex>:<dtag>` — a git repository announcement coordinate. */
const PACK_REF_REPO_COORD = /^30617:[0-9a-f]{64}:[a-zA-Z0-9._-]{1,200}$/;
/** The literal `repo` value a shipped-defaults `packRef` carries. */
const PACK_REF_SHIPPED_REPO = "app:shipped";
/** A loose semver-ish app version — digits/dots, optional `-`/`+` suffix. */
const PACK_REF_APP_VERSION = /^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$/;

/**
 * A `packRef` object: exactly the four keys `PackRef`
 * (LANE-L23, `crates/buzz-core/src/coding_session_payload.rs`) carries, each
 * on its own terms. `sha` is exact 40-hex (never the 7-40 shorthand
 * `beeStamp` allows — a pack is pinned to one commit, never a prefix), `role`
 * is the wire's own role-slug shape, and `repo` is a `30617:<owner>:<id>`
 * coordinate — or, since the setup-lives-inside-the-app addendum
 * (2026-09-03), the literal `app:shipped` paired with a version-shaped `sha`
 * instead of a commit, the app's own bundled-packs fallback. Mirrors the
 * desktop's own `readPackRef` (`codingSessionPackRef.ts`) exactly, so the two
 * decoders agree.
 */
/**
 * A `handover` object: exactly the four keys `SessionMetadataHandover`
 * (`crates/buzz-core/src/coding_session_payload.rs`, `deny_unknown_fields`)
 * carries, each a lowercase 64-hex id.
 *
 * Same omit-when-absent contract as `beeStamp` and `packRef`, and for the
 * same reason: `validate_session_metadata` refuses an explicit `null`
 * outright ("coding-session metadata handover must not be null"), so a null
 * here is a shape no real host emits — accepting it would let the desktop
 * render a session buzz-core considers malformed.
 *
 * A partial object is refused rather than read loosely: a fence naming a
 * claimant but no body is not a fence this reader can act on.
 */
function isMetadataHandover(value: unknown): boolean {
  return (
    isPlainObject(value) &&
    hasExactFields(value, [
      ["state", "claimant", "bodyPubkey", "acceptedEventId"],
    ]) &&
    // Closed, and the field that keeps a **voided** claim from reading as a
    // live one: there is no "none" token, because absence of the whole key is
    // how a provider says no claim stands.
    (value.state === "active" || value.state === "voided") &&
    typeof value.claimant === "string" &&
    HEX64_PUBKEY.test(value.claimant) &&
    typeof value.bodyPubkey === "string" &&
    HEX64_PUBKEY.test(value.bodyPubkey) &&
    typeof value.acceptedEventId === "string" &&
    HEX64_PUBKEY.test(value.acceptedEventId)
  );
}

const HEX64_PUBKEY = /^[0-9a-f]{64}$/;

/** `composeRef.appVersion`: 1..=64 bytes, non-blank. Mirrors Rust exactly. */
const MAX_COMPOSE_REF_APP_VERSION_BYTES = 64;
/** `composeRef.digest`: `sha256:` and exactly 64 lowercase hex characters. */
const COMPOSE_REF_DIGEST = /^sha256:[0-9a-f]{64}$/;

/**
 * A `composeRef` object: exactly the two keys `ComposeRef`
 * (`crates/buzz-core/src/coding_session_payload.rs`, `deny_unknown_fields`)
 * carries. `packRef` names the source bytes; this names the app version whose
 * template catalog resolved the includes and the digest of what actually ran,
 * so "which prompt ran" stays answerable after the app updates its templates.
 *
 * Same omit-when-absent contract as every amendment since `routing`: Rust
 * refuses an explicit `null` by name, and refuses the key without a `packRef`
 * at all, because a composition of nothing is not a fact.
 */
function isComposeRef(value: unknown): boolean {
  return (
    isPlainObject(value) &&
    hasExactFields(value, [["appVersion", "digest"]]) &&
    typeof value.appVersion === "string" &&
    value.appVersion.trim().length > 0 &&
    encoder.encode(value.appVersion).length <=
      MAX_COMPOSE_REF_APP_VERSION_BYTES &&
    typeof value.digest === "string" &&
    COMPOSE_REF_DIGEST.test(value.digest)
  );
}

function isPackRef(value: unknown): boolean {
  if (
    !isPlainObject(value) ||
    !hasExactFields(value, [["repo", "sha", "role", "path"]]) ||
    typeof value.repo !== "string" ||
    typeof value.sha !== "string" ||
    typeof value.role !== "string" ||
    !PACK_REF_ROLE_SLUG.test(value.role) ||
    typeof value.path !== "string" ||
    value.path.length === 0 ||
    value.path.length > 512
  ) {
    return false;
  }
  if (value.repo === PACK_REF_SHIPPED_REPO) {
    return PACK_REF_APP_VERSION.test(value.sha);
  }
  return PACK_REF_REPO_COORD.test(value.repo) && EXACT_SHA.test(value.sha);
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
  // An explicit `null` is refused, not short-circuited past: `routing` was
  // introduced with an omit-when-absent writer contract and
  // `decode_coding_session_metadata` rejects `"routing": null` **naming the
  // key**, so accepting it here would let this gate admit bytes the relay and
  // every Rust consumer call malformed (ledger 216). Unlike `role` and
  // `turnBudget`, whose serde `Option`s do read a null as absent.
  if (
    Object.hasOwn(content, "routing") &&
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
  // Same omit-when-absent contract as `beeStamp`: `packRef`'s producer never
  // writes an explicit `null` either, so a null here is a shape this build
  // has never seen a real host emit, not "no pack staged".
  if (
    Object.hasOwn(content, "packRef") &&
    (content.packRef === null || !isPackRef(content.packRef))
  ) {
    return false;
  }
  // §3.1's fence disclosure. A present-but-malformed object — including an
  // explicit `null`, which Rust refuses by name — is refused rather than
  // ignored: reading a half-written fence as "not fenced" is the exact lie
  // this decoder exists to prevent.
  if (
    Object.hasOwn(content, "handover") &&
    !isMetadataHandover(content.handover)
  ) {
    return false;
  }
  // Spec § 4.6's composition provenance. Refused alone, as null, or
  // malformed — exactly `validate_session_metadata`'s rule, which names
  // `composeRef requires a packRef: it describes how that pack was composed`.
  if (
    Object.hasOwn(content, "composeRef") &&
    (!Object.hasOwn(content, "packRef") || !isComposeRef(content.composeRef))
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
