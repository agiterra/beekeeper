import * as React from "react";
import { CircleAlert } from "lucide-react";

import {
  clearPendingCodingSessionTurns,
  noteTextSettledCodingSessionEchoes,
  pendingCodingSessionTurnKey,
  pendingCodingSessionTurnState,
  resolvePendingCodingSessionTurns,
  usePendingCodingSessionTurns,
  type PendingCodingSessionTurn,
  type PendingCodingSessionTurnEcho,
} from "@/features/coding-sessions/lib/codingSessionPendingTurns";
import { Markdown } from "@/shared/ui/markdown";
import { cn } from "@/shared/lib/cn";

/** How often a visible pending row re-checks how long it has been waiting. */
const PENDING_TURN_POLL_MS = 2_000;

/**
 * The turns this client has sent to one execution and not yet seen echoed.
 *
 * Resolution is separated from rendering because a surface has to know whether
 * any row survives *before* it decides what else to draw — a transcript that
 * says "No conversation yet" above the message you just sent is worse than the
 * delay this whole thing exists to remove.
 */
export function useVisibleCodingSessionPendingTurns({
  channelId,
  echoes,
  targetKey,
}: {
  channelId: string;
  /**
   * The projected transcript items for this execution. A verified `user_prompt`
   * in here is what retires the matching row.
   */
  echoes: readonly PendingCodingSessionTurnEcho[];
  targetKey: string | null;
}): { turns: readonly PendingCodingSessionTurn[]; now: number } {
  const pending = usePendingCodingSessionTurns();
  const now = useCodingSessionPendingClock(pending.length > 0);
  const resolved = React.useMemo(
    () =>
      targetKey === null
        ? { visible: [], consumedKeys: [], settlements: [] }
        : resolvePendingCodingSessionTurns(
            pending,
            { channelId, targetKey },
            echoes,
            now,
          ),
    [channelId, echoes, now, pending, targetKey],
  );
  // Resolution is pure; retiring the store rows is the effect it implies.
  const consumedKeys = resolved.consumedKeys;
  React.useEffect(() => {
    clearPendingCodingSessionTurns(consumedKeys);
  }, [consumedKeys]);
  // So is remembering which of them were only guessed at from their words.
  const settlements = resolved.settlements;
  React.useEffect(() => {
    const echoIds = settlements
      .filter((settlement) => settlement.by === "text")
      .map((settlement) => settlement.echoId)
      .filter((echoId): echoId is string => echoId !== undefined);
    noteTextSettledCodingSessionEchoes(echoIds);
  }, [settlements]);
  return { turns: resolved.visible, now };
}

/**
 * Rendered *below* the signed transcript, never inside it: these rows are this
 * machine's own claim about what it published, and the transcript is the
 * provider's. They mirror the prompt bubble's shape so the conversation reads
 * continuously, but say nothing about normal delivery: the message is what the
 * person just sent, while the session's own status announces work beginning.
 */
export function CodingSessionPendingTurnList({
  now,
  targetLabel = null,
  turns,
}: {
  now: number;
  /** Names the execution when a surface shows more than one. */
  targetLabel?: string | null;
  turns: readonly PendingCodingSessionTurn[];
}) {
  if (turns.length === 0) return null;
  return (
    <div
      className="flex flex-col gap-5"
      data-testid="coding-session-pending-turns"
    >
      {turns.map((turn) => (
        <CodingSessionPendingTurnRow
          key={pendingCodingSessionTurnKey(turn)}
          now={now}
          targetLabel={targetLabel}
          turn={turn}
        />
      ))}
    </div>
  );
}

/** Resolve and render in one step, for surfaces with nothing to gate on it. */
export function CodingSessionPendingTurns({
  channelId,
  echoes,
  targetKey,
  targetLabel = null,
}: {
  channelId: string;
  echoes: readonly PendingCodingSessionTurnEcho[];
  targetKey: string | null;
  targetLabel?: string | null;
}) {
  const { now, turns } = useVisibleCodingSessionPendingTurns({
    channelId,
    echoes,
    targetKey,
  });
  return (
    <CodingSessionPendingTurnList
      now={now}
      targetLabel={targetLabel}
      turns={turns}
    />
  );
}

function CodingSessionPendingTurnRow({
  now,
  targetLabel,
  turn,
}: {
  now: number;
  targetLabel: string | null;
  turn: PendingCodingSessionTurn;
}) {
  const state = pendingCodingSessionTurnState(turn, now);
  const stalled = state === "stalled";
  const caption = describePendingCodingSessionTurn(state);
  return (
    <div
      className="group flex flex-col items-end gap-1"
      data-pending-state={state}
      data-role="user-message"
      data-target-label={targetLabel ?? undefined}
      data-testid="coding-session-pending-turn"
    >
      <div
        className={cn(
          "min-w-0 max-w-[80%] rounded-2xl bg-muted px-4 py-3 text-base leading-6 text-foreground shadow-sm ring-1",
          stalled ? "ring-destructive/40" : "opacity-70 ring-border/40",
        )}
      >
        <Markdown content={turn.text.trim() || " "} mediaInset />
      </div>
      {caption === null ? null : (
        <p
          className={cn(
            "inline-flex items-center gap-1 pe-1 text-2xs",
            stalled ? "text-destructive" : "text-muted-foreground",
          )}
          data-testid="coding-session-pending-turn-status"
        >
          {stalled ? <CircleAlert aria-hidden className="size-3" /> : null}
          {caption}
        </p>
      )}
    </div>
  );
}

/**
 * What a row in flight says about itself — usually nothing.
 *
 * The first version spun a loader and narrated the relay/provider boundary.
 * Both were accurate infrastructure details and the wrong frame for the
 * conversation. Silence stops only once it would become a lie: a published
 * turn that nobody has picked up long after they should have, or one the
 * provider has signed for and parked behind other work. "Queued" is stated
 * plainly and not dressed up as progress — the turn has not started.
 */
export function describePendingCodingSessionTurn(
  state: ReturnType<typeof pendingCodingSessionTurnState>,
): string | null {
  if (state === "queued") return "Queued by the provider";
  return state === "stalled" ? "Not picked up yet" : null;
}

/**
 * A coarse clock that runs only while rows are on screen. The rows change what
 * they claim as time passes, and nothing else in the tree re-renders to tell
 * them; polling stops the moment the last row is retired.
 */
function useCodingSessionPendingClock(active: boolean): number {
  const [now, setNow] = React.useState(() => Date.now());
  React.useEffect(() => {
    if (!active) return;
    setNow(Date.now());
    const timer = window.setInterval(
      () => setNow(Date.now()),
      PENDING_TURN_POLL_MS,
    );
    return () => window.clearInterval(timer);
  }, [active]);
  return now;
}
