import { useMutation, useQuery } from "@tanstack/react-query";

import {
  createCodingSessionLifecycleCommandId,
  publishCodingSessionRestart,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { buildCodingSessionResumeInput } from "@/features/coding-sessions/lib/codingSessionResumeSeat";
import { codingSessionResumeSeatDeps } from "@/features/coding-sessions/lib/codingSessionResumeSeatDeps";
import { publishSeatedCodingSessionResume } from "@/features/coding-sessions/lib/codingSessionSeatedCreate";
import { listCodingSessionSeatWorktrees } from "@/shared/api/tauriCodingSessionWorktrees";
import { seatDefinitionDrift } from "@/shared/api/tauriRolePacks";
import type { SeatDefinitionDrift } from "@/shared/api/types";
import { Button } from "@/shared/ui/button";

import {
  liveStateOf,
  type ProjectAgentSession,
} from "../lib/projectAgentsModel";
import {
  DEFINITION_CHANGED,
  DEFINITION_CHECKING,
  DEFINITION_CURRENT_MOVED,
  DEFINITION_UNKNOWN,
  RESTART_REQUESTED,
  RESTART_WAIT_FOR_TURN,
  RESTART_WITH_CURRENT_DEFINITION,
  definitionDriftCause,
} from "./projectAgentsCopy";

/**
 * Spec § 4.9, second half: does this running seat still run the definition
 * this computer would stage for it now, and if not, one button that restarts
 * it on the current one.
 *
 * Every word here is what the host answered. `unknown` renders as unknown
 * with the host's reason — never as current — and nothing restarts without
 * the click. The button is disabled while the seat is mid-turn because the
 * provider refuses a restart then (`SESSION_BUSY`), and saying so beats
 * publishing a command that will be refused.
 */
export function ProjectAgentSessionDefinition({
  agentPubkey,
  projectRef,
  session,
}: {
  agentPubkey: string;
  projectRef: string;
  session: ProjectAgentSession;
}) {
  const role = session.role;
  const seatSha = session.packRef?.sha ?? null;
  const target = session.commandTarget;
  const enabled =
    !session.sessionClosed &&
    role !== null &&
    seatSha !== null &&
    target !== null;

  const drift = useQuery({
    queryKey: [
      "seat-definition-drift",
      projectRef,
      role,
      seatSha,
      session.sessionRef,
      target?.sessionId ?? null,
    ],
    enabled,
    staleTime: 60_000,
    queryFn: async (): Promise<{
      drift: SeatDefinitionDrift;
      worktree: string | null;
    }> => {
      if (!role || !seatSha || !target) throw new Error("no seat to compare");
      const worktree = await seatWorktreePath(
        session.sessionRef,
        target.sessionId,
      );
      const result = await seatDefinitionDrift({
        projectRef,
        role,
        seatSha,
        worktree,
      });
      return { drift: result, worktree };
    },
  });

  const restart = useMutation({
    mutationFn: async () => {
      if (!role || !target || !session.providerAuthorityPubkey) {
        throw new Error(
          "This execution cannot be addressed for a restart yet.",
        );
      }
      const commandId = createCodingSessionLifecycleCommandId();
      const providerAuthorityPubkey = session.providerAuthorityPubkey;
      await publishSeatedCodingSessionResume(
        buildCodingSessionResumeInput({
          commandId,
          seat: {
            actorPubkey: agentPubkey,
            role,
            projectRef,
            worktree: drift.data?.worktree ?? null,
          },
          deps: codingSessionResumeSeatDeps,
          publish: () =>
            publishCodingSessionRestart({
              channelId: session.channelId,
              commandId,
              target,
              providerAuthorityPubkey,
            }),
        }),
      );
      return commandId;
    },
  });

  if (!enabled) return null;
  if (drift.isPending) {
    return (
      <p
        className="text-2xs text-muted-foreground/80"
        data-testid="project-agent-session-definition"
        data-state="checking"
      >
        {DEFINITION_CHECKING}
      </p>
    );
  }
  if (drift.isError) {
    return (
      <p
        className="text-2xs text-muted-foreground/80"
        data-testid="project-agent-session-definition"
        data-state="unknown"
      >
        {DEFINITION_UNKNOWN}:{" "}
        {drift.error instanceof Error
          ? drift.error.message
          : String(drift.error)}
      </p>
    );
  }
  const answer = drift.data.drift;
  // The provider refuses a restart while a turn is open (`SESSION_BUSY`);
  // "working" is the model's word for exactly those statuses.
  const busy = liveStateOf(session.status) === "working";
  const canRestart =
    target !== null && session.providerAuthorityPubkey !== null;

  if (answer.state === "unknown") {
    return (
      <p
        className="text-2xs text-muted-foreground/80"
        data-testid="project-agent-session-definition"
        data-state="unknown"
      >
        {DEFINITION_UNKNOWN}: {answer.reason ?? ""}
      </p>
    );
  }
  if (answer.state === "current") {
    if (answer.cause.length === 0 && answer.warnings.length === 0) return null;
    return (
      <p
        className="text-2xs text-muted-foreground/80"
        data-testid="project-agent-session-definition"
        data-state="current"
      >
        {answer.cause.length > 0
          ? `${DEFINITION_CURRENT_MOVED}: ${answer.cause}`
          : null}
        {answer.warnings.map((warning) => (
          <span className="block" key={warning}>
            {warning}
          </span>
        ))}
      </p>
    );
  }
  return (
    <div
      className="flex flex-col gap-1 rounded-md border border-amber-500/40 bg-amber-500/5 px-2 py-1.5"
      data-testid="project-agent-session-definition"
      data-state="changed"
      role="note"
    >
      <p className="text-xs text-foreground">
        <span className="font-medium">{DEFINITION_CHANGED}</span>
        {": "}
        {definitionDriftCause(answer)}
      </p>
      {answer.warnings.map((warning) => (
        <p className="text-2xs text-muted-foreground" key={warning}>
          {warning}
        </p>
      ))}
      {restart.isSuccess ? (
        <p
          className="text-2xs text-muted-foreground"
          data-testid="project-agent-restart-requested"
        >
          {RESTART_REQUESTED}
        </p>
      ) : (
        <div className="flex flex-wrap items-center gap-2">
          <Button
            className="h-6 px-2 text-xs"
            data-testid="project-agent-restart-definition"
            disabled={busy || !canRestart || restart.isPending}
            onClick={() => restart.mutate()}
            size="sm"
            title={busy ? RESTART_WAIT_FOR_TURN : undefined}
            type="button"
            variant="outline"
          >
            {RESTART_WITH_CURRENT_DEFINITION}
          </Button>
          {busy ? (
            <span className="text-2xs text-muted-foreground">
              {RESTART_WAIT_FOR_TURN}
            </span>
          ) : null}
        </div>
      )}
      {restart.isError ? (
        <p
          className="text-2xs text-destructive"
          data-testid="project-agent-restart-error"
          role="alert"
        >
          {restart.error instanceof Error
            ? restart.error.message
            : String(restart.error)}
        </p>
      ) : null}
    </div>
  );
}

/**
 * The seat's own worktree, when this host recorded one for its execution.
 * Joined by the provider session id the tree was cut for; a tree cut
 * before the execution had one, or any error, is `null` — "not known", so
 * the drift check compares against `main`.
 */
async function seatWorktreePath(
  sessionRef: string | null,
  sessionId: string,
): Promise<string | null> {
  if (!sessionRef) return null;
  try {
    const rows = await listCodingSessionSeatWorktrees([
      {
        sessionRef,
        sessionSettled: false,
        executionLive: true,
        tipOnRelay: null,
        settledForSecs: null,
      },
    ]);
    return rows.find((row) => row.sessionId === sessionId)?.path ?? null;
  } catch {
    return null;
  }
}
