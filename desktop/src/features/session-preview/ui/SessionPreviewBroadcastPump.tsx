import * as React from "react";
import { listen } from "@tauri-apps/api/event";

import { parsePreviewWatchEvent } from "@/features/session-preview/lib/previewBroadcastWatch";
import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import {
  isRateLimited,
  rateLimitRemainingMs,
} from "@/shared/api/relayRateLimitGate";
import {
  SESSION_PREVIEW_BROADCAST_PUBLISH_EVENT,
  SESSION_PREVIEW_SHARE_STATE_EVENT,
  type SessionPreviewBroadcastPublish,
  type SessionPreviewShareState,
  sessionPreviewShareNotePublish,
  sessionPreviewShareWatch,
} from "@/shared/api/tauriSessionPreviewShare";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SURFACE_WATCH } from "@/shared/constants/kinds";

/**
 * The preview-sharing pump (C5), mounted app-wide; renders nothing.
 *
 * Bridges the Rust preview broadcaster and the relay WebSocket both ways,
 * like `ShellBroadcastPump`: signed kind 24321 frames arrive on
 * `session-preview://broadcast-publish` and are published to the relay, each
 * outcome reported back through `session_preview_share_note_publish` (a
 * `rate-limited:` refusal backs the cadence off); kind 24320 watches
 * addressed to this identity with `surface=preview` are forwarded to
 * `session_preview_share_watch`, which re-checks everything itself.
 *
 * Frames are droppable: while the relay's rate gate is armed they are
 * reported as refused and not sent (queueing them would burst out exactly
 * what tripped it). `paused` and `end` are never dropped.
 *
 * The watch subscription is held only while this desktop announces an open
 * preview for some channel (`session-preview://share-state` with
 * `announced: "open"`): a person who never opens the Browser costs the relay
 * nothing. Watchers re-send every 15 s, so one opened late still hears them.
 */
export function SessionPreviewBroadcastPump() {
  const identity = useIdentityQuery();
  const myPubkey = identity.data?.pubkey ?? null;

  React.useEffect(() => {
    if (!myPubkey) return;
    let disposed = false;
    let unlisten: (() => void) | null = null;
    let unsubscribe: (() => void) | null = null;

    const report = (channelId: string, accepted: boolean, message: string) => {
      void sessionPreviewShareNotePublish({
        channelId,
        accepted,
        message,
      }).catch(() => {
        // The broadcaster may be gone; nothing to back off.
      });
    };

    const publish = ({
      channelId,
      event: json,
    }: SessionPreviewBroadcastPublish) => {
      let event: RelayEvent;
      try {
        event = JSON.parse(json) as RelayEvent;
      } catch {
        return;
      }
      const isPicture = event.tags.some(
        (tag) => tag[0] === "t" && tag[1] === "frame",
      );
      if (isPicture && isRateLimited()) {
        const seconds = Math.max(1, Math.ceil(rateLimitRemainingMs() / 1_000));
        report(
          channelId,
          false,
          `rate-limited: gate armed; retry in ${seconds}s`,
        );
        return;
      }
      void relayClient
        .publishEvent(
          event,
          "Timed out while sharing the Browser.",
          "Failed to share the Browser.",
        )
        .then(
          () => report(channelId, true, "ok"),
          (error: unknown) =>
            report(
              channelId,
              false,
              error instanceof Error ? error.message : String(error),
            ),
        );
    };

    const announcing = new Set<string>();
    let opening = false;

    const openWatchSubscription = async () => {
      if (opening || unsubscribe || disposed) return;
      opening = true;
      try {
        const handle = await relayClient.subscribeLive(
          {
            kinds: [KIND_SURFACE_WATCH],
            "#p": [myPubkey],
            since: Math.floor(Date.now() / 1_000) - 30,
            limit: 500,
          },
          (event) => {
            if (disposed) return;
            const request = parsePreviewWatchEvent(event, myPubkey);
            if (!request) return;
            void sessionPreviewShareWatch(request).catch(() => {
              // Not shared for that session, or sharing is off: nothing to send.
            });
          },
        );
        if (disposed || announcing.size === 0) {
          void handle?.();
          return;
        }
        unsubscribe = handle ? () => void handle() : null;
      } catch {
        // The relay is unreachable; the next share-state change retries.
      } finally {
        opening = false;
      }
    };

    const onShareState = (state: SessionPreviewShareState) => {
      if (!state?.channelId) return;
      if (state.announced === "open") announcing.add(state.channelId);
      else announcing.delete(state.channelId);
      if (announcing.size > 0) {
        void openWatchSubscription();
      } else if (unsubscribe) {
        const stop = unsubscribe;
        unsubscribe = null;
        stop();
      }
    };

    void (async () => {
      const stopPublish = await listen<SessionPreviewBroadcastPublish>(
        SESSION_PREVIEW_BROADCAST_PUBLISH_EVENT,
        (e) => {
          if (e.payload && typeof e.payload.event === "string") {
            publish(e.payload);
          }
        },
      );
      const stopState = await listen<SessionPreviewShareState>(
        SESSION_PREVIEW_SHARE_STATE_EVENT,
        (e) => onShareState(e.payload),
      );
      const stop = () => {
        stopPublish();
        stopState();
      };
      if (disposed) {
        stop();
        return;
      }
      unlisten = stop;
    })();

    return () => {
      disposed = true;
      unlisten?.();
      unsubscribe?.();
    };
  }, [myPubkey]);

  return null;
}
