import { Outlet } from "@tanstack/react-router";

import * as BuzzTheme from "@/app/BuzzThemeSurfaces";
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
          <BuzzTheme.GradientLayer />
          <BuzzTheme.ContentSurface>
            <Outlet />
          </BuzzTheme.ContentSurface>
        </div>
      </ChannelNavigationProvider>
    </PreventSleepProvider>
  );
}
