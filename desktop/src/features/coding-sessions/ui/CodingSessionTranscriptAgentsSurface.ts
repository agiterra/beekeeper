import * as React from "react";

/**
 * Opens the workspace's Agents surface, when the transcript sits in a
 * workspace that has one (SV-06, SV-24). A subagent row in the conversation
 * opens that surface rather than expanding in place, as T3 Code's does.
 *
 * `null` — the default — means no Agents surface is reachable from here
 * (Mission's umbrella timeline, a test, a caller that has not wired it); the
 * row then expands inline, so its detail is never less than one click away.
 *
 * A React context, not a module-level cache: nothing outlives the render
 * tree, so `resetCommunityState()` has nothing to reset.
 */
export const CodingSessionOpenAgentsSurfaceContext = React.createContext<
  (() => void) | null
>(null);

/** The Agents-surface opener for rows rendered here, or `null`. */
export function useCodingSessionOpenAgentsSurface(): (() => void) | null {
  return React.useContext(CodingSessionOpenAgentsSurfaceContext);
}
