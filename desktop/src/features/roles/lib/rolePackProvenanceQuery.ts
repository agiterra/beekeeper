/**
 * The reads behind pack-revision provenance: the signed lifecycle evidence a
 * reported 44223 has to be joined back to before it may claim anything.
 *
 * Four kinds, four reads, four budgets — 44221 commands, 44224 receipts, 44223
 * reports, 44226 genesis records — scoped by `#h` to the project's own
 * channels. One filter naming all four would share a single `limit` across
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

/** Upper bound on one kind's read; reaching it means older proof is missing. */
export const ROLE_PACK_PROVENANCE_QUERY_LIMIT = 1000;

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
): readonly unknown[] {
  return [
    ROLE_PACK_PROVENANCE_QUERY_PREFIX,
    relayUrl,
    projectRef,
    [...new Set(channelIds)].sort(),
    [...new Set(metadataEventIds)].sort(),
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
async function admissibleEvents(events: readonly RelayEvent[]): Promise<{
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
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
    }
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
  deps: { fetchEvents?: RolePackProvenanceFetcher } = {},
): Promise<RolePackProvenanceEvents> {
  const fetchEvents: RolePackProvenanceFetcher =
    deps.fetchEvents ?? ((filter) => relayClient.fetchEvents(filter));
  const sourceErrors: RolePackProvenanceSourceError[] = [];
  const events: RelayEvent[] = [];

  for (const channels of channelChunks(channelIds)) {
    for (const read of PROVENANCE_READS) {
      try {
        const page = await fetchEvents({
          kinds: [read.kind],
          "#h": channels,
          limit: ROLE_PACK_PROVENANCE_QUERY_LIMIT,
        });
        events.push(...page);
        if (page.length >= ROLE_PACK_PROVENANCE_QUERY_LIMIT) {
          sourceErrors.push({
            scope: read.scope,
            message: `${read.subject} were truncated at ${ROLE_PACK_PROVENANCE_QUERY_LIMIT} events; older proof may be missing.`,
            channelIds: channels,
          });
        }
      } catch (error) {
        sourceErrors.push({
          scope: read.scope,
          message: `${read.subject} could not be read: ${errorMessage(error)}.`,
          channelIds: channels,
        });
      }
    }
  }

  const admissible = await admissibleEvents(events);
  return {
    events: admissible.events,
    sourceErrors: [...sourceErrors, ...admissible.sourceErrors],
  };
}
