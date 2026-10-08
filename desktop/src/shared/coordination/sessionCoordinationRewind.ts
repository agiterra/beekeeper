/**
 * SV-29 rewind: the strict shapes of a 44221 `session.rewind` and of the
 * optional `rewind` key a 44224 receipt carries (WIRE-C3b §1, §2).
 *
 * A sibling of `sessionCoordinationStrictJson.ts`, and like it dependency-free
 * and erasable-syntax-only, so the conformance binder can load it under plain
 * `node --test`. Every reader that decides "this 44221/44224 is well-formed"
 * asks these two questions here, so a rewind has exactly one shape in the app.
 *
 * Exact-key discipline is absolute: a rewind action is exactly five keys, a
 * `rewind` receipt field exactly seven, and an unknown key is a rejection,
 * never a partial accept. Absent is not null.
 */

import {
  hasExactFields,
  isPlainObject,
} from "./sessionCoordinationJsonShapes.ts";

/** What a rewind does to the working tree: leave it, or restore the turn's base. */
export type SessionRewindFilesRequest = "keep" | "restore";

/** What happened to the working tree, as the provider signed it. */
export type SessionRewindFilesOutcome = "kept" | "restored" | "restore_failed";

/** The 44224 `rewind` field, decoded. */
export type SessionRewindReceiptFacts = {
  /** The 44231 turn checkpoint the rewind was cut at. */
  checkpoint: string;
  /** Generation whose seq numbering `cutAfterSeq` is in (seqs reset per generation). */
  cutGeneration: number;
  /** The last kept seq: `fromSeq − 1` of the checkpoint's turn. */
  cutAfterSeq: number;
  /** The generation the rewind detached (N); the receipt's session is N+1. */
  previousGeneration: number;
  files: SessionRewindFilesOutcome;
  /** The `pre_rewind` 44231 captured before any write; null only for keep outside a repo. */
  preRewindCheckpoint: string | null;
  /** HEAD's oid, unchanged by the rewind; null outside a repository. */
  head: string | null;
};

export const SESSION_REWIND_ACTION_KEYS = [
  "type",
  "session",
  "providerAuthorityPubkey",
  "checkpoint",
  "files",
] as const;

export const SESSION_REWIND_RECEIPT_KEYS = [
  "checkpoint",
  "cutGeneration",
  "cutAfterSeq",
  "previousGeneration",
  "files",
  "preRewindCheckpoint",
  "head",
] as const;

const HEX64 = /^[0-9a-f]{64}$/;
const OID = /^(?:[0-9a-f]{40}|[0-9a-f]{64})$/;
const FILES_REQUESTS: ReadonlySet<unknown> = new Set(["keep", "restore"]);
const FILES_OUTCOMES: ReadonlySet<unknown> = new Set([
  "kept",
  "restored",
  "restore_failed",
]);
/**
 * The lifecycle statuses a `rewind` may ride. A rewind reuses `resumed` /
 * `resumed_without_context` for success and `failed` for every refusal
 * (WIRE-C3b §2); any other status carrying it is malformed.
 */
const REWIND_RECEIPT_STATUSES: ReadonlySet<unknown> = new Set([
  "resumed",
  "resumed_without_context",
  "failed",
]);

/**
 * The 44221 actions that mint generation N+1 of the execution they name: a
 * resume, a restart, and a rewind (which also cuts what N+1 is seeded with,
 * but never what the chain is).
 */
export function isSessionNextGenerationAction(type: unknown): boolean {
  return (
    type === "session.resume" ||
    type === "session.restart" ||
    type === "session.rewind"
  );
}

function positiveInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) > 0;
}

function nonNegativeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

/** True when `action` has exactly the five keys of a `session.rewind`. */
export function hasStrictSessionRewindActionFields(
  action: Record<string, unknown>,
): boolean {
  return (
    action.type === "session.rewind" &&
    hasExactFields(action, [SESSION_REWIND_ACTION_KEYS])
  );
}

/**
 * The values only a rewind adds — `checkpoint` (a 44231 event id) and
 * `files` (a closed enum). The shared target and authority checks stay with
 * the caller, exactly as they are for resume and restart.
 */
export function hasStrictSessionRewindActionValues(
  action: Record<string, unknown>,
): boolean {
  return (
    typeof action.checkpoint === "string" &&
    HEX64.test(action.checkpoint) &&
    FILES_REQUESTS.has(action.files)
  );
}

/** Decode a receipt's `rewind` value, or null when it is not exactly one. */
export function readStrictSessionRewindReceipt(
  value: unknown,
): SessionRewindReceiptFacts | null {
  if (!isPlainObject(value)) return null;
  if (!hasExactFields(value, [SESSION_REWIND_RECEIPT_KEYS])) return null;
  const {
    checkpoint,
    cutGeneration,
    cutAfterSeq,
    previousGeneration,
    files,
    preRewindCheckpoint,
    head,
  } = value;
  if (
    typeof checkpoint !== "string" ||
    !HEX64.test(checkpoint) ||
    !positiveInteger(cutGeneration) ||
    !nonNegativeInteger(cutAfterSeq) ||
    !positiveInteger(previousGeneration) ||
    cutGeneration > previousGeneration ||
    !FILES_OUTCOMES.has(files) ||
    !(
      preRewindCheckpoint === null ||
      (typeof preRewindCheckpoint === "string" &&
        HEX64.test(preRewindCheckpoint))
    ) ||
    !(head === null || (typeof head === "string" && OID.test(head)))
  ) {
    return null;
  }
  return {
    checkpoint,
    cutGeneration,
    cutAfterSeq,
    previousGeneration,
    files: files as SessionRewindFilesOutcome,
    preRewindCheckpoint: preRewindCheckpoint as string | null,
    head: head as string | null,
  };
}

/**
 * Whether a receipt object's optional `rewind` key is acceptable: absent, or
 * exactly one valid `rewind` coupled to its receipt as buzz-core couples it
 * (`validate_receipt_rewind`, `coding_session_rewind.rs`; WIRE-C3b-FINAL):
 *
 * - on `resumed` / `resumed_without_context`, the receipt's target is
 *   generation `previousGeneration + 1` and `files` is `kept` or `restored`;
 * - on `failed`, only with `REWIND_NOT_RESTARTED` (any `files`);
 * - on any other status, never.
 */
export function hasAcceptableSessionRewindReceiptKey(
  content: Record<string, unknown>,
): boolean {
  if (!Object.hasOwn(content, "rewind")) return true;
  const rewind = readStrictSessionRewindReceipt(content.rewind);
  if (rewind === null || !REWIND_RECEIPT_STATUSES.has(content.status)) {
    return false;
  }
  if (content.status === "failed") {
    return (
      isPlainObject(content.error) &&
      content.error.code === "REWIND_NOT_RESTARTED"
    );
  }
  return (
    isPlainObject(content.session) &&
    content.session.generation === rewind.previousGeneration + 1 &&
    rewind.files !== "restore_failed"
  );
}

/**
 * The receipt's keys with `rewind` set aside, so an exact-key reader written
 * for the five-key envelope keeps checking exactly that.
 */
export function withoutSessionRewindReceiptKey(
  content: Record<string, unknown>,
): Record<string, unknown> {
  if (!Object.hasOwn(content, "rewind")) return content;
  const { rewind: _rewind, ...rest } = content;
  return rest;
}

/**
 * Attach a receipt's decoded `rewind` to the envelope a reader already
 * decoded from the same object; the envelope passes through untouched when
 * there is none. Call {@link hasAcceptableSessionRewindReceiptKey} first.
 */
export function withSessionRewindFacts<T extends object>(
  envelope: Readonly<T> | null,
  content: Record<string, unknown>,
): Readonly<T & { rewind?: Readonly<SessionRewindReceiptFacts> }> | null {
  if (envelope === null || !Object.hasOwn(content, "rewind")) return envelope;
  const rewind = readStrictSessionRewindReceipt(content.rewind);
  if (rewind === null) return null;
  return Object.freeze({ ...envelope, rewind: Object.freeze(rewind) });
}

/**
 * Decode a receipt whose reader was written for the exact envelope: refuse a
 * bad `rewind`, hand the envelope reader the object without it, then attach
 * the decoded `rewind` to what that reader returned.
 */
export function decodeWithSessionRewind<T extends object>(
  content: unknown,
  decodeEnvelope: (value: Record<string, unknown>) => Readonly<T> | null,
): Readonly<T & { rewind?: Readonly<SessionRewindReceiptFacts> }> | null {
  if (
    !isPlainObject(content) ||
    !hasAcceptableSessionRewindReceiptKey(content)
  ) {
    return null;
  }
  return withSessionRewindFacts(
    decodeEnvelope(withoutSessionRewindReceiptKey(content)),
    content,
  );
}
