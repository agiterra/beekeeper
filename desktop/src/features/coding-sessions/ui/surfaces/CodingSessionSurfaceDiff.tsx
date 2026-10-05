import { FileDiff } from "lucide-react";

import { CodingSessionChangesRail } from "../CodingSessionChangesRail";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfaceDiffBadge } from "./CodingSessionSurfaceDiffBadge";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/** Diff can open once anything was observed in the transcript (§3). */
export function codingSessionSurfaceDiffAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return ctx.transcript.length > 0
    ? { available: true }
    : { available: false, reason: "Nothing has been observed yet." };
}

/**
 * The Diff surface: the observed-changes rail, labelled Diff as in T3 Code.
 * It keeps its disclosure that it shows **observed** edits plus the
 * unreported remainder; a git-backed Diff is SV-30.
 */
export function CodingSessionSurfaceDiffPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceDiffAvailability(ctx);
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={FileDiff}
        id="diff"
        label="Diff"
        reason={availability.reason}
      />
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-diff"
    >
      <CodingSessionChangesRail
        files={[...ctx.observedChanges.files]}
        unreportedEditCount={ctx.observedChanges.unreportedEditCount}
      />
    </div>
  );
}

export const codingSessionSurfaceDiff: CodingSessionSurfaceDefinition = {
  id: "diff",
  label: "Diff",
  icon: FileDiff,
  shortcut: "D",
  order: 20,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfaceDiffAvailability,
  Badge: CodingSessionSurfaceDiffBadge,
  Panel: CodingSessionSurfaceDiffPanel,
};
