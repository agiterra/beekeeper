import { useGiveAgentItsOwnNestMutation } from "@/features/agents/hooks";
import type { ManagedAgent } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";

/**
 * The one-click remedy beside the "shared home — packs refused" badge.
 *
 * Renders for exactly the agents the host says are refused
 * (`packRefusedSharedHome === true`) — never on an unanswered field, and never
 * on an agent whose pack is landing fine. It states what it will do and what
 * it will not: the working directory of a process that is already running
 * cannot be changed from outside it, so the move takes effect at the agent's
 * next start.
 */
export function AgentSharedHomeAction({
  agent,
}: {
  agent: ManagedAgent | null | undefined;
}) {
  const mutation = useGiveAgentItsOwnNestMutation();
  if (agent?.packRefusedSharedHome !== true) return null;
  const pubkey = agent.pubkey;
  return (
    <div className="flex flex-col items-center gap-1">
      <Button
        data-testid="agent-give-own-nest"
        disabled={mutation.isPending}
        onClick={() => mutation.mutate({ pubkey })}
        size="sm"
        variant="outline"
      >
        {mutation.isPending ? "Giving it a nest…" : "Give it its own nest"}
      </Button>
      <p className="text-2xs text-muted-foreground">
        Takes effect the next time this agent starts.
      </p>
      {mutation.isError ? (
        <p className="text-2xs text-destructive" data-testid="agent-nest-error">
          {mutation.error instanceof Error
            ? mutation.error.message
            : String(mutation.error)}
        </p>
      ) : null}
    </div>
  );
}
