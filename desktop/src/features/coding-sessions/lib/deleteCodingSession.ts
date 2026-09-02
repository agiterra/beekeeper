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
 * Select the events belonging to one umbrella from a channel fetch.
 *
 * Grouping is by the `d` tag, which every owned kind carries and which
 * equals the `sessionRef`. Sorted and de-duplicated so the count shown to
 * the person and the deletion that follows describe the same set.
 */
export function sessionOwnedEventIds(
  events: readonly RelayEvent[],
  sessionRef: string,
): string[] {
  const ids = new Set<string>();
  for (const event of events) {
    if (!SESSION_OWNED_KINDS.includes(event.kind as never)) continue;
    const belongs = event.tags.some(
      (tag) => tag[0] === "d" && tag[1] === sessionRef,
    );
    if (!belongs || !event.id) continue;
    ids.add(event.id);
  }
  return [...ids].sort();
}

/** True when the selection can actually be deleted as a session. */
export function selectionHasGenesis(
  events: readonly RelayEvent[],
  sessionRef: string,
): boolean {
  return events.some(
    (event) =>
      event.kind === KIND_CODING_SESSION_GENESIS &&
      event.tags.some((tag) => tag[0] === "d" && tag[1] === sessionRef),
  );
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
}: {
  channelId: string;
  sessionRef: string;
}): Promise<{ deleted: number }> {
  const events = await relayClient.fetchEvents({
    kinds: [...SESSION_OWNED_KINDS],
    "#h": [channelId],
    limit: 2000,
  });
  if (!selectionHasGenesis(events, sessionRef)) {
    // Without a genesis the relay has nothing to authorize against and would
    // refuse every target as "must be event author". Saying so plainly beats
    // a refusal the person has to decode.
    throw new Error(
      "This session has no genesis event, so it cannot be deleted as a session.",
    );
  }
  const ids = sessionOwnedEventIds(events, sessionRef);
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
