import type { ManagedAgentRuntimeStatus } from "@/shared/api/types";
import { Badge } from "@/shared/ui/badge";
import { cn } from "@/shared/lib/cn";

import {
  agentCommunityAvailability,
  agentCommunityBadgeVariant,
  agentCommunityStatusDetail,
} from "../managedAgentRuntimeStatus";

/**
 * One agent+community pair's availability, as a pill and the sentence that
 * backs it.
 *
 * The pill says one of the words `agentCommunityAvailability` projects from
 * the host's status row — `Here`, `Idle`, `Stopped`, `Execution not
 * reported`, `Provider not heard from`, `Not running`, and the local-runtime
 * words — coloured by `agentCommunityBadgeVariant` so
 * the same evidence reads the same everywhere. The detail is the evidence
 * sentence itself. It is drawn on screen when `showDetail` is set and always
 * carried in `title`, so a hover answers "why that word" without the sentence
 * crowding a dense list.
 */
export function AgentCommunityStatusPill({
  className,
  runtime,
  showDetail = false,
  testId,
}: {
  className?: string;
  runtime: ManagedAgentRuntimeStatus;
  showDetail?: boolean;
  testId?: string;
}) {
  const label = agentCommunityAvailability(runtime);
  const detail = agentCommunityStatusDetail(runtime);
  return (
    <div className={cn("flex flex-col gap-1", className)}>
      <Badge
        className="w-fit normal-case tracking-normal"
        data-availability={label}
        data-execution={runtime.execution ?? undefined}
        data-provider-presence={runtime.providerPresence ?? undefined}
        data-testid={testId}
        title={detail ?? undefined}
        variant={agentCommunityBadgeVariant(runtime)}
      >
        {label}
      </Badge>
      {showDetail && detail ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid={testId ? `${testId}-detail` : undefined}
        >
          {detail}
        </p>
      ) : null}
    </div>
  );
}
