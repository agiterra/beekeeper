import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionProviderReachability } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { isTurnTerminal } from "@/features/coding-sessions/lib/codingSessionTranscriptModelPredicates";
import { isCodingSessionProsePiece } from "@/features/coding-sessions/lib/codingSessionTranscriptModelText";

/**
 * Rule 7 of `conformance/transcript-prose-join/CONTRACT.md` — whether a
 * message is still `arriving` — for the desktop reader. Pure.
 *
 * Two halves. {@link isCodingSessionProducerWriting} says whether the exact
 * target has not **ended**: no higher generation, no session-ending status
 * (`completed`, `stopped`, `disconnected`), and live lease evidence. The
 * lease half is the coordination fold's `provider_reachable`
 * (`shared/coordination/sessionCoordinationFold.ts`: current generation and
 * an unexpired `live` kind:24223 lease), read through
 * `useCodingSessionReachabilityResolver`. A reachability that is not known,
 * or known unreachable (lapsed, released, superseded, ambiguous), is no
 * evidence anyone is writing, so nothing is arriving. The fold's own
 * `terminal` omits `completed`, so the status is checked here too.
 *
 * {@link codingSessionArrivingPieceId} says which piece of one turn, if any,
 * that liveness applies to: the turn's last item, when it is prose and the
 * turn has no `result` or `interrupted` item.
 *
 * Never derived from `isWorking`: that is the workspace's reading of status
 * plus reachability for the working line, and an answer whose producer died
 * must stop reading "Writing…" on wire facts alone.
 */
export type CodingSessionProducerLivenessInput = {
  /** The exact generation's lease reading; `undefined` is not known. */
  reachability: CodingSessionProviderReachability | undefined;
  /** The generation's latest 44223 status, or `null` without metadata. */
  status: CodingSessionStatus | null;
  /** A newer generation of the same execution exists. */
  generationSuperseded: boolean;
};

/** Statuses that end the target (rule 7). `interrupted`/`failed` do not. */
const SESSION_ENDING_STATUSES: ReadonlySet<CodingSessionStatus> = new Set([
  "completed",
  "stopped",
  "disconnected",
]);

/** Whether the exact target has live evidence that its producer is writing. */
export function isCodingSessionProducerWriting(
  input: CodingSessionProducerLivenessInput,
): boolean {
  if (input.generationSuperseded) return false;
  if (input.status !== null && SESSION_ENDING_STATUSES.has(input.status)) {
    return false;
  }
  return input.reachability?.known === true && input.reachability.reachable;
}

/**
 * The id of the piece in `turnItems` that is still being written, or `null`.
 *
 * `turnItems` are one turn's items of one exact target, in `eventSeq` order
 * (the model's raw turn, before anything is hidden — a status after the
 * prose ends it). Items with no `turnId` are never arriving.
 */
export function codingSessionArrivingPieceId(
  turnItems: readonly TranscriptItem[],
  producerWriting: boolean,
): string | null {
  if (!producerWriting) return null;
  const last = turnItems.at(-1);
  if (!last?.turnId || !isCodingSessionProsePiece(last)) return null;
  if (turnItems.some(isTurnTerminal)) return null;
  return last.id;
}
