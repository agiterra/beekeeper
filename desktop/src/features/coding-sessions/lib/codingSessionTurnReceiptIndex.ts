/**
 * The per-stage turn receipts (44224 `turn_queued` / `turn_started` /
 * `turn_injected` / `turn_degraded` / `turn_delivery_unknown` /
 * `turn_dropped` / `turn_refused` / `interrupt_delivered`), kept apart from
 * the lifecycle ones.
 *
 * Two facts force the separation, and both are correctness rather than tidiness:
 *
 * - **One turn produces several receipts.** Queued, then started; or degraded,
 *   then queued, then started; or queued, then dropped. A commandId-keyed
 *   bucket would read those as several disagreeing copies of one immutable
 *   fact and resolve the lot to a conflict, which is how the later receipts
 *   would go missing.
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
  /**
   * The sender asked for a delivery class the provider could not give this
   * turn, so it waits for the next boundary like any other. A stage rather
   * than a failure precisely because the turn still runs.
   *
   * `code` and `message` are the provider's own, and they are carried rather
   * than dropped because the reasons are not interchangeable. A runtime that
   * never advertised steering answers `STEER_UNSUPPORTED`; one that steers
   * perfectly well but whose turn ended before the input reached it answers
   * `STEER_TURN_ENDED`. Rendering the second as the first tells the person
   * their provider cannot do something it can, and sends them looking for a
   * capability problem that does not exist.
   */
  | { stage: "degraded"; code: string; message: string }
  | { stage: "started"; turnId: string }
  /**
   * The native steer landed: the runtime acknowledged the input as joined
   * into the turn already running, `turnId`. Ranked at least as high as
   * `started` because it is the later fact about the same command — and it
   * never opens a turn of its own.
   */
  | { stage: "injected"; turnId: string };

/**
 * How a turn ended without a run the provider can vouch for, in the
 * provider's own code and words.
 *
 * `"refused"` and `"dropped"` both mean the words never ran. `"unknown"` is
 * deliberately neither: a native steer was written to the runtime and its
 * delivery could not be established — the provider will not replay it, and
 * nothing here may claim the words ran or that they did not. A surface that
 * folded it into `"dropped"` would put the words back in the editor as if
 * refused, which is the double delivery the classes exist to prevent.
 */
export type CodingSessionTurnFailure = {
  code: string;
  message: string;
  outcome: "refused" | "dropped" | "unknown";
};

/** In-memory index of verified turn receipts, keyed per (channel, command, stage). */
export class CodingSessionTurnReceiptIndex {
  private readonly buckets = new Map<string, ReceiptBucket>();
  private readonly startedByTurn = new Map<string, ReceiptBucket>();

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
    if (status === "turn_started" && value.value.status === "turn_started") {
      const startedKey = startedIndexKey(channelId, value.value.turnId);
      const startedBucket = this.startedByTurn.get(startedKey) ?? new Map();
      startedBucket.set(value.eventId, value);
      this.startedByTurn.set(startedKey, startedBucket);
    }
  }

  /**
   * Signed provider receipt time for one provider turn, in Unix milliseconds.
   *
   * Duplicate identical receipts choose the earliest signed publication. Two
   * different payloads from one authority are a conflict and return `null`.
   */
  resolveStartedAtMs(
    channelId: string,
    turnId: string,
    providerAuthorityPubkey: string,
  ): number | null {
    const bucket = this.startedByTurn.get(startedIndexKey(channelId, turnId));
    if (!bucket) return null;
    const records = [...bucket.values()].filter(
      (record) => record.signerPubkey === providerAuthorityPubkey,
    );
    if (records.length === 0) return null;
    if (new Set(records.map((record) => record.canonicalPayload)).size !== 1) {
      return null;
    }
    const first = records.sort(
      (left, right) =>
        left.createdAt - right.createdAt ||
        left.eventId.localeCompare(right.eventId),
    )[0];
    return first ? first.createdAt * 1_000 : null;
  }

  /**
   * The terminal outcome that ended this turn without a run the provider can
   * vouch for, or `null` if it still has one.
   *
   * Refused outranks dropped only because a provider that somehow published
   * both said something about the sender, which is the more actionable of the
   * two; in practice exactly one is published. Both outrank delivery-unknown:
   * a late acknowledgement that reconciles an unknown attempt as never
   * delivered publishes `turn_dropped` (`STEER_NOT_DELIVERED`) under the same
   * command, and that later, definite answer is the one to show. And an
   * unknown is *withdrawn* by a `turn_injected` under the same command — the
   * other reconciliation — so once that receipt exists this reports `null`
   * rather than an answer the provider has since replaced.
   */
  resolveFailure(
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ): CodingSessionTurnFailure | null {
    for (const [status, outcome] of [
      ["turn_refused", "refused"],
      ["turn_dropped", "dropped"],
      ["turn_delivery_unknown", "unknown"],
    ] as const) {
      const receipt = this.resolve(
        channelId,
        commandId,
        status,
        providerAuthorityPubkey,
      );
      if (!receipt || receipt.error === null) continue;
      if (
        outcome === "unknown" &&
        this.resolve(
          channelId,
          commandId,
          "turn_injected",
          providerAuthorityPubkey,
        )
      ) {
        continue;
      }
      return {
        code: receipt.error.code,
        message: receipt.error.message,
        outcome,
      };
    }
    return null;
  }

  /**
   * How far the turn got.
   *
   * Later facts outrank earlier ones about the same turn, because they are not
   * competing claims: `turn_injected` over `turn_started` over `turn_degraded`
   * over `turn_queued`. A degraded steer is published alongside the
   * `turn_queued` that follows it — the provider says both — and the row shows
   * the degradation, which is the half the person did not ask for. An injected
   * steer is the terminal success of the native path and outranks everything
   * a boundary delivery would say. A provider that publishes none of the four
   * reports `null`, and the surfaces reading this say nothing rather than
   * guessing a stage.
   */
  resolveProgress(
    channelId: string,
    commandId: string,
    providerAuthorityPubkey: string,
  ): CodingSessionTurnProgress | null {
    const injected = this.resolve(
      channelId,
      commandId,
      "turn_injected",
      providerAuthorityPubkey,
    );
    if (injected && injected.status === "turn_injected") {
      return { stage: "injected", turnId: injected.turnId };
    }
    const started = this.resolve(
      channelId,
      commandId,
      "turn_started",
      providerAuthorityPubkey,
    );
    if (started && started.status === "turn_started") {
      return { stage: "started", turnId: started.turnId };
    }
    const degraded = this.resolve(
      channelId,
      commandId,
      "turn_degraded",
      providerAuthorityPubkey,
    );
    // The decoder requires `{code, message}` on a `turn_degraded`, so the
    // guard is a type narrowing rather than a real branch — but it degrades
    // into "the provider said something we could not read" instead of
    // asserting a reason, which is the whole point of carrying the field.
    if (degraded && degraded.error !== null) {
      return {
        stage: "degraded",
        code: degraded.error.code,
        message: degraded.error.message,
      };
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

function startedIndexKey(channelId: string, turnId: string): string {
  return encodeStructuredKey(
    "coding-session-turn-started-store/v1",
    channelId,
    turnId,
  );
}
