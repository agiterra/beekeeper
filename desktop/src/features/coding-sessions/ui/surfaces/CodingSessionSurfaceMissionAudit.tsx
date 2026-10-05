import { ScrollText } from "lucide-react";

import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";

/**
 * Mission's Audit, rendered exactly as before: the Mission surface hook
 * builds it (`useCodingSessionMissionSurface`) and hands it over in
 * `ctx.mission`. Listed only in the Mission lens, which is a lens choice, not
 * a hidden unavailable surface (§3).
 */
export function CodingSessionSurfaceMissionAuditPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  return <>{ctx.mission?.audit ?? null}</>;
}

export const codingSessionSurfaceMissionAudit: CodingSessionSurfaceDefinition =
  {
    id: "mission-audit",
    label: "Audit",
    icon: ScrollText,
    shortcut: "X",
    order: 3,
    placement: "right",
    lenses: ["mission"],
    availability: () => ({ available: true }),
    Panel: CodingSessionSurfaceMissionAuditPanel,
  };
