/**
 * The prompts you have already sent in a session, newest first.
 *
 * Recall exists so a prompt can be re-sent or edited rather than retyped, and
 * that only holds if the list is *yours*: a coding session is multi-operator,
 * so a founder and a granted operator both steer the same execution. Walking
 * back into someone else's instruction and pressing Enter would put words in
 * their mouth, so attributed turns from other operators are dropped.
 *
 * Unattributed items — published before the provider stamped attribution — are
 * kept. They predate the multi-operator surface, so in practice they are the
 * viewer's own, and dropping them would empty the history of every older
 * session.
 *
 * Pure so the ordering and de-duplication can be unit tested.
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";

export type PromptHistorySource = {
  /** Settled transcript items, oldest first as the timeline renders them. */
  transcript: readonly TranscriptItem[];
  /**
   * Turns published but not yet echoed by the provider. Carries the draft as
   * typed, before @handle stripping — recalling the wire text would hand back
   * something the person never wrote.
   */
  pending: readonly {
    draft?: string;
    text: string;
    operatorPubkey?: string | null;
  }[];
  /** Lowercased hex pubkey of the viewer, when known. */
  currentPubkey?: string | null;
};

/** Ceiling on remembered prompts; recall is for the recent past, not an archive. */
export const MAX_PROMPT_HISTORY = 100;

function isOwnOperator(
  operatorPubkey: string | null | undefined,
  currentPubkey: string | null | undefined,
): boolean {
  if (!operatorPubkey) return true;
  if (!currentPubkey) return false;
  return operatorPubkey.toLowerCase() === currentPubkey.toLowerCase();
}

export function buildCodingSessionPromptHistory({
  transcript,
  pending,
  currentPubkey,
}: PromptHistorySource): string[] {
  const ordered: string[] = [];

  for (const item of transcript) {
    if (item.type !== "message" || item.role !== "user") continue;
    if (!isOwnOperator(item.operatorPubkey, currentPubkey)) continue;
    const text = item.text.trim();
    if (text.length > 0) ordered.push(text);
  }

  for (const turn of pending) {
    if (!isOwnOperator(turn.operatorPubkey, currentPubkey)) continue;
    const text = (turn.draft ?? turn.text).trim();
    if (text.length > 0) ordered.push(text);
  }

  // Newest first, and collapse repeats: pressing ⌘↑ three times through the
  // same instruction sent three times is walking on the spot.
  const history: string[] = [];
  for (let index = ordered.length - 1; index >= 0; index -= 1) {
    const text = ordered[index];
    if (history[history.length - 1] === text) continue;
    history.push(text);
    if (history.length >= MAX_PROMPT_HISTORY) break;
  }
  return history;
}

export type PromptRecallState = {
  /** Position in the history; -1 means "not recalling". */
  cursor: number;
  /** What was in the composer before recall started, restored on the way back. */
  stashedDraft: string;
};

export const IDLE_PROMPT_RECALL: PromptRecallState = Object.freeze({
  cursor: -1,
  stashedDraft: "",
});

export type PromptRecallStep = {
  state: PromptRecallState;
  /** Text the composer should show, or null to leave it alone. */
  text: string | null;
};

/**
 * Advance recall one step.
 *
 * `older` walks back and stops at the oldest prompt rather than wrapping —
 * wrapping to the newest looks like the list restarting and loses the person's
 * place. `newer` walks forward and, one step past the newest, hands back the
 * draft that was interrupted.
 */
export function stepPromptRecall(
  direction: "older" | "newer",
  state: PromptRecallState,
  history: readonly string[],
  currentText: string,
): PromptRecallStep {
  if (history.length === 0) return { state, text: null };

  if (direction === "older") {
    if (state.cursor === -1) {
      return {
        state: { cursor: 0, stashedDraft: currentText },
        text: history[0],
      };
    }
    const next = Math.min(state.cursor + 1, history.length - 1);
    if (next === state.cursor) return { state, text: null };
    return { state: { ...state, cursor: next }, text: history[next] };
  }

  if (state.cursor === -1) return { state, text: null };
  if (state.cursor === 0) {
    return { state: IDLE_PROMPT_RECALL, text: state.stashedDraft };
  }
  const next = state.cursor - 1;
  return { state: { ...state, cursor: next }, text: history[next] };
}
