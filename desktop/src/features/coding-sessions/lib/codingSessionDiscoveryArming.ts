import type { ConnectionState } from "@/shared/api/relayClientShared";

/** The relay-connection surface a session read needs to re-arm itself. */
export type CodingSessionDiscoveryArmingClient = {
  subscribeToReconnects?(listener: () => void): () => void;
  subscribeToConnectionState?(
    listener: (state: ConnectionState) => void,
  ): () => void;
};

/**
 * Re-run a session read every time the relay reaches `connected`.
 *
 * `subscribeToReconnects` deliberately says nothing about the *first* connect:
 * `emitReconnectIfNeeded` gates on `hasConnectedOnce`, so a read that lost its
 * race with a cold start — the socket was not up yet, or the first attempt
 * failed and a scheduled one succeeded — was left with no trigger at all. The
 * bounded retry budget is spent within seconds, the reconnect event never
 * comes, and the projection stays empty for the rest of the app run unless the
 * effect's channel scope happens to change. On the sessions shelf that is not
 * a blank surface but a *wrong* one: the rows still paint from the persisted
 * shelf cache, while the closure and genesis reads that decide "Open" versus
 * "Recent" are the ones missing, so closed sessions read as open work.
 *
 * Connection state is the honest trigger — it fires on every transition into
 * `connected`, the first one included. `subscribe` also replays the current
 * state immediately, so arming after the caller's own first attempt costs one
 * extra `rearm()`; every caller coalesces it (the discovery controller drops a
 * request while one is in flight or scheduled, and `establishLive` drops one
 * while a subscribe is pending).
 */
export function armCodingSessionDiscoveryOnConnect(
  client: CodingSessionDiscoveryArmingClient,
  rearm: () => void,
): () => void {
  const unsubscribeReconnect = client.subscribeToReconnects?.(rearm);
  const unsubscribeConnectionState = client.subscribeToConnectionState?.(
    (state) => {
      if (state === "connected") rearm();
    },
  );
  return () => {
    unsubscribeReconnect?.();
    unsubscribeConnectionState?.();
  };
}
