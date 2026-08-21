import * as React from "react";

import { setDesktopAppBadge } from "@/features/notifications/lib/desktop";
import { useForegroundQueryRefresh } from "@/features/workflows/hooks";
import { relayClient } from "@/shared/api/relayClient";
import { useRelayResumeTriggers } from "@/shared/api/useRelayResumeTriggers";

type AppShellLifecycleEffectsOptions = {
  desktopBadgeEnabled: boolean;
  homeBadgeCountExcludingHighPriority: number;
  unreadChannelIds: ReadonlySet<string>;
  unreadChannelNotificationCount: number;
};

export function useAppShellLifecycleEffects({
  desktopBadgeEnabled,
  homeBadgeCountExcludingHighPriority,
  unreadChannelIds,
  unreadChannelNotificationCount,
}: AppShellLifecycleEffectsOptions) {
  // Event-driven reconnect: network online / focus / visibility short-circuit
  // the backoff timer when the relay session is degraded (CMD+R gap G1).
  useRelayResumeTriggers();
  useForegroundQueryRefresh();

  // Prevent webview file:/// navigation on file drop outside the composer.
  // Scoped to file drags only (text drag-and-drop into inputs still works).
  // Composer's onDrop fires first (React synthetic before window bubble).
  React.useEffect(() => {
    function preventNavigation(e: DragEvent) {
      if (e.dataTransfer?.types.includes("Files")) {
        e.preventDefault();
      }
    }
    window.addEventListener("dragover", preventNavigation);
    window.addEventListener("drop", preventNavigation);
    return () => {
      window.removeEventListener("dragover", preventNavigation);
      window.removeEventListener("drop", preventNavigation);
    };
  }, []);

  // Connect eagerly: every relay-backed sidebar section (sessions, repos,
  // project containers) is serialized behind WS connect + NIP-42 AUTH, so
  // deferring preconnect to idle time pushed their first paint out by up to
  // 1.5s on boot.
  React.useEffect(() => {
    let isCancelled = false;

    void relayClient.preconnect().catch((error) => {
      if (!isCancelled) {
        console.error("Failed to preconnect to relay", error);
      }
    });

    return () => {
      isCancelled = true;
    };
  }, []);

  React.useEffect(() => {
    if (!desktopBadgeEnabled) {
      return;
    }

    const count =
      unreadChannelNotificationCount + homeBadgeCountExcludingHighPriority;
    void setDesktopAppBadge(
      count
        ? { kind: "count", count }
        : { kind: unreadChannelIds.size ? "dot" : "none" },
    );
  }, [
    desktopBadgeEnabled,
    homeBadgeCountExcludingHighPriority,
    unreadChannelIds,
    unreadChannelNotificationCount,
  ]);
}
