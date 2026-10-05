import { codingSessionPlanBadgeFromCtx } from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModelCtx";
import { CodingSessionSurfaceBadgePill } from "../CodingSessionSurfaceBadgePill";
import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceBadgeSlot } from "./codingSessionSurfaceRegistry";

/**
 * The Plan surface's badge (SV-22): tasks in progress while the focused seat
 * is working. A plan an idle session left behind is a snapshot, not work.
 */
export function CodingSessionSurfacePlanBadge({
  ctx,
  slot,
}: {
  ctx: CodingSessionSurfaceCtx;
  slot: CodingSessionSurfaceBadgeSlot;
}) {
  return (
    <CodingSessionSurfaceBadgePill
      badge={codingSessionPlanBadgeFromCtx(ctx)}
      slot={slot}
      surfaceId="plan"
    />
  );
}
