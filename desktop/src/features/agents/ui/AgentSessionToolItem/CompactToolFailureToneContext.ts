import * as React from "react";

import type { CompactToolFailureTone } from "./CompactToolSummaryRowFailure";

/**
 * Where a collapsed tool row sits, for how a failed call reads (SV-02, D2).
 *
 * The fold that hides a settled turn's work provides `quiet` around the
 * entries it hides, so a failed step shown by opening the fold reads as a
 * muted, still-marked step (dimmed icon, "· exit 2"). Everywhere else the
 * default `alarm` applies: a row that does not know where it sits keeps the
 * loud presentation, so a missing provider can only over-warn, never hide.
 *
 * A React context, not a module-level cache: nothing here outlives a render
 * tree, so `resetCommunityState()` has nothing to reset.
 */
export const CompactToolFailureToneContext =
  React.createContext<CompactToolFailureTone>("alarm");

/** The failure tone for rows rendered here; `alarm` outside any fold. */
export function useCompactToolFailureTone(): CompactToolFailureTone {
  return React.useContext(CompactToolFailureToneContext);
}
