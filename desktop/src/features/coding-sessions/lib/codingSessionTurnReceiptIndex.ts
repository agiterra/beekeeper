/**
 * The per-stage turn receipts (44224 `turn_queued` / `turn_started` /
 * `turn_dropped` / `turn_refused`), kept apart from the lifecycle ones.
 *
 * Two facts force the separation, and both are correctness rather than tidiness:
 *
 * - **One turn produces several receipts.** Queued, then started; or queued,
 *   then dropped. A commandId-keyed bucket would read those as three
 *   disagreeing copies of one immutable fact and resolve the lot to a
 *   conflict, which is how the second receipt would go missing.
 * - **A turn receipt must never decide a generation's state.** A turn is an
 *   event *inside* a generation, never a change to one. Keeping the two in
 *   separate indexes makes that structural instead of a filter every future
 *   fold has to remember to write.
 *
 * Everything here is read-only bookkeeping over already-verified records: the
 * signature, authority, channel scope, and exact-key checks all happened in
 * `codingSessionTrustedIngress.ts` before anything reaches this index.
 */
import type {
  CodingSessionLifecycleReceipt,
  CodingSessionTurnReceiptStatus,
} from "./codingSessionIngressPayloads";
import { encodeStructuredKey } from "./codingSessionKeys";

/** One verified record, with the provenance every resolution reads. */
export type StoredValue<T> = {
  eventId: string;
  createdAt: number;
  signerPubkey: string;
  canonicalPayload: string;
  value: T;
};

/** The verified receipts stored under one key, by event id. */
export type ReceiptBucket = Map<
  string,
  StoredValue<Readonly<CodingSessionLifecycleReceipt>>
>;

/**
 * Newest first, then by signer, then by event id.
 *
 * The two tiebreakers exist so a resolution is a function of the events and
 * nothing else — two records sharing a second must not resolve differently
 * depending on which arrived first.
 */
export function compareStoredValueFreshness<T>(
  left: StoredValue<T>,
  right: StoredValue<T>,
): number {
  return (
    right.createdAt - left.createdAt ||
    left.signerPubkey.localeCompare(right.signerPubkey) ||
    left.eventId.localeCompare(right.eventId)
  );
}

/**
 * The single agreeing receipt from one authority.
 *
 * `undefined` = that authority has said nothing. `null` = it has said two
 * different things, which is a conflict and never an answer. A receipt is
 * immutable by contract, so disagreement is corruption rather than an update.
 */
export function resolveImmutableReceipt(
  bucket: ReceiptBucket,
  providerAuthorityPubkey: string,
): Readonly<CodingSessionLifecycleReceipt> | null | undefined {
  const records = [...bucket.values()].filter(
    (record) => record.signerPubkey === providerAuthorityPubkey,
  );
  if (records.length === 0) return undefined;
  const payloads = new Set(records.map((record) => record.canonicalPayload));
  return payloads.size === 1 ? records[0].value : null;
}

/**
 * How far a sent turn has got, as the provider's own signed receipts say.
 *
 * Deliberately not a lifecycle resolution: a turn establishes nothing. This is
 * only ever used to make an optimistic row tell the truth about itself —
 * "queued" is queued, not started, and neither is "the model is thinking".
 */
export type CodingSessionTurnProgress =
  | { stage: "queued" }
  | { stage: "started"; turnId: string };

/** Why a turn will never run, in the provider's own code and words. */
export type CodingSessionTurnFailure = {
  code: string;
  message: string;
  outcome: "refused" | "dropped";
};

/** In-memory index of verified turn receipts, keyed per (channel, command, stage). */
export class CodingSessionTurnReceiptIndex {
  private readonly buckets = new Map<string, ReceiptBucket>();

  /** Store one already-verified turn receipt. */
  record(
    channelId: string,
    commandId: string,
    status: CodingSessionTurnReceiptStatus,
    value: StoredValue<Readonly<CodingSessionLifecycleReceipt>>,
  ): void {
    const key = indexKey(channelId, commandId, status);
    const bucket = this.buckets.get(key) ?? new Map();
    bucket.set(value.eventId, value);
    this.buckets.set(key, bucket);
  }

  /**
   * The outcome that cost this turn its run, or `null` if it still has one.
   *
   * Refused outranks dropped only because a provider that somehow published
   * both said something about the sender, which is the more actionable of the
   * two; in practice exactly one is published.
   */
  resolveFailure(
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ): CodingSessionTurnFailure | null {
    for (const status of ["turn_refused", "turn_dropped"] as const) {
      const receipt = this.resolve(
        channelId,
        commandId,
        status,
        providerAuthorityPubkey,
      );
      if (!receipt || receipt.error === null) continue;
      return {
        code: receipt.error.code,
        message: receipt.error.message,
        outcome: status === "turn_dropped" ? "dropped" : "refused",
      };
    }
    return null;
  }

  /**
   * How far the turn got.
   *
   * `turn_started` outranks `turn_queued` because it is the later fact about
   * the same turn, not a competing claim. A provider that publishes neither
   * reports `null`, and the surfaces reading this say nothing rather than
   * guessing a stage.
   */
  resolveProgress(
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ): CodingSessionTurnProgress | null {
    const started = this.resolve(
      channelId,
      commandId,
      "turn_started",
      providerAuthorityPubkey,
    );
    if (started && started.status === "turn_started") {
      return { stage: "started", turnId: started.turnId };
    }
    return this.resolve(
      channelId,
      commandId,
      "turn_queued",
      providerAuthorityPubkey,
    )
      ? { stage: "queued" }
      : null;
  }

  private resolve(
    channelId: string,
    commandId: string,
    status: CodingSessionTurnReceiptStatus,
    providerAuthorityPubkey: string,
  ): Readonly<CodingSessionLifecycleReceipt> | null {
    const bucket = this.buckets.get(indexKey(channelId, commandId, status));
    if (!bucket || bucket.size === 0) return null;
    return resolveImmutableReceipt(bucket, providerAuthorityPubkey) ?? null;
  }
}

function indexKey(
  channelId: string,
  commandId: string,
  status: CodingSessionTurnReceiptStatus,
): string {
  return encodeStructuredKey(
    "coding-session-turn-receipt-store/v1",
    channelId,
    commandId,
    status,
  );
}
