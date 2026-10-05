import { ListChecks } from "lucide-react";

import { CodingSessionTaskRail } from "../CodingSessionTaskRail";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfacePlanBadge } from "./CodingSessionSurfacePlanBadge";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/**
 * Plan: when it can open, or the sentence why not (§3, SV-23). Any signed
 * plan opens it — an explicitly empty one too (`state: "empty"`), which the
 * rail says in its own words; only no plan at all reads "not published".
 */
export function codingSessionSurfacePlanAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return ctx.taskModel !== null
    ? { available: true }
    : { available: false, reason: "The agent has not published a plan." };
}

/**
 * The Plan surface: the focused execution's latest signed plan, the task
 * rail's `surface` variant. The one-line dock above the composer stays where
 * it is; this is the whole list, one letter away.
 */
export function CodingSessionSurfacePlanPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfacePlanAvailability(ctx);
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={ListChecks}
        id="plan"
        label="Plan"
        reason={availability.reason}
      />
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-plan"
    >
      <CodingSessionTaskRail model={ctx.taskModel} variant="surface" />
    </div>
  );
}

export const codingSessionSurfacePlan: CodingSessionSurfaceDefinition = {
  id: "plan",
  label: "Plan",
  icon: ListChecks,
  shortcut: "P",
  order: 50,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfacePlanAvailability,
  Badge: CodingSessionSurfacePlanBadge,
  Panel: CodingSessionSurfacePlanPanel,
};
