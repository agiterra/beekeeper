/**
 * Strict decoders for the 44224 lifecycle receipt and 44223 metadata payloads,
 * plus the semantic keys their events carry.
 */
import { isCodingSessionRoleSlug } from "./codingSessionActorSeat";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import { encodeStructuredKey } from "./codingSessionKeys";
import {
  isStrictCodingSessionRoutingRecord,
  type CodingSessionRoutingRecord,
} from "./codingSessionRouting";
import {
  type ComposeRef,
  type PackRef,
  readComposeRef,
  readPackRef,
} from "./codingSessionPackRef";
import { readSeatBeeStamp, type SeatBeeStamp } from "./codingSessionSeatBee";
import type {
  CodingSessionCapabilities,
  CodingSessionStatus,
} from "./codingSessionTypes";
import {
  boundedNonempty,
  boundedNullable,
  decodeTarget,
  hasAllOrNoneKeys,
  hasExactKeys,
  hasRequiredAndOptionalKeys,
  isCodingSessionSessionRef,
  isPlainRecord,
  parseBoundedJson,
} from "./codingSessionWireDecode";

export const CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA =
  "buzz-coding-session-lifecycle-receipt/v1" as const;
export const CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION = "cslr1-1" as const;
export const BEEKEEPER_CODING_SESSION_METADATA_SCHEMA =
  "buzz-coding-session-metadata/v1" as const;
export const CODING_SESSION_METADATA_TAG_VERSION = "csm1-1" as const;

const MAX_RECEIPT_CONTENT_BYTES = 16 * 1024;
const MAX_METADATA_CONTENT_BYTES = 32 * 1024;
const MAX_RECEIPT_COMMAND_ID_BYTES = 256;
/** A provider's own turn id, bounded exactly like the command id it answers. */
const MAX_RECEIPT_TURN_ID_BYTES = 256;
/**
 * A lifecycle receipt's error code, bounded as `validate_lifecycle_receipt`'s
 * generic check bounds it (`crates/beekeeper-core/src/coding_session_payload.rs:826`,
 * `MAX_IDENTIFIER_BYTES`).
 */
const MAX_ERROR_CODE_BYTES = 256;
/**
 * A **turn** failure code, bounded at `MAX_RECEIPT_ERROR_CODE_BYTES`
 * (`coding_session_payload.rs:278`) — 64, the narrower of the two. The
 * asymmetry is the writer's: `is_receipt_error_code` is applied only to
 * `turn_dropped`/`turn_refused`/`turn_degraded`/`turn_delivery_unknown`, whose
 * code vocabulary is deliberately open and therefore kept tight. This reader
 * used 256 for both until lane 216, so it accepted turn codes the writer's own
 * validator — and therefore the relay — call malformed.
 */
const MAX_TURN_ERROR_CODE_BYTES = 64;
/**
 * `1024 + '…'.len_utf8()` — `validate_lifecycle_receipt`
 * (`coding_session_payload.rs:829`) bounds a receipt error message at exactly
 * this, the width of a message a host truncated with an ellipsis. This reader
 * used the generic 2 KiB reference bound.
 */
const MAX_ERROR_MESSAGE_BYTES = 1024 + 3;
const MAX_REFERENCE_BYTES = 2 * 1024;
const MAX_LABEL_BYTES = 2 * 1024;

/**
 * `created_with_failed_initial_turn` is its own status because it is its own
 * fact: the session exists and is usable, but the initial turn never reached
 * the agent. A turn that reached the agent and then failed is a transcript
 * `result{error}`, not this.
 */
export type CodingSessionLifecycleReceipt =
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "created";
      session: CodingSessionCommandTarget;
      error: null;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "created_with_failed_initial_turn";
      session: CodingSessionCommandTarget;
      error: { code: "INITIAL_TURN_FAILED"; message: string };
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "failed";
      session: null;
      error: { code: string; message: string };
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "resumed" | "stopped";
      session: CodingSessionCommandTarget;
      error: null;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "resumed_without_context";
      session: CodingSessionCommandTarget;
      error: { code: "CONTEXT_NOT_RECOVERED"; message: string };
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "turn_queued";
      session: CodingSessionCommandTarget;
      error: null;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "turn_started";
      session: CodingSessionCommandTarget;
      error: null;
      /** The provider's own id for the turn that just began. */
      turnId: string;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "turn_injected";
      session: CodingSessionCommandTarget;
      error: null;
      /**
       * The provider's own id for the turn already running that this
       * `deliver: "steer"` input was injected into. Not a new turn: no
       * second `turn_started` follows, and the `user_prompt{steered: true}`
       * echo that precedes this receipt is what settles the sender's row.
       */
      turnId: string;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status:
        | "turn_dropped"
        | "turn_refused"
        | "turn_degraded"
        | "turn_delivery_unknown";
      session: CodingSessionCommandTarget;
      error: { code: string; message: string };
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "interrupt_delivered";
      session: CodingSessionCommandTarget;
      error: null;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "continuation_registered";
      session: CodingSessionCommandTarget;
      error: null;
    };

/** Every status a 44224 can carry, lifecycle and turn alike. */
export type CodingSessionLifecycleReceiptStatus =
  CodingSessionLifecycleReceipt["status"];

/**
 * The per-stage turn statuses.
 *
 * A turn receipt reports what happened to one 44220 command — a
 * `thread.turn.start`, a `thread.turn.interrupt`, or a
 * `thread.turn.continue_on_ci`; it never creates, confirms, or ends a
 * generation, which is why every fold that reads a receipt to decide a
 * generation's state must skip these.
 *
 * `continuation_registered` is the odd one and is deliberately here anyway:
 * it answers a 44220, so a generation fold must skip it — but it is **not** a
 * mailbox stage and not terminal. Nothing was queued, no turn exists, and no
 * budget was spent; the provider has only stored a registration and promised
 * to watch for one exact CI result until its `expiresAt`. If that result
 * arrives and the signer may still steer the target then, the ordinary
 * `turn_queued`/`turn_started` stages follow under the same `commandId`;
 * otherwise a `turn_refused` names why. A surface that read it as delivery
 * would report a turn nothing has accepted.
 *
 * `turn_queued`, `turn_started`, `turn_injected`, and `interrupt_delivered`
 * carry `error: null`. `turn_degraded` (`STEER_UNSUPPORTED`, `STEER_TURN_ENDED`,
 * `STEER_REJECTED`, `STEER_ATTACHMENTS_UNSUPPORTED`),
 * `turn_delivery_unknown` (`STEER_WRITE_FAILED`, `STEER_ACK_LOST`,
 * `STEER_ACK_TIMEOUT`, `STEER_ACK_UNRECOGNIZED`, `STEER_UNRESOLVED_AT_RESTART`,
 * `STEER_UNOBSERVED_NEW_TURN`), `turn_dropped` (`QUEUE_FULL`,
 * `NO_LIVE_EXECUTION`, `STEER_SATURATED`, `STEER_NOT_DELIVERED`) and `turn_refused`
 * (`UNAUTHORIZED_OPERATOR`, `UNKNOWN_TARGET`, `STALE_GENERATION`,
 * `SESSION_CLOSED`) carry a `{code, message}`. The code is read as a bounded
 * string rather than pinned to those lists, exactly as the lifecycle `failed`
 * status already is: a provider that grows a new reason must not be decoded as
 * malformed.
 *
 * `turn_degraded` is not a failure. It says the provider could not steer the
 * running turn and will run this one at the next boundary instead — the turn
 * still happens, so the row is relabelled rather than retired.
 *
 * `turn_injected` is the native steer's success: the runtime acknowledged the
 * input as joined into the turn already running, whose `turnId` it carries
 * (the second six-key status, beside `turn_started`). No new turn begins.
 *
 * `turn_delivery_unknown` is terminal and is neither a drop nor a refusal: a
 * native steer was written to the runtime and nothing establishes whether it
 * arrived. The provider will not replay it — a replay of an input the runtime
 * may already hold is the double delivery the classes exist to prevent — and
 * the sender decides whether to send again. A surface must not read it as
 * "never ran" (that would restore the words as if refused) nor as delivered.
 */
export const CODING_SESSION_TURN_RECEIPT_STATUSES = [
  "turn_queued",
  "turn_started",
  "turn_injected",
  "turn_degraded",
  "turn_delivery_unknown",
  "turn_dropped",
  "turn_refused",
  "interrupt_delivered",
  "continuation_registered",
] as const;

/** A per-stage turn status, as opposed to a generation lifecycle status. */
export type CodingSessionTurnReceiptStatus =
  (typeof CODING_SESSION_TURN_RECEIPT_STATUSES)[number];

const TURN_RECEIPT_STATUSES: ReadonlySet<string> = new Set(
  CODING_SESSION_TURN_RECEIPT_STATUSES,
);

/**
 * True for any member of {@link CODING_SESSION_TURN_RECEIPT_STATUSES},
 * false for every generation lifecycle status.
 *
 * Deliberately not a count: the list above grows, and a comment naming a
 * number goes stale silently. The same off-by-two in the Rust CLI mirror is
 * what routed turn receipts into session resolution until `158f323e`.
 */
export function isCodingSessionTurnReceiptStatus(
  status: string,
): status is CodingSessionTurnReceiptStatus {
  return TURN_RECEIPT_STATUSES.has(status);
}

/** A receipt that reports one turn stage rather than a generation change. */
export type CodingSessionTurnReceipt = Extract<
  CodingSessionLifecycleReceipt,
  { status: CodingSessionTurnReceiptStatus }
>;

/** Narrow a decoded receipt to its turn-stage half. */
export function isCodingSessionTurnReceipt(
  receipt: Readonly<CodingSessionLifecycleReceipt>,
): receipt is Readonly<CodingSessionTurnReceipt> {
  return isCodingSessionTurnReceiptStatus(receipt.status);
}

/**
 * Per-generation metadata. `projectRef` is nullable — an explicit null is a
 * standalone session — and `branch` is always null in v1: the provider never
 * re-binds a generation to a branch.
 *
 * `agentRef` is the seat's actor: the public key of the managed agent whose
 * identity the provider injected into this execution, or null for an
 * execution created by a person. `role` is that seat's role slug and is an
 * *optional* key that appears exactly when `agentRef` is non-null — a
 * metadata without an actor keeps the historical shapes byte-for-byte, so an
 * older provider's events decode unchanged.
 *
 * `sessionRef` is the umbrella session reference echoed from the create.
 * It is an *optional* key, never an explicit null: the provider emits it only
 * when the create claimed one, so pre-umbrella metadata stays byte-identical
 * and old clients lose enrichment only for umbrella-claiming sessions.
 *
 * The four code-coordinate facts (`observedCommit`, `dirty`, `relayReachable`,
 * `verifiedAt`) are the B1 amendment (`crates/beekeeper-core`
 * `coding_session_payload.rs`, `METADATA_FACT_FIELDS`): a provider that
 * observes the worktree serializes all four unconditionally (nulls included),
 * a pre-amendment provider serializes none — a partial subset is malformed,
 * never a dialect. `verifiedAt` is non-null exactly when `relayReachable` is
 * non-null: both come from the same verification probe.
 *
 * `turnBudget` is D9's crew allowance: how many turns this umbrella has
 * started (`used`) against the ceiling the provider's host set (`limit`).
 * Another optional key, never an explicit null — the provider emits it only
 * for an execution that claimed a `sessionRef` on a host that set a finite
 * budget. `used` may exceed `limit`, because the session founder is never
 * refused; that is reported as it happened rather than clamped.
 */
export type BeekeeperCodingSessionMetadataV1 = {
  schema: typeof BEEKEEPER_CODING_SESSION_METADATA_SCHEMA;
  session: CodingSessionCommandTarget;
  projectRef: string | null;
  repoRef: string | null;
  title: string | null;
  agentRef: string | null;
  role?: string;
  /**
   * The routing decision that chose this seat's execution target, echoed by
   * the provider from the create it acted on. Absent on an unrouted seat.
   */
  routing?: CodingSessionRoutingRecord;
  provider: string | null;
  runtime: string | null;
  model: string | null;
  status: CodingSessionStatus;
  branch: string | null;
  capabilities: CodingSessionCapabilities;
  sessionRef?: string;
  observedCommit?: string | null;
  dirty?: boolean | null;
  relayReachable?: boolean | null;
  verifiedAt?: number | null;
  turnBudget?: CodingSessionTurnBudget;
  /**
   * Which `bee` this seat is actually running, as the host observed it (L12).
   * Additive and omit-when-absent like `routing`: an older host's 44223
   * carries no key at all, and an explicit `beeStamp: null` is not a shape
   * either decoder accepts — see `codingSessionSeatBee.ts`.
   */
  beeStamp?: Readonly<SeatBeeStamp>;
  /**
   * Which persona pack this seat actually staged, as the host resolved it
   * (LANE-L23, the seventh independent additive key on this payload).
   * Additive and omit-when-absent like `beeStamp`: a host with no 30624
   * source (or one that predates this key) carries no key at all, and an
   * explicit `packRef: null` is not a shape either decoder accepts — see
   * `codingSessionPackRef.ts`.
   */
  packRef?: Readonly<PackRef>;
  /**
   * How that pack was composed (spec § 4.6): the app version whose template
   * catalog resolved its includes and the digest of the result. Additive,
   * omit-when-absent, and never present without `packRef` — the Rust
   * decoder refuses the orphan, and so does this one.
   */
  composeRef?: Readonly<ComposeRef>;
  /**
   * Who holds this session and on which execution body, present exactly when
   * a handover claim stands (spec § 3.1, the eighth additive key). Refused
   * as an explicit `null` or as a partial object: reading a half-written
   * fence as "not fenced" tells a person nobody took this session over.
   *
   * Missing from this decoder until lane 216, while the Pulse gate beside it
   * already accepted it — so a 44223 carrying both `handover` and
   * `composeRef` was readable by no desktop reader at all.
   */
  handover?: Readonly<CodingSessionMetadataHandover>;
};

/** How much of one crew session's turn allowance has been spent (D9). */
export type CodingSessionTurnBudget = {
  /** Turns started under this umbrella, as the provider durably counted them. */
  used: number;
  /** The ceiling turns from anyone but the founder are refused at. */
  limit: number;
};

/** Collision-free immutable receipt key shared with the provider. */
export function lifecycleReceiptSemanticKey(commandId: string): string {
  return encodeStructuredKey("coding-session-lifecycle-receipt/v1", commandId);
}

/**
 * The publish-queue fence key a 44224 actually carries.
 *
 * The relay's publish queue de-duplicates on `(kind, semantic key)`, so a key
 * is only honest when distinct facts get distinct keys. One create publishes
 * one lifecycle receipt, so those keep the historical single-field key
 * unchanged. One turn publishes up to three receipts (`turn_queued`, then
 * `turn_started`, or a `turn_dropped`/`turn_refused` instead), so a turn
 * receipt's key names its stage as well — keying by command id alone would
 * fence the second receipt out as a duplicate of the first.
 */
export function codingSessionReceiptSemanticKey(
  commandId: string,
  status: CodingSessionLifecycleReceiptStatus,
): string {
  return isCodingSessionTurnReceiptStatus(status)
    ? encodeStructuredKey(
        "coding-session-lifecycle-receipt/v1",
        commandId,
        status,
      )
    : lifecycleReceiptSemanticKey(commandId);
}

/** Collision-free exact-generation metadata key shared with the provider. */
export function codingSessionMetadataSemanticKey(
  target: CodingSessionCommandTarget,
): string {
  return encodeStructuredKey(
    "coding-session-metadata/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}

/**
 * The five keys every 44224 carries. `turn_started` and `turn_injected` are
 * the two exceptions: they carry these plus `turnId`, and nothing else.
 * Exact-key discipline is absolute in both directions — an unexpected key is
 * a rejection, never a partial accept.
 */
const RECEIPT_ENVELOPE_KEYS = [
  "schema",
  "commandId",
  "status",
  "session",
  "error",
] as const;

export function parseCodingSessionLifecycleReceipt(
  content: unknown,
): Readonly<CodingSessionLifecycleReceipt> | null {
  const value = parseBoundedJson(content, MAX_RECEIPT_CONTENT_BYTES);
  if (!isPlainRecord(value) || typeof value.status !== "string") return null;
  const expectedKeys =
    value.status === "turn_started" || value.status === "turn_injected"
      ? [...RECEIPT_ENVELOPE_KEYS, "turnId"]
      : RECEIPT_ENVELOPE_KEYS;
  if (
    !hasExactKeys(value, expectedKeys) ||
    value.schema !== CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA ||
    !boundedNonempty(value.commandId, MAX_RECEIPT_COMMAND_ID_BYTES)
  ) {
    return null;
  }
  if (isCodingSessionTurnReceiptStatus(value.status)) {
    return parseTurnReceipt(value, value.status);
  }
  if (value.status === "created") {
    const session = decodeTarget(value.session);
    if (!session || value.error !== null) return null;
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId: value.commandId,
      status: "created",
      session,
      error: null,
    });
  }
  if (value.status === "created_with_failed_initial_turn") {
    const session = decodeTarget(value.session);
    if (
      !session ||
      !isPlainRecord(value.error) ||
      !hasExactKeys(value.error, ["code", "message"]) ||
      value.error.code !== "INITIAL_TURN_FAILED" ||
      !boundedNonempty(value.error.message, MAX_ERROR_MESSAGE_BYTES)
    ) {
      return null;
    }
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId: value.commandId,
      status: "created_with_failed_initial_turn",
      session,
      error: Object.freeze({
        code: "INITIAL_TURN_FAILED" as const,
        message: value.error.message,
      }),
    });
  }
  if (value.status === "resumed" || value.status === "stopped") {
    const session = decodeTarget(value.session);
    if (!session || value.error !== null) return null;
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId: value.commandId,
      status: value.status,
      session,
      error: null,
    });
  }
  if (value.status === "resumed_without_context") {
    const session = decodeTarget(value.session);
    if (
      !session ||
      !isPlainRecord(value.error) ||
      !hasExactKeys(value.error, ["code", "message"]) ||
      value.error.code !== "CONTEXT_NOT_RECOVERED" ||
      !boundedNonempty(value.error.message, MAX_ERROR_MESSAGE_BYTES)
    ) {
      return null;
    }
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId: value.commandId,
      status: "resumed_without_context",
      session,
      error: Object.freeze({
        code: "CONTEXT_NOT_RECOVERED" as const,
        message: value.error.message,
      }),
    });
  }
  if (
    value.status !== "failed" ||
    value.session !== null ||
    !isPlainRecord(value.error) ||
    !hasExactKeys(value.error, ["code", "message"]) ||
    !boundedNonempty(value.error.code, MAX_ERROR_CODE_BYTES) ||
    !boundedNonempty(value.error.message, MAX_ERROR_MESSAGE_BYTES)
  ) {
    return null;
  }
  return Object.freeze({
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId: value.commandId,
    status: "failed",
    session: null,
    error: Object.freeze({
      code: value.error.code,
      message: value.error.message,
    }),
  });
}

/**
 * The turn half of the receipt decoder.
 *
 * `session` is the 44220's own target for every turn status — a turn receipt
 * always names the execution it was addressed to, even when the answer is
 * "that generation is stale" — so an absent or malformed target is a
 * rejection rather than a nullable field.
 */
function parseTurnReceipt(
  value: Record<string, unknown>,
  status: CodingSessionTurnReceiptStatus,
): Readonly<CodingSessionLifecycleReceipt> | null {
  const session = decodeTarget(value.session);
  if (!session) return null;
  const commandId = value.commandId as string;
  if (
    status === "turn_queued" ||
    status === "interrupt_delivered" ||
    // A registration carries no error for the same reason a queued turn does
    // not: everything a CI continuation can be refused for happens later, as
    // a turn stage of this same command.
    status === "continuation_registered"
  ) {
    if (value.error !== null) return null;
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId,
      status,
      session,
      error: null,
    });
  }
  // The two six-key statuses: the turn that began, or the running turn an
  // input was injected into. Same bound, same rule — a blank or missing id
  // names no turn.
  if (status === "turn_started" || status === "turn_injected") {
    if (
      value.error !== null ||
      !boundedNonempty(value.turnId, MAX_RECEIPT_TURN_ID_BYTES)
    ) {
      return null;
    }
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId,
      status,
      session,
      error: null,
      turnId: value.turnId,
    });
  }
  if (
    !isPlainRecord(value.error) ||
    !hasExactKeys(value.error, ["code", "message"]) ||
    !boundedNonempty(value.error.code, MAX_TURN_ERROR_CODE_BYTES) ||
    !boundedNonempty(value.error.message, MAX_ERROR_MESSAGE_BYTES)
  ) {
    return null;
  }
  return Object.freeze({
    schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
    commandId,
    status,
    session,
    error: Object.freeze({
      code: value.error.code,
      message: value.error.message,
    }),
  });
}

/**
 * A seat actor as the wire defines it: lowercase 64-hex, or an explicit null.
 *
 * The same bound `validate_actor_pubkey` applies in
 * `crates/beekeeper-core/src/coding_session_lifecycle_command.rs`, and the two
 * decoders must agree: a length-only check here would accept a display name
 * where the Rust half refuses one, so the desktop would resolve a seat no key
 * can hold while the pulse fold silently dropped the same signed event.
 */
/** The fence disclosure a 44223 carries: `SessionMetadataHandover` exactly. */
export type CodingSessionMetadataHandover = {
  /** `active` while the claim stands; `voided` once the claimant lost it. */
  state: "active" | "voided";
  claimant: string;
  bodyPubkey: string;
  acceptedEventId: string;
};

const HEX64 = /^[0-9a-f]{64}$/;

/**
 * Read a `handover`, or null for anything that is not exactly one.
 *
 * Four keys, all required, each a lowercase 64-hex id, and a two-token
 * `state` — `SessionMetadataHandover` in
 * `crates/beekeeper-core/src/coding_session_payload.rs` is `deny_unknown_fields`
 * and every field is required. There is no `none` token: absence of the whole
 * key is how a provider says no claim stands, so a partial object is refused
 * rather than read loosely.
 */
function readMetadataHandover(
  value: unknown,
): Readonly<CodingSessionMetadataHandover> | null {
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, [
      "state",
      "claimant",
      "bodyPubkey",
      "acceptedEventId",
    ]) ||
    (value.state !== "active" && value.state !== "voided") ||
    typeof value.claimant !== "string" ||
    !HEX64.test(value.claimant) ||
    typeof value.bodyPubkey !== "string" ||
    !HEX64.test(value.bodyPubkey) ||
    typeof value.acceptedEventId !== "string" ||
    !HEX64.test(value.acceptedEventId)
  ) {
    return null;
  }
  return Object.freeze({
    state: value.state,
    claimant: value.claimant,
    bodyPubkey: value.bodyPubkey,
    acceptedEventId: value.acceptedEventId,
  });
}

function isSeatActorPubkeyOrNull(value: unknown): value is string | null {
  return (
    value === null ||
    (typeof value === "string" && /^[0-9a-f]{64}$/.test(value))
  );
}

export function parseBeekeeperCodingSessionMetadata(
  content: unknown,
): Readonly<BeekeeperCodingSessionMetadataV1> | null {
  const value = parseBoundedJson(content, MAX_METADATA_CONTENT_BYTES);
  const required = [
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
  ] as const;
  const factFields = [
    "observedCommit",
    "dirty",
    "relayReachable",
    "verifiedAt",
  ] as const;
  // `contextSummary`, `diffSummary` and `planSummary` were here until lane
  // 216 and are gone: `METADATA_BASE_FIELDS` never named them, the only
  // writer of a 44223 is `serde_json::to_string(&SessionMetadata)` in the
  // provider (`crates/beekeeper-session-provider/src/lib.rs:8351`), and
  // `SessionMetadata` has no such fields — so no signed event has ever
  // carried one, and a relay would refuse it if one did. Nothing in this app
  // read them either. A key only one reader knows is exactly ledger 204.
  const optional = [
    "sessionRef",
    "role",
    "turnBudget",
    "routing",
    "beeStamp",
    "packRef",
    "composeRef",
    "handover",
    ...factFields,
  ] as const;
  if (
    !isPlainRecord(value) ||
    !hasRequiredAndOptionalKeys(value, required, optional) ||
    value.schema !== BEEKEEPER_CODING_SESSION_METADATA_SCHEMA
  ) {
    return null;
  }
  const session = decodeTarget(value.session);
  const capabilities = decodeCapabilities(value.capabilities);
  if (!session || !capabilities) return null;
  if (
    !boundedNullable(value.projectRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(value.repoRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(value.title, MAX_LABEL_BYTES) ||
    !isSeatActorPubkeyOrNull(value.agentRef) ||
    !boundedNullable(value.provider, MAX_LABEL_BYTES) ||
    !boundedNullable(value.runtime, MAX_LABEL_BYTES) ||
    !boundedNullable(value.model, MAX_LABEL_BYTES) ||
    !isCodingSessionStatus(value.status) ||
    !boundedNullable(value.branch, MAX_LABEL_BYTES)
  ) {
    return null;
  }
  // A seat's role travels with its actor. A `role` without an `agentRef` is
  // malformed, not a partial dialect: it would label an execution with a seat
  // nobody holds. An explicit `role: null` is not that claim and is not
  // rejected — the Rust decoder's `(_, None)` arm accepts it, and dropping the
  // whole metadata over a key the producer should not have emitted would also
  // drop the status, model and capabilities carried beside it.
  if (
    Object.hasOwn(value, "role") &&
    value.role !== null &&
    (typeof value.agentRef !== "string" || !isCodingSessionRoleSlug(value.role))
  ) {
    return null;
  }
  // A present `sessionRef` must be exactly the canonical UUID shape the create
  // validated — the echo is a projection convenience, never a looser claim.
  if (
    Object.hasOwn(value, "sessionRef") &&
    !isCodingSessionSessionRef(value.sessionRef)
  ) {
    return null;
  }
  // A crew allowance describes an umbrella, so it cannot travel without a
  // `sessionRef`, and a `limit` of zero would read as "no turns allowed"
  // rather than "unbudgeted" — the producer omits the key instead. Both are
  // rejections rather than tolerated dialects, matching the Rust decoder.
  // An explicit `turnBudget: null` is neither claim: Rust's serde-defaulted
  // `Option<TurnBudget>` reads it as absent, exactly as the `role: null` arm
  // above, so dropping the metadata over it would lose every fact beside it.
  if (
    Object.hasOwn(value, "turnBudget") &&
    value.turnBudget !== null &&
    !isCodingSessionTurnBudget(value.turnBudget, value.sessionRef)
  ) {
    return null;
  }
  // Unlike historical `role` and `turnBudget`, routing was introduced with an
  // omit-when-absent writer contract. An explicit null is therefore not a
  // valid amendment shape. Rust enforces this same rule before deserializing
  // its `Option`, keeping one signed 44223 visible to both readers or neither.
  if (
    Object.hasOwn(value, "routing") &&
    !isStrictCodingSessionRoutingRecord(value.routing)
  ) {
    return null;
  }
  // Same omit-when-absent contract as routing (`bee_stamp`'s Rust field carries
  // the identical `#[serde(skip_serializing_if = "Option::is_none")]`): an
  // explicit `beeStamp: null` is not a shape the Rust decoder accepts either
  // (`coding-session metadata beeStamp must not be null`), so this desktop
  // reader rejects the whole metadata over it or a malformed shape, rather
  // than accepting a looser dialect Rust would refuse to sign.
  if (
    Object.hasOwn(value, "beeStamp") &&
    readSeatBeeStamp(value.beeStamp) === null
  ) {
    return null;
  }
  // Same omit-when-absent contract as `beeStamp` and `routing` (finding 31's
  // rule): a host that resolved no pack (no 30624 source, or an older host
  // that predates this key) omits `packRef` entirely; an explicit
  // `packRef: null` is not a shape the Rust decoder accepts either, so this
  // reader rejects the whole metadata over it or a malformed shape rather
  // than accepting a looser dialect Rust would refuse to sign.
  if (Object.hasOwn(value, "packRef") && readPackRef(value.packRef) === null) {
    return null;
  }
  // `composeRef` describes how the `packRef` pack was composed, so it is
  // refused alone, as null, or malformed — exactly the Rust decoder's rule.
  if (
    Object.hasOwn(value, "composeRef") &&
    (!Object.hasOwn(value, "packRef") ||
      readComposeRef(value.composeRef) === null)
  ) {
    return null;
  }
  // § 3.1's fence disclosure. A present-but-malformed object — including an
  // explicit `null`, which the Rust decoder refuses by name — is refused
  // rather than ignored, exactly as the Pulse gate refuses it.
  if (
    Object.hasOwn(value, "handover") &&
    readMetadataHandover(value.handover) === null
  ) {
    return null;
  }
  // The B1 code-coordinate facts travel all-four-or-none (the Rust producer's
  // METADATA_FACT_FIELDS discipline); a partial subset is corruption.
  if (!hasAllOrNoneKeys(value, factFields)) return null;
  const hasFacts = Object.hasOwn(value, "observedCommit");
  if (
    hasFacts &&
    (!boundedNullable(value.observedCommit, MAX_LABEL_BYTES) ||
      !(value.dirty === null || typeof value.dirty === "boolean") ||
      !(
        value.relayReachable === null ||
        typeof value.relayReachable === "boolean"
      ) ||
      !(value.verifiedAt === null || Number.isSafeInteger(value.verifiedAt)) ||
      (value.relayReachable === null) !== (value.verifiedAt === null))
  ) {
    return null;
  }
  return Object.freeze({
    schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
    session,
    projectRef: value.projectRef,
    repoRef: value.repoRef,
    title: value.title,
    agentRef: value.agentRef,
    provider: value.provider,
    runtime: value.runtime,
    model: value.model,
    status: value.status,
    branch: value.branch,
    capabilities,
    ...(typeof value.sessionRef === "string"
      ? { sessionRef: value.sessionRef }
      : {}),
    ...(typeof value.role === "string" ? { role: value.role } : {}),
    ...(isCodingSessionTurnBudget(value.turnBudget, value.sessionRef)
      ? { turnBudget: value.turnBudget }
      : {}),
    ...(isStrictCodingSessionRoutingRecord(value.routing)
      ? { routing: value.routing as CodingSessionRoutingRecord }
      : {}),
    ...(Object.hasOwn(value, "beeStamp")
      ? { beeStamp: readSeatBeeStamp(value.beeStamp) as SeatBeeStamp }
      : {}),
    ...(Object.hasOwn(value, "packRef")
      ? { packRef: readPackRef(value.packRef) as PackRef }
      : {}),
    ...(Object.hasOwn(value, "composeRef")
      ? { composeRef: readComposeRef(value.composeRef) as ComposeRef }
      : {}),
    ...(Object.hasOwn(value, "handover")
      ? {
          handover: readMetadataHandover(
            value.handover,
          ) as CodingSessionMetadataHandover,
        }
      : {}),
    ...(hasFacts
      ? {
          observedCommit: value.observedCommit as string | null,
          dirty: value.dirty as boolean | null,
          relayReachable: value.relayReachable as boolean | null,
          verifiedAt: value.verifiedAt as number | null,
        }
      : {}),
  });
}

/**
 * A well-formed crew allowance beside the umbrella it describes.
 *
 * Both numbers are non-negative safe integers and `limit` is positive: zero is
 * how the producer spells "no budget", and it spells it by omitting the key.
 * `used` is deliberately *not* bounded by `limit` — a founder's turns are
 * counted and never refused, so an over-spent umbrella is a real state.
 */
function isCodingSessionTurnBudget(
  value: unknown,
  sessionRef: unknown,
): value is CodingSessionTurnBudget {
  if (typeof sessionRef !== "string") return false;
  if (!isPlainRecord(value)) return false;
  if (!hasRequiredAndOptionalKeys(value, ["used", "limit"] as const, [])) {
    return false;
  }
  return (
    Number.isSafeInteger(value.used) &&
    (value.used as number) >= 0 &&
    Number.isSafeInteger(value.limit) &&
    (value.limit as number) > 0
  );
}

function decodeCapabilities(value: unknown): CodingSessionCapabilities | null {
  const keys = [
    "threadTurnStart",
    "threadTurnInterrupt",
    "threadSteer",
    "context",
    "diff",
    "plan",
  ] as const;
  // `promptImage` is optional, not exact-keyed: a provider that predates it
  // publishes six keys and must keep decoding. Requiring it would turn every
  // capability vector from an older provider into `null`, which reads as "this
  // execution can do nothing" — a far worse lie than a missing attach button.
  const optionalKeys = ["promptImage"] as const;
  if (
    !isPlainRecord(value) ||
    !hasRequiredAndOptionalKeys(value, keys, optionalKeys) ||
    !keys.every((key) => typeof value[key] === "boolean") ||
    (Object.hasOwn(value, "promptImage") &&
      typeof value.promptImage !== "boolean")
  ) {
    return null;
  }
  return Object.freeze({
    threadTurnStart: value.threadTurnStart as boolean,
    threadTurnInterrupt: value.threadTurnInterrupt as boolean,
    threadSteer: value.threadSteer as boolean,
    context: value.context as boolean,
    diff: value.diff as boolean,
    plan: value.plan as boolean,
    promptImage: value.promptImage === true,
  });
}

function isCodingSessionStatus(value: unknown): value is CodingSessionStatus {
  return (
    typeof value === "string" &&
    new Set([
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
    ]).has(value)
  );
}
