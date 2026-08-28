import type * as React from "react";
import * as BuzzTheme from "@/app/BuzzThemeSurfaces";
import { HuddleRoomHeader, HuddleStartingView } from "@/features/huddle";
import { MainInsetProvider } from "@/shared/layout/MainInsetContext";
import { chromeCssVarDefaults } from "@/shared/layout/chromeLayout";
import { cn } from "@/shared/lib/cn";
import { SidebarInset } from "@/shared/ui/sidebar";

type AppShellChannelSurfaceProps = {
  children: React.ReactNode;
  isHuddleRoom: boolean;
  isHuddleRoomStarting: boolean;
  mainInsetRef: React.RefObject<HTMLElement | null>;
  /** Active project's tint (`#rrggbb`), washed over the content surface. */
  projectTintColor?: string | null;
  terminal?: React.ReactNode;
};

export function AppShellChannelSurface({
  children,
  isHuddleRoom,
  isHuddleRoomStarting,
  mainInsetRef,
  projectTintColor = null,
  terminal,
}: AppShellChannelSurfaceProps) {
  // Element-local, never :root — the tint scopes to this inset only, and the
  // var + attribute appear and disappear together (see theme.css).
  const tint = isHuddleRoom ? null : projectTintColor;
  return (
    <MainInsetProvider mainInsetRef={mainInsetRef}>
      <SidebarInset
        ref={mainInsetRef}
        className={cn(
          "isolate z-0 min-h-0 min-w-0 overflow-hidden",
          isHuddleRoom ? "bg-background" : "bg-sidebar",
        )}
        data-buzz-content-surface={isHuddleRoom ? true : undefined}
        data-buzz-content-unframed={isHuddleRoom ? true : undefined}
        data-buzz-glass-inset
        data-buzz-shadow-viewport
        data-project-tint={tint ? "" : undefined}
        style={
          {
            ...chromeCssVarDefaults,
            ...(tint ? { "--project-tint": tint } : {}),
          } as React.CSSProperties
        }
      >
        {isHuddleRoom && !isHuddleRoomStarting ? <HuddleRoomHeader /> : null}
        <BuzzTheme.ContentSurface terminal={terminal} unframed={isHuddleRoom}>
          {isHuddleRoomStarting ? <HuddleStartingView /> : children}
        </BuzzTheme.ContentSurface>
      </SidebarInset>
    </MainInsetProvider>
  );
}
