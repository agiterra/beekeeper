import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_GOAL } from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";
import { isCodingSessionSessionRef } from "./codingSessionWireDecode";

export const CODING_SESSION_GOAL_TAG_VERSION = "csgl1-1" as const;
export const MAX_CODING_SESSION_GOAL_BYTES = 4096;

export type CodingSessionGoal = {
  channelId: string;
  content: string;
  createdAt: number;
  eventId: string;
  founderPubkey: string;
  sessionRef: string;
};

export function buildCodingSessionGoalFilter(
  channelIds: readonly string[],
  limit = 1000,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_GOAL],
    "#h": [...new Set(channelIds)],
    limit,
  };
}

/**
 * Whether a goal is too large to publish, and by how much.
 *
 * The refusal a person reads has to name the two numbers they can act on —
 * what they wrote and what fits — so this reports both rather than a boolean.
 * Measured on the trimmed text in UTF-8 bytes, exactly as
 * {@link buildCodingSessionGoalEvent} measures it, so the launch button and
 * the signer can never disagree about whether a goal fits.
 *
 * Returns null for a goal that fits, **and for a blank one**: an empty goal is
 * refused separately, with its own sentence, and reporting it as an overflow
 * would be a size claim about nothing.
 */
export function codingSessionGoalOverflow(
  content: string,
): { bytes: number; cap: number } | null {
  const bytes = new TextEncoder().encode(content.trim()).byteLength;
  return bytes > MAX_CODING_SESSION_GOAL_BYTES
    ? { bytes, cap: MAX_CODING_SESSION_GOAL_BYTES }
    : null;
}

/**
 * The sentence an over-cap goal is refused with, in one place.
 *
 * Two surfaces say it — the crew tab's byte counter, at the field, and the
 * launch block's own line under the button — and they must say the same
 * words with the same numbers, or a person who fixes one reading is told a
 * different story by the other.
 */
export function codingSessionGoalOverflowSentence(overflow: {
  bytes: number;
  cap: number;
}): string {
  return (
    `This goal is ${overflow.bytes.toLocaleString()} UTF-8 bytes and the ` +
    `cap is ${overflow.cap.toLocaleString()} — shorten it by ` +
    `${(overflow.bytes - overflow.cap).toLocaleString()} bytes, or the ` +
    "session would launch with no goal published."
  );
}

export function buildCodingSessionGoalEvent(input: {
  channelId: string;
  content: string;
  sessionRef: string;
}) {
  const content = input.content.trim();
  if (!input.channelId.trim()) throw new Error("channelId must not be empty");
  if (!isCodingSessionSessionRef(input.sessionRef)) {
    throw new Error("sessionRef must be a canonical lowercase hyphenated UUID");
  }
  const size = new TextEncoder().encode(content).byteLength;
  if (size === 0 || size > MAX_CODING_SESSION_GOAL_BYTES) {
    throw new Error("Session goal must be between 1 and 4096 UTF-8 bytes.");
  }
  return {
    kind: KIND_CODING_SESSION_GOAL,
    content,
    tags: [
      ["h", input.channelId],
      ["d", input.sessionRef],
      ["csgl-v", CODING_SESSION_GOAL_TAG_VERSION],
    ],
  };
}

export function parseCodingSessionGoal(
  event: RelayEvent,
): CodingSessionGoal | null {
  if (
    event.kind !== KIND_CODING_SESSION_GOAL ||
    event.tags.length !== 3 ||
    event.tags[0]?.length !== 2 ||
    event.tags[0]?.[0] !== "h" ||
    !event.tags[0]?.[1] ||
    event.tags[1]?.length !== 2 ||
    event.tags[1]?.[0] !== "d" ||
    !isCodingSessionSessionRef(event.tags[1]?.[1] ?? "") ||
    event.tags[2]?.length !== 2 ||
    event.tags[2]?.[0] !== "csgl-v" ||
    event.tags[2]?.[1] !== CODING_SESSION_GOAL_TAG_VERSION ||
    !event.content.trim() ||
    new TextEncoder().encode(event.content).byteLength >
      MAX_CODING_SESSION_GOAL_BYTES ||
    !hasValidSignature(event)
  ) {
    return null;
  }
  return {
    channelId: event.tags[0][1],
    content: event.content,
    createdAt: event.created_at,
    eventId: event.id,
    founderPubkey: event.pubkey.toLowerCase(),
    sessionRef: event.tags[1][1],
  };
}

/** Fold append-only revisions deterministically while retaining source ids. */
export function foldLatestCodingSessionGoals(
  events: readonly RelayEvent[],
): Map<string, CodingSessionGoal> {
  const latest = new Map<string, CodingSessionGoal>();
  for (const event of events) {
    const goal = parseCodingSessionGoal(event);
    if (!goal) continue;
    const key = `${goal.channelId}\u0000${goal.sessionRef}`;
    const previous = latest.get(key);
    if (
      !previous ||
      goal.createdAt > previous.createdAt ||
      (goal.createdAt === previous.createdAt && goal.eventId > previous.eventId)
    ) {
      latest.set(key, goal);
    }
  }
  return latest;
}

/** Preflight-authority view: a foreign signer cannot shadow founder history. */
export function foldLatestCodingSessionGoalsByFounder(
  events: readonly RelayEvent[],
): Map<string, CodingSessionGoal> {
  const latest = new Map<string, CodingSessionGoal>();
  for (const event of events) {
    const goal = parseCodingSessionGoal(event);
    if (!goal) continue;
    const key = `${goal.channelId}\u0000${goal.sessionRef}\u0000${goal.founderPubkey}`;
    const previous = latest.get(key);
    if (
      !previous ||
      goal.createdAt > previous.createdAt ||
      (goal.createdAt === previous.createdAt && goal.eventId > previous.eventId)
    ) {
      latest.set(key, goal);
    }
  }
  return latest;
}

export async function publishCodingSessionGoal(
  input: Parameters<typeof buildCodingSessionGoalEvent>[0],
  dependencies: {
    publisher?: typeof relayClient;
    signer?: typeof signRelayEvent;
  } = {},
): Promise<RelayEvent> {
  const event = await (dependencies.signer ?? signRelayEvent)(
    buildCodingSessionGoalEvent(input),
  );
  return await (dependencies.publisher ?? relayClient).publishEvent(
    event,
    "Timed out while updating the session goal.",
    "Failed to update the session goal.",
  );
}

/**
 * What the goal reader itself can currently say — as opposed to what the wire
 * holds.
 *
 * Critique A1, seen live 12:36 on a session whose 44227 was signed at launch:
 * `CURRENT GOAL — No accepted mission goal published.` Three different facts
 * had collapsed into that one sentence — this channel holds no record; the
 * reader has not settled, or errored; and a record exists that the surface's
 * identity gate refused. Only the first is what the sentence claims, and it is
 * I6's exact failure mode: a claim about the wire made from the absence of a
 * local lookup.
 *
 * `resolved` here means only that `useCodingSessionGoals`' history fetch
 * settled. It says nothing about whether a record was found, and nothing about
 * whether one was refused on identity — that second question belongs to the
 * goal *selection*, which discloses its own `rejected` state and its own
 * sentence. Two readers, two answers, neither guessing the other's.
 */
export type CodingSessionGoalReader =
  | { kind: "unresolved" }
  | { kind: "errored"; message: string }
  | { kind: "resolved" };

/** The reader's own condition, from what `useCodingSessionGoals` returned. */
export function deriveCodingSessionGoalReader(snapshot: {
  errorMessage: string | null;
  resolved: boolean;
}): CodingSessionGoalReader {
  if (snapshot.errorMessage !== null) {
    return { kind: "errored", message: snapshot.errorMessage };
  }
  return snapshot.resolved ? { kind: "resolved" } : { kind: "unresolved" };
}

/** `Goal not read yet.` — the reader has not settled. */
export const CODING_SESSION_GOAL_UNRESOLVED = "Goal not read yet.";

/**
 * `The goal for this session could not be read — {message}`
 *
 * The reader's own message, verbatim. An error the surface paraphrases is an
 * error nobody can act on, and `useCodingSessionGoals` has carried this string
 * since it was written — it simply had no reader.
 */
export function codingSessionGoalErrorSentence(message: string): string {
  return `The goal for this session could not be read — ${message}`;
}
