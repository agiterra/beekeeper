/**
 * Adapt the local coding-session shelf into per-session presentation detail.
 *
 * The shelf is an *enrichment*, not the spine. It has already collapsed
 * provider executions sharing a `sessionRef` into one durable row, derived a
 * label, and carried the transcript this client happens to hold — all useful,
 * none of it evidence of liveness. The lanes themselves come from the signed
 * coordination read; this file only tells each lane what it can be called and
 * what it was last seen doing.
 *
 * The join is on `sessionRef` and nothing else. A shelf row with no
 * `sessionRef` is a pre-umbrella execution the coordination fold keys as
 * `implicit:<executionKey>`; matching those by label or channel would attach a
 * transcript to a session it does not belong to, so they are left unjoined and
 * their lanes simply have no activity line.
 */
import type {
  AgentProgressLocalDetail,
  AgentProgressTranscriptItem,
} from "./agentProgressFold";

/** Structural shelf seam supplied by the app composition root. */
export type AgentProgressShelfEntry = {
  sessionRef: string | null;
  label: string | null;
  runtimeLabel: string | null;
  channelId: string;
  generationId: string;
  session: { transcript: readonly AgentProgressTranscriptItem[] };
};

export function agentProgressDetailBySessionRef(
  entries: readonly AgentProgressShelfEntry[],
): Map<string, AgentProgressLocalDetail> {
  const details = new Map<string, AgentProgressLocalDetail>();
  for (const entry of entries) {
    if (!entry.sessionRef) continue;
    const incumbent = details.get(entry.sessionRef);
    // One durable session can surface on the shelf more than once when its
    // executions live in different channels. Keep the row with the most
    // transcript, so the activity line comes from the execution this client
    // actually observed rather than from whichever row was enumerated first.
    if (
      incumbent &&
      incumbent.transcript.length >= entry.session.transcript.length
    ) {
      continue;
    }
    details.set(entry.sessionRef, {
      sessionRef: entry.sessionRef,
      label: entry.label,
      runtimeLabel: entry.runtimeLabel,
      transcript: entry.session.transcript,
      openTarget: {
        channelId: entry.channelId,
        generationId: entry.generationId,
      },
    });
  }
  return details;
}
