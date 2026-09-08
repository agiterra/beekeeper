import * as React from "react";
import { listen } from "@tauri-apps/api/event";

import {
  type BroadcastCadence,
  SHELL_BROADCAST_CADENCE_EVENT,
  shellBroadcastCadence,
} from "@/shared/api/tauriShell";

/**
 * The owner-side frame cadence of one shared session: the pull answer from
 * `shell_broadcast_cadence`, kept current by the `shell-broadcast-cadence`
 * Tauri event the broadcaster emits whenever it backs off or recovers.
 * `null` until the first answer arrives.
 */
export function useShellBroadcastCadence(
  sessionId: string,
): BroadcastCadence | null {
  const [cadence, setCadence] = React.useState<BroadcastCadence | null>(null);
  React.useEffect(() => {
    let disposed = false;
    let unlisten: (() => void) | null = null;
    shellBroadcastCadence(sessionId)
      .then((initial) => {
        if (!disposed) setCadence(initial);
      })
      .catch(() => {
        // Not a shared session yet; the event below still reports later.
      });
    void listen<BroadcastCadence>(SHELL_BROADCAST_CADENCE_EVENT, (event) => {
      if (event.payload.sessionId === sessionId) setCadence(event.payload);
    })
      .then((stop) => {
        if (disposed) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      disposed = true;
      unlisten?.();
      setCadence(null);
    };
  }, [sessionId]);
  return cadence;
}
