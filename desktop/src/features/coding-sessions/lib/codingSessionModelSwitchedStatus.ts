/**
 * SV-35: the transcript row for a NIP-CST `model_switched` status item.
 *
 * The provider publishes it only after the adapter accepted a
 * `thread.model.set` at a turn boundary:
 * `{kind:"status", status:"model_switched", model, requested, commandId}`.
 * `model` is what took effect (the adapter's acknowledgement, the value 44223
 * `model` now carries); `requested` is the selection as sent. A token the
 * adapter did not offer is left out of `model`, so the row names the request
 * beside it only when the two differ — "Asked X · running Y" — and never
 * claims the request took effect.
 *
 * The signer named is the 44225's: the provider that applied the switch and
 * reported it. Who *asked* is the 44220's author, which this projection does
 * not hold, so it is not guessed.
 */
import { safeString } from "./codingSessionDefensive";

/** The status slug this row renders. */
export const CODING_SESSION_MODEL_SWITCHED_STATUS = "model_switched";

const MAX_SELECTION_CHARS = 200;

/**
 * The row's `{title, text}`, or `undefined` when the item is not a
 * well-formed `model_switched` (the generic "Status" row then covers it).
 */
export function codingSessionModelSwitchedRow(
  item: Record<string, unknown>,
  signerLabel: string | null | undefined,
): { title: string; text: string } | undefined {
  if (
    item.status !== CODING_SESSION_MODEL_SWITCHED_STATUS ||
    typeof item.model !== "string" ||
    item.model.trim().length === 0
  ) {
    return undefined;
  }
  const model = safeString(item.model, MAX_SELECTION_CHARS);
  const requested =
    typeof item.requested === "string" && item.requested.trim().length > 0
      ? safeString(item.requested, MAX_SELECTION_CHARS)
      : null;
  const signer = signerLabel?.trim() ? safeString(signerLabel, 120) : null;
  return {
    title: signer ? `Switched to ${model} — ${signer}` : `Switched to ${model}`,
    text:
      requested !== null && requested !== model
        ? `Asked ${requested} · running ${model}`
        : "",
  };
}
