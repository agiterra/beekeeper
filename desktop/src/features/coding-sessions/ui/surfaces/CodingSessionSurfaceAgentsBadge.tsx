import * as React from "react";

import {
  codingSessionAgentsBadgeFromCtx,
  codingSessionLiveSubagentCount,
} from "@/features/coding-sessions/lib/codingSessionSurfaceBadgeModelCtx";
import { CodingSessionSurfaceBadgePill } from "../CodingSessionSurfaceBadgePill";
import type { CodingSessionSurfaceCtx } from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceBadgeSlot } from "./codingSessionSurfaceRegistry";

/**
 * The Agents surface's badge (SV-22): subagents running now — open Task
 * calls of a seat on a turn — plus seats on a turn in a team session; amber
 * while a `decision.request` waits on a person's ruling. Clears when the
 * work ends — five finished subagents, or two left open by an abandoned
 * turn, draw nothing.
 */
export function CodingSessionSurfaceAgentsBadge({
  ctx,
  slot,
}: {
  ctx: CodingSessionSurfaceCtx;
  slot: CodingSessionSurfaceBadgeSlot;
}) {
  const executions = ctx.executions;
  const liveSubagents = React.useMemo(
    () => codingSessionLiveSubagentCount(executions),
    [executions],
  );
  return (
    <CodingSessionSurfaceBadgePill
      badge={codingSessionAgentsBadgeFromCtx(ctx, liveSubagents)}
      slot={slot}
      surfaceId="agents"
    />
  );
}
