import {
  type CodingSessionFoundedUmbrella,
  resolveFoundedCodingSessions,
} from "./codingSessionFoundedModel";
import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  CodingSessionUmbrellaRecord,
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
export function umbrellaGenerations(
  umbrella: CodingSessionUmbrellaRecord,
): CodingSessionCatalogRecord[] {
  return umbrella.executions
    .flatMap((execution) => [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ])
    .sort((left, right) => right.lastEventAt.localeCompare(left.lastEventAt));
}

/**
 * The gate every row offered from a channel's own chrome passes.
 *
 * The snapshot-channel check prevents a render during channel transitions from
 * exposing the previous channel's sessions. Authority failures are fail-closed;
 * malformed, rejected-author, and invalid-signature events never enter the
 * catalog in the first place.
 */
function channelIngressIsOpen(input: {
  activeChannelId: string | null;
  catalog: CodingSessionCatalogSnapshot;
}): input is {
  activeChannelId: string;
  catalog: CodingSessionCatalogSnapshot;
} {
  return Boolean(
    input.activeChannelId &&
      input.catalog.channelId === input.activeChannelId &&
      !input.catalog.authorityErrorMessage,
  );
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
 * A founded umbrella with no execution is not among these rows — nothing can
 * be opened at a generation it does not have. `resolveChannelFoundedCodingSessions`
 * lists those separately, behind the same gate.
 */
export function resolveChannelCodingSessionIngress(input: {
  activeChannelId: string | null;
  catalog: CodingSessionCatalogSnapshot;
}): ChannelCodingSessionIngressEntry[] {
  if (!channelIngressIsOpen(input)) return [];

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

/**
 * The founded-but-unstarted umbrellas a channel's chrome may list, newest
 * founding first. Same gate as the openable rows; a snapshot that collected
 * no geneses yields none, never a guess.
 */
export function resolveChannelFoundedCodingSessions(input: {
  activeChannelId: string | null;
  catalog: CodingSessionCatalogSnapshot;
}): CodingSessionFoundedUmbrella[] {
  if (!channelIngressIsOpen(input)) return [];
  return resolveFoundedCodingSessions({
    channelId: input.activeChannelId,
    geneses: input.catalog.geneses ?? [],
    entries: input.catalog.entries,
    creates: input.catalog.creates ?? [],
  });
}
