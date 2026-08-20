import { clearSavedCommunitySnapshot } from "@/features/agents/activeAgentTurnsStore";
import { removeChannelSnapshotForRelay } from "@/features/channels/channelSnapshot";
import { removeThreadActivityForRelay } from "@/features/channels/threadActivityStorage";
import { removeMessageSnapshotsForRelay } from "@/features/messages/lib/messageSnapshot";
import { removeSelfProfileCachesForRelay } from "@/features/profile/lib/selfProfileStorage";
import { removeUserLabelCacheForRelay } from "@/features/profile/lib/userLabelStorage";
import { invokeTauri } from "@/shared/api/tauri";

import { removeCommunityDestination } from "./communityNavigationStorage";

/**
 * Tear down every browser-side cache scoped to one community's relay URL or
 * community id.
 *
 * This is the single inventory used by two callers with different triggers:
 * removing a community (the user left), and re-keying a community after its
 * relay came back with a different NIP-11 `self` identity. Both mean "nothing
 * cached under this relay URL describes reality any more".
 *
 * Every underlying helper swallows storage failures, so this never throws.
 *
 * Deliberately **not** included: the native (Rust) stores. Removing a
 * community keeps its local archive and retention databases; only a relay
 * identity change invalidates those, and that caller purges them explicitly
 * via {@link purgeNativeRelayScopedStores}.
 */
export function evictRelayScopedCommunityCaches({
  communityId,
  relayUrl,
}: {
  communityId: string;
  relayUrl: string;
}): void {
  removeSelfProfileCachesForRelay(relayUrl);
  removeUserLabelCacheForRelay(relayUrl);
  removeChannelSnapshotForRelay(relayUrl);
  removeMessageSnapshotsForRelay(relayUrl);
  // Home/Inbox thread-activity rows and their tombstones. Keyed by relay URL,
  // so without this a relay reinstalled at the same address inherits the
  // previous instance's rows and shows them as live items whose events no
  // longer exist anywhere.
  removeThreadActivityForRelay(relayUrl);
  clearSavedCommunitySnapshot(communityId);
  removeCommunityDestination(communityId);
}

/** Row/file counts purged from the native relay-URL-scoped stores. */
export type NativeRelayScopePurgeReport = {
  /** Rows deleted across the `archive.db` tables for this identity + relay. */
  archiveRows: number;
  /** Rows deleted from the scoped retention database. */
  retentionRows: number;
};

/**
 * Purge the Rust-side stores keyed by relay URL: the per-`(owner, relay)`
 * retention database and the `(identity_pubkey, relay_url, …)` rows in
 * `archive.db`.
 *
 * Rejects if the backend call fails (including when the app is not running
 * under Tauri). Callers decide whether a partial purge is fatal.
 */
export function purgeNativeRelayScopedStores(
  relayUrl: string,
): Promise<NativeRelayScopePurgeReport> {
  return invokeTauri<NativeRelayScopePurgeReport>(
    "purge_relay_scoped_local_stores",
    { relayUrl },
  );
}
