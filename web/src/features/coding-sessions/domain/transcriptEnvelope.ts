/**
 * Strict mirror of the provider's 44225 CST envelope encoder.
 *
 * Copied from the envelope half of
 * `desktop/src/features/coding-sessions/lib/codingSessionTranscriptPresentation.ts`.
 */
import { canonicalizeProjectionPayload } from "./payload.ts";
import type { CodingSessionTranscriptItemV1 } from "./transcriptItemContract.ts";
import type { CodingSessionTarget } from "./types.ts";
import {
  boundedNonempty,
  decodeTarget,
  hasExactKeys,
  isPlainRecord,
  isWithinDepth,
  parseBoundedJson,
} from "./wireDecode.ts";

export const BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA =
  "buzz-coding-session-transcript/v1" as const;
export const CODING_SESSION_TRANSCRIPT_TAG_VERSION = "cst1-1" as const;

const MAX_TRANSCRIPT_CONTENT_BYTES = 32 * 1024;
const MAX_TRANSCRIPT_ITEM_DEPTH = 24;
const MAX_TRANSCRIPT_IDENTITY_BYTES = 512;

export type BeekeeperCodingSessionTranscriptV1 = {
  schema: typeof BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA;
  session: CodingSessionTarget;
  eventSeq: number;
  timestamp: number;
  turnId: string | null;
  item: CodingSessionTranscriptItemV1;
};

/** Decode one 44225 content, or null. Never throws. */
export function parseBeekeeperCodingSessionTranscript(
  content: unknown,
): Readonly<BeekeeperCodingSessionTranscriptV1> | null {
  const value = parseBoundedJson(content, MAX_TRANSCRIPT_CONTENT_BYTES);
  if (
    !isPlainRecord(value) ||
    !hasExactKeys(value, [
      "schema",
      "session",
      "eventSeq",
      "timestamp",
      "turnId",
      "item",
    ]) ||
    value.schema !== BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA
  ) {
    return null;
  }
  const session = decodeTarget(value.session, MAX_TRANSCRIPT_IDENTITY_BYTES);
  if (
    !session ||
    !Number.isSafeInteger(value.eventSeq) ||
    (value.eventSeq as number) <= 0 ||
    typeof value.timestamp !== "number" ||
    !Number.isFinite(value.timestamp) ||
    !Number.isFinite(new Date(value.timestamp).getTime()) ||
    !(
      value.turnId === null ||
      boundedNonempty(value.turnId, MAX_TRANSCRIPT_IDENTITY_BYTES)
    ) ||
    !isPlainRecord(value.item) ||
    !boundedNonempty(value.item.kind, MAX_TRANSCRIPT_IDENTITY_BYTES) ||
    !isWithinDepth(value.item, MAX_TRANSCRIPT_ITEM_DEPTH) ||
    canonicalizeProjectionPayload(value.item) === null
  ) {
    return null;
  }
  return Object.freeze({
    schema: BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session,
    eventSeq: value.eventSeq as number,
    timestamp: value.timestamp,
    turnId: value.turnId,
    item: Object.freeze(value.item) as CodingSessionTranscriptItemV1,
  });
}
