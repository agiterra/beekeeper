import { relayClient } from "@/shared/api/relayClient";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  KIND_CODING_SESSION_TRANSCRIPT,
  KIND_DELETION,
} from "@/shared/constants/kinds";

/**
 * The kinds one coding session owns.
 *
 * A closed list, not "everything in the channel". The relay grants a whole
 * session deletion an authorship exemption over exactly these — a session's
 * events are signed by the provider and by agent seats, not by the person
 * deleting it — so naming anything outside the list earns a refusal, and
 * naming a teammate's chat message would earn something worse.
 */
export const SESSION_OWNED_KINDS = [
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
] as const;

/**
 * How a session's events actually attach to their umbrella.
 *
 * There is no single grouping tag, and assuming there was is what made the
 * first version of this file refuse every real session with "no genesis
 * event". Three different linkages, one per family:
 *
 * | Kind | Attaches by |
 * |---|---|
 * | 44226 genesis | its **event id** — `["csg-session", …]` exists for the relay's uniqueness probe and diagnostics, and NIP-CSG says consumers must not select by it |
 * | 44227 goal, 44229 name, 44230 closure, 44244 team txn | `["d", sessionRef]` |
 * | 44223 metadata | `sessionRef` in its **content**; its tags carry only `cs-target` |
 * | 44225 transcript | `["cs-target", …]` only — no sessionRef anywhere on it |
 *
 * So a transcript reaches its umbrella in two hops: the metadata for an
 * execution names both that execution's `cs-target` and the umbrella's
 * `sessionRef`, and the transcript names the same `cs-target`. An execution
 * whose metadata never landed contributes no transcript — correctly, since
 * nothing on the wire ties one to this session.
 */
function tagValue(event: RelayEvent, name: string): string | null {
  const found = event.tags.find(
    (tag) => tag[0] === name && typeof tag[1] === "string",
  );
  return found?.[1] ?? null;
}

/** `sessionRef` claimed by an event's JSON content, if it has one. */
function contentSessionRef(event: RelayEvent): string | null {
  try {
    const parsed = JSON.parse(event.content) as { sessionRef?: unknown };
    return typeof parsed.sessionRef === "string" ? parsed.sessionRef : null;
  } catch {
    // A payload this build cannot parse is a version skew, not a reason to
    // sweep the event in — an unattributable event is left alone.
    return null;
  }
}

/** Kinds that carry their umbrella in a plain `d` tag. */
const D_TAG_KINDS: readonly number[] = [
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
];

/**
 * The genesis of this umbrella, selected by canonical event id.
 *
 * `genesisRef` is what every session surface already holds and what the
 * authority chain is rooted at. When a caller has only a `sessionRef` (the
 * CLI), the genesis is found by its content's own `sessionRef` claim
 * instead — the payload, never the `csg-session` tag.
 */
export function findGenesis(
  events: readonly RelayEvent[],
  sessionRef: string,
  genesisRef: string | null,
): RelayEvent | null {
  const candidates = events.filter(
    (event) => event.kind === KIND_CODING_SESSION_GENESIS,
  );
  if (genesisRef) {
    return candidates.find((event) => event.id === genesisRef) ?? null;
  }
  return (
    candidates.find((event) => contentSessionRef(event) === sessionRef) ?? null
  );
}

/** True when this umbrella can be deleted as a session at all. */
export function selectionHasGenesis(
  events: readonly RelayEvent[],
  sessionRef: string,
  genesisRef: string | null = null,
): boolean {
  return findGenesis(events, sessionRef, genesisRef) !== null;
}

/**
 * Every event belonging to one umbrella, as ids, sorted and de-duplicated so
 * the count shown to the person and the deletion that follows describe the
 * same set.
 */
export function sessionOwnedEventIds(
  events: readonly RelayEvent[],
  sessionRef: string,
  genesisRef: string | null = null,
): string[] {
  const ids = new Set<string>();

  const genesis = findGenesis(events, sessionRef, genesisRef);
  if (genesis?.id) ids.add(genesis.id);

  // Pass one: everything that names the umbrella directly, and the metadata
  // that maps an execution onto it.
  const targets = new Set<string>();
  for (const event of events) {
    if (!event.id) continue;
    if (D_TAG_KINDS.includes(event.kind)) {
      if (tagValue(event, "d") === sessionRef) ids.add(event.id);
      continue;
    }
    if (event.kind === KIND_CODING_SESSION_METADATA) {
      if (contentSessionRef(event) !== sessionRef) continue;
      ids.add(event.id);
      const target = tagValue(event, "cs-target");
      if (target) targets.add(target);
    }
  }

  // Pass two: the transcript of each execution the metadata attributed to
  // this umbrella. Needs pass one's `cs-target` set, so it cannot merge.
  if (targets.size > 0) {
    for (const event of events) {
      if (event.kind !== KIND_CODING_SESSION_TRANSCRIPT || !event.id) continue;
      const target = tagValue(event, "cs-target");
      if (target && targets.has(target)) ids.add(event.id);
    }
  }

  return [...ids].sort();
}

/**
 * Delete one coding session outright.
 *
 * ## Why this is one event over a whole chain
 *
 * The relay refuses a genesis or a closure deleted on its own: the first
 * would strand the session's closure revisions, the second would roll shared
 * state back with no counter-revision. It also refuses a chain that leaves
 * any live closure behind. So the whole session goes in one kind:5, or not
 * at all — assembling the set here is the only shape the relay accepts, not
 * a convenience.
 *
 * ## What is not undone
 *
 * The session's `sessionRef` stays claimed for good. A deleted session's
 * reference is never released and nothing can be re-founded under it — the
 * same rule a deleted repository's name follows. The transcript is gone; the
 * fact that this reference was used is not.
 *
 * ## Stopping first
 *
 * This publishes tombstones and nothing else. An execution still running on
 * its host is stopped by a lifecycle command or by the session's closure,
 * not by this — the caller sequences that, and reports honestly when the
 * host was not listening.
 */
export async function deleteCodingSession({
  channelId,
  sessionRef,
  genesisRef = null,
}: {
  channelId: string;
  sessionRef: string;
  genesisRef?: string | null;
}): Promise<{ deleted: number }> {
  const events = await relayClient.fetchEvents({
    kinds: [...SESSION_OWNED_KINDS],
    "#h": [channelId],
    limit: 2000,
  });
  if (!selectionHasGenesis(events, sessionRef, genesisRef)) {
    // Without a genesis the relay has nothing to authorize against and would
    // refuse every target as "must be event author". Saying so plainly beats
    // a refusal the person has to decode.
    throw new Error(
      "This session has no genesis event, so it cannot be deleted as a session.",
    );
  }
  const ids = sessionOwnedEventIds(events, sessionRef, genesisRef);
  const event = await signRelayEvent({
    kind: KIND_DELETION,
    content: `Delete session ${sessionRef}`,
    tags: ids.map((id) => ["e", id]),
  });
  await relayClient.publishEvent(
    event,
    "Timed out deleting the session.",
    "Failed to delete the session.",
  );
  return { deleted: ids.length };
}
