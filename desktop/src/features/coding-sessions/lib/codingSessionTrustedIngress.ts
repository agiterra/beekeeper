/**
 * Verified ingress for the five signed coding-session event kinds this client
 * reads: 44223 metadata, 44224 lifecycle receipts, and 44225 transcript items.
 *
 * Native only. This fork owns its relay, so there is no compatibility
 * transport to sift ordinary chat traffic out of — every candidate arrives on
 * a kind that means exactly one thing, and anything wearing the right tags on
 * the wrong kind is malformed rather than merely uninteresting.
 */
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
} from "./codingSessionCommand";
import type { CodingSessionIngressAuthority } from "./codingSessionIngressAuthority";
import {
  type BeekeeperCodingSessionMetadataV1,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  type CodingSessionLifecycleReceipt,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  isCodingSessionTurnReceipt,
  parseBeekeeperCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./codingSessionIngressPayloads";
import { encodeStructuredKey } from "./codingSessionKeys";
import { canonicalizeProjectionPayload } from "./codingSessionPayload";
import {
  type BeekeeperCodingSessionTranscriptV1,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
  parseBeekeeperCodingSessionTranscript,
  type TrustedCodingSessionTranscriptEntry,
} from "./codingSessionTranscriptPresentation";
import {
  CodingSessionTurnReceiptIndex,
  compareStoredValueFreshness,
  type CodingSessionTurnProgress,
  type ReceiptBucket,
  resolveImmutableReceipt,
  type StoredValue,
} from "./codingSessionTurnReceiptIndex";
import {
  hasTagNamed,
  isExactProviderAuthorityPubkey,
  normalizePubkey,
  parseExactTags,
} from "./codingSessionWireDecode";

export type { CodingSessionTurnProgress } from "./codingSessionTurnReceiptIndex";
export {
  BUZZ_CODING_SESSION_METADATA_SCHEMA,
  type BeekeeperCodingSessionMetadataV1,
  CODING_SESSION_LIFECYCLE_RECEIPT_SCHEMA,
  CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION,
  CODING_SESSION_METADATA_TAG_VERSION,
  CODING_SESSION_TURN_RECEIPT_STATUSES,
  type CodingSessionLifecycleReceipt,
  type CodingSessionLifecycleReceiptStatus,
  codingSessionMetadataSemanticKey,
  codingSessionReceiptSemanticKey,
  type CodingSessionTurnReceipt,
  type CodingSessionTurnReceiptStatus,
  isCodingSessionTurnReceipt,
  isCodingSessionTurnReceiptStatus,
  lifecycleReceiptSemanticKey,
  parseBeekeeperCodingSessionMetadata,
  parseCodingSessionLifecycleReceipt,
} from "./codingSessionIngressPayloads";
export {
  BUZZ_CODING_SESSION_TRANSCRIPT_SCHEMA,
  type BeekeeperCodingSessionTranscriptV1,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionTranscriptSemanticKey,
  parseBeekeeperCodingSessionTranscript,
  type TrustedCodingSessionTranscriptEntry,
} from "./codingSessionTranscriptPresentation";
export { isExactProviderAuthorityPubkey } from "./codingSessionWireDecode";

/**
 * Raw events retained per generation so a pop-out window can bootstrap from
 * what this store already verified instead of re-querying the relay. Bounded:
 * a long session must not grow this without limit, and the oldest events are
 * the ones a bootstrap can most afford to refetch.
 */
export const MAX_RETAINED_RAW_EVENTS_PER_GENERATION = 2_000;

/** Oldest first: created_at, then id for a stable tie-break. */
const byAge = (left: RelayEvent, right: RelayEvent): number =>
  left.created_at - right.created_at || left.id.localeCompare(right.id);

export type CodingSessionGenerationScope = {
  channelId: string;
  targetKey: string;
  signerPubkey: string;
};

export type TrustedCodingSessionMetadataEntry = {
  channelId: string;
  targetKey: string;
  signerPubkey: string;
  metadata: Readonly<BeekeeperCodingSessionMetadataV1>;
  eventId: string;
  createdAt: number;
  conflictCount: number;
};

export type CodingSessionLifecycleResolution =
  | { state: "pending"; commandId: string }
  | {
      state: "failed";
      commandId: string;
      error: { code: string; message: string };
      rewind?: CodingSessionLifecycleReceipt["rewind"]; // SV-29, on a cut
    }
  | {
      state: "awaiting-metadata";
      commandId: string;
      target: CodingSessionCommandTarget;
      /** Metadata events for this target rejected as unreadable — a nonzero
       * count means schema drift, not a slow provider. */
      malformedMetadataCount: number;
    }
  | {
      state: "awaiting-metadata-after-failed-initial-turn";
      commandId: string;
      target: CodingSessionCommandTarget;
      error: { code: "INITIAL_TURN_FAILED"; message: string };
      malformedMetadataCount: number;
    }
  | {
      state: "created";
      commandId: string;
      target: CodingSessionCommandTarget;
      metadata: Readonly<BeekeeperCodingSessionMetadataV1>;
    }
  | {
      state: "created-with-failed-initial-turn";
      commandId: string;
      target: CodingSessionCommandTarget;
      metadata: Readonly<BeekeeperCodingSessionMetadataV1>;
      error: { code: "INITIAL_TURN_FAILED"; message: string };
    }
  /**
   * The provider reattached to this execution but could not restore what it
   * had before. Its own fact, not a flavour of `created`: the session and its
   * durable transcript are intact, and the agent behind it starts from nothing
   * — a difference the person prompting it has to be told about, because only
   * they can tell it what it has forgotten.
   */
  | {
      state: "resumed-without-context";
      commandId: string;
      target: CodingSessionCommandTarget;
      metadata: Readonly<BeekeeperCodingSessionMetadataV1>;
      error: { code: "CONTEXT_NOT_RECOVERED"; message: string };
    }
  | { state: "conflict"; commandId: string };

/**
 * The provider's signed refusal of one command — its code and its own words.
 *
 * A turn is refused (unauthorized operator, a stale generation, a session
 * already closed) or dropped (the provider's queue was full) with the same
 * 44224 receipt shape a lifecycle command is refused with, but nothing about a
 * turn is a lifecycle transition: there is no target to establish and no
 * metadata to wait for, so the only fact worth reading back is the error.
 */
export type CodingSessionCommandRefusal = {
  code: string;
  message: string;
  /**
   * How the provider ended this turn, when it said so per stage.
   *
   * `"refused"` is a decision about the sender or the target (an operator it
   * has not granted, a generation that has moved on); `"dropped"` is the
   * provider's own queue overflowing with the turn already accepted. The two
   * deserve different words, so the composer is told which it is. Absent on
   * the pre-stage `failed` receipt shape, which said only "no".
   *
   * `"unknown"` is a native steer whose delivery the provider could not
   * establish (`turn_delivery_unknown`). Terminal like the other two — it
   * will not be replayed — but *not* a statement that the words never ran, so
   * the composer must not restore them as if refused; the pending row says
   * "delivery unknown" and the person decides.
   */
  outcome?: "refused" | "dropped" | "unknown";
};

/**
 * The target of a resolution that established a usable session, or `null`.
 *
 * Three receipt outcomes establish one: a plain create, a create whose initial
 * turn failed, and a resume that recovered no prior context. All three name a
 * real target a screen may open and must not offer to retry; the difference
 * between them is what the person is told, never whether the session exists.
 */
export function establishedCodingSessionTarget(
  lifecycle: CodingSessionLifecycleResolution | null | undefined,
): CodingSessionCommandTarget | null {
  switch (lifecycle?.state) {
    case "created":
    case "created-with-failed-initial-turn":
    case "resumed-without-context":
      return lifecycle.target;
    default:
      return null;
  }
}

export type TrustedCodingSessionIngressSnapshot = {
  metadata: TrustedCodingSessionMetadataEntry[];
  transcripts: TrustedCodingSessionTranscriptEntry[];
  malformedCount: number;
  rejectedAuthorCount: number;
  invalidSignatureCount: number;
};

type ParsedTrustedIngressEvent =
  | {
      kind: "receipt";
      channelId: string;
      signerPubkey: string;
      receipt: Readonly<CodingSessionLifecycleReceipt>;
      canonicalPayload: string;
    }
  | {
      kind: "metadata";
      channelId: string;
      signerPubkey: string;
      metadata: Readonly<BeekeeperCodingSessionMetadataV1>;
      targetKey: string;
      canonicalPayload: string;
    }
  | {
      kind: "transcript";
      channelId: string;
      signerPubkey: string;
      transcript: Readonly<BeekeeperCodingSessionTranscriptV1>;
      targetKey: string;
      canonicalPayload: string;
    };

export type TrustedIngressClassification =
  | ParsedTrustedIngressEvent
  | { kind: "irrelevant" }
  | {
      kind: "malformed";
      /**
       * Set only when a metadata payload failed to decode after its tags
       * already verified: the `cs-target` tag names the session the rejected
       * bytes were about, so a lifecycle wait on that target can report
       * "metadata is arriving but this app cannot read it" (schema drift)
       * instead of an indistinguishable silence.
       */
      metadataTarget?: { channelId: string; targetKey: string };
    }
  | { kind: "rejected-author" }
  | { kind: "invalid-signature" };

type MetadataBucket = Map<
  string,
  StoredValue<Readonly<BeekeeperCodingSessionMetadataV1>>
>;
type StoredMetadataBucket = {
  channelId: string;
  targetKey: string;
  records: MetadataBucket;
};
type TranscriptBucket = Map<
  string,
  StoredValue<Readonly<BeekeeperCodingSessionTranscriptV1>>
>;
type StoredTranscriptBucket = {
  channelId: string;
  targetKey: string;
  eventSeq: number;
  signerPubkey: string;
  records: TranscriptBucket;
};

/**
 * Verify a receipt/metadata/transcript candidate at the governed boundary.
 *
 * The version tag decides which decoder runs, the kind must agree with it, and
 * the tag list must match the producer's exactly — order, count, and all.
 * Malformed candidates stay visible to diagnostics rather than vanishing.
 */
export function classifyTrustedCodingSessionIngressEvent(
  event: RelayEvent,
  allowedChannelIds: ReadonlySet<string>,
  authority: CodingSessionIngressAuthority,
): TrustedIngressClassification {
  const supportedKind =
    event.kind === KIND_CODING_SESSION_METADATA ||
    event.kind === KIND_CODING_SESSION_LIFECYCLE_RECEIPT ||
    event.kind === KIND_CODING_SESSION_TRANSCRIPT;
  if (!supportedKind || !Array.isArray(event.tags)) {
    return { kind: "irrelevant" };
  }
  const looksLikeReceipt = hasTagNamed(event.tags, "cslr-v");
  const looksLikeMetadata = hasTagNamed(event.tags, "csm-v");
  const looksLikeTranscript = hasTagNamed(event.tags, "cst-v");
  if (
    Number(looksLikeReceipt) +
      Number(looksLikeMetadata) +
      Number(looksLikeTranscript) !==
    1
  ) {
    // A coding-session kind with no version tag, or with two, is a broken
    // producer rather than unrelated traffic — the kind already committed it.
    return { kind: "malformed" };
  }
  if (
    (looksLikeReceipt &&
      event.kind !== KIND_CODING_SESSION_LIFECYCLE_RECEIPT) ||
    (looksLikeMetadata && event.kind !== KIND_CODING_SESSION_METADATA) ||
    (looksLikeTranscript && event.kind !== KIND_CODING_SESSION_TRANSCRIPT)
  ) {
    return { kind: "malformed" };
  }
  if (authority.state === "invalid") return { kind: "malformed" };
  const signerPubkey = normalizePubkey(event.pubkey);
  if (authority.state === "valid" && !authority.byPubkey.has(signerPubkey)) {
    return { kind: "rejected-author" };
  }
  const source = { pubkey: signerPubkey };
  if (!hasValidSignature(event)) return { kind: "invalid-signature" };

  if (looksLikeReceipt) {
    const tags = parseExactTags(event.tags, [
      "h",
      "cslr-v",
      "csl-command",
      "csl-key",
    ]);
    if (
      !tags ||
      !allowedChannelIds.has(tags[0]) ||
      tags[1] !== CODING_SESSION_LIFECYCLE_RECEIPT_TAG_VERSION
    ) {
      return { kind: "malformed" };
    }
    const receipt = parseCodingSessionLifecycleReceipt(event.content);
    if (
      !receipt ||
      tags[2] !== receipt.commandId ||
      // Per-stage for a turn receipt, single-field for a lifecycle one: the
      // tag must match the key the publish queue actually fenced on, or the
      // producer and this reader disagree about what is a duplicate.
      tags[3] !==
        codingSessionReceiptSemanticKey(receipt.commandId, receipt.status)
    ) {
      return { kind: "malformed" };
    }
    return {
      kind: "receipt",
      channelId: tags[0],
      signerPubkey: source.pubkey,
      receipt,
      canonicalPayload: JSON.stringify(receipt),
    };
  }

  if (looksLikeTranscript) {
    const tags = parseExactTags(event.tags, [
      "h",
      "cst-v",
      "cs-target",
      "cst-seq",
      "cst-key",
    ]);
    if (
      !tags ||
      !allowedChannelIds.has(tags[0]) ||
      tags[1] !== CODING_SESSION_TRANSCRIPT_TAG_VERSION
    ) {
      return { kind: "malformed" };
    }
    const transcript = parseBeekeeperCodingSessionTranscript(event.content);
    if (!transcript) return { kind: "malformed" };
    const targetKey = buildCodingSessionTargetKey(transcript.session);
    if (
      tags[2] !== targetKey ||
      tags[3] !== String(transcript.eventSeq) ||
      tags[4] !==
        codingSessionTranscriptSemanticKey(
          transcript.session,
          transcript.eventSeq,
        )
    ) {
      return { kind: "malformed" };
    }
    const canonicalPayload = canonicalizeProjectionPayload(transcript);
    if (canonicalPayload === null) return { kind: "malformed" };
    return {
      kind: "transcript",
      channelId: tags[0],
      signerPubkey: source.pubkey,
      transcript,
      targetKey,
      canonicalPayload,
    };
  }

  const tags = parseExactTags(event.tags, [
    "h",
    "csm-v",
    "cs-target",
    "csm-key",
  ]);
  if (
    !tags ||
    !allowedChannelIds.has(tags[0]) ||
    tags[1] !== CODING_SESSION_METADATA_TAG_VERSION
  ) {
    return { kind: "malformed" };
  }
  const metadata = parseBeekeeperCodingSessionMetadata(event.content);
  if (!metadata) {
    // Tags verified but the payload did not decode. Attribute the rejection
    // to its target so the wait state can surface schema drift.
    return {
      kind: "malformed",
      metadataTarget: { channelId: tags[0], targetKey: tags[2] },
    };
  }
  const targetKey = buildCodingSessionTargetKey(metadata.session);
  if (
    tags[2] !== targetKey ||
    tags[3] !== codingSessionMetadataSemanticKey(metadata.session)
  ) {
    return { kind: "malformed" };
  }
  return {
    kind: "metadata",
    channelId: tags[0],
    signerPubkey: source.pubkey,
    metadata,
    targetKey,
    canonicalPayload: JSON.stringify(metadata),
  };
}

/** In-memory verified ingress store. It owns no community-global state. */
export class TrustedCodingSessionIngressStore {
  private readonly receipts = new Map<string, ReceiptBucket>();
  /** Turn receipts, kept apart — see `codingSessionTurnReceiptIndex.ts`. */
  private readonly turnReceipts = new CodingSessionTurnReceiptIndex();
  private readonly metadata = new Map<string, StoredMetadataBucket>();
  private readonly transcripts = new Map<string, StoredTranscriptBucket>();
  private readonly rawEvents = new Map<string, Map<string, RelayEvent>>();
  private readonly dispositions = new Map<
    string,
    TrustedIngressClassification["kind"]
  >();
  private malformedCount = 0;
  private rejectedAuthorCount = 0;
  private invalidSignatureCount = 0;
  /**
   * Malformed metadata payloads attributed to their `cs-target`, keyed by
   * `compositeKey(channelId, targetKey)`. This is the schema-drift tripwire:
   * a lifecycle wait that sees a nonzero count here knows metadata for its
   * session is arriving but unreadable, which is a version mismatch to
   * report, not a silence to wait out. Note `dispositions` memoizes verdicts
   * per event id, so a decoder fix only re-examines old events once the
   * store is rebuilt (app restart or scope change).
   */
  private readonly malformedMetadataByTarget = new Map<string, number>();

  private readonly maxRetainedRawEventsPerGeneration: number;

  /**
   * The retention bound is injectable so a caller that knows its own bootstrap
   * budget — and the tests that have to reach the bound — can set one without
   * moving the default for everybody.
   */
  constructor(
    maxRetainedRawEventsPerGeneration = MAX_RETAINED_RAW_EVENTS_PER_GENERATION,
  ) {
    this.maxRetainedRawEventsPerGeneration = maxRetainedRawEventsPerGeneration;
  }

  private revision = 0;

  /** Monotonic revision of stored evidence, shared by every mounted reader. */
  getRevision(): number {
    return this.revision;
  }

  /** Returns whether newly ingested evidence or diagnostics need publication. */
  ingestRelayEvents(
    events: readonly RelayEvent[],
    channelIds: readonly string[],
    authority: CodingSessionIngressAuthority,
  ): boolean {
    let changed = false;
    const allowedChannels = new Set(channelIds);
    for (const event of events) {
      if (this.dispositions.has(event.id)) continue;
      const classified = classifyTrustedCodingSessionIngressEvent(
        event,
        allowedChannels,
        authority,
      );
      this.dispositions.set(event.id, classified.kind);
      if (classified.kind !== "irrelevant") changed = true;
      switch (classified.kind) {
        case "receipt": {
          if (isCodingSessionTurnReceipt(classified.receipt)) {
            this.turnReceipts.record(
              classified.channelId,
              classified.receipt.commandId,
              classified.receipt.status,
              {
                eventId: event.id,
                createdAt: event.created_at,
                signerPubkey: classified.signerPubkey,
                canonicalPayload: classified.canonicalPayload,
                value: classified.receipt,
              },
            );
            // Deliberately not retained as a raw event: the pop-out bootstrap
            // replays the bytes that establish a generation, and a turn
            // receipt establishes none. Retaining them would evict the
            // metadata that does, for a fact no bootstrap reads.
            break;
          }
          const key = compositeKey(
            classified.channelId,
            classified.receipt.commandId,
          );
          const bucket = this.receipts.get(key) ?? new Map();
          bucket.set(event.id, {
            eventId: event.id,
            createdAt: event.created_at,
            signerPubkey: classified.signerPubkey,
            canonicalPayload: classified.canonicalPayload,
            value: classified.receipt,
          });
          this.receipts.set(key, bucket);
          if (classified.receipt.session) {
            this.retainRawEvent(event, {
              channelId: classified.channelId,
              targetKey: buildCodingSessionTargetKey(
                classified.receipt.session,
              ),
              signerPubkey: classified.signerPubkey,
            });
          }
          break;
        }
        case "metadata": {
          const key = compositeKey(classified.channelId, classified.targetKey);
          const bucket = this.metadata.get(key) ?? {
            channelId: classified.channelId,
            targetKey: classified.targetKey,
            records: new Map(),
          };
          bucket.records.set(event.id, {
            eventId: event.id,
            createdAt: event.created_at,
            signerPubkey: classified.signerPubkey,
            canonicalPayload: classified.canonicalPayload,
            value: classified.metadata,
          });
          this.metadata.set(key, bucket);
          this.retainRawEvent(event, classified);
          break;
        }
        case "transcript": {
          const key = compositeKey(
            classified.channelId,
            encodeStructuredKey(
              "coding-session-transcript-store/v1",
              classified.targetKey,
              classified.signerPubkey,
              String(classified.transcript.eventSeq),
            ),
          );
          const bucket = this.transcripts.get(key) ?? {
            channelId: classified.channelId,
            targetKey: classified.targetKey,
            eventSeq: classified.transcript.eventSeq,
            signerPubkey: classified.signerPubkey,
            records: new Map(),
          };
          bucket.records.set(event.id, {
            eventId: event.id,
            createdAt: event.created_at,
            signerPubkey: classified.signerPubkey,
            canonicalPayload: classified.canonicalPayload,
            value: classified.transcript,
          });
          this.transcripts.set(key, bucket);
          this.retainRawEvent(event, classified);
          break;
        }
        case "malformed": {
          this.malformedCount += 1;
          if (classified.metadataTarget) {
            const key = compositeKey(
              classified.metadataTarget.channelId,
              classified.metadataTarget.targetKey,
            );
            this.malformedMetadataByTarget.set(
              key,
              (this.malformedMetadataByTarget.get(key) ?? 0) + 1,
            );
          }
          break;
        }
        case "rejected-author":
          this.rejectedAuthorCount += 1;
          break;
        case "invalid-signature":
          this.invalidSignatureCount += 1;
          break;
        case "irrelevant":
          break;
      }
    }
    if (changed) this.revision += 1;
    return changed;
  }

  snapshot(channelIds: readonly string[]): TrustedCodingSessionIngressSnapshot {
    const allowedChannels = new Set(channelIds);
    const metadata: TrustedCodingSessionMetadataEntry[] = [];
    const transcripts: TrustedCodingSessionTranscriptEntry[] = [];
    for (const bucket of this.metadata.values()) {
      if (!allowedChannels.has(bucket.channelId)) continue;
      const signerPubkeys = new Set(
        [...bucket.records.values()].map((record) => record.signerPubkey),
      );
      for (const signerPubkey of signerPubkeys) {
        const selected = resolveNewestMetadata(bucket.records, signerPubkey);
        if (!selected.value) continue;
        metadata.push({
          channelId: bucket.channelId,
          targetKey: bucket.targetKey,
          signerPubkey,
          metadata: selected.value.value,
          eventId: selected.value.eventId,
          createdAt: selected.value.createdAt,
          conflictCount: selected.conflictCount,
        });
      }
    }
    metadata.sort(
      (left, right) =>
        right.createdAt - left.createdAt ||
        left.channelId.localeCompare(right.channelId) ||
        left.targetKey.localeCompare(right.targetKey) ||
        left.signerPubkey.localeCompare(right.signerPubkey),
    );
    for (const bucket of this.transcripts.values()) {
      if (!allowedChannels.has(bucket.channelId)) continue;
      const selected = resolveImmutableTranscript(bucket.records);
      if (!selected.value) continue;
      transcripts.push({
        channelId: bucket.channelId,
        targetKey: bucket.targetKey,
        signerPubkey: bucket.signerPubkey,
        transcript: selected.value.value,
        eventId: selected.value.eventId,
        createdAt: selected.value.createdAt,
        conflictCount: selected.conflictCount,
      });
    }
    transcripts.sort(
      (left, right) =>
        left.channelId.localeCompare(right.channelId) ||
        left.targetKey.localeCompare(right.targetKey) ||
        left.signerPubkey.localeCompare(right.signerPubkey) ||
        left.transcript.eventSeq - right.transcript.eventSeq,
    );
    return {
      metadata,
      transcripts,
      malformedCount: this.malformedCount,
      rejectedAuthorCount: this.rejectedAuthorCount,
      invalidSignatureCount: this.invalidSignatureCount,
    };
  }

  /**
   * The verified raw events for one generation, oldest first.
   *
   * Handed to a pop-out window as its bootstrap set: it can rebuild the same
   * store from signed bytes and re-verify them itself, rather than trusting a
   * projection passed across a window boundary.
   */
  retainedRawEvents(scope: CodingSessionGenerationScope): RelayEvent[] {
    return [...(this.rawEvents.get(generationKey(scope))?.values() ?? [])].sort(
      byAge,
    );
  }

  /**
   * The bounded raw-event set a sidebar shelf cache persists: the newest
   * accepted 44223 metadata event per (channel, target, signer). Transcripts
   * are deliberately excluded — the shelf needs status/title/projectRef/
   * sessionRef, all carried by metadata, and transcript retention is
   * unbounded in a way a localStorage cache cannot afford. Like the pop-out
   * bootstrap, these are signed bytes: a rehydrating store re-runs the full
   * classifier (signature, authority, channel scope) rather than trusting a
   * projection.
   */
  retainedShelfEvents(): RelayEvent[] {
    const out: RelayEvent[] = [];
    for (const bucket of this.metadata.values()) {
      const signerPubkeys = new Set(
        [...bucket.records.values()].map((record) => record.signerPubkey),
      );
      for (const signerPubkey of signerPubkeys) {
        const selected = resolveNewestMetadata(bucket.records, signerPubkey);
        if (!selected.value) continue;
        const raw = this.rawEvents
          .get(
            generationKey({
              channelId: bucket.channelId,
              targetKey: bucket.targetKey,
              signerPubkey,
            }),
          )
          ?.get(selected.value.eventId);
        if (raw) out.push(raw);
      }
    }
    return out.sort(byAge);
  }

  /**
   * One command's generation lifecycle, from the lifecycle receipts alone.
   *
   * Turn receipts cannot reach this: they live in their own index, so a
   * `turn_refused` never fails a generation and a `turn_queued` never creates
   * one. A turn is an event inside a generation, never a change to it.
   */
  resolveLifecycle(
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ): CodingSessionLifecycleResolution {
    if (!isExactProviderAuthorityPubkey(providerAuthorityPubkey)) {
      return { state: "conflict", commandId };
    }
    const receiptBucket = this.receipts.get(compositeKey(channelId, commandId));
    if (!receiptBucket || receiptBucket.size === 0) {
      return { state: "pending", commandId };
    }
    const receipt = resolveImmutableReceipt(
      receiptBucket,
      providerAuthorityPubkey,
    );
    if (receipt === undefined) return { state: "pending", commandId };
    if (!receipt) return { state: "conflict", commandId };
    if (receipt.status === "failed") {
      const { error, rewind } = receipt;
      return { state: "failed", commandId, error, ...(rewind && { rewind }) };
    }
    const target = receipt.session;
    const failedInitialTurn =
      receipt.status === "created_with_failed_initial_turn"
        ? receipt.error
        : null;
    // A resume that recovered nothing is still a resume: the session exists,
    // so the wait for its metadata is the ordinary one. The fact only becomes
    // reportable once there is a session to report it about.
    const resumedWithoutContext =
      receipt.status === "resumed_without_context" ? receipt.error : null;
    const targetCompositeKey = compositeKey(
      channelId,
      buildCodingSessionTargetKey(target),
    );
    const metadataBucket = this.metadata.get(targetCompositeKey);
    const selected = metadataBucket
      ? resolveNewestMetadata(metadataBucket.records, providerAuthorityPubkey)
      : null;
    if (!selected || selected.matchedCount === 0 || !selected.value) {
      const malformedMetadataCount =
        this.malformedMetadataByTarget.get(targetCompositeKey) ?? 0;
      return failedInitialTurn
        ? {
            state: "awaiting-metadata-after-failed-initial-turn",
            commandId,
            target,
            error: failedInitialTurn,
            malformedMetadataCount,
          }
        : {
            state: "awaiting-metadata",
            commandId,
            target,
            malformedMetadataCount,
          };
    }
    if (failedInitialTurn) {
      return {
        state: "created-with-failed-initial-turn",
        commandId,
        target,
        metadata: selected.value.value,
        error: failedInitialTurn,
      };
    }
    if (resumedWithoutContext) {
      return {
        state: "resumed-without-context",
        commandId,
        target,
        metadata: selected.value.value,
        error: resumedWithoutContext,
      };
    }
    return {
      state: "created",
      commandId,
      target,
      metadata: selected.value.value,
    };
  }

  /**
   * Read the outcome that cost one turn its run, or `null` if it still has one.
   *
   * Three shapes say it. A provider that publishes per-stage receipts refuses
   * with `turn_refused` (the operator, the target, or the generation) or drops
   * with `turn_dropped` (its own queue overflowed). A provider from before
   * those statuses existed says the same thing with a plain lifecycle
   * `failed`, and that reading stays — the point of the fallback is a member
   * on a newer client talking to an older provider, which is the ordinary
   * case during a rollout.
   *
   * Silence still means the turn is alive: a turn that ran publishes no
   * failure, and a turn addressed to some other provider's session is ignored
   * without a word, because several providers watch one channel. Every gate
   * the lifecycle path applies applies here: an exact provider authority, that
   * authority's own signature, and a single agreeing payload (a disagreement
   * is a conflict, never a refusal).
   */
  resolveTurnRefusal(
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ): CodingSessionCommandRefusal | null {
    if (!isExactProviderAuthorityPubkey(providerAuthorityPubkey)) return null;
    const staged = this.turnReceipts.resolveFailure(
      channelId,
      commandId,
      providerAuthorityPubkey,
    );
    if (staged) return staged;
    const receiptBucket = this.receipts.get(compositeKey(channelId, commandId));
    if (!receiptBucket || receiptBucket.size === 0) return null;
    const receipt = resolveImmutableReceipt(
      receiptBucket,
      providerAuthorityPubkey,
    );
    // A missing receipt (nothing from this authority) and a null one (records
    // from this authority that disagree) are both absences of a refusal this
    // composer may claim, exactly as they are for a lifecycle command.
    if (receipt?.status !== "failed") return null;
    return receipt.error;
  }

  /**
   * How far a sent turn got, from its own per-stage receipts.
   *
   * `turn_started` outranks `turn_queued` because it is the later fact about
   * the same turn, not a competing claim. A provider that publishes neither
   * (every provider before this contract) reports `null`, and the surfaces
   * that read this say nothing rather than guessing a stage.
   */
  resolveTurnProgress(
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ): CodingSessionTurnProgress | null {
    if (!isExactProviderAuthorityPubkey(providerAuthorityPubkey)) return null;
    return this.turnReceipts.resolveProgress(
      channelId,
      commandId,
      providerAuthorityPubkey,
    );
  }

  /** Provider-signed `turn_started` receipt time for an execution turn. */
  resolveTurnStartedAtMs(
    channelId: string,
    turnId: string,
    providerAuthorityPubkey: string,
  ): number | null {
    if (!isExactProviderAuthorityPubkey(providerAuthorityPubkey)) return null;
    return this.turnReceipts.resolveStartedAtMs(
      channelId,
      turnId,
      providerAuthorityPubkey,
    );
  }

  private retainRawEvent(
    event: RelayEvent,
    scope: CodingSessionGenerationScope,
  ): void {
    const key = generationKey(scope);
    const retained = this.rawEvents.get(key) ?? new Map<string, RelayEvent>();
    retained.set(event.id, event);
    // Evict the oldest by created_at, not the first ingested: per-kind pages
    // and live events arrive out of time order. (Oldest = cheapest to refetch.)
    while (retained.size > this.maxRetainedRawEventsPerGeneration) {
      let oldest: RelayEvent | null = null;
      for (const candidate of retained.values()) {
        if (oldest === null || byAge(candidate, oldest) < 0) oldest = candidate;
      }
      if (oldest === null) break;
      retained.delete(oldest.id);
    }
    this.rawEvents.set(key, retained);
  }
}

function resolveImmutableTranscript(bucket: TranscriptBucket): {
  value: StoredValue<Readonly<BeekeeperCodingSessionTranscriptV1>> | null;
  conflictCount: number;
} {
  const records = [...bucket.values()];
  const payloads = new Set(records.map((record) => record.canonicalPayload));
  return {
    value: records.sort(compareStoredValueFreshness)[0] ?? null,
    conflictCount: Math.max(0, payloads.size - 1),
  };
}

/**
 * Pick the newest metadata for one signer.
 *
 * Metadata is last-writer-wins, and the writer is a process that legitimately
 * moves a session through several states inside one second (`starting` ->
 * `idle` -> `running`). Second-granularity `created_at` therefore cannot order
 * a burst, so two different payloads sharing the newest timestamp are resolved
 * deterministically on event id rather than treated as a disagreement — a
 * burst must never wedge a session on a conflict it will never resolve.
 * `conflictCount` still reports how many distinct payloads shared that second,
 * so the ambiguity stays visible without being fatal.
 */
function resolveNewestMetadata(
  bucket: MetadataBucket,
  providerAuthorityPubkey?: string,
): {
  value: StoredValue<Readonly<BeekeeperCodingSessionMetadataV1>> | null;
  conflictCount: number;
  matchedCount: number;
} {
  const records = [...bucket.values()].filter(
    (record) =>
      providerAuthorityPubkey === undefined ||
      record.signerPubkey === providerAuthorityPubkey,
  );
  if (records.length === 0) {
    return { value: null, conflictCount: 0, matchedCount: 0 };
  }
  const newestAt = Math.max(...records.map((record) => record.createdAt));
  const newest = records.filter((record) => record.createdAt === newestAt);
  const payloads = new Set(newest.map((record) => record.canonicalPayload));
  return {
    value: newest.sort(compareStoredValueFreshness)[0],
    conflictCount: Math.max(0, payloads.size - 1),
    matchedCount: records.length,
  };
}

function generationKey(scope: CodingSessionGenerationScope): string {
  return encodeStructuredKey(
    "coding-session-generation-raw/v1",
    scope.channelId,
    scope.targetKey,
    scope.signerPubkey,
  );
}

function compositeKey(channelId: string, semanticKey: string): string {
  return encodeStructuredKey(
    "coding-session-ingress-index/v1",
    channelId,
    semanticKey,
  );
}
