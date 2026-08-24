/**
 * Keep a verified ingress store alive across a session switch.
 *
 * Every coding session lives in its own transport channel, so moving between
 * two sessions changes the ingress scope — and the scope used to own its store
 * outright. Leaving a session therefore threw away every event this client had
 * already fetched, verified, and classified, and coming back paid for all of it
 * again: a live subscription round trip, *then* a history fetch, with a
 * "Loading" screen over the whole thing. Switching between two open sessions
 * paid that twice, every time.
 *
 * Nothing here weakens the trust boundary. A retained store holds only events
 * that already passed the full classifier, and the cache is keyed by the
 * *authority identity* as well as the channel set, so a store verified under
 * one allowlist can never be handed to a scope reading under another — that is
 * a different key, and a different store. What is retained is work, not trust.
 *
 * The retained snapshot is by definition slightly behind: nothing was
 * subscribed while the scope was away. Re-mounting re-subscribes and refetches
 * history immediately, so the retained facts are what the surface paints
 * *while* that catch-up runs, never a substitute for it.
 */
import { TrustedCodingSessionIngressStore } from "./codingSessionTrustedIngress";

/**
 * How many ingress scopes stay warm.
 *
 * A retained store holds that scope's verified transcripts, so this is a real
 * memory cost and not a free win. Six covers the working set a person actually
 * flips between; the seventh evicts the least recently used, which then simply
 * pays the original load cost once.
 */
export const MAX_RETAINED_CODING_SESSION_INGRESS_STORES = 6;

const stores = new Map<string, TrustedCodingSessionIngressStore>();

/**
 * The store for `identity`, created on first use. Re-acquiring marks it as most
 * recently used, so the scope being switched *to* is never the one evicted.
 */
export function acquireCodingSessionIngressStore(
  identity: string,
): TrustedCodingSessionIngressStore {
  const existing = stores.get(identity);
  if (existing) {
    // Map iteration is insertion-ordered, so re-inserting is what makes the
    // eviction below least-recently-used rather than oldest-created.
    stores.delete(identity);
    stores.set(identity, existing);
    return existing;
  }
  const store = new TrustedCodingSessionIngressStore();
  stores.set(identity, store);
  while (stores.size > MAX_RETAINED_CODING_SESSION_INGRESS_STORES) {
    const oldest = stores.keys().next();
    if (oldest.done) break;
    stores.delete(oldest.value);
  }
  return store;
}

/**
 * The store for `identity` if one is already warm, without creating one.
 *
 * Read during render so a scope change can paint from facts this client has
 * already verified instead of an empty frame. Creating here would be wrong:
 * render must not have side effects, and an absent store is exactly the
 * "nothing known yet, still loading" answer the caller needs.
 */
export function peekCodingSessionIngressStore(
  identity: string,
): TrustedCodingSessionIngressStore | null {
  return stores.get(identity) ?? null;
}

/** Community switch teardown — see `resetCommunityState()`. */
export function resetCodingSessionIngressStores(): void {
  stores.clear();
}

/** How many scopes are currently warm, for tests and diagnostics. */
export function retainedCodingSessionIngressStoreCount(): number {
  return stores.size;
}
