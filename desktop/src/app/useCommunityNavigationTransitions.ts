import { useRouter } from "@tanstack/react-router";
import * as React from "react";

import type { deriveShellRoute } from "@/app/AppShell.helpers";
import type { useAppNavigation } from "@/app/navigation/useAppNavigation";
import {
  replaceCommunityDestinationRoute,
  runCommunityViewTransition,
} from "@/app/communityViewTransition";
import { readChannelSnapshot } from "@/features/channels/channelSnapshot";
import {
  isChannelKnownInSet,
  loadCommunityDestination,
  markPendingCommunityRestore,
  saveCommunityDestination,
} from "@/features/communities/communityNavigationStorage";
import { markCommunityDiscoveryAfterLeave } from "@/features/communities/communityStorage";
import type { useCommunities } from "@/features/communities/useCommunities";
import { leaveCommunity } from "@/features/communities/leaveCommunity";
import { useIdentityQuery } from "@/shared/api/hooks";

type Communities = ReturnType<typeof useCommunities>;
type ShellRoute = ReturnType<typeof deriveShellRoute>;
type GoHome = ReturnType<typeof useAppNavigation>["goHome"];

export function useCommunityNavigationTransitions({
  communities,
  goHome,
  selectedChannelId,
  selectedView,
}: {
  communities: Communities;
  goHome: GoHome;
  selectedChannelId: ShellRoute["selectedChannelId"];
  selectedView: ShellRoute["selectedView"];
}) {
  const router = useRouter();
  const ownerPubkey = useIdentityQuery().data?.pubkey ?? null;

  // A remembered channel may be written into the route only once it is known
  // to exist in the destination community's channel set. The community's
  // React-Query client remounts fresh on every switch (dataUpdatedAt resets
  // to 0), so there is no live query to consult yet at this point in the
  // transition — the persisted per-relay snapshot (populated after a past
  // live fetch) is the only "known to exist" signal available synchronously.
  // A cold community (no snapshot yet) is treated as unvalidated: the switch
  // routes to the neutral landing and the AppShell repair effect promotes
  // the remembered channel once its own live read succeeds, sharing the same
  // `isChannelKnownInSet` predicate so the two paths cannot disagree.
  const destinationChannelIsValidated = React.useCallback(
    (relayUrl: string | undefined, channelId: string) => {
      if (!relayUrl || !ownerPubkey) return false;
      const snapshot = readChannelSnapshot(relayUrl, ownerPubkey);
      return isChannelKnownInSet(snapshot?.channels, channelId);
    },
    [ownerPubkey],
  );

  const saveActiveDestination = React.useCallback(() => {
    const activeCommunityId = communities.activeCommunity?.id;
    if (!activeCommunityId) return;
    saveCommunityDestination(
      activeCommunityId,
      selectedView === "channel" && selectedChannelId
        ? { kind: "channel", channelId: selectedChannelId }
        : { kind: "home" },
    );
  }, [communities.activeCommunity?.id, selectedChannelId, selectedView]);

  // Home is a teardown barrier: the outgoing channel must unmount before the
  // relay changes, or its read effect can advance markers on the wrong relay.
  const switchCommunity = React.useCallback(
    async (id: string) => {
      const activeCommunityId = communities.activeCommunity?.id;
      if (id === activeCommunityId) return;
      if (!activeCommunityId) {
        communities.switchCommunity(id);
        return;
      }

      const targetRelayUrl = communities.communities.find(
        (community) => community.id === id,
      )?.relayUrl;

      await runCommunityViewTransition(async () => {
        saveActiveDestination();
        await goHome({ replace: true });
        markPendingCommunityRestore(id);
        const destination = loadCommunityDestination(id);
        if (
          destination?.kind === "channel" &&
          destinationChannelIsValidated(targetRelayUrl, destination.channelId)
        ) {
          replaceCommunityDestinationRoute(
            destination.channelId,
            router.history,
          );
        }
        communities.switchCommunity(id);
      });
    },
    [
      communities,
      destinationChannelIsValidated,
      goHome,
      router.history,
      saveActiveDestination,
    ],
  );

  const removeCommunity = React.useCallback(
    async (id: string) => {
      const target = communities.communities.find(
        (community) => community.id === id,
      );
      if (!target) return;

      const fallback = communities.communities.find(
        (community) => community.id !== id,
      );

      // Do not touch local state until this relay has explicitly accepted the
      // signed NIP-43 leave request. Rejections and timeouts bubble back to the
      // dialog so the person can retry without losing their community config.
      const leaveResult = await leaveCommunity(
        target.relayUrl,
        communities.activeCommunity?.relayUrl,
      );

      if (id !== communities.activeCommunity?.id) {
        communities.removeCommunity(id);
        return leaveResult;
      }

      if (!fallback) {
        if (!markCommunityDiscoveryAfterLeave()) {
          throw new Error(
            "Membership was removed, but community discovery state could not be saved. Restart Beekeeper and try again.",
          );
        }
        await goHome({ replace: true });
        communities.removeCommunity(id);
        return leaveResult;
      }

      await runCommunityViewTransition(async () => {
        saveActiveDestination();
        await goHome({ replace: true });
        markPendingCommunityRestore(fallback.id);
        const destination = loadCommunityDestination(fallback.id);
        if (
          destination?.kind === "channel" &&
          destinationChannelIsValidated(
            fallback.relayUrl,
            destination.channelId,
          )
        ) {
          replaceCommunityDestinationRoute(
            destination.channelId,
            router.history,
          );
        }
        communities.removeCommunity(id);
      });
      return leaveResult;
    },
    [
      communities,
      destinationChannelIsValidated,
      goHome,
      router.history,
      saveActiveDestination,
    ],
  );

  return { removeCommunity, switchCommunity };
}
