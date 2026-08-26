/**
 * Strict decoders for the 44224 lifecycle receipt and 44223 metadata payloads,
 * plus the semantic keys their events carry.
 */
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import { encodeStructuredKey } from "./codingSessionKeys";
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
export const BUZZ_CODING_SESSION_METADATA_SCHEMA =
  "buzz-coding-session-metadata/v1" as const;
export const CODING_SESSION_METADATA_TAG_VERSION = "csm1-1" as const;

const MAX_RECEIPT_CONTENT_BYTES = 16 * 1024;
const MAX_METADATA_CONTENT_BYTES = 32 * 1024;
const MAX_RECEIPT_COMMAND_ID_BYTES = 256;
/** A provider's own turn id, bounded exactly like the command id it answers. */
const MAX_RECEIPT_TURN_ID_BYTES = 256;
const MAX_ERROR_CODE_BYTES = 256;
const MAX_REFERENCE_BYTES = 2 * 1024;
const MAX_LABEL_BYTES = 2 * 1024;
const MAX_SUMMARY_BYTES = 16 * 1024;

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
      status: "turn_dropped" | "turn_refused";
      session: CodingSessionCommandTarget;
      error: { code: string; message: string };
    };

/** Every status a 44224 can carry, lifecycle and turn alike. */
export type CodingSessionLifecycleReceiptStatus =
  CodingSessionLifecycleReceipt["status"];

/**
 * The four per-stage turn statuses.
 *
 * A turn receipt reports what happened to one 44220 `thread.turn.start`; it
 * never creates, confirms, or ends a generation, which is why every fold that
 * reads a receipt to decide a generation's state must skip these.
 *
 * `turn_queued` and `turn_started` carry `error: null`; `turn_dropped`
 * (`QUEUE_FULL`) and `turn_refused` (`UNAUTHORIZED_OPERATOR`,
 * `UNKNOWN_TARGET`, `STALE_GENERATION`, `SESSION_CLOSED`) carry a
 * `{code, message}`. The code is read as a bounded string rather than pinned
 * to that list, exactly as the lifecycle `failed` status already is: a
 * provider that grows a new reason must not be decoded as malformed.
 */
export const CODING_SESSION_TURN_RECEIPT_STATUSES = [
  "turn_queued",
  "turn_started",
  "turn_dropped",
  "turn_refused",
] as const;

/** A per-stage turn status, as opposed to a generation lifecycle status. */
export type CodingSessionTurnReceiptStatus =
  (typeof CODING_SESSION_TURN_RECEIPT_STATUSES)[number];

const TURN_RECEIPT_STATUSES: ReadonlySet<string> = new Set(
  CODING_SESSION_TURN_RECEIPT_STATUSES,
);

/** True for the four turn statuses, false for every lifecycle status. */
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
 * `verifiedAt`) are the B1 amendment (`crates/buzz-core`
 * `coding_session_payload.rs`, `METADATA_FACT_FIELDS`): a provider that
 * observes the worktree serializes all four unconditionally (nulls included),
 * a pre-amendment provider serializes none — a partial subset is malformed,
 * never a dialect. `verifiedAt` is non-null exactly when `relayReachable` is
 * non-null: both come from the same verification probe.
 */
export type BuzzCodingSessionMetadataV1 = {
  schema: typeof BUZZ_CODING_SESSION_METADATA_SCHEMA;
  session: CodingSessionCommandTarget;
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
  contextSummary?: string;
  diffSummary?: string;
  planSummary?: string;
  sessionRef?: string;
  observedCommit?: string | null;
  dirty?: boolean | null;
  relayReachable?: boolean | null;
  verifiedAt?: number | null;
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
    value.status === "turn_started"
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
      !boundedNonempty(value.error.message, MAX_REFERENCE_BYTES)
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
      !boundedNonempty(value.error.message, MAX_REFERENCE_BYTES)
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
    !boundedNonempty(value.error.message, MAX_REFERENCE_BYTES)
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
  if (status === "turn_queued") {
    if (value.error !== null) return null;
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId,
      status,
      session,
      error: null,
    });
  }
  if (status === "turn_started") {
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
    !boundedNonempty(value.error.code, MAX_ERROR_CODE_BYTES) ||
    !boundedNonempty(value.error.message, MAX_REFERENCE_BYTES)
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

export function parseBuzzCodingSessionMetadata(
  content: unknown,
): Readonly<BuzzCodingSessionMetadataV1> | null {
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
  const optionalSummaries = [
    "contextSummary",
    "diffSummary",
    "planSummary",
  ] as const;
  const factFields = [
    "observedCommit",
    "dirty",
    "relayReachable",
    "verifiedAt",
  ] as const;
  const optional = [...optionalSummaries, "sessionRef", ...factFields] as const;
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
  for (const key of optionalSummaries) {
    if (
      Object.hasOwn(value, key) &&
      !boundedNonempty(value[key], MAX_SUMMARY_BYTES)
    ) {
      return null;
    }
  }
  // A present `sessionRef` must be exactly the canonical UUID shape the create
  // validated — the echo is a projection convenience, never a looser claim.
  if (
    Object.hasOwn(value, "sessionRef") &&
    !isCodingSessionSessionRef(value.sessionRef)
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
    ...(typeof value.contextSummary === "string"
      ? { contextSummary: value.contextSummary }
      : {}),
    ...(typeof value.diffSummary === "string"
      ? { diffSummary: value.diffSummary }
      : {}),
    ...(typeof value.planSummary === "string"
      ? { planSummary: value.planSummary }
      : {}),
    ...(typeof value.sessionRef === "string"
      ? { sessionRef: value.sessionRef }
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

function decodeCapabilities(value: unknown): CodingSessionCapabilities | null {
  const keys = [
    "threadTurnStart",
    "threadTurnInterrupt",
    "threadSteer",
    "context",
    "diff",
    "plan",
  ] as const;
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, keys) ||
    !keys.every((key) => typeof value[key] === "boolean")
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
