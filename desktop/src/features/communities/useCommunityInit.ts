import { useEffect, useRef, useState } from "react";
import { type QueryClient, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { isMacPlatform } from "@/shared/lib/platform";

import { relayClient } from "@/shared/api/relayClient";
import { resetRateLimitGate } from "@/shared/api/relayRateLimitGate";
import {
  applyCommunity,
  autoConnectDefaultRelayEnabled,
  getDefaultRelayUrl,
} from "@/shared/api/tauri";
import { getIdentity } from "@/shared/api/tauriIdentity";
import { clearTrayAgentActivity } from "@/shared/api/trayMenu";
import { getOverrides } from "@/shared/features";
import { resetMediaCaches } from "@/shared/lib/mediaUrl";
import { resetLinkPreviewMetadataCache } from "@/shared/lib/useResolvedLinkPreviews";
import { clearSearchHitEventCache } from "@/app/navigation/searchHitEventCache";
import { resetNavigationDeepLinkDrain } from "@/shared/deep-link";
import {
  clearAllDrafts,
  initDraftStore,
} from "@/features/messages/lib/useDrafts";
import { resetCodingSessionLaneVisibility } from "@/features/messages/lib/codingSessionLaneVisibility";
import { resetNewCodingSessionDialog } from "@/features/coding-sessions/newCodingSessionDialogStore";
import { resetRenderScopedReactionHydration } from "@/features/messages/lib/renderScopedReactions";
import { resetBackgroundMediaUploads } from "@/features/messages/lib/backgroundMediaUploadStore";
import { resetLinkPreviewPreparations } from "@/features/messages/lib/linkPreviewPreparationStore";
import {
  resetActiveAgentTurnsStore,
  saveActiveAgentTurnsForCommunity,
  restoreActiveAgentTurnsForCommunity,
} from "@/features/agents/activeAgentTurnsStore";
import { resetAgentWorkingSignal } from "@/features/agents/agentWorkingSignal";
import { resetAgentObserverStore } from "@/features/agents/observerRelayStore";
import { resetAvatarPresentations } from "@/features/profile/avatarPresentationStore";
import { resetAvatarProfileSync } from "@/features/profile/avatarProfileSync";
import { resetSidebarRelayConnectionCardState } from "@/features/sidebar/ui/useSidebarRelayConnectionCard";
import { clearMarkdownNodeCache } from "@/shared/ui/markdown/nodeCache";
import { resetRedactionDictionary } from "@/features/coding-sessions/useRedactionDictionary";
import { resetCodingSessionPopoutBootstrapCache } from "@/features/coding-sessions/lib/codingSessionBootstrap";
import { resetPendingCodingSessionLifecycle } from "@/features/coding-sessions/lib/codingSessionPendingLifecycle";
import { resetPendingCodingSessionTurns } from "@/features/coding-sessions/lib/codingSessionPendingTurns";
import { resetCodingSessionIngressStores } from "@/features/coding-sessions/lib/codingSessionIngressStoreCache";
import { resetProjectPulseState } from "@/features/project-pulse";
import { resetVideoPlayerState } from "@/shared/ui/videoPlayerState";
import { relaySelfQueryKey } from "@/features/moderation/lib/relaySelf";

import {
  initFirstCommunity,
  shouldAutoConnectDefaultRelay,
} from "./communityStorage";
import { reconcileRelayIdentity } from "./relayIdentityGuard";
import { useCommunities } from "./useCommunities";
import type { Community } from "./types";

/**
 * Tear down all community-scoped module singletons so the new
 * community starts with a clean slate. Hook-managed singletons
 * (e.g. ChannelMuteSyncManager, ChannelSectionSyncManager) are
 * destroyed via effect cleanup and do not need entries here.
 * See AGENTS.md "Community Switching" for the full contract.
 */
async function resetCommunityState({
  queryClient,
  resetAvatarState,
}: {
  queryClient: QueryClient;
  resetAvatarState: boolean;
}): Promise<void> {
  relayClient.disconnect();
  await resetNavigationDeepLinkDrain();
  resetRateLimitGate();
  clearAllDrafts();
  resetAgentObserverStore();
  resetActiveAgentTurnsStore();
  resetAgentWorkingSignal();
  if (isTauri() && isMacPlatform()) {
    void clearTrayAgentActivity();
  }
  if (resetAvatarState) {
    resetAvatarProfileSync();
    resetAvatarPresentations();
  }
  resetSidebarRelayConnectionCardState();
  resetMediaCaches();
  resetLinkPreviewMetadataCache();
  resetVideoPlayerState();
  resetRenderScopedReactionHydration();
  resetBackgroundMediaUploads();
  resetLinkPreviewPreparations();
  clearSearchHitEventCache();
  clearMarkdownNodeCache();
  resetCodingSessionPopoutBootstrapCache();
  // A provider identity is minted per relay, so every cached
  // (signer, session) redaction scope is meaningless after a switch.
  resetRedactionDictionary();
  // Pending create/stop overlays are relay-scoped commands; carrying them
  // across a community switch would synthesize rows for sessions the new
  // relay never saw.
  resetPendingCodingSessionLifecycle();
  // Same reasoning one channel down: a turn published to the old relay can only
  // ever be echoed by the old relay, so carrying its optimistic row across
  // would hang an unanswerable "waiting for the provider" over a new community.
  resetPendingCodingSessionTurns();
  // Ingress stores are kept warm across session switches so a return paints
  // instantly. They are scoped to a relay's channel ids and verified against
  // that relay's authority, so a community switch must drop every one of them
  // rather than let the new relay's scopes collide with the old relay's facts.
  resetCodingSessionIngressStores();
  // Which coding-session conversation lanes are openable is per-relay: keeping
  // the old community's refs would hide chat in the new one (channel ids are
  // UUIDs, but a hidden message with no lane to render in is the one outcome
  // the rule must never produce).
  resetCodingSessionLaneVisibility();
  // The create dialog holds a channel id — or a project id — belonging to the
  // relay it was opened against. Left open across a switch it would point at
  // a channel the new community has never heard of.
  resetNewCodingSessionDialog();
  // The relay's NIP-11 `self` pubkey is cached under a global React Query key
  // (call sites read it with an exact-key getQueryData, so the key cannot be
  // relay-scoped). The QueryClient outlives the community remount, so without
  // this the next community would classify moderation DMs and relay-signed
  // state against the PREVIOUS relay's identity until the entry went stale.
  queryClient.removeQueries({ queryKey: relaySelfQueryKey });
  // Folded Pulse digests are keyed by project coordinate, and a coordinate
  // names no relay: carrying them across a switch would paint one community's
  // claims and observed commits under another community's project.
  resetProjectPulseState();
}

type CommunityInitResult =
  | { isReady: true; needsSetup: false; appliedKey: string }
  | {
      isReady: false;
      needsSetup: true;
      defaultRelayUrl: string;
    }
  | { isReady: false; needsSetup: false; appliedKey: string | null }
  | { isReady: false; needsSetup: false; appliedKey: null; error: string };

/**
 * Applies the active community config to the Tauri backend and resets
 * all community-scoped module singletons when the community changes.
 *
 * Returns a discriminated union — only render the app after the
 * community is applied. When `needsSetup` is true, the caller
 * should show a first-run welcome screen.
 */
export function useCommunityInit(
  activeCommunity: Community | null,
  communityKey: string,
  isSharedIdentity: boolean,
  suppressAutoConnect = false,
): CommunityInitResult {
  const [result, setResult] = useState<CommunityInitResult>({
    isReady: false,
    needsSetup: false,
    appliedKey: null,
  });

  // Track whether this is the initial mount or a community switch.
  // On the initial mount we skip resetting singletons (they're fresh).
  const hasInitializedRef = useRef(false);

  // Track the previously-applied community ID so we can save its turn state
  // before resetting when the user switches to a different community.
  const prevCommunityIdRef = useRef<string | null>(null);
  // Deferred avatar work owns the relay captured when it was queued. A
  // same-relay reconnect during onboarding must not cancel that work, while an
  // actual relay boundary must clear both the queue and its presentation probe.
  const appliedRelayUrlRef = useRef<string | null>(null);

  // Read through refs so the init effect keeps its narrow dependency list —
  // `updateCommunity` is re-created whenever the active community id changes,
  // and the QueryClient identity is stable but not guaranteed to be.
  const queryClient = useQueryClient();
  const queryClientRef = useRef(queryClient);
  queryClientRef.current = queryClient;
  const { updateCommunity } = useCommunities();
  const updateCommunityRef = useRef(updateCommunity);
  updateCommunityRef.current = updateCommunity;

  // biome-ignore lint/correctness/useExhaustiveDependencies: we intentionally depend on specific properties (id/relayUrl/token/reposDir) — depending on the whole object would trigger resets on name-only changes
  useEffect(() => {
    let cancelled = false;

    async function init() {
      if (!activeCommunity) {
        if (hasInitializedRef.current) {
          if (prevCommunityIdRef.current) {
            saveActiveAgentTurnsForCommunity(prevCommunityIdRef.current);
            prevCommunityIdRef.current = null;
          }
          try {
            await resetCommunityState({
              queryClient: queryClientRef.current,
              resetAvatarState: true,
            });
          } catch (error) {
            console.error("Failed to reset community state:", error);
            if (!cancelled) {
              setResult({
                isReady: false,
                needsSetup: false,
                appliedKey: null,
                error:
                  error instanceof Error
                    ? `Could not safely leave community: ${error.message}`
                    : "Could not safely leave community",
              });
            }
            return;
          }
          appliedRelayUrlRef.current = null;
          hasInitializedRef.current = false;
        }
        try {
          const defaultRelayUrl = await getDefaultRelayUrl();
          const autoConnectDefaultRelay =
            await autoConnectDefaultRelayEnabled();

          // Internal builds explicitly opt into treating their reviewed default
          // relay as the first community. Public builds retain community
          // selection even when BUZZ_RELAY_URL is overridden at runtime.
          if (
            !suppressAutoConnect &&
            (isSharedIdentity ||
              (autoConnectDefaultRelay &&
                shouldAutoConnectDefaultRelay(defaultRelayUrl)))
          ) {
            const identity = await getIdentity();
            if (cancelled) return;
            const community = initFirstCommunity(
              defaultRelayUrl,
              identity.pubkey,
            );
            if (community && !cancelled) {
              window.location.reload();
              return;
            }
            if (!cancelled) {
              setResult({
                isReady: false,
                needsSetup: true,
                defaultRelayUrl,
              });
            }
            return;
          }

          if (!cancelled) {
            setResult({
              isReady: false,
              needsSetup: true,
              defaultRelayUrl,
            });
          }
        } catch {
          if (!cancelled) {
            setResult({
              isReady: false,
              needsSetup: true,
              defaultRelayUrl: "ws://localhost:3000",
            });
          }
        }
        return;
      }

      // Mark this community config as pending while it is applied to the
      // backend. App.tsx also checks appliedKey against the active communityKey,
      // which prevents rendering community-scoped UI for a new community until
      // that exact config has finished applying.
      setResult({
        isReady: false,
        needsSetup: false,
        appliedKey: communityKey,
      });

      // On community switch (not initial mount), reset module singletons
      // so the new tree starts with a clean slate.
      if (hasInitializedRef.current) {
        // Save the outgoing community's turn state before wiping the store so
        // timers survive a round-trip (A → B → A keeps A's elapsed time).
        if (prevCommunityIdRef.current) {
          saveActiveAgentTurnsForCommunity(prevCommunityIdRef.current);
          // Null out immediately so a rapid community switch (A→B→C before
          // B's applyCommunity resolves) doesn't re-save the now-empty
          // store under the outgoing community ID and delete its snapshot.
          prevCommunityIdRef.current = null;
        }
        try {
          await resetCommunityState({
            queryClient: queryClientRef.current,
            resetAvatarState:
              appliedRelayUrlRef.current !== activeCommunity.relayUrl,
          });
        } catch (error) {
          console.error("Failed to reset community state:", error);
          if (!cancelled) {
            setResult({
              isReady: false,
              needsSetup: false,
              appliedKey: null,
              error:
                error instanceof Error
                  ? `Could not safely switch communities: ${error.message}`
                  : "Could not safely switch communities",
            });
          }
          return;
        }
      }
      hasInitializedRef.current = true;
      appliedRelayUrlRef.current = activeCommunity.relayUrl;

      // Apply community config to the Tauri backend.
      //
      // Note: we deliberately do NOT pass an nsec here. The persisted
      // `identity.key` file (resolved at startup by `resolve_persisted_identity`,
      // and updated atomically by `import_identity`) is the single source of
      // truth for the active key. Older builds stored the nsec in localStorage
      // and re-applied it on every reload, which silently overwrote any
      // imported key. `loadCommunities()` strips lingering `nsec` fields from
      // legacy entries; this site refuses to apply one even if present.
      try {
        await applyCommunity(
          activeCommunity.relayUrl,
          undefined,
          activeCommunity.token,
          activeCommunity.reposDir,
          getOverrides().agentManagedProfiles === true,
        );
      } catch (error) {
        // A bad `repos_dir` no longer reaches here — `apply_workspace` treats
        // it as non-fatal (relay/keys apply, bad value not persisted, REPOS
        // falls back to a real dir, a `repos-dir-error` toast surfaces it) and
        // returns Ok, so the app boots into a working state where the user can
        // fix the value in community settings. This catch now only fires on a
        // genuine relay/key apply failure (e.g. an invalid nsec or a poisoned
        // lock). For those, marking the community ready would render
        // community-scoped UI against a backend that never applied — park on
        // the loading gate (isReady:false, no appliedKey) instead.
        console.error("Failed to apply community to backend:", error);
        if (!cancelled) {
          setResult({
            isReady: false,
            needsSetup: false,
            appliedKey: null,
            error:
              error instanceof Error
                ? error.message
                : "Failed to apply community configuration",
          });
        }
        return;
      }

      if (cancelled) return;

      // The backend now points at this community's relay, so the NIP-11 probe
      // reads the right document. Reconcile the relay's *identity* here —
      // before any community-scoped UI renders — because every persisted cache
      // (channel/message snapshots, the retention DB, archive.db) is keyed by
      // relay URL alone, and a relay reinstalled at the same URL with a fresh
      // keypair is a different community. Wiping after render would leave
      // components already hydrated from the stale snapshot.
      //
      // Bounded and fail-closed: an unreachable or silent relay yields "no
      // action", so a community whose relay is down still boots on its caches.
      try {
        await reconcileRelayIdentity({
          community: activeCommunity,
          adoptRelayPubkey: (relayPubkey) => {
            updateCommunityRef.current(activeCommunity.id, { relayPubkey });
          },
        });
      } catch (error) {
        // reconcileRelayIdentity is contracted never to reject; a throw is a
        // bug in it, and never a reason to block the community from booting.
        console.error(
          "[useCommunityInit] relay identity reconcile failed:",
          error,
        );
      }

      if (!cancelled) {
        // Refresh relay-derived media state only after the backend has installed
        // this community's relay override. On cold launch, mediaUrl.ts may have
        // eagerly cached the default relay origin before applyCommunity ran;
        // leaving that stale value makes authenticated relay media look external
        // and bypass the localhost proxy.
        resetMediaCaches();

        try {
          const identity = await getIdentity();
          if (cancelled) return;
          initDraftStore(identity.pubkey, activeCommunity.relayUrl);
        } catch (err) {
          if (cancelled) return;
          console.error(
            "[useCommunityInit] getIdentity failed, draft store uninitialized:",
            err,
          );
        }
        // Restore any turn state saved for this community (a prior A→B round-
        // trip). This runs after applyCommunity succeeds and before the app
        // renders so components see the restored timers on first render.
        restoreActiveAgentTurnsForCommunity(activeCommunity.id);
        // Prime the ref so the NEXT switch saves this community's state.
        prevCommunityIdRef.current = activeCommunity.id;
        setResult({
          isReady: true,
          needsSetup: false,
          appliedKey: communityKey,
        });
      }
    }

    void init();

    return () => {
      cancelled = true;
    };
  }, [
    activeCommunity?.id,
    activeCommunity?.relayUrl,
    activeCommunity?.token,
    activeCommunity?.reposDir,
    isSharedIdentity,
    suppressAutoConnect,
    communityKey,
  ]);

  return result;
}
