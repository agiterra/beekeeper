/**
 * The read side of the browser's coding-session observer.
 *
 * Surfaces should need nothing from this feature except the hook and the
 * snapshot selectors; the engine and the filter helpers are exported for the
 * tests and for any surface that has to read a channel outside React.
 */
export {
  CodingSessionObserverEngine,
  type CancelSchedule,
  codingSessionLiveFilters,
  LEASE_REFRESH_MS,
  type ObserverEngineOptions,
  type SubscribeEventsFn,
} from "./observerEngine.ts";
export {
  buildChannelSessionSnapshot,
  type ChannelSessionObserverSnapshot,
  type ChannelSessionSnapshotOptions,
  type CodingSessionObserverConnection,
  type CodingSessionObserverCounts,
  createEmptyChannelSessionSnapshot,
  selectChannelSession,
  selectSessionTranscriptBlocks,
} from "./observerSnapshot.ts";
export {
  type ChannelSessionObserverResult,
  type UseChannelSessionObserverOptions,
  useChannelSessionObserver,
} from "./useChannelSessionObserver.ts";
