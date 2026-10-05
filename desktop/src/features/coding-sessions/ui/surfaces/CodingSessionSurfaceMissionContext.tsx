import { Scale } from "lucide-react";

import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";

/**
 * Mission's Context, rendered exactly as before: the Mission surface hook
 * builds it (`useCodingSessionMissionSurface`) and hands it over in
 * `ctx.mission`. Listed only in the Mission lens, which is a lens choice, not
 * a hidden unavailable surface (§3).
 */
export function CodingSessionSurfaceMissionContextPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  return <>{ctx.mission?.context ?? null}</>;
}

export const codingSessionSurfaceMissionContext: CodingSessionSurfaceDefinition =
  {
    id: "mission-context",
    label: "Context",
    icon: Scale,
    shortcut: "C",
    order: 2,
    placement: "right",
    lenses: ["mission"],
    availability: () => ({ available: true }),
    Panel: CodingSessionSurfaceMissionContextPanel,
  };
