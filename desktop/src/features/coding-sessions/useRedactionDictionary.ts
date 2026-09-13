import * as React from "react";

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { resolveCodingSessionRedactions } from "@/shared/api/tauriSessionProvider";
import type { ResolvedRedaction } from "@/shared/ui/redactionDictionary";
import {
  collectRedactionLookupScope,
  type RedactionLookupScope,
} from "./lib/redactionDigests";

/**
 * Digest → plaintext, for every scope this process has already asked about.
 *
 * Deliberately **not** React Query. What is being fetched is neither server
 * state nor mutable: a digest names one fixed value, the answer comes from a
 * file on this machine over IPC, and it can never change for a given
 * `(signer, session, digest)`. Query caching, refetching, and retry buy nothing
 * against that, and `useQuery` needs a `QueryClientProvider` — which the
 * transcript is deliberately renderable without, so its behaviour can be
 * asserted from static markup.
 *
 * Keyed by signer *and* session as well as digest: two machines can
 * legitimately redact the same path, and the vault is per-session.
 */
const resolvedByKey = new Map<string, ResolvedRedaction>();

/**
 * Scopes that completed with nothing, so a miss is asked once rather than on
 * every render — a machine that answered nothing answers nothing again.
 *
 * Only *settled-empty* scopes live here. An in-flight lookup is in
 * `pendingLookups` instead: marking a scope "asked" the moment the request
 * fired was the original sin — a subscriber arriving between fire and land
 * (StrictMode's second effect run, a pop-out, a channel switch-and-back) found
 * the scope already asked, attached to nothing, and rendered the unresolved
 * pill forever while the answer sat in the cache below.
 */
const emptyScopes = new Set<string>();

/**
 * One shared promise per scope currently on the wire. Every subscriber —
 * however many mounts are looking at the same transcript — attaches to the
 * same lookup, and the entry is dropped when it settles so a rejected lookup
 * may be retried by a later mount.
 */
const pendingLookups = new Map<
  string,
  Promise<ReadonlyMap<string, ResolvedRedaction> | null>
>();

const EMPTY: ReadonlyMap<string, ResolvedRedaction> = new Map();

/**
 * Clear everything this process learned about local redactions.
 *
 * Registered in `resetCommunityState()`: a provider identity is minted per
 * relay, so switching communities makes every cached `(signer, session)` scope
 * meaningless. Nothing here would leak *across* communities on its own — the
 * keys are globally unique — but a cache that survives a switch is exactly the
 * class of module-level state that inventory exists to track.
 */
export function resetRedactionDictionary() {
  resolvedByKey.clear();
  emptyScopes.clear();
  pendingLookups.clear();
}

function cacheKey(signer: string, session: string, digest: string): string {
  return `${signer}:${session}:${digest}`;
}

function scopeCacheKey(scope: RedactionLookupScope): string {
  return `${scope.signerPubkey}:${scope.sessionId}:${scope.digests.join(",")}`;
}

/** How a lookup is performed; injected in tests, IPC everywhere else. */
export type RedactionResolver = (request: {
  digests: string[];
  providerPubkey: string;
  sessionId: string;
}) => Promise<Record<string, ResolvedRedaction>>;

/**
 * Answer `scope` from the vault, calling `onResolved` when something is known.
 *
 * Synchronously when the cache already holds it, later when a lookup (this
 * subscriber's or an earlier one still in flight) lands with entries, never
 * when the machine has nothing to say. The returned function cancels only this
 * subscriber's callback — the lookup itself keeps going, because its answer is
 * immutable and the next mount will want it from the cache.
 *
 * An empty result is never a claim about *why*. "Never recorded", "expired",
 * and "not this machine" are indistinguishable, and all three render the same
 * unresolved pill.
 */
export function subscribeRedactionResolution(
  scope: RedactionLookupScope,
  onResolved: (resolved: ReadonlyMap<string, ResolvedRedaction>) => void,
  resolver: RedactionResolver = resolveCodingSessionRedactions,
): () => void {
  // What the cache already holds is delivered at once — a remount, a pop-out,
  // a channel switch-and-back pay no second IPC round trip for it. But a
  // cache hit on *some* digests is not an answer for the rest: a transcript
  // being streamed grows a marker at a time, and answering the whole scope
  // from the first marker's cache entry left every later marker an
  // unresolved pill until a restart emptied the cache (Andy, 2026-09-12: the
  // values showed only after restarting the app). So the lookup is for the
  // digests the cache does not hold, and the two halves are merged on
  // landing.
  const cached = readCached(scope);
  if (cached) onResolved(cached);
  const missing = scope.digests.filter(
    (digest) =>
      !resolvedByKey.has(cacheKey(scope.signerPubkey, scope.sessionId, digest)),
  );
  if (missing.length === 0) return () => {};
  const missingScope: RedactionLookupScope = { ...scope, digests: missing };
  const scopeKey = scopeCacheKey(missingScope);
  if (emptyScopes.has(scopeKey)) return () => {};

  let lookup = pendingLookups.get(scopeKey);
  if (!lookup) {
    lookup = resolver({
      digests: missing,
      providerPubkey: scope.signerPubkey,
      sessionId: scope.sessionId,
    })
      .then((answer) => {
        const entries = Object.entries(answer);
        if (entries.length === 0) {
          emptyScopes.add(scopeKey);
          return null;
        }
        for (const [digest, entry] of entries) {
          resolvedByKey.set(
            cacheKey(scope.signerPubkey, scope.sessionId, digest),
            entry,
          );
        }
        return new Map(entries);
      })
      .finally(() => {
        pendingLookups.delete(scopeKey);
      });
    pendingLookups.set(scopeKey, lookup);
  }

  let cancelled = false;
  lookup
    .then((resolved) => {
      if (cancelled || !resolved) return;
      // Everything known for the scope, not only what this lookup fetched:
      // the subscriber replaces its map rather than merging.
      onResolved(readCached(scope) ?? resolved);
    })
    .catch((error) => {
      // A vault this machine cannot read is a machine that cannot answer,
      // not a transient fault: the honest fallback is the pill. But it is
      // logged, because a *rejected* lookup and an empty one render
      // identically, and swallowing the difference is what hid the original
      // session-id bug for a whole round of live testing.
      if (!cancelled) console.warn("redaction lookup failed", error);
    });
  return () => {
    cancelled = true;
  };
}

/**
 * Resolve the redaction markers in one transcript against this machine's own
 * vault.
 *
 * Asks only when the transcript carries markers *and* names a single session
 * and signer. The query firing is not the authorization — the backend checks
 * the signer again against the identities this desktop provisioned. This only
 * avoids a pointless call for someone else's session.
 */
export function useRedactionDictionary(
  items: readonly TranscriptItem[],
): ReadonlyMap<string, ResolvedRedaction> {
  const scope = React.useMemo(
    () => collectRedactionLookupScope(items),
    [items],
  );
  const scopeKey = scope ? scopeCacheKey(scope) : null;
  const [resolved, setResolved] =
    React.useState<ReadonlyMap<string, ResolvedRedaction>>(EMPTY);

  // The effect keys on `scopeKey`, a string, and reads the scope itself
  // through a ref. Depending on the `scope` *object* would resubscribe on
  // every render that rebuilt an equal scope.
  const scopeRef = React.useRef(scope);
  scopeRef.current = scope;

  React.useEffect(() => {
    const current = scopeRef.current;
    if (!current || !scopeKey) {
      setResolved(EMPTY);
      return;
    }
    return subscribeRedactionResolution(current, setResolved);
  }, [scopeKey]);

  return resolved;
}

/** Everything already known for `scope`, or `null` when nothing is. */
function readCached(
  scope: RedactionLookupScope,
): ReadonlyMap<string, ResolvedRedaction> | null {
  const found = new Map<string, ResolvedRedaction>();
  for (const digest of scope.digests) {
    const entry = resolvedByKey.get(
      cacheKey(scope.signerPubkey, scope.sessionId, digest),
    );
    if (entry) found.set(digest, entry);
  }
  return found.size === 0 ? null : found;
}
