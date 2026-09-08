/**
 * The reads behind pack-revision provenance: the signed lifecycle evidence a
 * reported 44223 has to be joined back to before it may claim anything.
 *
 * Separate kinds, separate bounded history budgets — 44221 commands, 44224 receipts, 44223
 * reports, 44226 genesis records, plus 44228 authority and 40099 receipts — scoped by `#h` to the project's own
 * channels. One filter naming all kinds would share a single `limit` across
 * them, and receipts outrun commands by orders of magnitude on a channel that
 * has run real work, so the newest N rows would eventually be all receipts and
 * the commands and genesis records that prove anything would fall off the end.
 * The proof would then quietly read "unavailable" on a project with complete
 * evidence on the relay. Project Pulse learned this the same way; the chunking
 * and the per-kind loop below follow `pulseQueries.ts` deliberately.
 *
 * Every failure and every truncation becomes a `{scope, message}` sentence the
 * model carries into its reasons. A read that did not happen must never render
 * as evidence that does not exist.
 */

import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_SYSTEM_MESSAGE,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";

import {
  ROLE_PACK_PROVENANCE_PROOF_SCOPES,
  type RolePackProvenanceSourceError,
} from "./rolePackProvenance";

/** Rows requested from the relay for each bounded history page. */
export const ROLE_PACK_PROVENANCE_QUERY_LIMIT = 1000;

/** Maximum dependent history requests for one kind and one channel chunk. */
export const ROLE_PACK_PROVENANCE_MAX_REQUESTS = 8;

/** Defense in depth for a fetcher that does not honor the requested page size. */
export const ROLE_PACK_PROVENANCE_MAX_UNIQUE_EVENTS =
  ROLE_PACK_PROVENANCE_QUERY_LIMIT * ROLE_PACK_PROVENANCE_MAX_REQUESTS;

/**
 * Relay hard cap for the aggregate explicit `#h` values in one request — the
 * same bound Project Pulse chunks against (`PULSE_CHANNELS_PER_QUERY`), copied
 * rather than imported so a roles read never depends on a Pulse constant
 * moving for Pulse's reasons.
 */
export const ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY = 128;

/**
 * What one provenance read returned, and everything it could not read.
 *
 * Every failure and every truncation carries the exact channels its read
 * covered. Without that, one channel's timeout would have to be treated as a
 * partial answer for the whole project — or, worse, be dropped and let a row
 * in an unaffected channel and a row in the failed one read the same.
 */
export type RolePackProvenanceEvents = {
  events: RelayEvent[];
  sourceErrors: RolePackProvenanceSourceError[];
};

/** The one relay read this module needs; injectable so tests fold real bytes. */
export type RolePackProvenanceFetcher = (
  filter: RelaySubscriptionFilter,
) => Promise<RelayEvent[]>;

/**
 * The three proof scopes, taken from the model so the two cannot drift: the
 * model decides which incomplete read suppresses a positive, and it can only
 * do that if the scope strings this module writes are the ones it matches on.
 */
const [COMMANDS_SCOPE, RECEIPTS_SCOPE, GENESIS_SCOPE] =
  ROLE_PACK_PROVENANCE_PROOF_SCOPES;

/** A missing 44223 is a row that never renders, not a hidden contradiction. */
const REPORTS_SCOPE = "session-reports";

type ProvenanceRead = {
  kind: number;
  scope: string;
  /** Plural subject of both sentences, e.g. "Lifecycle commands". */
  subject: string;
};

/**
 * One read per kind, in evidence order: the command that commissioned, the
 * receipt that answered it, the report that claims a pack, and the genesis the
 * chain is bound to.
 */
const PROVENANCE_READS: readonly ProvenanceRead[] = [
  {
    kind: KIND_CODING_SESSION_LIFECYCLE_COMMAND,
    scope: COMMANDS_SCOPE,
    subject: "Lifecycle commands",
  },
  {
    kind: KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
    scope: RECEIPTS_SCOPE,
    subject: "Lifecycle receipts",
  },
  {
    kind: KIND_CODING_SESSION_METADATA,
    scope: REPORTS_SCOPE,
    subject: "Session reports",
  },
  {
    kind: KIND_CODING_SESSION_GENESIS,
    scope: GENESIS_SCOPE,
    subject: "Session genesis records",
  },
  {
    kind: KIND_CODING_SESSION_AUTHORITY_TRANSITION,
    scope: "authority-transitions",
    subject: "Authority transitions",
  },
  {
    kind: KIND_SYSTEM_MESSAGE,
    scope: "authority-receipts",
    subject: "Authority acceptance receipts",
  },
];

function channelChunks(channelIds: readonly string[]): string[][] {
  const sorted = [...new Set(channelIds)].sort();
  const chunks: string[][] = [];
  for (
    let index = 0;
    index < sorted.length;
    index += ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY
  ) {
    chunks.push(
      sorted.slice(index, index + ROLE_PACK_PROVENANCE_CHANNELS_PER_QUERY),
    );
  }
  return chunks;
}

/** The failure text, trimmed of its own full stop so the sentence keeps one. */
function errorMessage(error: unknown): string {
  const message = error instanceof Error ? error.message : String(error);
  return message.trim().replace(/\.+$/, "");
}

function abortError(): Error {
  const error = new Error("Role provenance history recovery was cancelled.");
  error.name = "AbortError";
  return error;
}

function throwIfAborted(signal?: AbortSignal): void {
  if (signal?.aborted) throw abortError();
}

async function yieldToBrowser(signal?: AbortSignal): Promise<void> {
  throwIfAborted(signal);
  await new Promise<void>((resolve) => setTimeout(resolve, 0));
  throwIfAborted(signal);
}

function hasRequestedChannel(
  event: RelayEvent,
  channelIds: ReadonlySet<string>,
): boolean {
  return event.tags.some(
    (tag) => tag[0] === "h" && channelIds.has(tag[1] ?? ""),
  );
}

function invalidPageReason(
  page: unknown,
  read: ProvenanceRead,
  channels: readonly string[],
  until: number | undefined,
): string | null {
  if (!Array.isArray(page)) return "the relay response was not an event list";
  if (page.length > ROLE_PACK_PROVENANCE_QUERY_LIMIT) {
    return `the relay returned ${page.length} events for a ${ROLE_PACK_PROVENANCE_QUERY_LIMIT}-event page`;
  }

  const channelSet = new Set(channels);
  for (const candidate of page) {
    if (
      typeof candidate !== "object" ||
      candidate === null ||
      typeof candidate.id !== "string" ||
      candidate.id.length === 0 ||
      typeof candidate.pubkey !== "string" ||
      !Number.isSafeInteger(candidate.created_at) ||
      candidate.created_at < 0 ||
      !Number.isSafeInteger(candidate.kind) ||
      !Array.isArray(candidate.tags) ||
      !candidate.tags.every(
        (tag: unknown) =>
          Array.isArray(tag) && tag.every((value) => typeof value === "string"),
      ) ||
      typeof candidate.content !== "string" ||
      typeof candidate.sig !== "string"
    ) {
      return "the relay returned a malformed event";
    }
    const event = candidate as RelayEvent;
    if (event.kind !== read.kind) {
      return `event ${event.id.slice(0, 8)}… had kind ${event.kind}, outside the requested kind ${read.kind}`;
    }
    if (!hasRequestedChannel(event, channelSet)) {
      return `event ${event.id.slice(0, 8)}… was outside the requested channel chunk`;
    }
    if (until !== undefined && event.created_at > until) {
      return `event ${event.id.slice(0, 8)}… was newer than the inclusive until cursor ${until}`;
    }
  }
  return null;
}

function incompleteRead(
  read: ProvenanceRead,
  channels: readonly string[],
  message: string,
): RolePackProvenanceSourceError {
  return {
    scope: read.scope,
    message,
    channelIds: channels,
  };
}

type ProvenanceHistory = {
  events: RelayEvent[];
  sourceError?: RolePackProvenanceSourceError;
};

/**
 * Recover one kind/chunk stream backwards without stepping over timestamp ties.
 * A short page proves exhaustion. Every full page keeps its oldest second as
 * an inclusive cursor, and id de-duplication removes the resulting overlap.
 */
async function fetchProvenanceHistory(
  read: ProvenanceRead,
  channels: readonly string[],
  fetchEvents: RolePackProvenanceFetcher,
  signal?: AbortSignal,
): Promise<ProvenanceHistory> {
  const eventsById = new Map<string, RelayEvent>();
  let until: number | undefined;

  for (
    let request = 1;
    request <= ROLE_PACK_PROVENANCE_MAX_REQUESTS;
    request += 1
  ) {
    throwIfAborted(signal);
    let page: RelayEvent[];
    try {
      page = await fetchEvents({
        kinds: [read.kind],
        "#h": [...channels],
        limit: ROLE_PACK_PROVENANCE_QUERY_LIMIT,
        ...(until === undefined ? {} : { until }),
      });
    } catch (error) {
      throwIfAborted(signal);
      if (error instanceof Error && error.name === "AbortError") throw error;
      return {
        events: [...eventsById.values()],
        sourceError: incompleteRead(
          read,
          channels,
          `${read.subject} could not be read: ${errorMessage(error)}.`,
        ),
      };
    }
    throwIfAborted(signal);

    const invalidReason = invalidPageReason(page, read, channels, until);
    if (invalidReason) {
      return {
        events: [...eventsById.values()],
        sourceError: incompleteRead(
          read,
          channels,
          `${read.subject} returned an invalid pagination response: ${invalidReason}; older proof may be missing.`,
        ),
      };
    }

    const before = eventsById.size;
    for (const event of page) {
      if (!eventsById.has(event.id)) eventsById.set(event.id, event);
    }
    if (eventsById.size > ROLE_PACK_PROVENANCE_MAX_UNIQUE_EVENTS) {
      return {
        events: [...eventsById.values()].slice(
          0,
          ROLE_PACK_PROVENANCE_MAX_UNIQUE_EVENTS,
        ),
        sourceError: incompleteRead(
          read,
          channels,
          `${read.subject} exceeded the bounded recovery limit of ${ROLE_PACK_PROVENANCE_MAX_UNIQUE_EVENTS} unique events; older proof may be missing.`,
        ),
      };
    }
    if (page.length < ROLE_PACK_PROVENANCE_QUERY_LIMIT) {
      return { events: [...eventsById.values()] };
    }

    const oldest = Math.min(...page.map((event) => event.created_at));
    if (until !== undefined && oldest === until) {
      return {
        events: [...eventsById.values()],
        sourceError: incompleteRead(
          read,
          channels,
          `${read.subject} could not cross a full relay page at timestamp ${until}; older proof at that timestamp may be missing.`,
        ),
      };
    }
    if (eventsById.size === before) {
      return {
        events: [...eventsById.values()],
        sourceError: incompleteRead(
          read,
          channels,
          `${read.subject} pagination made no unique-event progress after ${request} requests; older proof may be missing.`,
        ),
      };
    }
    if (request === ROLE_PACK_PROVENANCE_MAX_REQUESTS) {
      return {
        events: [...eventsById.values()],
        sourceError: incompleteRead(
          read,
          channels,
          `${read.subject} reached the bounded recovery budget of ${ROLE_PACK_PROVENANCE_MAX_REQUESTS} requests after ${eventsById.size} unique events; older proof may be missing.`,
        ),
      };
    }

    until = oldest;
    await yieldToBrowser(signal);
  }

  return { events: [...eventsById.values()] };
}

/** The prefix every provenance cache entry shares; also the reset selector. */
export const ROLE_PACK_PROVENANCE_QUERY_PREFIX = "role-pack-provenance";

/**
 * The cache key for one project's provenance answer.
 *
 * `relayUrl` leads, because a proof is a fact about *one relay's events*: the
 * same project coordinate, the same channel ids and the same report ids exist
 * on another community's relay with entirely different signers behind them.
 * Without the relay in the key, switching community would serve the previous
 * relay's "commissioned" for the next relay's rows — the worst failure this
 * module can have, because it is a false positive that looks settled.
 *
 * The reported rows are part of the key through their metadata event ids: a
 * new 44223 arriving is a new claim to adjudicate, and an answer computed
 * before it arrived must not be served for it. Channels are in the key because
 * they are the read's whole scope.
 */
export function rolePackProvenanceQueryKey(
  relayUrl: string | null,
  projectRef: string | null,
  channelIds: readonly string[],
  metadataEventIds: readonly string[],
  trustedRelayPubkey: string | null = null,
): readonly unknown[] {
  return [
    ROLE_PACK_PROVENANCE_QUERY_PREFIX,
    relayUrl,
    projectRef,
    [...new Set(channelIds)].sort(),
    [...new Set(metadataEventIds)].sort(),
    trustedRelayPubkey,
  ] as const;
}

/**
 * Admit only events this client verified for itself.
 *
 * The classifiers downstream verify again — that is the established boundary,
 * and it is why nothing is stripped here. What this gate adds is the record: a
 * dropped event is an observation the reader does not have, and dropping it
 * silently turns a forged or corrupted row into "no proof exists" with no way
 * to tell the two apart.
 */
async function admissibleEvents(
  events: readonly RelayEvent[],
  signal?: AbortSignal,
): Promise<{
  events: RelayEvent[];
  sourceErrors: RolePackProvenanceSourceError[];
}> {
  const admitted: RelayEvent[] = [];
  const sourceErrors: RolePackProvenanceSourceError[] = [];
  const seen = new Set<string>();
  let checked = 0;
  for (const event of events) {
    // Yield to input/paint while validating a cold history. A microtask yield
    // does not release the browser's main thread to scrolling.
    if (checked > 0 && checked % 8 === 0) {
      await yieldToBrowser(signal);
    }
    throwIfAborted(signal);
    checked += 1;
    if (seen.has(event.id)) continue;
    seen.add(event.id);
    if (!hasValidSignature(event)) {
      sourceErrors.push({
        scope: "invalid-event",
        message: `A lifecycle event (${event.id.slice(0, 8)}…, kind ${event.kind}) failed signature validation and was not used as proof.`,
      });
      continue;
    }
    admitted.push(event);
  }
  return { events: admitted, sourceErrors };
}

/**
 * Fetch the lifecycle evidence for one project's channels.
 *
 * Exported for tests and for any caller that already holds the channel set;
 * components go through {@link useRolePackProvenance}.
 */
export async function fetchRolePackProvenanceEvents(
  channelIds: readonly string[],
  deps: {
    fetchEvents?: RolePackProvenanceFetcher;
    signal?: AbortSignal;
  } = {},
): Promise<RolePackProvenanceEvents> {
  const fetchEvents: RolePackProvenanceFetcher =
    deps.fetchEvents ?? ((filter) => relayClient.fetchEvents(filter));
  const sourceErrors: RolePackProvenanceSourceError[] = [];
  const events: RelayEvent[] = [];

  for (const channels of channelChunks(channelIds)) {
    for (const read of PROVENANCE_READS) {
      const history = await fetchProvenanceHistory(
        read,
        channels,
        fetchEvents,
        deps.signal,
      );
      events.push(...history.events);
      if (history.sourceError) sourceErrors.push(history.sourceError);
    }
  }

  throwIfAborted(deps.signal);
  const admissible = await admissibleEvents(events, deps.signal);
  return {
    events: admissible.events,
    sourceErrors: [...sourceErrors, ...admissible.sourceErrors],
  };
}
