import { FileDiff } from "lucide-react";

import { CodingSessionChangesRail } from "../CodingSessionChangesRail";
import {
  CodingSessionDiffSurface,
  useCodingSessionDiffGeneration,
} from "../CodingSessionDiffSurface";
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
 * The Diff surface (SV-30): git-backed from the session's signed turn
 * checkpoints when it has any, with the observed-edits rail one click away;
 * the observed rail alone, still saying it is not a git diff, when it has
 * none.
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
  return <CodingSessionSurfaceDiffContent ctx={ctx} />;
}

function CodingSessionSurfaceDiffContent({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const generation = useCodingSessionDiffGeneration(ctx);
  const observed = (
    <CodingSessionChangesRail
      files={[...ctx.observedChanges.files]}
      gitBacked={generation !== null}
      unreportedEditCount={ctx.observedChanges.unreportedEditCount}
    />
  );
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-source={generation ? "git" : "observed"}
      data-testid="coding-session-surface-panel-diff"
    >
      {generation ? (
        <CodingSessionDiffSurface
          ctx={ctx}
          generation={generation}
          observed={observed}
        />
      ) : (
        observed
      )}
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
