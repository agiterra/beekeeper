import * as React from "react";

import { relayClient } from "@/shared/api/relayClient";
import { isRateLimited } from "@/shared/api/relayRateLimitGate";
import { buildShellWatchEvent } from "@/shared/api/tauriShell";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SHELL_FRAME } from "@/shared/constants/kinds";

import { ObserveStream, parseShellFrame } from "./shellObserveProtocol";
import { cadenceTagMs } from "./shellBroadcastCadence";

/** Keepalive cadence; the owner expires a watcher after 45 s (3 beats). */
const KEEPALIVE_MS = 15_000;
/** No frame within this after the first watch → "owner not streaming". */
const HANDSHAKE_TIMEOUT_MS = 10_000;
/** No frame for this while live → "stalled". */
const STALL_MS = 30_000;

export type ObserverStatus = "connecting" | "live" | "stalled" | "ended";

export type ShellObserverTarget = {
  ownerPubkey: string;
  sessionId: string;
  projectRef: string;
};

/**
 * Observe one shared terminal read-only: subscribes to the owner's frames,
 * heartbeats a kind:24310 watch, and feeds parsed frames to the caller's
 * terminal via `onAction` callbacks. There is deliberately no input path
 * anywhere in this hook.
 */
export function useShellObserver(
  target: ShellObserverTarget | null,
  callbacks: {
    onWrite: (bytes: Uint8Array) => void;
    onResize: (rows: number, cols: number) => void;
  },
) {
  const [status, setStatus] = React.useState<ObserverStatus>("connecting");
  /** The owner's current frame spacing, from the last frame's `cadence` tag. */
  const [cadenceMs, setCadenceMs] = React.useState<number | null>(null);
  const callbacksRef = React.useRef(callbacks);
  callbacksRef.current = callbacks;

  React.useEffect(() => {
    if (!target) return;
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    const stream = new ObserveStream();
    let lastFrameAt = 0;
    setStatus("connecting");
    setCadenceMs(null);

    const publishWatch = async (action: "watch" | "stop" | "resync") => {
      try {
        const json = await buildShellWatchEvent({
          ownerPubkey: target.ownerPubkey,
          sessionId: target.sessionId,
          projectRef: target.projectRef,
          action,
        });
        await relayClient.publishEvent(
          JSON.parse(json) as RelayEvent,
          "Timed out while contacting the terminal's owner.",
          "Failed to contact the terminal's owner.",
        );
      } catch (error) {
        if (action !== "stop") {
          console.warn("shell-observe: watch publish failed", error);
        }
      }
    };

    const onEvent = (event: RelayEvent) => {
      if (disposed) return;
      const frame = parseShellFrame(event, {
        ownerPubkey: target.ownerPubkey,
        sessionId: target.sessionId,
      });
      if (!frame) return;
      lastFrameAt = Date.now();
      const cadence = cadenceTagMs(event);
      if (cadence !== null) setCadenceMs(cadence);
      const action = stream.apply(frame);
      if (action.resize) {
        callbacksRef.current.onResize(action.resize.rows, action.resize.cols);
      }
      if (action.write) {
        callbacksRef.current.onWrite(action.write);
      }
      if (action.needsResync) {
        void publishWatch("resync");
      }
      if (action.ended) {
        setStatus("ended");
      } else {
        setStatus("live");
      }
    };

    void (async () => {
      const handle = await relayClient.subscribeLive(
        {
          kinds: [KIND_SHELL_FRAME],
          authors: [target.ownerPubkey],
          "#d": [target.sessionId],
          since: Math.floor(Date.now() / 1_000) - 30,
          limit: 500,
        },
        onEvent,
      );
      if (disposed) {
        handle?.();
        return;
      }
      unsubscribe = handle ?? null;
      await publishWatch("watch");
    })();

    const keepalive = window.setInterval(() => {
      // Droppable: while the relay's rate-limit gate is armed a beat would
      // only queue behind it and burst out with everything else; the owner
      // tolerates two missed beats before expiring a watcher.
      if (isRateLimited()) return;
      void publishWatch("watch");
    }, KEEPALIVE_MS);

    const liveness = window.setInterval(() => {
      if (disposed) return;
      setStatus((current) => {
        if (current === "ended") return current;
        if (lastFrameAt === 0) return "connecting";
        return Date.now() - lastFrameAt > STALL_MS ? "stalled" : current;
      });
    }, 1_000);

    const handshake = window.setTimeout(() => {
      if (!disposed && lastFrameAt === 0) {
        setStatus("stalled");
      }
    }, HANDSHAKE_TIMEOUT_MS);

    return () => {
      disposed = true;
      window.clearInterval(keepalive);
      window.clearInterval(liveness);
      window.clearTimeout(handshake);
      unsubscribe?.();
      void publishWatch("stop");
    };
    // Callers memoize `target` on its three fields (see ShellObserveScreen).
  }, [target]);

  const resync = React.useCallback(async () => {
    if (!target) return;
    const json = await buildShellWatchEvent({ ...target, action: "resync" });
    await relayClient.publishEvent(
      JSON.parse(json) as RelayEvent,
      "Timed out while requesting a refresh.",
      "Failed to request a refresh.",
    );
  }, [target]);

  return { status, resync, cadenceMs };
}
