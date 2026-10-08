/**
 * SV-29 rewind: the optional `rewind` key a 44224 receipt carries
 * (NIP-CSL; WIRE-C3b-FINAL §2). The observer's copy of desktop's
 * `shared/coordination/sessionCoordinationRewind.ts` receipt half, coupled to
 * its receipt exactly as buzz-core's `validate_receipt_rewind` couples it.
 *
 * Exact-key discipline is absolute: a `rewind` value is exactly seven keys,
 * an unknown key is a rejection, and absent is not null.
 */
import { hasExactKeys, hasOwnKey, isPlainRecord } from "./wireDecode.ts";

/** What happened to the working tree, as the provider signed it. */
export type SessionRewindFilesOutcome = "kept" | "restored" | "restore_failed";

/** The 44224 `rewind` field, decoded. */
export type SessionRewindReceiptFacts = {
  /** The 44231 turn checkpoint the rewind was cut at. */
  checkpoint: string;
  /** Generation whose seq numbering `cutAfterSeq` is in. */
  cutGeneration: number;
  /** The last kept seq. */
  cutAfterSeq: number;
  /** The generation the rewind detached (N); a success names N+1. */
  previousGeneration: number;
  files: SessionRewindFilesOutcome;
  /** The `pre_rewind` 44231, or null for keep outside a repository. */
  preRewindCheckpoint: string | null;
  /** HEAD's oid, unchanged by the rewind; null outside a repository. */
  head: string | null;
};

const SESSION_REWIND_RECEIPT_KEYS = [
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
const FILES_OUTCOMES: ReadonlySet<unknown> = new Set([
  "kept",
  "restored",
  "restore_failed",
]);

function positiveInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) > 0;
}

function nonNegativeInteger(value: unknown): value is number {
  return Number.isSafeInteger(value) && (value as number) >= 0;
}

/** Decode a receipt's `rewind` value, or null when it is not exactly one. */
export function readStrictSessionRewindReceipt(
  value: unknown,
): SessionRewindReceiptFacts | null {
  if (!isPlainRecord(value)) return null;
  if (!hasExactKeys(value, SESSION_REWIND_RECEIPT_KEYS)) return null;
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
 * Whether a receipt's optional `rewind` key is acceptable: absent, or one
 * valid `rewind` on `resumed` / `resumed_without_context` naming generation
 * `previousGeneration + 1` with `files` kept or restored, or on `failed` with
 * `REWIND_NOT_RESTARTED` (any `files`). Never on any other status.
 */
export function hasAcceptableSessionRewindReceiptKey(
  content: Record<string, unknown>,
): boolean {
  if (!hasOwnKey(content, "rewind")) return true;
  const rewind = readStrictSessionRewindReceipt(content.rewind);
  if (rewind === null) return false;
  if (content.status === "failed") {
    return (
      isPlainRecord(content.error) &&
      content.error.code === "REWIND_NOT_RESTARTED"
    );
  }
  if (
    content.status !== "resumed" &&
    content.status !== "resumed_without_context"
  ) {
    return false;
  }
  return (
    isPlainRecord(content.session) &&
    content.session.generation === rewind.previousGeneration + 1 &&
    rewind.files !== "restore_failed"
  );
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
  if (!isPlainRecord(content)) return null;
  if (!hasAcceptableSessionRewindReceiptKey(content)) return null;
  if (!hasOwnKey(content, "rewind")) return decodeEnvelope(content);
  const { rewind: raw, ...rest } = content;
  const envelope = decodeEnvelope(rest);
  const rewind = readStrictSessionRewindReceipt(raw);
  if (envelope === null || rewind === null) return null;
  return Object.freeze({ ...envelope, rewind: Object.freeze(rewind) });
}
