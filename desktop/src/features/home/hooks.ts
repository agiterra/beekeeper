import { useQuery } from "@tanstack/react-query";

import {
  excludeHiddenFeedItems,
  relayEventFromFeedItem,
} from "@/features/home/lib/inbox";
import { isCodingSessionLaneMessageHiddenFromChannel } from "@/features/messages/lib/codingSessionLaneVisibility";
import { getChannelIdFromTags } from "@/features/messages/lib/threading";
import { getHomeFeed } from "@/shared/api/tauri";
import { useRelayConnection } from "@/shared/api/useRelayConnection";
import { useFocusedRefetchInterval } from "@/shared/lib/useDocumentVisible";

/** Keeps focused polling at the established 30-second cadence. */
export const HOME_FEED_REFETCH_INTERVAL_MS = 30_000;
/** Suppresses the expensive focus refetch until the home feed is old. */
export const HOME_FEED_FOCUS_STALE_TIME_MS = 5 * 60_000;

/** Focus-refetch policy for the home feed query; consumed by focusRefetchPolicy.test.mjs. */
export const homeFeedFocusRefetchPolicy = {
  staleTime: HOME_FEED_FOCUS_STALE_TIME_MS,
  refetchOnWindowFocus: false,
} as const;

export function useHomeFeedQuery() {
  const connectionState = useRelayConnection();
  const connected = connectionState === "connected";
  const refetchInterval = useFocusedRefetchInterval(
    connected ? HOME_FEED_REFETCH_INTERVAL_MS : false,
  );

  return useQuery({
    queryKey: ["home-feed"],
    queryFn: async () => {
      const response = await getHomeFeed({
        limit: 50,
        types: "mentions,needs_action,activity,agent_activity",
      });
      // A kind:9 carrying a `cs-session` tag is a coding-session lane message
      // when this client can open that lane — it renders in the session's
      // umbrella, not the channel, so it must not also arrive as a Home
      // mention. The relay cannot make that call, so it is applied here. The
      // rule fails open: an unresolved or forged ref stays ordinary chat.
      return excludeHiddenFeedItems(response, (item) =>
        isCodingSessionLaneMessageHiddenFromChannel(
          item.channelId ?? getChannelIdFromTags(item.tags),
          relayEventFromFeedItem(item),
        ),
      );
    },
    gcTime: 5 * 60 * 1_000,
    // Pause background polling on degraded/stalled/disconnected connections.
    // The relay can't serve the request anyway, and the spurious failures
    // consume quota that the recovery path needs.
    refetchInterval,
    ...homeFeedFocusRefetchPolicy,
  });
}
