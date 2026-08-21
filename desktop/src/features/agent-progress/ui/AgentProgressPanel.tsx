import type {
  AgentProgressAggregate,
  AgentProgressLane,
} from "../lib/agentProgressFold";

import { AgentProgressFooter } from "./AgentProgressFooter";
import { AgentProgressLaneRow } from "./AgentProgressLaneRow";

/**
 * The live per-session panel.
 *
 * Presentational on purpose: everything it renders was decided by
 * `foldAgentProgress` over a signed coordination read. Its three non-list
 * states stay distinct —
 *
 * - *loading*: no read has resolved, so there is nothing to claim;
 * - *incomplete*: a source did not answer, which is a failed read and **not**
 *   a claim that nothing is running;
 * - *confirmed empty*: every source answered and returned no sessions —
 *
 * because collapsing them is exactly how a surface ends up telling somebody
 * their agents are idle when it simply could not read. Even the confirmed-empty
 * copy is scoped to the read ("no sessions in what this read returned") rather
 * than to the world, because the read only ever covered channels this viewer
 * can see.
 */
export function AgentProgressPanel({
  lanes,
  aggregate,
  isLoading,
  complete,
  errors,
  ambiguities,
  onOpenLane,
}: {
  lanes: readonly AgentProgressLane[];
  aggregate: AgentProgressAggregate;
  isLoading: boolean;
  complete: boolean;
  errors: readonly { scope: string; message: string }[];
  ambiguities: readonly { scope: string; message: string }[];
  onOpenLane?: (lane: AgentProgressLane) => void;
}) {
  return (
    <section
      className="flex min-h-0 flex-col rounded-md border border-border/60 bg-background/40"
      data-testid="agent-progress-panel"
    >
      {!isLoading && !complete ? (
        <p
          className="border-b border-border/60 px-3 py-2 text-2xs text-muted-foreground"
          data-testid="agent-progress-incomplete"
        >
          This read did not complete:{" "}
          {errors[0]?.message ?? "a source did not answer"}. Counts below are a
          floor, not a census — this is a failed read, not a claim that nothing
          is running.
        </p>
      ) : null}

      {ambiguities.length > 0 ? (
        <p
          className="border-b border-border/60 px-3 py-2 text-2xs text-muted-foreground"
          data-testid="agent-progress-ambiguous"
        >
          {ambiguities.length} piece
          {ambiguities.length === 1 ? "" : "s"} of evidence could not be
          resolved and prove nothing: {ambiguities[0].message}.
        </p>
      ) : null}

      {lanes.length > 0 ? (
        <ul
          className="min-h-0 flex-1 overflow-y-auto"
          data-testid="agent-progress-lanes"
        >
          {lanes.map((lane) => (
            <AgentProgressLaneRow
              key={lane.laneId}
              lane={lane}
              onOpen={onOpenLane}
            />
          ))}
        </ul>
      ) : isLoading ? (
        <p
          className="px-3 py-6 text-center text-sm text-muted-foreground"
          data-testid="agent-progress-loading"
        >
          Reading sessions…
        </p>
      ) : (
        <p
          className="px-3 py-6 text-center text-sm text-muted-foreground"
          data-testid="agent-progress-empty"
        >
          No sessions in what this read returned.
        </p>
      )}

      <AgentProgressFooter aggregate={aggregate} />
    </section>
  );
}
