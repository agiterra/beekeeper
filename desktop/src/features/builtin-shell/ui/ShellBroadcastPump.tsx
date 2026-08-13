import * as React from "react";
import { listen } from "@tauri-apps/api/event";

import { relayClient } from "@/shared/api/relayClient";
import {
  SHELL_BROADCAST_PUBLISH_EVENT,
  shellBroadcastWatch,
} from "@/shared/api/tauriShell";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SHELL_WATCH } from "@/shared/constants/kinds";

/**
 * The NIP-ST owner-side pump, mounted app-wide (renders nothing).
 *
 * Bridges the Rust broadcaster and the relay WebSocket in both directions:
 * signed frame events arrive on the `shell-broadcast-publish` Tauri event and
 * are published to the relay; kind:24310 watch events addressed to this
 * identity are validated and forwarded to `shell_broadcast_watch`, whose
 * returned attach-bundle frames are published back out. Dropped frames
 * self-heal — an observer that sees a gap asks for a resync.
 */
export function ShellBroadcastPump() {
  const identity = useIdentityQuery();
  const myPubkey = identity.data?.pubkey ?? null;

  React.useEffect(() => {
    if (!myPubkey) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let unsubscribe: (() => void) | null = null;

    const publish = (json: string) => {
      try {
        const event = JSON.parse(json) as RelayEvent;
        void relayClient
          .publishEvent(
            event,
            "Timed out while streaming the terminal.",
            "Failed to stream the terminal.",
          )
          .catch(() => {
            // Dropped frames self-heal via observer resync.
          });
      } catch {
        // Malformed payloads are a programming error; never throw here.
      }
    };

    void (async () => {
      const stop = await listen<string>(SHELL_BROADCAST_PUBLISH_EVENT, (e) => {
        publish(e.payload);
      });
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;

      const handle = await relayClient.subscribeLive(
        {
          kinds: [KIND_SHELL_WATCH],
          "#p": [myPubkey],
          since: Math.floor(Date.now() / 1_000) - 60,
          limit: 500,
        },
        (event) => {
          if (disposed) return;
          const sessionId = event.tags.find((t) => t[0] === "d")?.[1];
          if (!sessionId || typeof sessionId !== "string") return;
          let action: string | undefined;
          try {
            action = (JSON.parse(event.content) as { action?: string }).action;
          } catch {
            return;
          }
          if (action !== "watch" && action !== "stop" && action !== "resync") {
            return;
          }
          void shellBroadcastWatch(sessionId, event.pubkey, action)
            .then((bundle) => {
              for (const frame of bundle) {
                publish(frame);
              }
            })
            .catch(() => {
              // Unknown/unshared session — nothing to stream.
            });
        },
      );
      if (disposed) {
        handle?.();
        return;
      }
      unsubscribe = handle ?? null;
    })();

    return () => {
      disposed = true;
      unlisten?.();
      unsubscribe?.();
    };
  }, [myPubkey]);

  return null;
}
