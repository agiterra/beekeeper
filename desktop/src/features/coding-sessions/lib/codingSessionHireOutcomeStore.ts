/**
 * The published store of hire outcomes.
 *
 * Split out of `hooks/useCodingSessionHire.ts` when that file reached the
 * repository's 1000-line ceiling. It is a coherent unit on its own: one
 * module-level store, its subscription, and the React binding surfaces read it
 * through. The hook remains its only writer in the product, which is what the
 * note below depends on.
 */
import * as React from "react";

import type { CodingSessionHireOutcome } from "../hooks/useCodingSessionHire";

/**
 * Every outcome this host has produced since the community-scoped subtree
 * mounted, published so a surface with no access to the hook can read it.
 *
 * Module-level, and therefore community-scoped state — but it needs no entry
 * in `resetCommunityState()`, because its only writer is
 * `useCodingSessionHire`, which clears it both when its subscription
 * starts and when it is torn down. Switching communities remounts
 * `CodingSessionHireHost` (`App.tsx` keys the subtree), so the teardown runs
 * and no previous community's hires can be read here. If a second writer is
 * ever added, that stops being true and the reset belongs in
 * `useCommunityInit.ts`.
 */
let publishedHireOutcomes: readonly CodingSessionHireOutcome[] = [];
const hireOutcomeListeners = new Set<() => void>();

/** Replace the published list and wake every watcher. The hook's own writer. */
export function publishHireOutcomes(
  next: readonly CodingSessionHireOutcome[],
): void {
  publishedHireOutcomes = next;
  for (const listener of [...hireOutcomeListeners]) listener();
}

/** The outcomes this host has published, newest last. */
export function readCodingSessionHireOutcomes(): readonly CodingSessionHireOutcome[] {
  return publishedHireOutcomes;
}

/** Watch the published outcomes. Returns the unsubscribe. */
export function subscribeToCodingSessionHireOutcomes(
  listener: () => void,
): () => void {
  hireOutcomeListeners.add(listener);
  return () => {
    hireOutcomeListeners.delete(listener);
  };
}

/** Forget everything answered so far. Called by the hook, and by tests. */
export function resetCodingSessionHireOutcomes(): void {
  publishHireOutcomes([]);
}

/**
 * Seed the store. Tests only — the host is the one writer in the product, and
 * the doc above that store depends on it staying that way.
 */
export function publishCodingSessionHireOutcomes(
  outcomes: readonly CodingSessionHireOutcome[],
): void {
  publishHireOutcomes(outcomes);
}

/** The published outcomes, as React state, for surfaces outside this hook. */
export function useCodingSessionHireOutcomes(): readonly CodingSessionHireOutcome[] {
  return React.useSyncExternalStore(
    subscribeToCodingSessionHireOutcomes,
    readCodingSessionHireOutcomes,
    readCodingSessionHireOutcomes,
  );
}
