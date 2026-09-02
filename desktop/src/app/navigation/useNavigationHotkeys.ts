import * as React from "react";
import { useLocation, useParams, useRouter } from "@tanstack/react-router";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useShellSessionProjectRef } from "@/features/builtin-shell/hooks/useShellSessions";
import { useChannelsQuery } from "@/features/channels/hooks";
import { codingSessionProjectRef } from "@/features/projects-container/lib/activeCodingSession";
import {
  activateHotkeyTarget,
  setActiveHotkeyScope,
  type HotkeyScope,
} from "@/features/hotkeys/lib/hotkeyTargetRegistry";
import {
  getNavHotkeyBindings,
  useNavHotkeyBindingsSync,
} from "@/features/hotkeys/lib/navHotkeyBindingsStore";
import { matchNavHotkey } from "@/features/hotkeys/lib/navHotkeyBindings";
import { navigateToRememberedRoute } from "@/app/navigation/rememberedRoute";
import { DM_LAST_ROUTE_KEY } from "@/features/projects-container/lib/projectLastRouteStorage";
import {
  getRememberedRoute,
  rememberRoute,
  useProjectRouteMemorySync,
} from "@/features/projects-container/lib/projectRouteMemoryStore";
import { useActiveProjectContainer } from "@/features/projects-container/useActiveProjectTint";
import type { Channel } from "@/shared/api/types";

type NavigationHotkeysOptions = {
  /** The channel the shell currently has selected, if any. */
  activeChannel: Channel | null | undefined;
  disabled: boolean;
  pubkey: string | undefined;
  relayUrl: string | undefined;
};

/**
 * Hold-a-modifier navigation: the scope modifier reaches Dashboard, Projects,
 * Direct messages and the numbered projects; the item modifier reaches the
 * numbered rows of whatever surface is open.
 *
 * Positions are resolved through the hotkey target registry rather than
 * re-derived here, so the chord always lands on the row wearing that number —
 * see `features/hotkeys/lib/hotkeyTargetRegistry.ts`.
 */
export function useNavigationHotkeys({
  activeChannel,
  disabled,
  pubkey,
  relayUrl,
}: NavigationHotkeysOptions): void {
  const router = useRouter();
  const location = useLocation();
  const { goHome, goNewMessage, goProjects } = useAppNavigation();

  useNavHotkeyBindingsSync(pubkey, relayUrl);
  useProjectRouteMemorySync(pubkey, relayUrl);

  // A terminal route carries no channel, so the owning project comes off the
  // shell session itself. Scoped to shell routes: the shell-session store
  // polls the backend, and this hook is mounted on every route.
  const shellSessionId = useParams({
    strict: false,
    select: (params) => (params as { sessionId?: string }).sessionId,
  });
  const shellProjectRef = useShellSessionProjectRef(shellSessionId);

  // A coding-session route (`/coding-sessions/<channel>/<generation>`) also
  // carries no channel the shell can see: a session lives in a *transport*
  // channel, and `useChannelsQuery` hides those from every consumer by
  // default — so `activeChannel` is null there and the project fell through
  // to nothing. The scope then went null, no group numbered its rows, and
  // holding the item modifier over an open session showed no badges at all.
  //
  // Reading the transport-inclusive view costs nothing: the option filters
  // the returned list, it does not change the query, so this shares the
  // cache entry every other caller already populated.
  const codingSessionChannelId = useParams({
    strict: false,
    select: (params) => (params as { channelId?: string }).channelId,
  });
  const allChannelsQuery = useChannelsQuery({ includeSessionTransports: true });
  const sessionProjectRef = React.useMemo(
    () =>
      codingSessionProjectRef(
        location.pathname,
        codingSessionChannelId,
        allChannelsQuery.data,
      ),
    [allChannelsQuery.data, codingSessionChannelId, location.pathname],
  );

  const activeProject = useActiveProjectContainer(
    location.pathname,
    // The session's own project takes precedence: on a coding-session route
    // there is no active channel to disagree with it, and off one this is
    // null so the channel keeps its existing meaning.
    sessionProjectRef ?? activeChannel?.projectRef,
    shellProjectRef,
  );
  const activeProjectId = activeProject?.id ?? null;
  const isViewingDm = activeChannel?.channelType === "dm";

  const itemScope: HotkeyScope | null = activeProjectId
    ? `project:${activeProjectId}`
    : isViewingDm
      ? "dms"
      : null;

  React.useEffect(() => {
    setActiveHotkeyScope(disabled ? null : itemScope);
    return () => setActiveHotkeyScope(null);
  }, [disabled, itemScope]);

  // Remember where we are, so ⌥<n> can come back to it. Recorded per project,
  // plus one reserved slot for the direct-messages surface.
  React.useEffect(() => {
    const at = Date.now();
    if (activeProjectId) {
      rememberRoute(activeProjectId, location.href, at);
      return;
    }
    if (isViewingDm && activeChannel) {
      rememberRoute(DM_LAST_ROUTE_KEY, location.href, at);
    }
  }, [activeChannel, activeProjectId, isViewingDm, location.href]);

  const handleDirectMessages = React.useEffectEvent(() => {
    // Already in a DM: the useful next move is starting another conversation,
    // not re-opening the one on screen.
    if (isViewingDm) {
      void goNewMessage();
      return;
    }
    const href = getRememberedRoute(DM_LAST_ROUTE_KEY);
    navigateToRememberedRoute(
      router,
      href === location.href ? null : href,
      () => void goNewMessage(),
    );
  });

  const handleScopePosition = React.useEffectEvent((index: number) => {
    activateHotkeyTarget("projects", index);
  });

  const handleItemPosition = React.useEffectEvent((index: number) => {
    if (!itemScope) return;
    activateHotkeyTarget(itemScope, index);
  });

  const handleKeyDown = React.useEffectEvent((event: KeyboardEvent) => {
    if (event.repeat || event.defaultPrevented) return;

    const match = matchNavHotkey(event, getNavHotkeyBindings());
    if (!match) return;

    // Claim the event before acting: on macOS an unconsumed ⌥<key> inserts the
    // alternate glyph into whatever has focus, and on Windows a bare Alt chord
    // opens the menu bar.
    event.preventDefault();

    if (match.kind === "scope-position") {
      handleScopePosition(match.index);
      return;
    }
    if (match.kind === "item-position") {
      handleItemPosition(match.index);
      return;
    }

    if (match.action === "dashboard") {
      void goHome();
      return;
    }
    if (match.action === "projects") {
      void goProjects({ filter: "projects" });
      return;
    }
    handleDirectMessages();
  });

  React.useEffect(() => {
    if (disabled) return;
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [disabled]);
}
