import * as React from "react";
import { skipToken, useQuery } from "@tanstack/react-query";

import { projectWorkQueryKey } from "@/features/coding-sessions/hooks/useProjectWork";
import { deriveCodingSessionAgentsOrchestration } from "@/features/coding-sessions/lib/codingSessionAgentsOrchestrationModel";
import type { ProjectWorkResponse } from "@/shared/api/tauriProjectWork";

import { CodingSessionAgentsOrchestrationSpawns } from "./CodingSessionAgentsOrchestrationSpawns";
import { CodingSessionAgentsWorkflowCard } from "./CodingSessionAgentsWorkflowCard";
import type { CodingSessionSurfaceCtx } from "./surfaces/codingSessionSurfaceContext";

/**
 * The 44249 work coverage, only when this view already read it.
 *
 * Observes the Mission surface's own query (`useProjectWork`, keyed by the
 * session) with `skipToken`, so it shares that one read and never starts
 * another. `null` means not read — never "no declared work".
 */
function useReadDeclaredWork(sessionRef: string | null) {
  const query = useQuery<ProjectWorkResponse>({
    queryKey: projectWorkQueryKey({ sessionRef: sessionRef ?? "" }),
    queryFn: skipToken,
  });
  return sessionRef === null
    ? null
    : (query.data?.coverage.declarations ?? null);
}

/**
 * The orchestration view at the top of the Agents surface (SV-40): one card
 * per mission, its phase pipeline in route order, expandable phases with each
 * agent's live activity, and the direct spawns.
 *
 * Built from `ctx` alone — the executions, transcripts and subagents every
 * client derives from relay events — so a teammate on another computer sees
 * the same cards. Asks nothing of any agent.
 */
export function CodingSessionAgentsOrchestration({
  ctx,
}: {
  ctx: CodingSessionSurfaceCtx;
}) {
  // Signed kind-44244 rows; `null` means not read, so assignments read "not read".
  const transactions = ctx.teamTransactions;
  const declaredWork = useReadDeclaredWork(ctx.umbrella.sessionRef);
  const model = React.useMemo(
    () =>
      deriveCodingSessionAgentsOrchestration({
        title: ctx.umbrella.title,
        missionKey: ctx.umbrella.umbrellaKey,
        executions: ctx.executions,
        subagents: ctx.subagents,
        transactions,
        declaredWork,
        decisions: ctx.decisions,
        resolveActorName: ctx.resolveActorName,
      }),
    [
      ctx.decisions,
      ctx.executions,
      ctx.resolveActorName,
      ctx.subagents,
      ctx.umbrella.title,
      ctx.umbrella.umbrellaKey,
      declaredWork,
      transactions,
    ],
  );
  if (model.cards.length === 0 && model.spawns.length === 0) return null;
  // A governed session can carry assignments and declared work; when this
  // view did not read them, say so rather than let a missing phase read as
  // "no such phase".
  const unread = [
    model.assignmentsRead ? null : "assignments",
    model.declaredWorkRead ? null : "declared work",
  ].filter((part): part is string => part !== null);
  const disclosure =
    model.cards.length > 0 &&
    ctx.umbrella.genesisRef !== null &&
    unread.length > 0
      ? `Phases come from the seats' signed roles; ${unread.join(" and ")} not read in this view.`
      : null;
  return (
    <div
      className="flex max-h-[60%] shrink-0 flex-col gap-2 overflow-y-auto overscroll-contain border-b border-border/60 p-2"
      data-testid="coding-session-agents-orchestration"
    >
      {model.cards.map((card) => (
        <CodingSessionAgentsWorkflowCard card={card} key={card.key} />
      ))}
      {disclosure ? (
        <p
          className="px-1.5 text-2xs text-muted-foreground"
          data-testid="coding-session-agents-orchestration-unread"
        >
          {disclosure}
        </p>
      ) : null}
      <CodingSessionAgentsOrchestrationSpawns spawns={model.spawns} />
    </div>
  );
}
