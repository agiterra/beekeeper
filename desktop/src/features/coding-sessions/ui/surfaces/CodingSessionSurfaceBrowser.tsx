import * as React from "react";
import { Globe } from "lucide-react";

import { listCodingSessionUmbrellaParticipants } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { SessionPreviewBindingOption } from "@/features/session-preview/lib/previewModel";
import { sessionPreviewAvailability } from "@/features/session-preview/lib/previewModel";
import { sessionPreviewShowsRemote } from "@/features/session-preview/lib/previewShareModel";
import {
  type SessionPreviewRemoteRead,
  SessionPreviewRemoteView,
  useSessionPreviewRemoteRead,
} from "@/features/session-preview/ui/SessionPreviewRemoteView";
import { useSessionPreviewBrowserTab } from "@/features/session-preview/hooks/useSessionPreviewBrowserTab";
import { SessionPreviewHiddenBadge } from "@/features/session-preview/ui/SessionPreviewHiddenBadge";
import { SessionPreviewSurface } from "@/features/session-preview/ui/SessionPreviewSurface";
import { isMacPlatform } from "@/shared/lib/platform";

import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceBaseCtx,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/**
 * Browser: when it can open, or the sentence why not (§3, SV-23; WIRE-C4 §4).
 * `isMac` defaults to this computer's platform; the reasons test passes it.
 * Whether the native view starts is Rust's answer, shown in the panel.
 *
 * C5: the remote view (another computer's shared preview) needs no native
 * view, so off macOS the Browser still opens when it would show that view —
 * the session's agent runs elsewhere, or someone else's announce owns it.
 */
export function codingSessionSurfaceBrowserAvailability(
  ctx: Pick<CodingSessionSurfaceCtx, "channelId"> &
    Partial<
      Pick<
        CodingSessionSurfaceCtx,
        "umbrella" | "isLocalProvider" | "currentUserPubkey" | "extensions"
      >
    >,
  isMac: boolean = isMacPlatform(),
): CodingSessionSurfaceAvailability {
  const availability = sessionPreviewAvailability({
    channelId: ctx.channelId,
    isMac,
  });
  if (availability.available) return { available: true };
  if (
    availability.code === "not_macos" &&
    codingSessionBrowserShowsRemote(ctx)
  ) {
    return { available: true };
  }
  return { available: false, reason: availability.reason };
}

/** The Browser extension (`ctx.extensions.browser`): the remote read. */
export function codingSessionBrowserRemoteRead(
  ctx: Partial<Pick<CodingSessionSurfaceCtx, "extensions">>,
): SessionPreviewRemoteRead | null {
  const value = ctx.extensions?.browser;
  return value && typeof value === "object"
    ? (value as SessionPreviewRemoteRead)
    : null;
}

/** Whether this view shows the remote Browser rather than the local one. */
export function codingSessionBrowserShowsRemote(
  ctx: Partial<
    Pick<
      CodingSessionSurfaceCtx,
      "umbrella" | "isLocalProvider" | "currentUserPubkey" | "extensions"
    >
  >,
): boolean {
  return sessionPreviewShowsRemote({
    sessionRef: ctx.umbrella?.sessionRef ?? null,
    isLocalProvider: ctx.isLocalProvider ?? null,
    owner: codingSessionBrowserRemoteRead(ctx)?.owner ?? null,
    currentUserPubkey: ctx.currentUserPubkey ?? null,
  });
}

/**
 * The executions a person-opened preview may be bound to: each one whose
 * active generation names a target (the composer's target), labelled as the
 * session's participant chips label it. Only the bound session's grants may
 * drive the preview (WIRE-C4 contract change, 2026-10-07).
 */
export function codingSessionBrowserBindingOptions(
  ctx: Pick<CodingSessionSurfaceCtx, "umbrella" | "resolveActorName">,
): SessionPreviewBindingOption[] {
  return listCodingSessionUmbrellaParticipants(
    ctx.umbrella,
    ctx.resolveActorName,
  ).flatMap((participant) => {
    if (participant.kind !== "execution") return [];
    const target = participant.execution.activeGeneration.commandTarget;
    return target
      ? [
          {
            executionKey: participant.executionKey,
            label: participant.label,
            target,
          },
        ]
      : [];
  });
}

/**
 * The Browser panel: the local preview, bound to the focused execution (or
 * the one the person picks), or the reason it cannot open here.
 */
export function CodingSessionSurfaceBrowserPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceBrowserAvailability(ctx);
  const { umbrella, resolveActorName } = ctx;
  const options = React.useMemo(
    () => codingSessionBrowserBindingOptions({ umbrella, resolveActorName }),
    [umbrella, resolveActorName],
  );
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={Globe}
        id="browser"
        label="Browser"
        reason={availability.reason}
      />
    );
  }
  const remoteRead = codingSessionBrowserRemoteRead(ctx);
  const sessionRef = ctx.umbrella.sessionRef;
  if (remoteRead && sessionRef && codingSessionBrowserShowsRemote(ctx)) {
    return (
      <div
        className="flex min-h-0 flex-1 flex-col"
        data-available="true"
        data-testid="coding-session-surface-panel-browser"
      >
        <SessionPreviewRemoteView
          channelId={ctx.channelId}
          currentUserPubkey={ctx.currentUserPubkey}
          nameOf={ctx.resolveActorName}
          read={remoteRead}
          sessionRef={sessionRef}
        />
      </div>
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-browser"
    >
      <SessionPreviewSurface
        channelId={ctx.channelId}
        focusedExecutionKey={ctx.focusedExecution?.executionKey ?? null}
        isLocalProvider={ctx.isLocalProvider}
        options={options}
        sessionRef={ctx.umbrella.sessionRef}
      />
    </div>
  );
}

/**
 * The preview's tie to this view's Browser tab (`useSessionPreviewBrowserTab`):
 * an agent's open-request shows the tab instead of popping a window, and the
 * person closing the tab closes the page. Returns nothing.
 */
export function useCodingSessionBrowserOpenRequests(
  ctx: CodingSessionSurfaceBaseCtx,
): null {
  useSessionPreviewBrowserTab({
    channelId: ctx.channelId,
    sessionKey: ctx.sessionKey,
    lens: ctx.lens,
    panelState: ctx.panelState,
    panels: ctx.panels,
  });
  return null;
}

/**
 * The Browser's `readExtension`: the agent open-request listener, plus the
 * session's shared preview as the relay has it (owner and snapshots), read
 * once per view for both the availability and the panel.
 */
export function useCodingSessionBrowserExtension(
  ctx: CodingSessionSurfaceBaseCtx,
): SessionPreviewRemoteRead {
  useCodingSessionBrowserOpenRequests(ctx);
  return useSessionPreviewRemoteRead(
    ctx.channelId || null,
    ctx.umbrella.sessionRef,
  );
}

export const codingSessionSurfaceBrowser: CodingSessionSurfaceDefinition = {
  id: "browser",
  label: "Browser",
  icon: Globe,
  shortcut: "B",
  order: 90,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: (ctx) => codingSessionSurfaceBrowserAvailability(ctx),
  Badge: ({ ctx, slot }) => (
    <SessionPreviewHiddenBadge channelId={ctx.channelId} slot={slot} />
  ),
  Panel: CodingSessionSurfaceBrowserPanel,
  readExtension: useCodingSessionBrowserExtension,
};
