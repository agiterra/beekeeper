/**
 * Strict decoders for the 44224 lifecycle receipt and 44223 metadata payloads.
 *
 * Copied from
 * `desktop/src/features/coding-sessions/lib/codingSessionIngressPayloads.ts`;
 * the semantic-key builders moved to `keys.ts`.
 */
import type {
  CodingSessionCapabilities,
  CodingSessionStatus,
  CodingSessionTarget,
} from "./types.ts";
import {
  boundedNonempty,
  boundedNullable,
  decodeTarget,
  hasAllOrNoneKeys,
  hasExactKeys,
  hasOwnKey,
  hasRequiredAndOptionalKeys,
  isCodingSessionSessionRef,
  isPlainRecord,
  isStrictRoutingRecord,
  parseBoundedJson,
} from "./wireDecode.ts";

export const CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA =
  "buzz-coding-session-lifecycle-receipt/v1" as const;
export const CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION = "cslr1-1" as const;
export const BUZZ_CODING_SESSION_METADATA_SCHEMA =
  "buzz-coding-session-metadata/v1" as const;
export const CODING_SESSION_METADATA_TAG_VERSION = "csm1-1" as const;

const MAX_RECEIPT_CONTENT_BYTES = 16 * 1024;
const MAX_METADATA_CONTENT_BYTES = 32 * 1024;
const MAX_RECEIPT_COMMAND_ID_BYTES = 256;
/** A provider's own turn id, bounded exactly like the command id it answers. */
const MAX_RECEIPT_TURN_ID_BYTES = 256;
/**
 * A lifecycle receipt's error code, bounded as `validate_lifecycle_receipt`'s
 * generic check bounds it (`coding_session_payload.rs:826`).
 */
const MAX_ERROR_CODE_BYTES = 256;
/**
 * A **turn** failure code: `MAX_RECEIPT_ERROR_CODE_BYTES`
 * (`coding_session_payload.rs:278`) — 64. `is_receipt_error_code` is applied
 * only to the four turn-failure statuses, whose code vocabulary is
 * deliberately open and therefore kept tight. This copy used 256 for both
 * until lane 223.
 */
const MAX_TURN_ERROR_CODE_BYTES = 64;
/**
 * `1024 + '…'.len_utf8()` — the exact bound the writer's validator sets on a
 * receipt error message (`coding_session_payload.rs:829`). This copy used the
 * generic 2 KiB reference bound.
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
      session: CodingSessionTarget;
      error: null;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "created_with_failed_initial_turn";
      session: CodingSessionTarget;
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
      session: CodingSessionTarget;
      error: null;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "resumed_without_context";
      session: CodingSessionTarget;
      error: { code: "CONTEXT_NOT_RECOVERED"; message: string };
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "turn_queued";
      session: CodingSessionTarget;
      error: null;
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status: "turn_started" | "turn_injected";
      session: CodingSessionTarget;
      error: null;
      /** The provider's own id for the turn that began, or was steered into. */
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
      session: CodingSessionTarget;
      error: { code: string; message: string };
    }
  | {
      schema: typeof CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA;
      commandId: string;
      status:
        | "interrupt_delivered"
        | "continuation_registered"
        | "model_applied";
      session: CodingSessionTarget;
      error: null;
    };

/** Every status a 44224 can carry, lifecycle and turn alike. */
export type CodingSessionLifecycleReceiptStatus =
  CodingSessionLifecycleReceipt["status"];

/**
 * The six per-stage turn statuses.
 *
 * A turn receipt reports what happened to one 44220 command — a
 * `thread.turn.start` or a `thread.turn.interrupt`; it never creates,
 * confirms, or ends a generation, which is why every fold that reads a receipt
 * to decide a generation's state must skip these.
 *
 * `turn_queued`, `turn_started`, and `interrupt_delivered` carry
 * `error: null`. `turn_degraded` (`STEER_UNSUPPORTED`), `turn_dropped`
 * (`QUEUE_FULL`, `NO_LIVE_EXECUTION`) and `turn_refused`
 * (`UNAUTHORIZED_OPERATOR`, `UNKNOWN_TARGET`, `STALE_GENERATION`,
 * `SESSION_CLOSED`) carry a `{code, message}`. The code is read as a bounded
 * string rather than pinned to those lists, exactly as the lifecycle `failed`
 * status already is: a provider that grows a new reason must not be decoded as
 * malformed.
 *
 * `turn_degraded` is not a failure. It says the provider could not steer the
 * running turn and will run this one at the next boundary instead — the turn
 * still happens, so the row is relabelled rather than retired.
 */
export const CODING_SESSION_TURN_RECEIPT_STATUSES = [
  "turn_queued",
  "turn_started",
  // A `deliver: "steer"` input the runtime joined into the turn already
  // running. Carries that turn's `turnId`; no new turn begins.
  "turn_injected",
  "turn_degraded",
  "turn_dropped",
  "turn_refused",
  // A native steer written to the runtime whose delivery could not be
  // established. Terminal, and neither a drop nor a refusal: the words may
  // already be inside the running turn.
  "turn_delivery_unknown",
  "interrupt_delivered",
  // A stage of one 44220 saying a CI continuation was stored. It creates,
  // confirms and ends nothing, exactly as a queued turn does.
  "continuation_registered",
  // SV-35: a `thread.model.set` was accepted at the boundary. Terminal, no
  // turn; the model in effect is read from 44223, never from this receipt.
  // Its refusals are `turn_refused` with `MODEL_SWITCH_UNSUPPORTED`,
  // `MODEL_NOT_OFFERED` or `MODEL_SWITCH_FAILED`, read as bounded codes.
  "model_applied",
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
 * number goes stale silently.
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
 * standalone session — and `agentRef`/`branch` are always null in v1: the
 * provider binds no managed agent and never re-binds a generation to a branch.
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
 */
export type BeekeeperCodingSessionMetadataV1 = {
  schema: typeof BUZZ_CODING_SESSION_METADATA_SCHEMA;
  session: CodingSessionTarget;
  projectRef: string | null;
  repoRef: string | null;
  title: string | null;
  agentRef: string | null;
  provider: string | null;
  runtime: string | null;
  model: string | null;
  status: CodingSessionStatus;
  branch: string | null;
  capabilities: CodingSessionCapabilities;
  sessionRef?: string;
  /** The seat's role slug, present exactly when `agentRef` is. */
  role?: string;
  /** D9's crew turn allowance, present only beside an umbrella. */
  turnBudget?: { used: number; limit: number };
  /**
   * The routing decision that chose this seat's execution target (Brian's
   * routing ruling, 2026-08-30). Absent on a seat nothing routed.
   */
  routing?: Record<string, unknown>;
  observedCommit?: string | null;
  dirty?: boolean | null;
  relayReachable?: boolean | null;
  verifiedAt?: number | null;
};

/**
 * The five keys every 44224 carries. `turn_started` is the single exception:
 * it carries these plus `turnId`, and nothing else. Exact-key discipline is
 * absolute in both directions — an unexpected key is a rejection, never a
 * partial accept.
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
    status === "continuation_registered" ||
    status === "model_applied"
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
  // Every additive amendment buzz-core has landed on this payload, and the
  // list is the whole of what this observer will read. Three of these were
  // missing until 2026-08-30 — `role` and `turnBudget` shipped on the desktop
  // and in Rust and were silently dropping every seated, budgeted session
  // here, and `routing` is the new one — which is the failure mode this
  // comment exists to keep naming: a forgotten amendment is not a strictness
  // nuance, it is a blank session list.
  // `contextSummary`, `diffSummary` and `planSummary` were here until lane 223
  // and are gone: `METADATA_BASE_FIELDS` never named them, and the only writer
  // of a 44223 is `serde_json::to_string(&SessionMetadata)` in the provider,
  // whose struct has no such fields — so no signed event has ever carried one
  // and the relay would refuse it if one did (ledger 216(k)).
  //
  // `beeStamp`, `packRef`, `handover` and `composeRef` are the four additive
  // amendments this copy never grew, so every seat with an observed bee, a
  // staged pack, a fence or a composed pack decoded as corruption here while
  // both desktop readers accepted it. That is a blank session list, not a
  // strictness nuance, and it is exactly what the comment above kept warning
  // about.
  const optional = [
    "sessionRef",
    "role",
    "turnBudget",
    "routing",
    "beeStamp",
    "packRef",
    "handover",
    "composeRef",
    ...factFields,
  ] as const;
  if (
    !isPlainRecord(value) ||
    !hasRequiredAndOptionalKeys(value, required, optional) ||
    value.schema !== BUZZ_CODING_SESSION_METADATA_SCHEMA
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
    !boundedNullable(value.agentRef, MAX_REFERENCE_BYTES) ||
    !boundedNullable(value.provider, MAX_LABEL_BYTES) ||
    !boundedNullable(value.runtime, MAX_LABEL_BYTES) ||
    !boundedNullable(value.model, MAX_LABEL_BYTES) ||
    !isCodingSessionStatus(value.status) ||
    !boundedNullable(value.branch, MAX_LABEL_BYTES)
  ) {
    return null;
  }
  // A present `sessionRef` must be exactly the canonical UUID shape the create
  // validated — the echo is a projection convenience, never a looser claim.
  if (
    hasOwnKey(value, "sessionRef") &&
    !isCodingSessionSessionRef(value.sessionRef)
  ) {
    return null;
  }
  // The B1 code-coordinate facts travel all-four-or-none (the Rust producer's
  // METADATA_FACT_FIELDS discipline); a partial subset is corruption.
  if (!hasAllOrNoneKeys(value, factFields)) return null;
  // A seat's role travels with its actor; a budget travels with its umbrella;
  // a routing record is the closed shape or it is corruption. An explicit
  // `null` is how serde writes an absent `Option` and is read as absent.
  if (
    hasOwnKey(value, "role") &&
    value.role !== null &&
    (typeof value.agentRef !== "string" ||
      typeof value.role !== "string" ||
      !/^[a-z0-9-]{1,64}$/.test(value.role))
  ) {
    return null;
  }
  if (
    hasOwnKey(value, "turnBudget") &&
    value.turnBudget !== null &&
    !isMetadataTurnBudget(value.turnBudget, value.sessionRef)
  ) {
    return null;
  }
  // An explicit `null` is refused, not short-circuited past: `routing`,
  // `beeStamp`, `packRef`, `handover` and `composeRef` were every one of them
  // introduced with an omit-when-absent writer contract, and
  // `decode_coding_session_metadata` refuses each null **naming the key**.
  // `role` and `turnBudget` above are the exception — their serde `Option`s do
  // read a null as absent.
  if (hasOwnKey(value, "routing") && !isStrictRoutingRecord(value.routing)) {
    return null;
  }
  if (hasOwnKey(value, "beeStamp") && !isMetadataBeeStamp(value.beeStamp)) {
    return null;
  }
  if (hasOwnKey(value, "packRef") && !isMetadataPackRef(value.packRef)) {
    return null;
  }
  if (hasOwnKey(value, "handover") && !isMetadataHandover(value.handover)) {
    return null;
  }
  // A composition of nothing is not a fact: `validate_session_metadata`
  // refuses `composeRef` without a `packRef`.
  if (
    hasOwnKey(value, "composeRef") &&
    (!hasOwnKey(value, "packRef") || !isMetadataComposeRef(value.composeRef))
  ) {
    return null;
  }
  const hasFacts = hasOwnKey(value, "observedCommit");
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
    schema: BUZZ_CODING_SESSION_METADATA_SCHEMA,
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
    ...(isMetadataTurnBudget(value.turnBudget, value.sessionRef)
      ? { turnBudget: value.turnBudget as { used: number; limit: number } }
      : {}),
    ...(isStrictRoutingRecord(value.routing)
      ? { routing: value.routing as Record<string, unknown> }
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
 * D9's crew turn allowance: exactly `{used, limit}`, and only beside an
 * umbrella. A `limit` of zero would read as "no turn may ever pass" rather
 * than "unbudgeted", so the producer omits the key instead.
 */
function isMetadataTurnBudget(value: unknown, sessionRef: unknown): boolean {
  return (
    typeof sessionRef === "string" &&
    isPlainRecord(value) &&
    hasExactKeys(value, ["used", "limit"]) &&
    Number.isSafeInteger(value.used) &&
    (value.used as number) >= 0 &&
    Number.isSafeInteger(value.limit) &&
    (value.limit as number) > 0
  );
}

const HEX64 = /^[0-9a-f]{64}$/;
const EXACT_SHA = /^[0-9a-f]{40}$/;
const SHORT_SHA = /^[0-9a-f]{7,40}$/;
const ROLE_SLUG = /^[a-z0-9-]{1,64}$/;
/** `30617:<64-hex>:<dtag>` — a git repository announcement coordinate. */
const PACK_REF_REPO_COORD = /^30617:[0-9a-f]{64}:[a-zA-Z0-9._-]{1,200}$/;
/** The literal `repo` a shipped-defaults `packRef` carries. */
const PACK_REF_SHIPPED_REPO = "app:shipped";
const PACK_REF_APP_VERSION = /^[0-9]+\.[0-9]+\.[0-9]+(?:[-+][0-9A-Za-z.-]+)?$/;
const COMPOSE_REF_DIGEST = /^sha256:[0-9a-f]{64}$/;
const MAX_COMPOSE_REF_APP_VERSION_BYTES = 64;
const BEE_STAMP_SOURCES: ReadonlySet<string> = new Set(["bundled", "path"]);

/*
 * The four additive amendments this observer reads but does not surface.
 *
 * Read, because a reader that refuses a key `buzz-core` accepts drops the
 * whole session and shows nothing — the failure this file's own comment above
 * the optional list has been warning about since 2026-08-30, and the one
 * Astra measured here on 2026-09-21. Not surfaced, because this client renders
 * no seat bee, pack, fence or composition in v1; validating them is how the
 * decoder stays exactly as strict as the writer without pretending to more.
 *
 * Each mirrors the desktop gate's checker of the same name. The shared vectors
 * are what keep the copies honest.
 */

/** `BeeStamp`: exactly five keys, `source` a closed two-variant Rust enum. */
function isMetadataBeeStamp(value: unknown): boolean {
  return (
    isPlainRecord(value) &&
    hasExactKeys(value, ["path", "source", "version", "sha", "dirty"]) &&
    typeof value.path === "string" &&
    value.path.length > 0 &&
    BEE_STAMP_SOURCES.has(value.source as string) &&
    (value.version === null || typeof value.version === "string") &&
    (value.sha === null ||
      (typeof value.sha === "string" && SHORT_SHA.test(value.sha))) &&
    (value.dirty === null || typeof value.dirty === "boolean")
  );
}

/**
 * `PackRef`: exactly four keys. `sha` is an exact 40-hex commit — never the
 * 7-40 shorthand a `beeStamp` allows, because a pack is pinned to one commit
 * and not to a prefix — unless `repo` is the shipped-defaults literal, whose
 * `sha` is the app version that bundled the packs instead.
 */
function isMetadataPackRef(value: unknown): boolean {
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, ["repo", "sha", "role", "path"]) ||
    typeof value.repo !== "string" ||
    typeof value.sha !== "string" ||
    typeof value.role !== "string" ||
    !ROLE_SLUG.test(value.role) ||
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

/**
 * `SessionMetadataHandover`: exactly four keys, each a lowercase 64-hex id.
 *
 * There is no "none" token — absence of the whole key is how a provider says
 * no claim stands — and a partial object is refused rather than read loosely:
 * reading a half-written fence as "not fenced" tells a person nobody took this
 * session over.
 */
function isMetadataHandover(value: unknown): boolean {
  return (
    isPlainRecord(value) &&
    hasExactKeys(value, [
      "state",
      "claimant",
      "bodyPubkey",
      "acceptedEventId",
    ]) &&
    (value.state === "active" || value.state === "voided") &&
    typeof value.claimant === "string" &&
    HEX64.test(value.claimant) &&
    typeof value.bodyPubkey === "string" &&
    HEX64.test(value.bodyPubkey) &&
    typeof value.acceptedEventId === "string" &&
    HEX64.test(value.acceptedEventId)
  );
}

/** `ComposeRef`: exactly `{appVersion, digest}` (spec § 4.6). */
function isMetadataComposeRef(value: unknown): boolean {
  return (
    isPlainRecord(value) &&
    hasExactKeys(value, ["appVersion", "digest"]) &&
    boundedNonempty(value.appVersion, MAX_COMPOSE_REF_APP_VERSION_BYTES) &&
    typeof value.digest === "string" &&
    COMPOSE_REF_DIGEST.test(value.digest)
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
  // publishes six keys and must keep decoding, and a provider that has it must
  // not be refused. Requiring or forbidding it turns a whole newer host's
  // sessions into `null`, which reads as "this execution can do nothing" —
  // finding 34, and the disagreement Astra measured in this copy on
  // 2026-09-21.
  // `modelSwitch` (SV-35) is optional for the same reason; it is omitted
  // when false, so only a switchable execution carries it.
  const optionalKeys = ["promptImage", "modelSwitch"] as const;
  if (
    !isPlainRecord(value) ||
    !hasRequiredAndOptionalKeys(value, keys, optionalKeys) ||
    !keys.every((key) => typeof value[key] === "boolean") ||
    optionalKeys.some(
      (key) => hasOwnKey(value, key) && typeof value[key] !== "boolean",
    )
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
    ...(value.modelSwitch === true ? { modelSwitch: true } : {}),
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
