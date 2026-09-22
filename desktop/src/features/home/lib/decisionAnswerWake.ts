import {
  buildCodingSessionTargetKey,
  type CodingSessionCommandTarget,
  publishCodingSessionCommand,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionTeamWakeCommandId } from "@/features/coding-sessions/lib/codingSessionTeamWake";
import type { CodingSessionCatalogRecord } from "@/features/coding-sessions/lib/codingSessionTypes";

/**
 * Ledger 249(A). An answer nobody is told about never lands: `bee sessions
 * decide answer` wakes the asker by default, and so does the Inbox card.
 * This is the same wake — a kind:44220 `thread.turn.start` at a boundary,
 * whose text names the stored operation and nothing else, so the seat
 * fetches and verifies the signed 44244 rather than trusting prose
 * (`crates/buzz-cli/src/commands/sessions/crew_cmds.rs`
 * `send_team_operation_wake`).
 */

/** The asker's execution in this session, highest generation, or null. */
export function resolveAskerTarget(
  entries: readonly CodingSessionCatalogRecord[],
  sessionRef: string,
  askerPubkey: string,
): { target: CodingSessionCommandTarget; role: string | null } | null {
  let best: CodingSessionCatalogRecord | null = null;
  for (const entry of entries) {
    if (entry.sessionRef !== sessionRef) continue;
    if (entry.agentRef !== askerPubkey) continue;
    if (entry.commandTarget === null) continue;
    if (
      best === null ||
      (best.commandTarget?.generation ?? 0) < entry.commandTarget.generation
    ) {
      best = entry;
    }
  }
  return best?.commandTarget
    ? { target: best.commandTarget, role: best.role }
    : null;
}

/** The operation pointer the CLI's wake carries, byte for byte. */
export function decisionAnswerWakeText(answerEventId: string): string {
  return JSON.stringify({
    operationId: answerEventId,
    type: "decision.answer",
  });
}

/** What happened to the wake, in words the card prints. */
export type DecisionAnswerWake =
  | { status: "sent"; commandId: string; eventId: string; seat: string }
  | { status: "no-seat"; message: string }
  | { status: "failed"; message: string };

/**
 * Publish the wake for one stored answer. Never throws: the answer is
 * already on the wire, so a wake failure is reported beside it rather than
 * dressed up as a refused answer.
 */
export async function wakeAskerForAnswer(input: {
  channelRef: string;
  answerEventId: string;
  asker: { target: CodingSessionCommandTarget; role: string | null } | null;
  askerLabel: string;
  publish?: typeof publishCodingSessionCommand;
}): Promise<DecisionAnswerWake> {
  if (input.asker === null) {
    return {
      status: "no-seat",
      message: `${input.askerLabel} holds no running seat in this session that this computer can see, so nobody was woken. Tell it, or grant it a seat.`,
    };
  }
  const publish = input.publish ?? publishCodingSessionCommand;
  try {
    const commandId = await buildCodingSessionTeamWakeCommandId(
      {
        sourceEventId: input.answerEventId,
        sourceCreatedAtMs: 0,
        sourceEventSeq: null,
        sourceTargetKey: null,
        sourceActorPubkey: null,
        kind: "operation_ready",
        operationType: null,
        seatRole: input.asker.role ?? "",
        causedByCommandId: null,
        preferredCommandId: null,
      },
      input.asker.target,
    );
    const published = await publish({
      channelId: input.channelRef,
      commandId,
      target: input.asker.target,
      text: decisionAnswerWakeText(input.answerEventId),
      deliver: "boundary",
    });
    return {
      status: "sent",
      commandId,
      eventId: published.eventId,
      seat: input.asker.role ?? buildCodingSessionTargetKey(input.asker.target),
    };
  } catch (error) {
    return {
      status: "failed",
      message: `The answer was published, but the wake to ${input.askerLabel} was refused: ${
        error instanceof Error ? error.message : String(error)
      }`,
    };
  }
}
