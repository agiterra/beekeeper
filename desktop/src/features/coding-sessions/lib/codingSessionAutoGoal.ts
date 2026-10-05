import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  type CodingSessionNamingSettings,
  generateCodingSessionGoal,
  getCodingSessionNamingSettings,
  namingModelConsulted,
} from "@/shared/api/tauriCodingSessionNaming";
import {
  buildCodingSessionGoalFilter,
  foldLatestCodingSessionGoalsByFounder,
  publishCodingSessionGoal,
} from "./codingSessionGoal";
import { codingSessionGoalKey } from "../useCodingSessionGoals";

/**
 * Turn a Solo session's goal from its whole first message into one line.
 *
 * The founded page publishes the initial prompt as the 44227 goal, which is
 * honest before Start (it is what the session is for) and redundant after
 * it: the transcript shows the same text as the first turn, and a long
 * prompt above a transcript is the largest object on the screen. For a Solo
 * session the goal's job is to remind the person what the session is for,
 * in a little more detail than the name — so, when this computer's
 * session titles are on "Use my naming model" with an endpoint chosen
 * (Settings → Coding sessions; `namingModelConsulted`), the goal is
 * rewritten as one sentence once the create is accepted (Andy, 2026-09-14).
 * In the agent mode (the default) and Off, no naming model is consulted and
 * the goal stays the prompt as typed (D9, SV-56). A Team session's goal is
 * the lead's mission and is left as written.
 *
 * Honesty rules: a prompt that already fits one line is left alone; the
 * wire is read right before publishing and nothing is published unless the
 * goal there is still the prompt this Start sent (a goal typed meanwhile, on
 * the phone or in the header, wins); no namer, an empty answer or a refusal
 * end as stated outcomes, and the full prompt stays as the goal.
 */
export type CodingSessionAutoGoalOutcome =
  | { kind: "off" }
  | { kind: "already-short" }
  | { kind: "empty" }
  | { kind: "changed-meanwhile" }
  | { kind: "published"; goal: string }
  | { kind: "failed"; reason: string };

export type CodingSessionAutoGoalInput = {
  channelId: string;
  sessionRef: string;
  founderPubkey: string;
  firstMessage: string;
};

export type CodingSessionAutoGoalDeps = {
  getSettings: () => Promise<CodingSessionNamingSettings | null>;
  generate: (firstMessage: string) => Promise<string>;
  readGoals: (filter: RelaySubscriptionFilter) => Promise<RelayEvent[]>;
  publishGoal: (input: {
    channelId: string;
    content: string;
    sessionRef: string;
  }) => Promise<unknown>;
};

const GOAL_HISTORY_LIMIT = 1000;

/** A prompt at or under this, on one line, is its own summary. */
export const MAX_ONE_LINE_GOAL_CHARS = 140;

const DEFAULT_DEPS: CodingSessionAutoGoalDeps = {
  getSettings: () => getCodingSessionNamingSettings().catch(() => null),
  generate: generateCodingSessionGoal,
  readGoals: (filter) => relayClient.fetchEventsCoalesced(filter),
  publishGoal: publishCodingSessionGoal,
};

/** Whether a first message is longer than one line's worth of goal. */
export function codingSessionGoalNeedsSummary(firstMessage: string): boolean {
  const trimmed = firstMessage.trim();
  return trimmed.includes("\n") || trimmed.length > MAX_ONE_LINE_GOAL_CHARS;
}

/** Summarize the goal and publish it, unless the wire moved meanwhile. */
export async function autoSummarizeCodingSessionGoal(
  input: CodingSessionAutoGoalInput,
  deps: CodingSessionAutoGoalDeps = DEFAULT_DEPS,
): Promise<CodingSessionAutoGoalOutcome> {
  // No host (settings null) is no naming model, not an error.
  const settings = await deps.getSettings();
  if (!namingModelConsulted(settings)) return { kind: "off" };
  const firstMessage = input.firstMessage.trim();
  if (!codingSessionGoalNeedsSummary(firstMessage)) {
    return { kind: "already-short" };
  }
  let goal: string;
  try {
    goal = (await deps.generate(firstMessage)).trim();
  } catch (error) {
    return {
      kind: "failed",
      reason: error instanceof Error ? error.message : String(error),
    };
  }
  if (goal.length === 0) return { kind: "empty" };
  try {
    const history = await deps.readGoals({
      ...buildCodingSessionGoalFilter([input.channelId], GOAL_HISTORY_LIMIT),
      "#d": [input.sessionRef],
      authors: [input.founderPubkey.toLowerCase()],
    });
    const onWire = foldLatestCodingSessionGoalsByFounder(history).get(
      codingSessionGoalKey(
        input.channelId,
        input.sessionRef,
        input.founderPubkey,
      ),
    );
    // Only the prompt this Start sent is ours to rewrite. No goal on the wire
    // means the pre-Start publish was refused; leaving that alone is the
    // honest choice, not inventing a goal the session never had.
    if (!onWire || onWire.content.trim() !== firstMessage) {
      return { kind: "changed-meanwhile" };
    }
    await deps.publishGoal({
      channelId: input.channelId,
      content: goal,
      sessionRef: input.sessionRef,
    });
    return { kind: "published", goal };
  } catch (error) {
    return {
      kind: "failed",
      reason: error instanceof Error ? error.message : String(error),
    };
  }
}

/** What the prompt field says a Solo Start will do to the goal, or null. */
export function codingSessionAutoGoalSentence(
  settings: CodingSessionNamingSettings | null,
): string | null {
  if (settings === null || !namingModelConsulted(settings)) return null;
  const model = settings.model.trim();
  return model.length > 0
    ? `After Start, the goal shown with the session is a one-line summary of this prompt (${model}); the prompt itself is the first message.`
    : "After Start, the goal shown with the session is a one-line summary of this prompt; the prompt itself is the first message.";
}
