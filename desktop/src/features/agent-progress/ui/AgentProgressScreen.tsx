import type {
  AgentProgressAggregate,
  AgentProgressLane,
} from "../lib/agentProgressFold";
import { AgentProgressPanel } from "./AgentProgressPanel";

/**
 * The Agent Progress surface: every durable coding session this viewer can
 * read, across every project, as one live lane list.
 *
 * It is global by design and stays a route of its own rather than a tab inside
 * the coding-session host — that host is scoped to one session and already has
 * an Agents surface, while this screen answers the question that host cannot:
 * *what is happening across every session I can read?*
 *
 * The subtitle states the surface's exact reach and the exact basis of its
 * liveness claim. It says "coding sessions" because that is all this reads — a
 * header promising "your agents" over a list that structurally cannot contain
 * workflow runs or non-session agents would be the first lie on the screen —
 * and it names the lease, because "Reachable" is only as good as the evidence
 * behind it.
 */
export function AgentProgressScreen({
  state,
  onOpenLane,
}: {
  state: {
    lanes: readonly AgentProgressLane[];
    aggregate: AgentProgressAggregate;
    isLoading: boolean;
    complete: boolean;
    errors: readonly { scope: string; message: string }[];
    ambiguities: readonly { scope: string; message: string }[];
  };
  onOpenLane?: (lane: AgentProgressLane) => void;
}) {
  const { lanes, aggregate, isLoading, complete, errors, ambiguities } = state;
  return (
    <div className="flex h-full min-h-0 flex-col gap-3 p-4">
      <header>
        <h1 className="text-base font-medium text-foreground">
          Agent progress
        </h1>
        <p
          className="text-2xs text-muted-foreground"
          data-testid="agent-progress-subtitle"
        >
          Every coding session you can read. Reachable means a current signed
          lease says the provider is answering; everything else is what the
          session last reported, which is history.
        </p>
      </header>
      <AgentProgressPanel
        aggregate={aggregate}
        ambiguities={ambiguities}
        complete={complete}
        errors={errors}
        isLoading={isLoading}
        lanes={lanes}
        onOpenLane={onOpenLane}
      />
    </div>
  );
}
