import * as React from "react";
import { listen } from "@tauri-apps/api/event";

import { relayClient } from "@/shared/api/relayClient";
import {
  isRateLimited,
  rateLimitRemainingMs,
} from "@/shared/api/relayRateLimitGate";
import {
  SHELL_BROADCAST_PUBLISH_EVENT,
  shellBroadcastPublishResult,
  shellBroadcastWatch,
  shellRemoteInput,
} from "@/shared/api/tauriShell";
import { useIdentityQuery } from "@/shared/api/hooks";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SHELL_INPUT, KIND_SHELL_WATCH } from "@/shared/constants/kinds";

/**
 * The NIP-ST owner-side pump, mounted app-wide (renders nothing).
 *
 * Bridges the Rust broadcaster and the relay WebSocket in both directions:
 * signed frame events arrive on the `shell-broadcast-publish` Tauri event and
 * are published to the relay; kind:24310 watch events addressed to this
 * identity are validated and forwarded to `shell_broadcast_watch`, whose
 * returned attach-bundle frames are published back out. Dropped frames
 * self-heal — an observer that sees a gap asks for a resync.
 *
 * Every publish outcome goes back to the broadcaster through
 * `shell_broadcast_publish_result`: a `rate-limited:` refusal doubles its
 * frame interval. Streamed content frames are droppable — while the relay's
 * gate is armed they are reported as refused and not sent, because queueing
 * them behind the gate would burst out exactly what tripped it. Attach
 * bundles are not droppable: a watcher's first picture is worth the wait.
 *
 * Collaborator keystrokes (kind:24312, addressed to this identity) are
 * forwarded to `shell_remote_input` as the FULL raw event JSON — the Rust
 * side verifies the signature, roster, freshness, and rate caps itself and
 * never trusts anything this pump claims.
 */
export function ShellBroadcastPump() {
  const identity = useIdentityQuery();
  const myPubkey = identity.data?.pubkey ?? null;

  React.useEffect(() => {
    if (!myPubkey) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let unsubscribe: (() => void) | null = null;
    let unsubscribeInput: (() => void) | null = null;

    const report = (event: RelayEvent, accepted: boolean, message: string) => {
      const sessionId = event.tags.find((t) => t[0] === "d")?.[1];
      if (typeof sessionId !== "string" || !sessionId) return;
      void shellBroadcastPublishResult(sessionId, accepted, message).catch(
        () => {
          // The broadcaster may already be gone; nothing to back off.
        },
      );
    };

    const publish = (json: string, droppable: boolean) => {
      try {
        const event = JSON.parse(json) as RelayEvent;
        // `end` is never droppable: the broadcaster never throttles it either,
        // and an observer that misses it keeps a dead terminal open.
        const isEnd = event.tags.some(
          (tag) => tag[0] === "t" && tag[1] === "end",
        );
        if (droppable && !isEnd && isRateLimited()) {
          const seconds = Math.max(
            1,
            Math.ceil(rateLimitRemainingMs() / 1_000),
          );
          report(
            event,
            false,
            `rate-limited: gate armed; retry in ${seconds}s`,
          );
          return;
        }
        void relayClient
          .publishEvent(
            event,
            "Timed out while streaming the terminal.",
            "Failed to stream the terminal.",
          )
          .then(
            () => report(event, true, "ok"),
            (error: unknown) => {
              // Dropped frames self-heal via observer resync; the verdict
              // still reaches the broadcaster so it can pace itself.
              report(
                event,
                false,
                error instanceof Error ? error.message : String(error),
              );
            },
          );
      } catch {
        // Malformed payloads are a programming error; never throw here.
      }
    };

    void (async () => {
      const stop = await listen<string>(SHELL_BROADCAST_PUBLISH_EVENT, (e) => {
        publish(e.payload, true);
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
                publish(frame, false);
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

      const inputHandle = await relayClient.subscribeLive(
        {
          kinds: [KIND_SHELL_INPUT],
          "#p": [myPubkey],
          since: Math.floor(Date.now() / 1_000) - 60,
          limit: 500,
        },
        (event) => {
          if (disposed) return;
          // Forward the ENTIRE raw event — signature verification and all
          // authorization happen in Rust. Errors (unknown session, stranger,
          // rate cap) are logged and never crash the pump.
          void shellRemoteInput(JSON.stringify(event)).catch((error) => {
            console.warn("shell-broadcast: remote input refused", error);
          });
        },
      );
      if (disposed) {
        inputHandle?.();
        return;
      }
      unsubscribeInput = inputHandle ?? null;
    })();

    return () => {
      disposed = true;
      unlisten?.();
      unsubscribe?.();
      unsubscribeInput?.();
    };
  }, [myPubkey]);

  return null;
}
