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
 * Scopes already asked about, so a miss is asked once rather than on every
 * render. Holds attempted scopes too — a machine that answered nothing answers
 * nothing again.
 */
const askedScopes = new Set<string>();

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
  askedScopes.clear();
}

function cacheKey(signer: string, session: string, digest: string): string {
  return `${signer}:${session}:${digest}`;
}

/**
 * Resolve the redaction markers in one transcript against this machine's own
 * vault.
 *
 * Asks only when the transcript carries markers *and* names a single session
 * and signer. The query firing is not the authorization — the backend checks
 * the signer again against the identities this desktop provisioned. This only
 * avoids a pointless call for someone else's session.
 *
 * An empty result is never a claim about *why*. "Never recorded", "expired",
 * and "not this machine" are indistinguishable, and all three render the same
 * unresolved pill.
 */
export function useRedactionDictionary(
  items: readonly TranscriptItem[],
): ReadonlyMap<string, ResolvedRedaction> {
  const scope = React.useMemo(
    () => collectRedactionLookupScope(items),
    [items],
  );
  const scopeKey = scope
    ? `${scope.signerPubkey}:${scope.sessionId}:${scope.digests.join(",")}`
    : null;
  const [resolved, setResolved] =
    React.useState<ReadonlyMap<string, ResolvedRedaction>>(EMPTY);

  // The effect keys on `scopeKey`, a string, and reads the scope itself
  // through a ref. Depending on the `scope` *object* would re-run the effect
  // — and fire its cleanup — on every render that rebuilt an equal scope,
  // cancelling the in-flight lookup before it could ever land, while
  // `askedScopes` prevented a retry. The value would then never resolve, which
  // is exactly what happened the first time this was written.
  const scopeRef = React.useRef(scope);
  scopeRef.current = scope;

  React.useEffect(() => {
    const current = scopeRef.current;
    if (!current || !scopeKey) {
      setResolved(EMPTY);
      return;
    }
    // A remount — pop-out, channel switch, community-scoped rebuild — answers
    // from the module cache without a second IPC round trip.
    const cached = readCached(current);
    if (cached) {
      setResolved(cached);
      return;
    }
    if (askedScopes.has(scopeKey)) return;
    askedScopes.add(scopeKey);

    let cancelled = false;
    void resolveCodingSessionRedactions({
      digests: current.digests,
      providerPubkey: current.signerPubkey,
      sessionId: current.sessionId,
    })
      .then((answer) => {
        const entries = Object.entries(answer);
        if (entries.length === 0) return;
        for (const [digest, entry] of entries) {
          resolvedByKey.set(
            cacheKey(current.signerPubkey, current.sessionId, digest),
            entry,
          );
        }
        if (!cancelled) setResolved(new Map(entries));
      })
      .catch(() => {
        // A vault this machine cannot read is a machine that cannot answer,
        // not a transient fault. The honest fallback is the pill.
      });
    return () => {
      cancelled = true;
    };
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
