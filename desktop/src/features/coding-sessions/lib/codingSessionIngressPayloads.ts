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
    };

/**
 * Per-generation metadata. `projectRef` is nullable — an explicit null is a
 * standalone session — and `agentRef`/`branch` are always null in v1: the
 * provider binds no managed agent and never re-binds a generation to a branch.
 *
 * `sessionRef` is the umbrella session reference echoed from the create.
 * It is an *optional* key, never an explicit null: the provider emits it only
 * when the create claimed one, so pre-umbrella metadata stays byte-identical
 * and old clients lose enrichment only for umbrella-claiming sessions.
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
};

/** Collision-free immutable receipt key shared with the provider. */
export function lifecycleReceiptSemanticKey(commandId: string): string {
  return encodeStructuredKey("coding-session-lifecycle-receipt/v1", commandId);
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

export function parseCodingSessionLifecycleReceipt(
  content: unknown,
): Readonly<CodingSessionLifecycleReceipt> | null {
  const value = parseBoundedJson(content, MAX_RECEIPT_CONTENT_BYTES);
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, [
      "schema",
      "commandId",
      "status",
      "session",
      "error",
    ]) ||
    value.schema !== CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA ||
    !boundedNonempty(value.commandId, MAX_RECEIPT_COMMAND_ID_BYTES)
  ) {
    return null;
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
  const optional = [...optionalSummaries, "sessionRef"] as const;
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
      "failed",
      "interrupted",
      "disconnected",
      "unknown",
    ]).has(value)
  );
}
