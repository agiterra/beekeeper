import {
  advanceWatermark,
  readWatermark,
  runBootstrap,
  type FetchResult,
} from "@/features/sidebar/lib/sidebarSyncWatermark";
import { relayClient } from "@/shared/api/relayClient";
import {
  nip44DecryptFromSelf,
  nip44EncryptToSelf,
  signRelayEvent,
} from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_PROJECT_ORDER } from "@/shared/constants/kinds";
import {
  parseProjectOrderPayload,
  type ProjectOrderStore,
} from "./projectOrderStorage";

const D_TAG = "project-order";
const BLOB_TYPE = D_TAG;
const DEBOUNCE_MS = 2_000;

export type RemoteProjectOrder = {
  store: ProjectOrderStore;
  createdAt: number;
  eventId: string;
};

async function decryptAndParse(
  event: RelayEvent,
): Promise<RemoteProjectOrder | null> {
  try {
    const plaintext = await nip44DecryptFromSelf(event.content);
    const store = parseProjectOrderPayload(JSON.parse(plaintext));
    if (!store) return null;
    return { store, createdAt: event.created_at, eventId: event.id };
  } catch {
    return null;
  }
}

/**
 * Syncs the user's project order across their devices via encrypted NIP-78 app
 * data (kind 30078, d-tag `project-order`): NIP-44 encrypted-to-self content,
 * debounced writes, and whole-blob last-write-wins. Same shape as the sidebar's
 * four preference sync managers, and it shares their bootstrap/watermark
 * policy.
 */
export class ProjectOrderSyncManager {
  private pubkey: string;
  private relayUrl: string;
  private debounceTimer: number | null = null;
  private lastRemoteCreatedAt: number;
  private pendingStore: ProjectOrderStore | null = null;
  private lastPublishedStore: ProjectOrderStore | null = null;
  private destroyed = false;

  constructor(pubkey: string, relayUrl: string) {
    this.pubkey = pubkey;
    this.relayUrl = relayUrl;
    // Hydrate from localStorage so we never seed-publish if a remote blob has
    // been seen in a prior session.
    this.lastRemoteCreatedAt = readWatermark(pubkey, BLOB_TYPE, relayUrl);
  }

  async fetchRemoteOrder(): Promise<FetchResult<RemoteProjectOrder>> {
    try {
      const events = await relayClient.fetchEvents({
        kinds: [KIND_PROJECT_ORDER],
        authors: [this.pubkey],
        "#d": [D_TAG],
        limit: 1,
      });
      if (events.length === 0 || events[0].pubkey !== this.pubkey) {
        return { status: "absent" };
      }
      const event = events[0];
      // An event exists — record its created_at regardless of whether we can
      // decrypt it, so seed-publish is blocked even when the payload is
      // unreadable (e.g. wrong key).
      this.recordRemoteHead(event.created_at);
      const result = await decryptAndParse(event);
      if (!result) {
        return { status: "failed", createdAt: event.created_at };
      }
      return {
        status: "found",
        data: result,
        createdAt: result.createdAt,
        eventId: result.eventId,
      };
    } catch {
      return { status: "failed" };
    }
  }

  /** Update in-memory + persisted watermark. */
  private recordRemoteHead(createdAt: number): void {
    if (createdAt > this.lastRemoteCreatedAt) {
      this.lastRemoteCreatedAt = createdAt;
    }
    advanceWatermark(this.pubkey, BLOB_TYPE, this.relayUrl, createdAt);
  }

  cancelPendingPublish(): void {
    if (this.debounceTimer !== null) {
      window.clearTimeout(this.debounceTimer);
      this.debounceTimer = null;
    }
  }

  getPendingStore(): ProjectOrderStore | null {
    return this.pendingStore;
  }

  publishOrder(store: ProjectOrderStore): void {
    this.pendingStore = store;
    if (this.debounceTimer !== null) {
      window.clearTimeout(this.debounceTimer);
    }
    this.debounceTimer = window.setTimeout(() => {
      this.debounceTimer = null;
      void this.doPublish(store);
    }, DEBOUNCE_MS);
  }

  private async fetchOwnBlobBeforePublish(
    store: ProjectOrderStore,
  ): Promise<ProjectOrderStore> {
    try {
      const events = await relayClient.fetchEvents({
        kinds: [KIND_PROJECT_ORDER],
        authors: [this.pubkey],
        "#d": [D_TAG],
        limit: 1,
      });
      if (events.length === 0 || events[0].pubkey !== this.pubkey) return store;
      const event = events[0];
      // Snapshot the watermark before advancing it: after recordRemoteHead
      // runs, lastRemoteCreatedAt equals event.created_at, so the LWW
      // comparison remote.createdAt > lastRemoteCreatedAt would always be
      // false and silently suppress the merge.
      const headBeforeFetch = this.lastRemoteCreatedAt;
      this.recordRemoteHead(event.created_at);
      const remote = await decryptAndParse(event);
      if (!remote) return store;
      // Whole-blob LWW: take whichever is newer.
      if (remote.createdAt > headBeforeFetch) {
        return remote.store;
      }
      return store;
    } catch {
      return store;
    }
  }

  private isIdenticalToLastPublished(store: ProjectOrderStore): boolean {
    const last = this.lastPublishedStore;
    if (!last) return false;
    if (last.order.length !== store.order.length) return false;
    return last.order.every((id, index) => id === store.order[index]);
  }

  private async doPublish(store: ProjectOrderStore): Promise<void> {
    try {
      const merged = await this.fetchOwnBlobBeforePublish(store);
      // Guard: the manager may have been destroyed while
      // fetchOwnBlobBeforePublish was awaited (community switch during an
      // in-flight fetch). If so, abort before touching the relay.
      if (this.destroyed) return;
      if (this.isIdenticalToLastPublished(merged)) {
        this.pendingStore = null;
        return;
      }
      const payload = { version: 1, order: merged.order };
      const ciphertext = await nip44EncryptToSelf(JSON.stringify(payload));
      const createdAt = Math.max(
        Math.floor(Date.now() / 1_000),
        this.lastRemoteCreatedAt + 1,
      );
      const event = await signRelayEvent({
        kind: KIND_PROJECT_ORDER,
        content: ciphertext,
        createdAt,
        tags: [
          ["d", D_TAG],
          ["t", D_TAG], // relay discoverability; not used in our filters
        ],
      });
      // Final guard immediately before the network call — the relay socket may
      // have moved to a different community by the time we reach this point.
      if (this.destroyed) return;
      await relayClient.publishEvent(
        event,
        "Timed out publishing project order.",
        "Failed to publish project order.",
      );
      this.recordRemoteHead(event.created_at);
      this.lastPublishedStore = merged;
      this.pendingStore = null;
    } catch (error) {
      console.warn("[projectOrderSync] publish failed:", error);
    }
  }

  async subscribeToOrder(
    onUpdate: (remote: RemoteProjectOrder) => void,
  ): Promise<() => Promise<void>> {
    return relayClient.subscribeLive(
      {
        kinds: [KIND_PROJECT_ORDER],
        authors: [this.pubkey],
        "#d": [D_TAG],
        limit: 0,
      },
      (event: RelayEvent) => {
        if (event.pubkey !== this.pubkey) return;
        // Record the raw head before decrypt so an undecryptable live event
        // still advances the watermark and blocks future seed-publish.
        this.recordRemoteHead(event.created_at);
        void decryptAndParse(event).then((result) => {
          if (result) {
            onUpdate(result);
          }
        });
      },
    );
  }

  /**
   * Fetches the remote blob on first mount, records the remote head, and
   * delegates the seed/hold/apply-remote decision to `runBootstrap`.
   */
  async bootstrap(localStore: ProjectOrderStore) {
    const fetchResult = await this.fetchRemoteOrder();
    return runBootstrap({
      fetchResult,
      lastHead: this.lastRemoteCreatedAt,
      localStore,
      isLocalNonEmpty: (s) => s.order.length > 0,
      publishFn: (s) => this.publishOrder(s),
    });
  }

  destroy(): void {
    // Cancel any pending publish and mark this manager as destroyed so any
    // in-flight doPublish() calls abort before reaching relayClient. Pending
    // debounce-window changes are intentionally dropped: flushing could
    // publish relay A's order to relay B via the shared relayClient singleton.
    this.destroyed = true;
    this.cancelPendingPublish();
    this.pendingStore = null;
  }
}
