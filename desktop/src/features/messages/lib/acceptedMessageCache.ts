import type { QueryClient } from "@tanstack/react-query";
import type { RelayEvent } from "@/shared/api/types";
import { channelWindowKey, threadRepliesKey } from "./messageQueryKeys";
import { mergeMessages } from "./messageMerge";
import { getThreadReference } from "./threading";
import {
  emptyChannelWindowStore,
  mergeLiveChannelWindowEvent,
  type ChannelWindowStore,
} from "./channelWindowStore";
import { projectChannelWindowMessages } from "./projectChannelWindow";

/** Project a relay-accepted send immediately, without waiting for its live echo. */
export function projectAcceptedMessage(
  queryClient: QueryClient,
  channelId: string,
  optimisticId: string,
  message: RelayEvent,
): void {
  const key = channelWindowKey(channelId);
  const current =
    queryClient.getQueryData<ChannelWindowStore>(key) ??
    emptyChannelWindowStore();
  const withoutPending = {
    ...current,
    liveOverlay: current.liveOverlay.filter(
      (event) => event.id !== optimisticId,
    ),
  };
  const accepted = { ...message, localKey: optimisticId };
  queryClient.setQueryData(
    key,
    mergeLiveChannelWindowEvent(withoutPending, accepted),
  );
  projectChannelWindowMessages(queryClient, channelId);
  const { rootId, parentId } = getThreadReference(message.tags);
  const threadRoot = rootId ?? parentId;
  if (threadRoot) {
    queryClient.setQueryData<RelayEvent[]>(
      threadRepliesKey(channelId, threadRoot),
      (events = []) => mergeMessages(events, accepted),
    );
  }
}
