import * as React from "react";
import { CircleAlert, CircleHelp } from "lucide-react";

import {
  clearPendingCodingSessionTurns,
  forgetPendingCodingSessionTurn,
  formatPendingCodingSessionTurnAge,
  noteTextSettledCodingSessionEchoes,
  PENDING_CODING_SESSION_TURN_STALL_MS,
  pendingCodingSessionTurnKey,
  pendingCodingSessionTurnState,
  requestCodingSessionDraftRecovery,
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
  const unknown = state === "unknown";
  const caption = describePendingCodingSessionTurn(
    state,
    Math.max(0, now - turn.recordedAt),
    turn.deliveryUnknown,
    turn.degradedByProvider,
  );
  // The provider's terminal "I cannot say": the words stay on screen — they
  // may already be inside the running turn, so nothing here re-sends or
  // restores them on its own — and the person decides. Dismiss forgets the
  // row without touching the editor; Copy to draft does the opposite.
  const dismiss = React.useCallback(() => {
    forgetPendingCodingSessionTurn(turn.channelId, turn.commandId);
  }, [turn.channelId, turn.commandId]);
  // Recovery that costs nothing and claims nothing. The words the person
  // typed go back into the composer *beside* whatever they are typing now;
  // this row stays exactly where it is, still saying delivery is unknown,
  // because it still is. Nothing is published, dismissed, cancelled, or
  // marked delivered — an input the runtime may already hold must never be
  // re-sent on the client's initiative, and a button that quietly retired
  // the row would be doing the claiming the provider refused to do.
  const attachmentCount = turn.attachmentCount ?? 0;
  const copyToDraft = React.useCallback(() => {
    requestCodingSessionDraftRecovery({
      // Two presses of the same button on the same row are the same request.
      id: `recover:${turn.channelId}:${turn.commandId}`,
      channelId: turn.channelId,
      targetKey: turn.targetKey,
      // The draft is what they wrote; `text` is the wire form the umbrella
      // composer produced from it. Hand back the writing, not the wire.
      text: turn.draft ?? turn.text,
      attachmentCount,
    });
  }, [
    attachmentCount,
    turn.channelId,
    turn.commandId,
    turn.draft,
    turn.targetKey,
    turn.text,
  ]);
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
          stalled || unknown
            ? "ring-destructive/40"
            : "opacity-70 ring-border/40",
        )}
      >
        <Markdown content={turn.text.trim() || " "} mediaInset />
        {unknown ? (
          // Inside the bubble, not on the caption line: the plain workspace's
          // dock reserve (`pb-44`) is a constant, and at full scroll the last
          // row's caption sits under the dock's top edge — a control there
          // cannot be clicked. The bubble is always clear of the dock.
          <div className="mt-2 flex flex-col items-end gap-1">
            {/* On screen, not in a `title`: the one thing that decides
                whether copying these words is safe is whether they might
                already have arrived, and a tooltip saying so is invisible on
                touch and unannounced by most screen readers. */}
            <p className="text-2xs text-muted-foreground">
              Copying leaves this message where it is. It may already have
              reached the running turn.
              {attachmentCount > 0
                ? ` ${attachmentCount === 1 ? "Its image is" : `Its ${attachmentCount} images are`} not copied.`
                : ""}
            </p>
            <div className="flex items-center gap-2">
              <button
                className="rounded px-1 text-2xs font-medium text-foreground/75 underline-offset-2 hover:underline"
                data-testid="coding-session-pending-turn-copy-draft"
                onClick={copyToDraft}
                type="button"
              >
                Copy to draft
              </button>
              <button
                className="rounded px-1 text-2xs font-medium text-foreground/75 underline-offset-2 hover:underline"
                data-testid="coding-session-pending-turn-dismiss"
                onClick={dismiss}
                type="button"
              >
                Dismiss
              </button>
            </div>
          </div>
        ) : null}
      </div>
      {caption === null ? null : (
        <p
          className={cn(
            "inline-flex items-center gap-1 pe-1 text-2xs",
            stalled || unknown ? "text-destructive" : "text-muted-foreground",
          )}
          data-testid="coding-session-pending-turn-status"
        >
          {stalled ? <CircleAlert aria-hidden className="size-3" /> : null}
          {unknown ? <CircleHelp aria-hidden className="size-3" /> : null}
          {caption}
        </p>
      )}
    </div>
  );
}

/**
 * Why a turn the sender asked to deliver one way is waiting for the boundary
 * instead, in the provider's own terms.
 *
 * Two things were wrong with the single sentence this replaced, and both were
 * the kind of comfortable guess the product contract calls a bug.
 *
 * It said the turn had been **delivered** at the next boundary. It has not
 * been: `turn_degraded` is published beside the `turn_queued` that follows
 * it, and the turn is sitting in the provider's mailbox. "Delivered" is a
 * claim about something that has not happened yet.
 *
 * And it said **this provider cannot steer**, for every degrade, whatever the
 * provider actually reported. `STEER_TURN_ENDED` — the turn ended before the
 * input reached it, which says nothing about the runtime's capabilities — was
 * rendered as a permanent capability defect, sending the reader after a
 * problem that is not there. The code is on the receipt; this reads it.
 *
 * An unrecognized code falls back to the provider's own message rather than a
 * reason invented here: a newer provider may name a downgrade this build has
 * never heard of, and repeating what it said is the only honest option.
 */
export function describeCodingSessionTurnDegrade(
  degraded: { code: string; message: string } | undefined,
): string {
  const queued = "queued for the next turn boundary";
  switch (degraded?.code) {
    case "STEER_UNSUPPORTED":
      return `Not steered — this execution's runtime does not offer mid-turn steering; ${queued}`;
    case "STEER_TURN_ENDED":
      return `Not steered — the turn ended before this reached it; ${queued}`;
    case "STEER_REJECTED":
      return `Not steered — the runtime refused the mid-turn delivery; ${queued}`;
    case "STEER_ATTACHMENTS_UNSUPPORTED":
      return `Not steered — images cannot ride a mid-turn steer; ${queued}`;
    case "IMAGE_UNSUPPORTED":
      return `Images were dropped — this execution's runtime does not accept them; ${queued}`;
    default: {
      const detail = degraded?.message.trim() || degraded?.code.trim();
      return detail
        ? `Not delivered as asked — ${detail}; ${queued}`
        : `Not delivered as asked; ${queued}`;
    }
  }
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
 *
 * A held row no longer expires, so its age is part of the sentence once the
 * stall clock passes: "queued" without a number is fine at ten seconds and an
 * evasion at forty minutes. A degraded steer leads with the downgrade, because
 * that is the part the person did not ask for — their correction is not
 * reaching the turn that is running.
 *
 * Every held row also says it cannot be recalled, on screen rather than in a
 * `title`. "Queued by the provider" is the sentence the retired client-side
 * queue used, and that one had a Cancel button beside it; a tooltip saying
 * otherwise is invisible on touch and unannounced by most screen readers, so
 * the only people who learned the difference were the ones already using a
 * mouse.
 *
 * An injected steer says exactly that and nothing about waiting: the words
 * are already in the running turn. A delivery-unknown row leads with the
 * provider's own words, because the one thing the person needs is the reason
 * nobody can say whether their correction arrived.
 */
export function describePendingCodingSessionTurn(
  state: ReturnType<typeof pendingCodingSessionTurnState>,
  ageMs: number,
  deliveryUnknown?: { code: string; message: string },
  degradedByProvider?: { code: string; message: string },
): string | null {
  const stalled = ageMs > PENDING_CODING_SESSION_TURN_STALL_MS;
  const age = formatPendingCodingSessionTurnAge(ageMs);
  // Published, signed, and the provider's. There is no client-side cancel for
  // it and there is no 44220 that unsends one.
  const irrevocable = "it cannot be recalled";
  if (state === "unknown") {
    const detail = deliveryUnknown?.message.trim() || deliveryUnknown?.code;
    return detail
      ? `Delivery unknown — ${detail}`
      : "Delivery unknown — the provider could not confirm whether this reached the running turn";
  }
  if (state === "injected") {
    return "Injected into the running turn";
  }
  if (state === "degraded") {
    const degraded = describeCodingSessionTurnDegrade(degradedByProvider);
    return stalled
      ? `${degraded}; not started yet — ${age}; ${irrevocable}`
      : `${degraded}; ${irrevocable}`;
  }
  if (state === "queued") {
    return stalled
      ? `Queued by the provider, not started yet — ${age}; ${irrevocable}`
      : `Queued by the provider; ${irrevocable}`;
  }
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
