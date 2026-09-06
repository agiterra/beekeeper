/**
 * Gathering the signed records behind Project Pulse's Missions half.
 *
 * `pulse_mission_rows` folds nothing it was not handed. Until this module
 * existed the Desktop read handed it *nothing* — no sessions and no session
 * count — so the command answered with zero rows, zero errors, and one
 * sentence promising "the newest 8 open sessions by observation time" over a
 * read that had opened no session at all (REVIEW-L9 F1). This is the gather
 * step that sentence describes.
 *
 * The division of labour is strict and one-directional:
 *
 * - this module **queries** — genesis, authority transitions and their relay
 *   receipts, kind 44244/44245/44246 records — and passes whole signed events
 *   through untouched, because `buzz-core` verifies their signatures itself;
 * - `codingSessionMissionAuthority` **projects** the accepted chain, and is
 *   reused rather than reimplemented, because two projections of one chain is
 *   two answers to "who may steer";
 * - `buzz-core` **folds** and writes every sentence a person reads.
 *
 * Nothing here composes prose, computes a state, or decides what a row means.
 *
 * The read is fail-closed per umbrella. A session whose records could not be
 * read completely — a failed query, a truncated page, an unresolvable genesis,
 * an authority chain that would not project — is **omitted** from the request
 * and **named** in `readErrors`. Sending it with empty event lists would
 * render records nobody could read as records that do not exist, and those are
 * different facts; dropping it silently would render a session under work as a
 * quiet one, which is the lie this surface exists to prevent.
 */
import { projectCodingSessionMissionAuthority } from "@/features/coding-sessions/lib/codingSessionMissionAuthority";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_GENESIS,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_OBSERVATION,
  KIND_CODING_SESSION_POLICY,
  KIND_CODING_SESSION_TEAM_TRANSACTION,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";

import type {
  PulseMissionGrantInput,
  PulseMissionReadError,
  PulseMissionSeatInput,
  PulseMissionSessionInput,
} from "./invokePulseMissionRows";

/**
 * The native command's own cap (`MAX_PULSE_MISSION_ROWS`). A request carrying
 * more sessions than this is refused outright, so the selection below is not a
 * courtesy — it is the contract.
 */
export const MAX_PULSE_MISSION_SESSIONS = 8;

/** Relay hard cap on aggregate explicit `#h` values in one request. */
export const PULSE_MISSION_CHANNELS_PER_QUERY = 128;

/** Per-query budgets. Reaching one is a partial read, never a complete answer. */
export const PULSE_MISSION_GENESIS_QUERY_LIMIT = 500;
export const PULSE_MISSION_AUTHORITY_QUERY_LIMIT = 500;
export const PULSE_MISSION_RECEIPT_QUERY_LIMIT = 500;
export const PULSE_MISSION_RECORD_QUERY_LIMIT = 500;

const HEX64 = /^[0-9a-f]{64}$/;

/** One open umbrella, as the digest proved it. */
export type PulseMissionOpenSession = {
  sessionKey: string;
  sessionRef: string | null;
  name: string | null;
  latestObservationAt: number | null;
};

/** Where one umbrella was founded, and by whom. */
export type PulseMissionGenesis = {
  channelRef: string;
  genesisRef: string;
  founderPubkey: string;
};

/** What the gather produced: what it opened, and what it could not. */
export type PulseMissionSessionReadResult = {
  sessions: PulseMissionSessionInput[];
  readErrors: PulseMissionReadError[];
};

/** The one relay read this module needs; injectable so tests drive fixtures. */
export type PulseMissionEventFetcher = (
  filter: RelaySubscriptionFilter,
) => Promise<RelayEvent[]>;

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

function byteOrder(left: string, right: string): number {
  return left < right ? -1 : left > right ? 1 : 0;
}

function tagValue(event: RelayEvent, name: string): string | null {
  for (const tag of event.tags) {
    if (tag[0] === name && typeof tag[1] === "string") return tag[1];
  }
  return null;
}

/**
 * Events in the order the native command documents: ascending by signed time,
 * with the event id as a deterministic tiebreak inside one second.
 */
function ascending(events: readonly RelayEvent[]): RelayEvent[] {
  return [...events].sort(
    (left, right) =>
      left.created_at - right.created_at || byteOrder(left.id, right.id),
  );
}

/** Every open umbrella the digest proved. Closed ones are not missions. */
export function pulseMissionOpenSessions(
  digest: {
    sessions: readonly {
      sessionKey: string;
      sessionRef: string | null;
      name: string | null;
      lifecycle: string;
      latestObservationAt: number | null;
    }[];
  } | null,
): PulseMissionOpenSession[] {
  if (!digest) return [];
  return digest.sessions
    .filter((session) => session.lifecycle === "open")
    .map((session) => ({
      sessionKey: session.sessionKey,
      sessionRef: session.sessionRef,
      name: session.name,
      latestObservationAt: session.latestObservationAt,
    }));
}

/**
 * The umbrellas this read will open: newest observation first, ties broken on
 * the session key so two paints of the same digest choose the same eight.
 *
 * A session that has never been observed sorts last — it is the least likely
 * to be the one a reader is waiting on, and it must still be *reachable*, so
 * it is ordered rather than filtered.
 */
export function selectPulseMissionSessions(
  sessions: readonly PulseMissionOpenSession[],
  limit: number = MAX_PULSE_MISSION_SESSIONS,
): PulseMissionOpenSession[] {
  return [...sessions]
    .sort((left, right) => {
      if (left.latestObservationAt !== right.latestObservationAt) {
        if (left.latestObservationAt === null) return 1;
        if (right.latestObservationAt === null) return -1;
        return right.latestObservationAt - left.latestObservationAt;
      }
      return byteOrder(left.sessionKey, right.sessionKey);
    })
    .slice(0, Math.max(0, limit));
}

/**
 * Resolve each umbrella to the immutable founding record that names its
 * channel, its genesis id and its founder.
 *
 * Two genesis records claiming one session ref prove nothing about either, so
 * both are withdrawn and the session ref is reported ambiguous. Selecting one
 * would attribute an umbrella to a founder who may not have founded it.
 */
export function indexPulseMissionGenesis(events: readonly RelayEvent[]): {
  genesis: Map<string, PulseMissionGenesis>;
  ambiguous: string[];
} {
  const genesis = new Map<string, PulseMissionGenesis>();
  const ambiguous = new Set<string>();
  for (const event of events) {
    if (event.kind !== KIND_CODING_SESSION_GENESIS) continue;
    const sessionRef = tagValue(event, "csg-session");
    const channelRef = tagValue(event, "h");
    if (!sessionRef || !channelRef) continue;
    const existing = genesis.get(sessionRef);
    if (existing) {
      if (existing.genesisRef !== event.id) ambiguous.add(sessionRef);
      continue;
    }
    genesis.set(sessionRef, {
      channelRef,
      genesisRef: event.id,
      founderPubkey: event.pubkey,
    });
  }
  for (const sessionRef of ambiguous) genesis.delete(sessionRef);
  return { genesis, ambiguous: [...ambiguous].sort(byteOrder) };
}

/** Active seats on the wire: the actor and the role, and nothing else. */
export function pulseMissionSeatInputs(authority: {
  activeSeats: readonly { actorPubkey: string; role: string }[];
}): PulseMissionSeatInput[] {
  return authority.activeSeats.map((seat) => ({
    actorPubkey: seat.actorPubkey,
    role: seat.role,
  }));
}

/**
 * Active grants on the wire, joined back to the accepted transition that put
 * them there for its relay-accepted time and its type.
 *
 * Only grant transitions are considered in that join: a seat transition for
 * the same actor is a different fact, and letting a later `grant-seat` win
 * would report a standing operator as ungranted. An active grant with no
 * accepted grant transition behind it is a contradiction in the projection,
 * and `null` says the caller may not send this umbrella at all.
 */
export function pulseMissionGrantInputs(authority: {
  activeGrants: readonly {
    actorPubkey: string;
    grantEventRef: string;
    maySteer: boolean;
  }[];
  policyGrants: readonly {
    grantee: string;
    acceptedAt: number;
    transitionType: string;
  }[];
}): PulseMissionGrantInput[] | null {
  const accepted = new Map<string, { acceptedAt: number; granted: boolean }>();
  for (const grant of authority.policyGrants) {
    if (
      grant.transitionType !== "grant-operator" &&
      grant.transitionType !== "grant-viewer" &&
      grant.transitionType !== "revoke"
    ) {
      continue;
    }
    accepted.set(grant.grantee, {
      acceptedAt: grant.acceptedAt,
      granted: grant.transitionType === "grant-operator",
    });
  }
  const grants: PulseMissionGrantInput[] = [];
  for (const grant of authority.activeGrants) {
    const source = accepted.get(grant.actorPubkey);
    if (!source) return null;
    grants.push({
      actorPubkey: grant.actorPubkey,
      grantEventRef: grant.grantEventRef,
      maySteer: grant.maySteer,
      acceptedAt: source.acceptedAt,
      granted: source.granted,
    });
  }
  return grants;
}

/** Shape one umbrella for the wire, or say why it may not be sent. */
export function buildPulseMissionSessionInput(input: {
  session: PulseMissionOpenSession;
  genesis: PulseMissionGenesis;
  relayPubkey: string;
  transitions: readonly RelayEvent[];
  receipts: readonly RelayEvent[];
  teamEvents: readonly RelayEvent[];
  policyEvents: readonly RelayEvent[];
  observationEvents: readonly RelayEvent[];
  lifecycleCommands: readonly RelayEvent[];
  lifecycleReceipts: readonly RelayEvent[];
}):
  | { ok: true; value: PulseMissionSessionInput }
  | { ok: false; error: string } {
  const sessionRef = input.session.sessionRef;
  if (!sessionRef) {
    return { ok: false, error: "the umbrella has no session ref to read" };
  }
  const authority = projectCodingSessionMissionAuthority({
    channelRef: input.genesis.channelRef,
    genesisRef: input.genesis.genesisRef,
    founderPubkey: input.genesis.founderPubkey,
    relayPubkey: input.relayPubkey,
    transitions: input.transitions,
    receipts: input.receipts,
  });
  if (!authority.ok) return { ok: false, error: authority.error };
  const activeGrants = pulseMissionGrantInputs(authority.value);
  if (!activeGrants) {
    return {
      ok: false,
      error:
        "an active grant has no accepted transition behind it, so this umbrella's authority is not projectable",
    };
  }
  return {
    ok: true,
    value: {
      sessionKey: input.session.sessionKey,
      channelRef: input.genesis.channelRef,
      sessionRef,
      genesisRef: input.genesis.genesisRef,
      founderPubkey: input.genesis.founderPubkey,
      name: input.session.name,
      latestObservationAt: input.session.latestObservationAt,
      activeSeats: pulseMissionSeatInputs(authority.value),
      activeGrants,
      // This surface claims no seat of its own; the native command keeps a
      // claimed seat only when the projection already has it, so claiming
      // here could only ever add noise.
      claimedSeats: [],
      teamEvents: ascending(input.teamEvents),
      policyEvents: ascending(input.policyEvents),
      observationEvents: ascending(input.observationEvents),
      // The provider proof (S4). Ascending like everything else here; the
      // pairing rule is order-independent, and one order for every list is one
      // fewer thing for a later reader to have to check.
      lifecycleCommands: ascending(input.lifecycleCommands),
      lifecycleReceipts: ascending(input.lifecycleReceipts),
      // Kind 30618 is addressable by repo id, and an umbrella carries no repo
      // id on this surface — the digest never reads one. An empty list is the
      // honest answer; a guessed one would move refs nobody pushed.
      refState: [],
      // `checkpoint.files` is Lane L5's unlanded key. With no published paths
      // there is nothing to overlap on, and the native command draws no
      // overlap row from an empty list rather than guessing one from a commit.
      overlapFiles: [],
      overlapSha: null,
      overlapAsOf: null,
      overlapAuthor: null,
    },
  };
}

function channelChunks(channelIds: readonly string[]): string[][] {
  const sorted = [...new Set(channelIds)].sort(byteOrder);
  const chunks: string[][] = [];
  for (
    let index = 0;
    index < sorted.length;
    index += PULSE_MISSION_CHANNELS_PER_QUERY
  ) {
    chunks.push(sorted.slice(index, index + PULSE_MISSION_CHANNELS_PER_QUERY));
  }
  return chunks;
}

/**
 * One bounded read, where a thrown query and a full page are the same answer:
 * this read did not see everything it asked for.
 */
async function readBounded(
  fetchEvents: PulseMissionEventFetcher,
  filter: RelaySubscriptionFilter,
  what: string,
): Promise<{ ok: true; events: RelayEvent[] } | { ok: false; error: string }> {
  try {
    const events = await fetchEvents(filter);
    if (events.length >= filter.limit) {
      return {
        ok: false,
        error: `the ${what} read was truncated at ${filter.limit} events, so its records were not read completely`,
      };
    }
    return { ok: true, events };
  } catch (error) {
    return { ok: false, error: errorMessage(error) };
  }
}

/**
 * Gather the signed records behind every open umbrella in scope.
 *
 * The three record reads are deliberately three filters. A relay returns its
 * newest `limit` rows across every kind one filter names, and observations
 * (44246) outrun team records (44244) by orders of magnitude in any umbrella
 * doing work — one shared budget would let observation volume evict the very
 * records a seat's authority is proven from.
 */
export async function readPulseMissionSessions(
  input: {
    channelIds: readonly string[];
    openSessions: readonly PulseMissionOpenSession[];
  },
  dependencies: {
    fetchEvents: PulseMissionEventFetcher;
    relaySelf: () => Promise<string | null>;
  },
): Promise<PulseMissionSessionReadResult> {
  const readErrors: PulseMissionReadError[] = [];
  const selected = selectPulseMissionSessions(input.openSessions);
  if (selected.length === 0) return { sessions: [], readErrors };

  // Acceptance receipts are only trustworthy from the community's own relay
  // key. Without it nothing below can be verified, so nothing below is sent.
  let relayPubkey: string | null = null;
  try {
    relayPubkey = await dependencies.relaySelf();
  } catch (error) {
    readErrors.push({ scope: "authority", message: errorMessage(error) });
    return { sessions: [], readErrors };
  }
  if (!relayPubkey || !HEX64.test(relayPubkey)) {
    readErrors.push({
      scope: "authority",
      message:
        "the active community advertises no trusted NIP-11 self signing key, so no umbrella's authority chain could be verified",
    });
    return { sessions: [], readErrors };
  }

  const chunks = channelChunks(input.channelIds);
  const genesisEvents: RelayEvent[] = [];
  for (const channels of chunks) {
    const read = await readBounded(
      dependencies.fetchEvents,
      {
        kinds: [KIND_CODING_SESSION_GENESIS],
        "#h": channels,
        limit: PULSE_MISSION_GENESIS_QUERY_LIMIT,
      },
      "genesis",
    );
    if (!read.ok) {
      readErrors.push({ scope: "missions:genesis", message: read.error });
      continue;
    }
    genesisEvents.push(...read.events);
  }
  const indexed = indexPulseMissionGenesis(genesisEvents);

  const receiptsByChannel = new Map<
    string,
    { ok: true; events: RelayEvent[] } | { ok: false; error: string }
  >();
  const sessions: PulseMissionSessionInput[] = [];

  for (const session of selected) {
    const sessionRef = session.sessionRef;
    if (!sessionRef) {
      readErrors.push({
        scope: `missions:${session.sessionKey}`,
        message:
          "this umbrella was proven only from an implicit execution key, so it has no genesis record to read",
      });
      continue;
    }
    if (indexed.ambiguous.includes(sessionRef)) {
      readErrors.push({
        scope: `missions:${session.sessionKey}`,
        message:
          "two genesis records claim this umbrella, so neither its channel nor its founder is proven",
      });
      continue;
    }
    const genesis = indexed.genesis.get(sessionRef);
    if (!genesis) {
      readErrors.push({
        scope: `missions:${session.sessionKey}`,
        message:
          "no genesis record for this umbrella was readable in the project's channels, so its founder and authority chain are unknown",
      });
      continue;
    }
    const scope = `missions:${genesis.channelRef}`;

    const transitions = await readBounded(
      dependencies.fetchEvents,
      {
        kinds: [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
        "#h": [genesis.channelRef],
        "#csat-genesis": [genesis.genesisRef],
        limit: PULSE_MISSION_AUTHORITY_QUERY_LIMIT,
      },
      "authority transition",
    );
    if (!transitions.ok) {
      readErrors.push({
        scope: `authority:${session.sessionKey}`,
        message: transitions.error,
      });
      continue;
    }

    let receipts = receiptsByChannel.get(genesis.channelRef);
    if (!receipts) {
      receipts = await readBounded(
        dependencies.fetchEvents,
        {
          kinds: [KIND_SYSTEM_MESSAGE],
          authors: [relayPubkey],
          "#h": [genesis.channelRef],
          limit: PULSE_MISSION_RECEIPT_QUERY_LIMIT,
        },
        "authority receipt",
      );
      receiptsByChannel.set(genesis.channelRef, receipts);
    }
    if (!receipts.ok) {
      readErrors.push({
        scope: `authority:${session.sessionKey}`,
        message: receipts.error,
      });
      continue;
    }

    const records: Record<string, RelayEvent[]> = {};
    let recordsFailed = false;
    for (const [kind, what] of [
      [KIND_CODING_SESSION_TEAM_TRANSACTION, "team record"],
      [KIND_CODING_SESSION_POLICY, "policy record"],
      [KIND_CODING_SESSION_OBSERVATION, "observation"],
    ] as const) {
      const read = await readBounded(
        dependencies.fetchEvents,
        {
          kinds: [kind],
          "#h": [genesis.channelRef],
          "#d": [sessionRef],
          limit: PULSE_MISSION_RECORD_QUERY_LIMIT,
        },
        what,
      );
      if (!read.ok) {
        readErrors.push({ scope, message: read.error });
        recordsFailed = true;
        break;
      }
      records[String(kind)] = read.events;
    }
    if (recordsFailed) continue;

    // The provider proof (S4): kind 44221 and 44224 of this umbrella's
    // channel. **No `#d`** — a lifecycle command is tagged `csl-command`, not
    // with the umbrella's `d`, so a `#d` filter here would match nothing and
    // every mission would resolve no provider while looking as though it had
    // been asked. The native adapter splits the page by session and genesis.
    for (const [kind, what] of [
      [KIND_CODING_SESSION_LIFECYCLE_COMMAND, "lifecycle command"],
      [KIND_CODING_SESSION_LIFECYCLE_RECEIPT, "lifecycle receipt"],
    ] as const) {
      const read = await readBounded(
        dependencies.fetchEvents,
        {
          kinds: [kind],
          "#h": [genesis.channelRef],
          limit: PULSE_MISSION_RECORD_QUERY_LIMIT,
        },
        what,
      );
      if (!read.ok) {
        readErrors.push({ scope, message: read.error });
        recordsFailed = true;
        break;
      }
      records[String(kind)] = read.events;
    }
    if (recordsFailed) continue;

    const built = buildPulseMissionSessionInput({
      session,
      genesis,
      relayPubkey,
      transitions: transitions.events,
      receipts: receipts.events,
      teamEvents: records[String(KIND_CODING_SESSION_TEAM_TRANSACTION)] ?? [],
      policyEvents: records[String(KIND_CODING_SESSION_POLICY)] ?? [],
      observationEvents: records[String(KIND_CODING_SESSION_OBSERVATION)] ?? [],
      lifecycleCommands:
        records[String(KIND_CODING_SESSION_LIFECYCLE_COMMAND)] ?? [],
      lifecycleReceipts:
        records[String(KIND_CODING_SESSION_LIFECYCLE_RECEIPT)] ?? [],
    });
    if (!built.ok) {
      readErrors.push({
        scope: `authority:${session.sessionKey}`,
        message: built.error,
      });
      continue;
    }
    sessions.push(built.value);
  }

  return { sessions, readErrors };
}
