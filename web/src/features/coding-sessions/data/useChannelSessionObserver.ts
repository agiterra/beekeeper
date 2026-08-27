/**
 * The hook a coding-session surface reads a channel through.
 *
 * The engine owns the relay reads and the folding; this binds it to React with
 * `useSyncExternalStore` (so a live event repaints without a query round-trip)
 * and to TanStack Query (so the initial history read has one shared, retrying
 * owner across every component that asks for the same channel).
 */
import { useQuery } from "@tanstack/react-query";
import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useSyncExternalStore,
} from "react";
import { relayWsUrl } from "@/shared/lib/relay-url";
import {
  type CodingSessionObserverConnection as ObserverViewConnection,
  type CodingSessionObserverView,
  codingSessionObserverViewFromSnapshot,
} from "../ui/observer-contract.ts";
import { bindCodingSessionObserverSource } from "../ui/useCodingSessionObserver.ts";
import { CodingSessionObserverEngine } from "./observerEngine.ts";
import {
  type ChannelSessionObserverSnapshot,
  createEmptyChannelSessionSnapshot,
} from "./observerSnapshot.ts";

/** Rendered before a channel is known, and on the server. */
const IDLE_SNAPSHOT = createEmptyChannelSessionSnapshot();

export type UseChannelSessionObserverOptions = {
  enabled?: boolean;
  /** Overrides the community relay; the session E2E harness uses this. */
  wsUrl?: string;
};

export type ChannelSessionObserverResult = {
  snapshot: ChannelSessionObserverSnapshot;
  /** Re-read history and leases. Resolves when the history page completes. */
  refresh: () => Promise<void>;
};

/**
 * Observe every coding session in one channel, live.
 *
 * Read-only by construction: the engine has no publish path, and every fact
 * behind the returned snapshot had its signature verified on this device.
 */
export function useChannelSessionObserver(
  channelId: string | null | undefined,
  options: UseChannelSessionObserverOptions = {},
): ChannelSessionObserverResult {
  const enabled =
    (options.enabled ?? true) &&
    typeof channelId === "string" &&
    channelId.length > 0;
  const wsUrl = options.wsUrl ?? (enabled ? relayWsUrl() : "");
  const engineKey = `${wsUrl}|${channelId ?? ""}`;
  const engineRef = useRef<{
    key: string;
    engine: CodingSessionObserverEngine;
  } | null>(null);

  const getEngine = useCallback((): CodingSessionObserverEngine | null => {
    if (!enabled || typeof channelId !== "string") return null;
    if (engineRef.current?.key !== engineKey) {
      // The previous engine is stopped by its own effect cleanup, not here:
      // this runs during render (`useSyncExternalStore` reads before effects)
      // and stopping publishes a snapshot, which must never happen mid-render.
      engineRef.current = {
        key: engineKey,
        engine: new CodingSessionObserverEngine({ wsUrl, channelId }),
      };
    }
    return engineRef.current.engine;
  }, [enabled, engineKey, wsUrl, channelId]);

  const subscribe = useCallback(
    (listener: () => void) => getEngine()?.subscribeSnapshot(listener) ?? noop,
    [getEngine],
  );
  const getSnapshot = useCallback(
    () => getEngine()?.getSnapshot() ?? IDLE_SNAPSHOT,
    [getEngine],
  );
  const snapshot = useSyncExternalStore(
    subscribe,
    getSnapshot,
    () => IDLE_SNAPSHOT,
  );

  useEffect(() => {
    const engine = getEngine();
    if (!engine) return;
    engine.start();
    return () => engine.stop();
  }, [getEngine]);

  const query = useQuery({
    queryKey: ["coding-sessions", "observer", engineKey],
    enabled,
    // The live subscription keeps the data fresh; a background refetch would
    // only re-read a page the observer already holds.
    staleTime: Number.POSITIVE_INFINITY,
    refetchOnWindowFocus: false,
    queryFn: async () => {
      const engine = getEngine();
      if (!engine) return { readAt: 0 };
      engine.start();
      await engine.whenHistoryRead();
      return { readAt: Date.now() };
    },
  });

  const refetch = query.refetch;
  const refresh = useCallback(async () => {
    const engine = getEngine();
    if (!engine) return;
    const settled = engine.refresh();
    await refetch();
    // A failed re-read is already disclosed as `connection` and `lastError`;
    // rethrowing here would only turn a button press into an unhandled
    // rejection.
    await settled.catch(() => {});
  }, [getEngine, refetch]);

  return { snapshot, refresh };
}

function noop(): void {}

/**
 * How the surfaces are allowed to describe the socket.
 *
 * The engine's `error` means the transport is still retrying with backoff, so
 * the view says "reconnecting"; `closed` means nothing is retrying any more.
 * A `connecting` that follows a completed history read is a reconnect, not a
 * first load, and saying so keeps the skeleton from reappearing.
 */
function viewConnection(
  connection: "idle" | "connecting" | "open" | "error" | "closed",
  historyRead: boolean,
): ObserverViewConnection {
  switch (connection) {
    case "open":
      return "live";
    case "error":
      return "reconnecting";
    case "closed":
      return "closed";
    case "connecting":
      return historyRead ? "reconnecting" : "connecting";
    default:
      return "idle";
  }
}

/**
 * The relay-backed source behind the observer seam.
 *
 * It is the adapter and nothing else: the reads and the fold happen in the
 * engine, the rendering rules live in `observer-contract.ts`, and this only
 * restates one for the other.
 */
function useRelayCodingSessionObserver(
  channelId: string | null,
): CodingSessionObserverView {
  const { snapshot, refresh } = useChannelSessionObserver(channelId);
  const requestRefresh = useCallback(() => {
    // The button is fire-and-forget; a failed re-read is already disclosed
    // through `connection` and `lastError`.
    void refresh();
  }, [refresh]);

  return useMemo(
    () =>
      codingSessionObserverViewFromSnapshot(
        {
          umbrellas: snapshot.sessions,
          reachabilityByGenerationId: snapshot.reachabilityByGenerationId,
          signaturesVerified: snapshot.signaturesVerified,
          historyTruncated: snapshot.truncatedAt1000,
          malformedCount: snapshot.counts.malformed,
          invalidSignatureCount: snapshot.counts.invalidSignature,
          conflictCount: snapshot.counts.conflicts,
        },
        {
          channelId,
          connection: viewConnection(snapshot.connection, snapshot.historyRead),
          lastError: snapshot.lastError,
          historyLoaded: snapshot.historyRead,
          refresh: requestRefresh,
        },
      ),
    [snapshot, channelId, requestRefresh],
  );
}

// The seam is bound at module load: importing this module anywhere in the app
// (see `main.tsx`) is what makes the screens read the relay instead of
// rendering the honest "reader not wired up" state.
bindCodingSessionObserverSource(useRelayCodingSessionObserver);
