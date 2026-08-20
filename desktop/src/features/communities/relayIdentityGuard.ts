import { getRelaySelf } from "@/features/moderation/lib/relaySelf";

import {
  decideRelayIdentityAction,
  type RelayIdentityAction,
  type RelayIdentityObservation,
} from "./relayIdentity";
import {
  evictRelayScopedCommunityCaches,
  purgeNativeRelayScopedStores,
} from "./relayScopedCacheEviction";
import type { Community } from "./types";

/**
 * How long the NIP-11 `self` probe may hold up community readiness.
 *
 * The probe has to finish *before* the community-scoped UI renders — a wipe
 * after render would leave components already hydrated from the stale
 * snapshot — so it sits on the boot path. Bounding it keeps an unreachable
 * relay from turning into an indefinite splash screen; on timeout the guard
 * fails closed (does nothing) and the next connect re-checks.
 */
export const RELAY_IDENTITY_PROBE_TIMEOUT_MS = 2_500;

/**
 * Probe the active relay's NIP-11 `self` pubkey.
 *
 * Never rejects. A network failure, a malformed document, or exceeding
 * `timeoutMs` all resolve to `{ status: "unresolved" }`, which the decision
 * function treats as "do nothing".
 */
export async function observeRelayIdentity(
  timeoutMs: number = RELAY_IDENTITY_PROBE_TIMEOUT_MS,
): Promise<RelayIdentityObservation> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<RelayIdentityObservation>((resolve) => {
    timer = setTimeout(() => resolve({ status: "unresolved" }), timeoutMs);
  });

  try {
    return await Promise.race([
      getRelaySelf().then(
        (pubkey): RelayIdentityObservation => ({
          status: "resolved",
          pubkey,
        }),
        (): RelayIdentityObservation => ({ status: "unresolved" }),
      ),
      timeout,
    ]);
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
  }
}

/**
 * Reconcile a community's stored relay identity against the relay's live
 * NIP-11 `self`, applying the resulting action.
 *
 * - `adopt` — records the observed key. This is what every pre-existing
 *   community (no stored key) does on its first connect after upgrading:
 *   **no cache is touched**. Treating "unknown" as a mismatch would wipe every
 *   existing user's caches on upgrade, which is exactly the outcome the
 *   decision function refuses to produce.
 * - `rekey` — the relay is signing with a different key than the one recorded,
 *   so it is a new instance at a familiar URL. Every relay-URL-scoped cache is
 *   evicted (browser storage first, then the native stores) and the record is
 *   re-keyed to the observed identity.
 * - `none` — anything ambiguous. Nothing is written, nothing is deleted.
 *
 * Never rejects; returns the action that was applied so callers can log it.
 */
export async function reconcileRelayIdentity({
  community,
  adoptRelayPubkey,
  observe = observeRelayIdentity,
}: {
  community: Community;
  /** Persist the observed relay pubkey onto the community record. */
  adoptRelayPubkey: (relayPubkey: string) => void;
  /** Seam for tests. */
  observe?: () => Promise<RelayIdentityObservation>;
}): Promise<RelayIdentityAction> {
  const action = decideRelayIdentityAction({
    storedRelayPubkey: community.relayPubkey,
    observation: await observe(),
  });

  if (action.kind === "rekey") {
    console.warn(
      `[relayIdentityGuard] ${community.relayUrl} is now signing as ` +
        `${action.relayPubkey} (was ${action.previousRelayPubkey}); treating ` +
        "it as a new relay instance and clearing its cached scope.",
    );
    evictRelayScopedCommunityCaches({
      communityId: community.id,
      relayUrl: community.relayUrl,
    });
    try {
      await purgeNativeRelayScopedStores(community.relayUrl);
    } catch (error) {
      // The record is still re-keyed below. Leaving it un-keyed would re-wipe
      // the *new* instance's freshly built caches on every subsequent boot,
      // which costs more than the stale native rows this leaves behind — but
      // those rows are a real leak, so say so loudly rather than swallowing it.
      console.error(
        "[relayIdentityGuard] native relay-scoped purge failed; local archive " +
          "and retention rows from the previous relay instance remain:",
        error,
      );
    }
  }

  if (action.kind === "adopt" || action.kind === "rekey") {
    adoptRelayPubkey(action.relayPubkey);
  }

  return action;
}
