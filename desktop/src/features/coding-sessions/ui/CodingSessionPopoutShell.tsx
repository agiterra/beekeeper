import { Outlet } from "@tanstack/react-router";

import * as BeekeeperTheme from "@/app/BeekeeperThemeSurfaces";
import { PreventSleepProvider } from "@/features/agents/usePreventSleep";
import type { Channel } from "@/shared/api/types";
import { ChannelNavigationProvider } from "@/shared/context/ChannelNavigationContext";

/** Minimal Buzz-native shell for a coding-session pop-out window. */
export function CodingSessionPopoutShell({
  channels,
}: {
  channels: Channel[];
}) {
  return (
    <PreventSleepProvider>
      <ChannelNavigationProvider channels={channels}>
        <div className="relative flex h-dvh overflow-hidden bg-background">
          <BeekeeperTheme.GradientLayer />
          <BeekeeperTheme.ContentSurface>
            <Outlet />
          </BeekeeperTheme.ContentSurface>
        </div>
      </ChannelNavigationProvider>
    </PreventSleepProvider>
  );
}
