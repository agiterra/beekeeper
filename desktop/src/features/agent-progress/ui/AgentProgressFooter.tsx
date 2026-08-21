import type { AgentProgressAggregate } from "../lib/agentProgressFold";
import {
  agentProgressExecutionsText,
  agentProgressFooterText,
} from "../lib/agentProgressFormat";

/**
 * The compact aggregate line.
 *
 * Counts are durable sessions — one coding session each — so a session with
 * three provider executions is one session, not three. The execution count is
 * disclosed separately, and only when it differs, rather than being folded
 * into the headline number.
 *
 * When the read did not complete the line reads **at least N**. That is the
 * whole point of the flag: a partial read that prints a bare number has
 * silently promoted a floor into a census.
 *
 * There is deliberately no `Σ tok` here: nothing this surface reads carries
 * usage, and a total assembled from nothing would read as measured.
 */
export function AgentProgressFooter({
  aggregate,
}: {
  aggregate: AgentProgressAggregate;
}) {
  const executions = agentProgressExecutionsText(aggregate);
  return (
    <div
      className="flex items-center gap-2 border-t border-border/60 px-3 py-2 text-2xs text-muted-foreground"
      data-testid="agent-progress-footer"
    >
      <span data-testid="agent-progress-footer-counts">
        {agentProgressFooterText(aggregate)}
      </span>
      {executions ? (
        <span
          className="ml-auto"
          data-testid="agent-progress-footer-executions"
          title="Distinct provider executions collapsed into these session rows."
        >
          {executions}
        </span>
      ) : null}
    </div>
  );
}
