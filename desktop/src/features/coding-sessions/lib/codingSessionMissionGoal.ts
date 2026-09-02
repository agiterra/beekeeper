/**
 * Which 44227 is an umbrella's goal, and how a withheld file path reads.
 *
 * Split out of `codingSessionMissionInspectorModel.ts` so neither file passes
 * 1,000 lines. Both answers are joins over signed values and nothing else.
 */
import { hasRedactionMarker } from "@/shared/lib/redactionMarker";

/**
 * Which signed 44227 is this umbrella's goal.
 *
 * Live run 3, 12:36 (finding 23): the Inspector's Current goal card read `No
 * accepted mission goal published` with a `Set goal` button over a session
 * whose launch had published goal `f29f23e8` minutes earlier. The goal was on
 * the wire and correctly signed; what failed was the **join** — a map keyed on
 * the lowercase signer, read with whatever spelling the umbrella record
 * happened to hold, through three separate exact-equality comparisons in three
 * files. One of them missing is enough to make a published goal invisible.
 *
 * So the join is done once, here, case-folded, over the goals themselves
 * rather than through a precomputed key: newest `created_at` wins, ties break
 * on the event id descending, exactly as the policy fold resolves its own
 * newest-wins question.
 *
 * **Not reproduced; class removed.** The live record was run through the app's
 * own readers a second time (REVIEW-L2 F8, and again in fix round 1) —
 * `parseCodingSessionGoal` → `foldLatestCodingSessionGoalsByFounder` → the old
 * exact-key `Map.get` → the old three-way gate → this selection. It passes
 * every one of them. What shipped removes a whole class of join failure and is
 * **not** the fix for finding 23; the only branch here that answers `absent`
 * for a published goal is an umbrella whose `sessionRef` or `founderPubkey` is
 * still null, i.e. a reader that has not resolved — which is L4's `unresolved`
 * state, and is why the card must say which of the three it is.
 *
 * `rejected` is its own answer, and it names **which** of the three
 * identities disagreed. Rendering a record this surface refused as `absent`
 * would make a rejected goal and no goal look identical — critique A1's exact
 * defect — and rendering it as *the* goal would let any member retitle a
 * mission.
 */
export type CodingSessionUmbrellaGoalSelection =
  | {
      kind: "available";
      goal: CodingSessionGoalLike;
      foreignAuthors: readonly string[];
      disagreements: readonly CodingSessionGoalDisagreement[];
    }
  | {
      kind: "rejected";
      foreignAuthors: readonly string[];
      /** Which identities disagreed, in the order a sentence names them. */
      disagreements: readonly CodingSessionGoalDisagreement[];
    }
  | {
      kind: "absent";
      foreignAuthors: readonly string[];
      disagreements: readonly CodingSessionGoalDisagreement[];
    };

/** Which of the three identities a rejected 44227 disagreed on. */
export type CodingSessionGoalDisagreement = "channel" | "session" | "founder";

/** The parsed 44227 fields this selection needs; `CodingSessionGoal` satisfies it. */
export type CodingSessionGoalLike = {
  channelId: string;
  content: string;
  createdAt: number;
  eventId: string;
  founderPubkey: string;
  sessionRef: string;
};

function folded(value: string | null | undefined): string | null {
  const trimmed = value?.trim().toLowerCase();
  return trimmed && trimmed.length > 0 ? trimmed : null;
}

/** Select the umbrella's own newest goal, and disclose any that are not it. */
export function selectCodingSessionUmbrellaGoal(input: {
  goals: Iterable<CodingSessionGoalLike>;
  channelId: string;
  sessionRef: string | null;
  founderPubkey: string | null;
}): CodingSessionUmbrellaGoalSelection {
  const channel = folded(input.channelId);
  const session = folded(input.sessionRef);
  const founder = folded(input.founderPubkey);
  if (channel === null || session === null || founder === null) {
    return { kind: "absent", foreignAuthors: [], disagreements: [] };
  }
  let best: CodingSessionGoalLike | null = null;
  const foreign = new Set<string>();
  // A record refused on identity is *not* silence, and which identity refused
  // it is the difference between "somebody else set a goal here" and "this
  // client is looking at the wrong session". Both are collected; neither is
  // guessed.
  const disagreed = new Set<CodingSessionGoalDisagreement>();
  for (const goal of input.goals) {
    let rejected = false;
    if (folded(goal.channelId) !== channel) {
      disagreed.add("channel");
      rejected = true;
    }
    if (folded(goal.sessionRef) !== session) {
      disagreed.add("session");
      rejected = true;
    }
    if (folded(goal.founderPubkey) !== founder) {
      disagreed.add("founder");
      foreign.add(goal.founderPubkey);
      rejected = true;
    }
    if (rejected) continue;
    if (
      best === null ||
      goal.createdAt > best.createdAt ||
      (goal.createdAt === best.createdAt && goal.eventId > best.eventId)
    ) {
      best = goal;
    }
  }
  const foreignAuthors = [...foreign].sort();
  // The order a sentence names them, not insertion order.
  const disagreements = (["channel", "session", "founder"] as const).filter(
    (field) => disagreed.has(field),
  );
  if (best !== null) {
    return { kind: "available", goal: best, foreignAuthors, disagreements };
  }
  return disagreements.length > 0
    ? { kind: "rejected", foreignAuthors, disagreements }
    : { kind: "absent", foreignAuthors, disagreements };
}

/** The §L4.1 `rejected` sentence, naming what disagreed. */
export function codingSessionGoalRejectionSentence(
  disagreements: readonly CodingSessionGoalDisagreement[],
): string {
  const named = disagreements.includes("founder")
    ? "founder"
    : disagreements.includes("session")
      ? "session"
      : "channel";
  return (
    `A goal is published on this channel but it names a different ${named}. ` +
    "This surface will not show a goal it cannot bind to this mission."
  );
}

/**
 * Whether a candidate file path is really the host's elision marker.
 *
 * Live run 3 (finding 24 / critique A2) printed
 * `[elided private context: 183 bytes, sha256:b35397…]` into the Inspector's
 * Files card as though it were a path. It is not a path — it is the receipt
 * for the paths that were withheld.
 *
 * The marker's shape has exactly one owner, `shared/lib/redactionMarker.ts`,
 * and this asks that owner rather than carrying a second copy of the regex: a
 * second spelling of a wire shape is how two readers end up disagreeing about
 * what was redacted. A path that merely *contains* the word `elided`
 * (`src/elided.rs`) is untouched, because the owner's pattern is the full
 * marker and nothing less.
 */
export function isCodingSessionPrivateContextMarker(value: string): boolean {
  return hasRedactionMarker(value.trim());
}

/**
 * What a Files row says in place of the withheld paths.
 *
 * The count when the report gave one — the edits happened and their number is
 * not private — and no count when it did not. The bytes and the digest are
 * **not** re-surfaced here: a redaction disclosed as a redaction is the point,
 * and the marker is already rendered as a pill wherever the transcript carries
 * it.
 */
export function codingSessionPrivateContextLine(
  editCount: number | null,
): string {
  const suffix = "paths private to the seat's host";
  if (editCount === null) return `File paths private to the seat's host`;
  return `${editCount} file edit${editCount === 1 ? "" : "s"} · ${suffix}`;
}
