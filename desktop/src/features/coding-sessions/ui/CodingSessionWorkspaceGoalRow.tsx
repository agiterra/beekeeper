import type { CodingSessionGoal } from "@/features/coding-sessions/lib/codingSessionGoal";

import { CodingSessionFounderLine } from "./CodingSessionFounderLine";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";

/**
 * True when the goal says nothing the title above it does not.
 *
 * A Solo session is often named and goaled from the same first message, and
 * the same sentence twice — once as the title, once in a pill under it — was
 * the clutter this row removes. Compared loosely (case, spacing, a trailing
 * full stop) because those are the differences a reader would not notice.
 */
export function codingSessionGoalRestatesTitle(
  goal: string | null | undefined,
  title: string | null | undefined,
): boolean {
  if (!goal || !title) return false;
  const normalize = (value: string) =>
    value
      .trim()
      .replace(/\s+/g, " ")
      .replace(/[.!…]+$/u, "")
      .toLowerCase();
  const normalizedGoal = normalize(goal);
  return normalizedGoal.length > 0 && normalizedGoal === normalize(title);
}

/**
 * The single-session workspace's one quiet row under the header: the goal,
 * then who founded the session.
 *
 * It replaces two rows — a "Founded by" bar and a bordered goal pill. Nothing
 * left: a goal that only restates the title is withheld (the title is on
 * screen), but the founder keeps the edit control; and when the row has
 * nothing to say at all, the founder is still one click away in the header's
 * provenance popover, which names them from the same signed facts.
 */
export function CodingSessionWorkspaceGoalRow({
  channelId,
  currentUserPubkey,
  founderPubkey,
  genesisRef,
  goal,
  sessionRef,
  title,
  workspaceExpanded,
}: {
  channelId: string;
  currentUserPubkey: string | null;
  founderPubkey: string | null;
  genesisRef: string | null;
  goal: CodingSessionGoal | null;
  sessionRef: string | null;
  title: string | null;
  workspaceExpanded: boolean;
}) {
  return (
    <CodingSessionGoalPill
      channelId={channelId}
      currentUserPubkey={currentUserPubkey}
      founderPubkey={founderPubkey}
      goal={goal}
      headerCarriesGoal={codingSessionGoalRestatesTitle(goal?.content, title)}
      quiet
      sessionRef={sessionRef}
      trailing={
        founderPubkey ? (
          <CodingSessionFounderLine
            founderPubkey={founderPubkey}
            genesisRef={genesisRef}
            variant="inline"
          />
        ) : null
      }
      workspaceExpanded={workspaceExpanded}
    />
  );
}
