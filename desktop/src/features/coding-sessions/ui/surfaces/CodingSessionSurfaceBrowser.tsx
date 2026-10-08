import * as React from "react";
import { Globe } from "lucide-react";

import { listCodingSessionUmbrellaParticipants } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { SessionPreviewBindingOption } from "@/features/session-preview/lib/previewModel";
import { sessionPreviewAvailability } from "@/features/session-preview/lib/previewModel";
import { SessionPreviewSurface } from "@/features/session-preview/ui/SessionPreviewSurface";
import { listenSessionPreview } from "@/shared/api/tauriSessionPreview";
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
 */
export function codingSessionSurfaceBrowserAvailability(
  ctx: Pick<CodingSessionSurfaceCtx, "channelId">,
  isMac: boolean = isMacPlatform(),
): CodingSessionSurfaceAvailability {
  const availability = sessionPreviewAvailability({
    channelId: ctx.channelId,
    isMac,
  });
  return availability.available
    ? { available: true }
    : { available: false, reason: availability.reason };
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
      />
    </div>
  );
}

/**
 * An agent's `bee preview open` with no Browser slot mounted: Rust has popped
 * the preview out; this offers the Browser tab as the view's own move
 * (`openProactive`, refused once the person has arranged the panels), so the
 * person can bring it back beside the session. Returns nothing.
 */
export function useCodingSessionBrowserOpenRequests(
  ctx: CodingSessionSurfaceBaseCtx,
): null {
  const { channelId } = ctx;
  // The panel actions object may be fresh each render; subscribe once per
  // channel and call whatever the latest one is.
  const openRef = React.useRef(ctx.panels.openProactive);
  openRef.current = ctx.panels.openProactive;
  React.useEffect(() => {
    if (!channelId) return;
    let cancelled = false;
    let unlisten: (() => void) | null = null;
    void listenSessionPreview(channelId, {
      onOpenRequested: () => openRef.current?.(["browser"], "browser"),
    })
      .then((stop) => {
        if (cancelled) stop();
        else unlisten = stop;
      })
      .catch(() => {});
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [channelId]);
  return null;
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
  Panel: CodingSessionSurfaceBrowserPanel,
  readExtension: useCodingSessionBrowserOpenRequests,
};
