import { KIND_SYSTEM_MESSAGE } from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";

/**
 * A whole-session deletion, as the mounted session surfaces learn of it.
 *
 * The relay soft-deletes a session's records and every later read omits
 * them, but the stores in a running app only ever add: a genesis a store
 * has admitted stays admitted until the scope changes or the app reloads
 * (`codingSessionCreateObservations.ts`, its `dispositions`). So a session
 * deleted from this app kept its "Not started" row and its page until a
 * restart — the row Andy saw under General after discarding a session that
 * never ran (2026-09-11). Two signals close that gap, both routed here:
 *
 * - **this app's own delete**: `useDeleteCodingSessionDialog` publishes the
 *   fact the moment the relay accepts the kind:5, and every mounted store
 *   forgets the session;
 * - **anybody's delete**: the relay announces an applied whole-session
 *   deletion under its own key as a kind 40099 in the session's channel
 *   (`emit_coding_session_deletion_receipt`, `side_effects.rs`); the stores
 *   subscribe to that receipt, signer-checked against the relay's `self`.
 *
 * Forgetting is a projection change only. Nothing is published from here.
 */
export type CodingSessionForgotten = {
  channelId: string;
  sessionRef: string;
  /** The genesis the deletion named; null when the caller had none. */
  genesisRef: string | null;
};

type Listener = (forgotten: CodingSessionForgotten) => void;

const listeners = new Set<Listener>();

/** Hear of sessions this app deleted, or saw deleted. */
export function subscribeToForgottenCodingSessions(
  listener: Listener,
): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

/** Tell every mounted store a session is gone from the relay. */
export function publishForgottenCodingSession(
  forgotten: CodingSessionForgotten,
): void {
  for (const listener of listeners) {
    try {
      listener(forgotten);
    } catch (error) {
      console.error("coding-session forget listener failed", error);
    }
  }
}

/** The relay's receipt `type` for an applied whole-session deletion. */
export const CODING_SESSION_DELETION_RECEIPT_TYPE =
  "coding_session_deletion_accepted";

const HEX64 = /^[0-9a-f]{64}$/;

/**
 * Read a relay-signed deletion receipt, or null.
 *
 * `relayPubkey` is the relay's NIP-11 `self`; a receipt signed by anybody
 * else is a member's message and hides nothing. The four facts are the
 * relay's own shape (`coding_session_deletion_receipt_content`); the channel
 * must match the receipt's channel tag, so a receipt cannot reach across.
 */
export function parseCodingSessionDeletionReceipt(
  event: RelayEvent,
  relayPubkey: string,
): CodingSessionForgotten | null {
  if (event.kind !== KIND_SYSTEM_MESSAGE) return null;
  if (event.pubkey.toLowerCase() !== relayPubkey.toLowerCase()) return null;
  let content: unknown;
  try {
    content = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (typeof content !== "object" || content === null) return null;
  const record = content as Record<string, unknown>;
  if (record.type !== CODING_SESSION_DELETION_RECEIPT_TYPE) return null;
  const { channelId, sessionRef, genesisRef } = record;
  if (typeof channelId !== "string" || channelId.length === 0) return null;
  if (typeof sessionRef !== "string" || sessionRef.length === 0) return null;
  if (typeof genesisRef !== "string" || !HEX64.test(genesisRef)) return null;
  const taggedChannel = event.tags.find((tag) => tag[0] === "h")?.[1];
  if (taggedChannel !== undefined && taggedChannel !== channelId) return null;
  return { channelId, sessionRef, genesisRef: genesisRef.toLowerCase() };
}
