/**
 * SV-35: what a `thread.model.set` this composer published has become, folded
 * from the provider's own signed answers (WIRE-C3a § 2–4).
 *
 * The fold reads three facts and nothing else:
 *
 * - the request this client signed (its `commandId` and `selection`);
 * - the **one** terminal 44224 under that `commandId` — `model_applied`, or a
 *   `turn_refused` / `turn_dropped` with the provider's code;
 * - the 44223 metadata for the same generation observed **after** that
 *   receipt. Its `model` is the only place the model in effect is read from:
 *   the receipt does not carry one, and the request is never shown as the
 *   running model.
 *
 * Until the receipt arrives the switch is pending — being in the provider's
 * mailbox is not being applied. A `model_applied` with no later 44223 yet is
 * still not a model: the state says so instead of showing the old metadata
 * under a new label.
 */
import type { CodingSessionLifecycleReceiptStatus } from "./codingSessionIngressPayloads";

/** The switch this client signed. */
export type CodingSessionModelSwitchRequest = {
  commandId: string;
  /** The selection string, verbatim as signed. */
  selection: string;
};

/** One verified 44224 under some command id, in arrival order. */
export type CodingSessionModelSwitchReceipt = {
  eventId: string;
  commandId: string;
  status: CodingSessionLifecycleReceiptStatus;
  error: { code: string; message: string } | null;
  createdAt: number;
  /** Position in this client's own arrival order. */
  order: number;
};

/** One verified 44223 for the switched generation, in arrival order. */
export type CodingSessionModelSwitchMetadata = {
  eventId: string;
  model: string | null;
  createdAt: number;
  order: number;
};

/** The provider's three refusal codes for a switch (WIRE-C3a § 2). */
export const CODING_SESSION_MODEL_SWITCH_REFUSAL_CODES = [
  "MODEL_SWITCH_UNSUPPORTED",
  "MODEL_NOT_OFFERED",
  "MODEL_SWITCH_FAILED",
] as const;

export type CodingSessionModelSwitchState =
  /** Nothing asked in this view. */
  | { kind: "idle" }
  /** Signed; the provider has not answered. */
  | { kind: "pending"; requested: string }
  /** `model_applied` arrived; no 44223 after it has been observed yet. */
  | { kind: "accepted"; requested: string }
  /**
   * `model_applied`, and a 44223 after it. `running` is that metadata's
   * `model`, which may legitimately differ from `requested` (a token the
   * adapter does not offer is left out, or the adapter reports another base).
   */
  | {
      kind: "applied";
      requested: string;
      running: string | null;
      matches: boolean;
    }
  /** Refused or dropped: the execution keeps its previous model. */
  | {
      kind: "refused";
      requested: string;
      outcome: "refused" | "dropped";
      code: string;
      message: string;
    };

/**
 * Fold one request against everything observed so far.
 *
 * Receipts under other command ids are ignored, so the caller may pass every
 * receipt it has seen. A refusal outranks an acceptance only because a
 * provider that published both has contradicted itself and the refusal is the
 * one that leaves the model unchanged — it never claims a switch happened.
 */
export function foldCodingSessionModelSwitch(input: {
  request: CodingSessionModelSwitchRequest | null;
  receipts: readonly CodingSessionModelSwitchReceipt[];
  metadata: readonly CodingSessionModelSwitchMetadata[];
}): CodingSessionModelSwitchState {
  const { request } = input;
  if (!request) return { kind: "idle" };
  const own = input.receipts.filter(
    (receipt) => receipt.commandId === request.commandId,
  );
  const refusal = own.find(
    (receipt) =>
      (receipt.status === "turn_refused" ||
        receipt.status === "turn_dropped") &&
      receipt.error !== null,
  );
  if (refusal?.error) {
    return {
      kind: "refused",
      requested: request.selection,
      outcome: refusal.status === "turn_dropped" ? "dropped" : "refused",
      code: refusal.error.code,
      message: refusal.error.message,
    };
  }
  const applied = own.find((receipt) => receipt.status === "model_applied");
  if (!applied) return { kind: "pending", requested: request.selection };
  // After the receipt in both senses: observed later here, and not signed
  // before it. Either alone could pick up the turn-end republish that raced
  // the receipt within the same second.
  const after = input.metadata
    .filter(
      (entry) =>
        entry.order > applied.order && entry.createdAt >= applied.createdAt,
    )
    .sort((left, right) => right.order - left.order)[0];
  if (!after) return { kind: "accepted", requested: request.selection };
  return {
    kind: "applied",
    requested: request.selection,
    running: after.model,
    matches: after.model === request.selection,
  };
}

/** True while the chips must not offer another switch over this one. */
export function isCodingSessionModelSwitchInFlight(
  state: CodingSessionModelSwitchState,
): boolean {
  return state.kind === "pending" || state.kind === "accepted";
}
