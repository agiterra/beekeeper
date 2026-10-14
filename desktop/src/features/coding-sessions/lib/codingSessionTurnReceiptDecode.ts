// The turn half of the 44224 receipt decoder: the per-stage statuses whose
// extra key (`turnId`, `quiescence`) and error bounds differ from the
// generation lifecycle half. Split out of `codingSessionIngressPayloads.ts`,
// which owns the envelope, the status list and the types, to keep that
// decoder under the repository file-size ceiling.
import type {
  CodingSessionLifecycleReceipt,
  CodingSessionTurnReceiptStatus,
} from "./codingSessionIngressPayloads";
import {
  boundedNonempty,
  decodeTarget,
  hasExactKeys,
  isPlainRecord,
} from "./codingSessionWireDecode";

export const CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA =
  "buzz-coding-session-lifecycle-receipt/v1" as const;

/** A provider's own turn id, bounded exactly like the command id it answers. */
export const MAX_RECEIPT_TURN_ID_BYTES = 256;
/**
 * A **turn** failure code, bounded at `MAX_RECEIPT_ERROR_CODE_BYTES`
 * (`coding_session_payload.rs:278`) — 64, the narrower of the two. The
 * asymmetry is the writer's: `is_receipt_error_code` is applied only to
 * `turn_dropped`/`turn_refused`/`turn_degraded`/`turn_delivery_unknown`, whose
 * code vocabulary is deliberately open and therefore kept tight. This reader
 * used 256 for both until lane 216, so it accepted turn codes the writer's own
 * validator — and therefore the relay — call malformed.
 */
export const MAX_TURN_ERROR_CODE_BYTES = 64;
/**
 * `1024 + '…'.len_utf8()` — `validate_lifecycle_receipt`
 * (`coding_session_payload.rs:829`) bounds a receipt error message at exactly
 * this, the width of a message a host truncated with an ellipsis. This reader
 * used the generic 2 KiB reference bound.
 */
export const MAX_ERROR_MESSAGE_BYTES = 1024 + 3;

/**
 * The turn half of the receipt decoder.
 *
 * `session` is the 44220's own target for every turn status — a turn receipt
 * always names the execution it was addressed to, even when the answer is
 * "that generation is stale" — so an absent or malformed target is a
 * rejection rather than a nullable field.
 */
export function parseTurnReceipt(
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
  // The third six-key status: the quiescence key is the whole claim, and
  // `unconfirmed` is its only legal value. No error — the turn did run.
  if (status === "turn_interrupted") {
    if (value.error !== null || value.quiescence !== "unconfirmed") return null;
    return Object.freeze({
      schema: CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
      commandId,
      status,
      session,
      error: null,
      quiescence: "unconfirmed",
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
