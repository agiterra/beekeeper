import { PanelRight } from "lucide-react";

import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";

/**
 * Mission's Inspector, rendered exactly as before: the Mission surface hook
 * builds it (`useCodingSessionMissionSurface`) and hands it over in
 * `ctx.mission`. Listed only in the Mission lens, which is a lens choice, not
 * a hidden unavailable surface (§3).
 */
export function CodingSessionSurfaceMissionInspectorPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  return <>{ctx.mission?.inspector ?? null}</>;
}

export const codingSessionSurfaceMissionInspector: CodingSessionSurfaceDefinition =
  {
    id: "mission-inspector",
    label: "Inspector",
    icon: PanelRight,
    shortcut: "I",
    order: 1,
    placement: "right",
    lenses: ["mission"],
    availability: () => ({ available: true }),
    Panel: CodingSessionSurfaceMissionInspectorPanel,
  };
