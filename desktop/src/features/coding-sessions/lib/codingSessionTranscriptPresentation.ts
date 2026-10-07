import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionCommandTarget } from "./codingSessionCommand";
import type { CodingSessionIngressSource } from "./codingSessionIngressAuthority";
import { encodeStructuredKey } from "./codingSessionKeys";
import { canonicalizeProjectionPayload } from "./codingSessionPayload";
import type { CodingSessionTranscriptItemV1 } from "./codingSessionTranscriptItemContract";
import { projectCodingSessionTranscript } from "./codingSessionTranscriptProjection";
import type { CodingSessionTranscriptProjectionContext } from "./codingSessionTranscriptProjector";
import {
  boundedNonempty,
  decodeTarget,
  hasExactKeys,
  isPlainRecord,
  isWithinDepth,
  parseBoundedJson,
} from "./codingSessionWireDecode";

export const BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA =
  "buzz-coding-session-transcript/v1" as const;
export const CODING_SESSION_TRANSCRIPT_TAG_VERSION = "cst1-1" as const;

const MAX_TRANSCRIPT_CONTENT_BYTES = 32 * 1024;
const MAX_TRANSCRIPT_ITEM_DEPTH = 24;
const MAX_TRANSCRIPT_IDENTITY_BYTES = 512;

export type BeekeeperCodingSessionTranscriptV1 = {
  schema: typeof BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA;
  session: CodingSessionCommandTarget;
  eventSeq: number;
  timestamp: number;
  turnId: string | null;
  item: CodingSessionTranscriptItemV1;
};

export type TrustedCodingSessionTranscriptEntry = {
  channelId: string;
  targetKey: string;
  signerPubkey: string;
  transcript: Readonly<BeekeeperCodingSessionTranscriptV1>;
  eventId: string;
  createdAt: number;
  conflictCount: number;
};

/** Collision-free immutable transcript item key shared with the provider. */
export function codingSessionTranscriptSemanticKey(
  target: CodingSessionCommandTarget,
  eventSeq: number,
): string {
  return encodeStructuredKey(
    "coding-session-transcript/v1",
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
    String(eventSeq),
  );
}

/** Strict mirror of the provider's CST envelope encoder. */
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
    value.schema !== BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA
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
    schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session,
    eventSeq: value.eventSeq as number,
    timestamp: value.timestamp,
    turnId: value.turnId,
    item: Object.freeze(value.item) as CodingSessionTranscriptItemV1,
  });
}

/** Stable provider-neutral generation identity for catalog and presentation use. */
export function buildCodingSessionTranscriptGenerationId(
  channelId: string,
  signerPubkey: string,
  target: CodingSessionCommandTarget,
): string {
  return encodeStructuredKey(
    "coding-session-transcript-generation/v1",
    channelId,
    signerPubkey,
    target.driver,
    target.instanceId,
    target.sessionId,
    String(target.generation),
  );
}

/**
 * Project one generation's verified transcript entries into the shared
 * transcript renderer's item list.
 *
 * The entries go straight into the native projector — the only adaptation is
 * naming the CST envelope's `session` field `target`, which is what the
 * projector calls the same tuple. The donor needed a whole fake Hive
 * generation key here; that is what the native envelope layer removed.
 *
 * Ordering is by `eventSeq`, with the event id as a tie-break so a duplicate
 * sequence number cannot make the transcript order depend on arrival.
 */
export function projectTrustedCodingSessionTranscriptsToTranscript(
  entries: readonly TrustedCodingSessionTranscriptEntry[],
  channelId: string,
  signerPubkey: string,
  target: CodingSessionCommandTarget,
  source?: CodingSessionIngressSource | null,
): TranscriptItem[] {
  const exact = selectExactTrustedCodingSessionTranscriptEntries(
    entries,
    channelId,
    signerPubkey,
    target,
  );
  if (exact.length === 0) return [];

  const context = buildTrustedCodingSessionTranscriptProjectionContext(
    channelId,
    signerPubkey,
    target,
    source,
  );
  return projectCodingSessionTranscript(
    exact.map(trustedCodingSessionTranscriptEnvelope),
    {
      channelId: context.channelId,
      generationId: context.generationId,
      bridgeSource: { ...context.bridgeSource },
    },
  );
}

/**
 * The one generation a transcript shows: exact channel, signer and target,
 * with every conflicted entry withheld, in `eventSeq` order with the event id
 * as the tie-break. Shared by the full projection and the retained projector
 * so both see the same list.
 */
export function selectExactTrustedCodingSessionTranscriptEntries(
  entries: readonly TrustedCodingSessionTranscriptEntry[],
  channelId: string,
  signerPubkey: string,
  target: CodingSessionCommandTarget,
): TrustedCodingSessionTranscriptEntry[] {
  return entries
    .filter(
      (entry) =>
        entry.channelId === channelId &&
        entry.signerPubkey === signerPubkey &&
        entry.conflictCount === 0 &&
        sameTarget(entry.transcript.session, target),
    )
    .sort(
      (left, right) =>
        left.transcript.eventSeq - right.transcript.eventSeq ||
        left.eventId.localeCompare(right.eventId),
    );
}

/** The caller-owned scope one generation's items are projected under. */
export function buildTrustedCodingSessionTranscriptProjectionContext(
  channelId: string,
  signerPubkey: string,
  target: CodingSessionCommandTarget,
  source?: CodingSessionIngressSource | null,
): CodingSessionTranscriptProjectionContext {
  return {
    channelId,
    generationId: buildCodingSessionTranscriptGenerationId(
      channelId,
      signerPubkey,
      target,
    ),
    bridgeSource: {
      pubkey: signerPubkey,
      label: source?.label ?? "Trusted coding-session provider",
    },
  };
}

/**
 * The CST envelope as the native projector reads it: only `session` is renamed
 * `target`. The `turnId` key is always present, so a `null` stays the
 * provider's "no turn" rather than inviting synthetic reconstruction.
 */
export function trustedCodingSessionTranscriptEnvelope(
  entry: TrustedCodingSessionTranscriptEntry,
) {
  return {
    target: entry.transcript.session,
    eventSeq: entry.transcript.eventSeq,
    timestamp: entry.transcript.timestamp,
    turnId: entry.transcript.turnId,
    item: entry.transcript.item,
    sourceEventId: entry.eventId,
  };
}

function sameTarget(
  left: CodingSessionCommandTarget,
  right: CodingSessionCommandTarget,
): boolean {
  return (
    left.driver === right.driver &&
    left.instanceId === right.instanceId &&
    left.sessionId === right.sessionId &&
    left.generation === right.generation
  );
}
