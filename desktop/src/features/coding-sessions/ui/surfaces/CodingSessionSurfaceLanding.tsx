import { GitMerge } from "lucide-react";

import { CodingSessionLandingPanel } from "../CodingSessionLandingPanel";
import { useCodingSessionLandingExtension } from "../CodingSessionLandingRuleRead";
import type {
  CodingSessionSurfaceAvailability,
  CodingSessionSurfaceCtx,
} from "./codingSessionSurfaceContext";
import type { CodingSessionSurfaceDefinition } from "./codingSessionSurfaceRegistry";
import { CodingSessionSurfaceLandingBadge } from "./CodingSessionSurfaceLandingBadge";
import { CodingSessionSurfacePlaceholder } from "./CodingSessionSurfaceDevicePlaceholder";

/**
 * Whether this session has a landing fact to show without a repository: a
 * signed gate row or gate start in the observation fold, or a genesis (the
 * mission's verdict and land rule, and the decision requests Landing's badge
 * counts). Landing's badge is built only from these facts, so a badge can
 * never point at a Landing that will not open.
 */
export function codingSessionLandingHasFacts(
  ctx: Pick<CodingSessionSurfaceCtx, "observations" | "umbrella">,
): boolean {
  if (ctx.umbrella.genesisRef !== null) return true;
  const read = ctx.observations.state === "read" ? ctx.observations : null;
  if (read === null) return false;
  return (
    read.view.gates.length > 0 ||
    (read.result?.fold?.gateStarts?.length ?? 0) > 0
  );
}

/**
 * Landing: when it can open, or the sentence why not (§3, SV-23). A
 * repository opens it; so does any gate row, gate start or genesis without
 * one (the Land and Landed rows then say there is no repository to land
 * into), so a failing gate in a channel-folder session is still reachable
 * from the badge that names it.
 */
export function codingSessionSurfaceLandingAvailability(
  ctx: CodingSessionSurfaceCtx,
): CodingSessionSurfaceAvailability {
  return ctx.repoRef !== null || codingSessionLandingHasFacts(ctx)
    ? { available: true }
    : {
        available: false,
        reason: "This session has no repository to land into.",
      };
}

/**
 * Landing (DB4): Gate, Verdict, Land and Landed for this session's newest
 * head. Where the session names no repository, Gate and Verdict still show
 * what was signed and Land and Landed say there is nothing to land into.
 */
export function CodingSessionSurfaceLandingPanel({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  const availability = codingSessionSurfaceLandingAvailability(ctx);
  if (!availability.available) {
    return (
      <CodingSessionSurfacePlaceholder
        icon={GitMerge}
        id="landing"
        label="Landing"
        reason={availability.reason}
      />
    );
  }
  return (
    <div
      className="flex min-h-0 flex-1 flex-col"
      data-available="true"
      data-testid="coding-session-surface-panel-landing"
    >
      <CodingSessionLandingPanel ctx={ctx} />
    </div>
  );
}

export const codingSessionSurfaceLanding: CodingSessionSurfaceDefinition = {
  id: "landing",
  label: "Landing",
  icon: GitMerge,
  shortcut: "L",
  order: 60,
  placement: "right",
  lenses: ["conversation", "mission"],
  availability: codingSessionSurfaceLandingAvailability,
  Badge: CodingSessionSurfaceLandingBadge,
  Panel: CodingSessionSurfaceLandingPanel,
  // The land rule's answer, once per view: a refusing newest verdict reaches
  // the badge and the header's dot with the panel closed (DB5), and the
  // panel reads the same answer (`ctx.extensions.landing`).
  readExtension: useCodingSessionLandingExtension,
};
