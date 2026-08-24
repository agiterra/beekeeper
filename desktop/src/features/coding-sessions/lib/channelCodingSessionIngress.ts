import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";
import { groupCodingSessionCatalog } from "./codingSessionUmbrellaModel";
import { deriveCodingSessionWorkspaceStatus } from "./codingSessionWorkspaceModel";

export type ChannelCodingSessionIngressEntry = {
  /** The generation this row opens: the umbrella's most recently active one. */
  session: CodingSessionCatalogRecord;
  status: CodingSessionWorkspaceStatus;
  /** Provider executions this one row stands for. */
  executionCount: number;
  /** Signed generations behind it, resumes included. */
  generationCount: number;
};

/** Every generation an umbrella owns, newest activity first. */
function umbrellaGenerations(
  umbrella: ReturnType<typeof groupCodingSessionCatalog>[number],
): CodingSessionCatalogRecord[] {
  return umbrella.executions
    .flatMap((execution) => [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ])
    .sort((left, right) => right.lastEventAt.localeCompare(left.lastEventAt));
}

/**
 * Resolve the sessions that may be offered from a channel's own chrome.
 *
 * **One row per durable session, not per generation.** A resumed session has a
 * generation per resume, and listing each made the trigger read "Coding
 * sessions (2)" over two rows that opened the same umbrella (§2 item 38). The
 * count people reason about is sessions; how many times a session was resumed
 * is a property of the row, not another row. Agent Progress made the same
 * choice for the same reason (§2 item 36: "the footer counts distinct
 * `executionKey` identities rather than resumed generations").
 *
 * The snapshot-channel check prevents a render during channel transitions from
 * exposing the previous channel's generations. Authority failures are
 * fail-closed; malformed, rejected-author, and invalid-signature events never
 * enter `catalog.entries` in the first place.
 */
export function resolveChannelCodingSessionIngress(input: {
  activeChannelId: string | null;
  catalog: CodingSessionCatalogSnapshot;
}): ChannelCodingSessionIngressEntry[] {
  if (
    !input.activeChannelId ||
    input.catalog.channelId !== input.activeChannelId ||
    input.catalog.authorityErrorMessage
  ) {
    return [];
  }

  const rows: ChannelCodingSessionIngressEntry[] = [];
  for (const umbrella of groupCodingSessionCatalog(
    input.catalog.entries,
    input.catalog.creates,
  )) {
    const generations = umbrellaGenerations(umbrella);
    const session = generations[0];
    // An umbrella with no generation cannot be opened; it is not a row.
    if (session === undefined) continue;
    rows.push({
      session,
      status: deriveCodingSessionWorkspaceStatus(
        session.transcript,
        session.status,
        session.statusAt,
      ),
      executionCount: umbrella.executions.length,
      generationCount: generations.length,
    });
  }
  return rows;
}
