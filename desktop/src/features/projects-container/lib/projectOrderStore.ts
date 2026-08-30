import * as React from "react";

import { useCommunities } from "@/features/communities/useCommunities";
import { relayClient } from "@/shared/api/relayClient";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useFeatureEnabled } from "@/shared/features";
import {
  DEFAULT_STORE,
  boundProjectOrderStore,
  readProjectOrderStore,
  writeProjectOrderStore,
  type ProjectOrderStore,
} from "./projectOrderStorage";
import {
  ProjectOrderSyncManager,
  type RemoteProjectOrder,
} from "./projectOrderSync";

/**
 * The user's project order as a module-level singleton.
 *
 * Deliberately not a per-mount hook like `useChannelSections`:
 * `useProjectContainers` is mounted by the sidebar, the manage panel, the
 * projects view, the container screen, and two create dialogs at once. A sync
 * manager per mount would open that many live subscriptions and race that many
 * debounced publishers against each other for one blob.
 *
 * Scoped to (pubkey, relayUrl). Switching either tears the previous scope down
 * — and `resetProjectOrderStore` is wired into `resetCommunityState` so a
 * community switch cannot leak one community's order into the next.
 */

type Scope = { pubkey: string; relayUrl: string };

let scopeKey: string | null = null;
let scope: Scope | null = null;
let manager: ProjectOrderSyncManager | null = null;
let store: ProjectOrderStore = DEFAULT_STORE;
let unsubscribeLive: (() => Promise<void>) | null = null;
let unsubscribeReconnects: (() => void) | null = null;
let lastAppliedRemoteTs = 0;
let lastAppliedEventId = "";
const listeners = new Set<() => void>();

function scopeKeyFor(pubkey: string, relayUrl: string): string {
  return `${pubkey} ${relayUrl}`;
}

function emit(): void {
  for (const listener of listeners) listener();
}

function setStore(next: ProjectOrderStore): void {
  if (next === store) return;
  store = next;
  emit();
}

/**
 * Whole-blob LWW against the remote head, mirroring the sidebar preference
 * managers: an older event, or a same-second event with a lower id, loses.
 */
function applyRemote(remote: RemoteProjectOrder): void {
  const current = scope;
  if (!current) return;
  if (remote.createdAt < lastAppliedRemoteTs) return;
  if (
    remote.createdAt === lastAppliedRemoteTs &&
    remote.eventId <= lastAppliedEventId
  ) {
    return;
  }
  lastAppliedRemoteTs = remote.createdAt;
  lastAppliedEventId = remote.eventId;
  manager?.cancelPendingPublish();
  if (!writeProjectOrderStore(current.pubkey, remote.store, current.relayUrl)) {
    return;
  }
  setStore(remote.store);
}

function teardown(): void {
  manager?.destroy();
  manager = null;
  if (unsubscribeLive) {
    const dispose = unsubscribeLive;
    unsubscribeLive = null;
    void dispose();
  }
  unsubscribeReconnects?.();
  unsubscribeReconnects = null;
  scope = null;
  scopeKey = null;
  lastAppliedRemoteTs = 0;
  lastAppliedEventId = "";
  setStore(DEFAULT_STORE);
}

function activate(pubkey: string, relayUrl: string): void {
  const key = scopeKeyFor(pubkey, relayUrl);
  if (key === scopeKey) return;
  teardown();

  scopeKey = key;
  scope = { pubkey, relayUrl };
  store = readProjectOrderStore(pubkey, relayUrl);
  emit();

  const active = new ProjectOrderSyncManager(pubkey, relayUrl);
  manager = active;

  void active.bootstrap(store).then((result) => {
    if (manager !== active) return;
    if (result.action === "apply-remote") applyRemote(result.data);
  });

  void active
    .subscribeToOrder((remote) => {
      if (manager !== active) return;
      applyRemote(remote);
    })
    .then((dispose) => {
      if (manager !== active) {
        void dispose();
        return;
      }
      unsubscribeLive = dispose;
    });

  unsubscribeReconnects = relayClient.subscribeToReconnects(() => {
    if (manager !== active) return;
    void active.fetchRemoteOrder().then((result) => {
      if (manager !== active) return;
      if (result.status === "found") applyRemote(result.data);
      const pending = active.getPendingStore();
      if (pending) active.publishOrder(pending);
    });
  });
}

/** Clear every trace of the current community's order. */
export function resetProjectOrderStore(): void {
  teardown();
}

function subscribe(listener: () => void): () => void {
  listeners.add(listener);
  return () => {
    listeners.delete(listener);
  };
}

function getSnapshot(): ProjectOrderStore {
  return store;
}

function getScopeKey(): string | null {
  return scopeKey;
}

/**
 * The user's project order plus the mutator the sidebar drag calls.
 *
 * `order` is empty until a scope is active (no identity or no community yet),
 * which leaves the alphabetical base order from `sortProjectContainers` in
 * place — the correct fallback rather than an arbitrary one.
 */
export function useProjectOrder(): {
  order: string[];
  reorderProjects: (orderedIds: string[]) => void;
} {
  const { activeCommunity } = useCommunities();
  const relayUrl = activeCommunity?.relayUrl;
  const identityQuery = useIdentityQuery();
  const pubkey = identityQuery.data?.pubkey?.toLowerCase();
  // Gated on the experiment, not merely mounted alongside it: the projects
  // sidebar is the only surface that reads or writes this order, so with the
  // flag off the fetch and live subscription below would be a relay round
  // trip on every boot that nothing consumes.
  const projectsEnabled = useFeatureEnabled("projects");

  // Depending on the live scope key (not just pubkey/relayUrl) is what makes
  // the store re-activatable. `resetCommunityState` can fire for a change that
  // leaves the relay URL alone — a token or repos-dir edit — and without this
  // dep the effect would never re-run, leaving the store torn down and every
  // drag a no-op until the next relay switch.
  const activeScopeKey = React.useSyncExternalStore(
    subscribe,
    getScopeKey,
    getScopeKey,
  );

  React.useEffect(() => {
    if (!projectsEnabled) {
      // Flag turned off mid-session: drop the subscription rather than leave
      // it feeding a store nothing reads.
      if (activeScopeKey !== null) resetProjectOrderStore();
      return;
    }
    if (!pubkey || !relayUrl) return;
    activate(pubkey, relayUrl);
  }, [projectsEnabled, pubkey, relayUrl, activeScopeKey]);

  const current = React.useSyncExternalStore(
    subscribe,
    getSnapshot,
    getSnapshot,
  );

  const reorderProjects = React.useCallback((orderedIds: string[]) => {
    const active = scope;
    if (!active) return;
    const next = boundProjectOrderStore({ version: 1, order: orderedIds });
    if (!writeProjectOrderStore(active.pubkey, next, active.relayUrl)) return;
    setStore(next);
    manager?.publishOrder(next);
  }, []);

  return { order: current.order, reorderProjects };
}

export const __projectOrderStoreTest = {
  activate,
  applyRemote,
  getSnapshot,
  reset: resetProjectOrderStore,
};
