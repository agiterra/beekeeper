/**
 * Agent Progress's coordination read: a real lease read over the exact `#h`
 * channels this viewer can read.
 *
 * **Why this exists at all.** The panel used to answer "is this alive?" from
 * the age of the newest 44223 metadata event on the sessions shelf. That is
 * history, not liveness: a machine that dies mid-turn leaves its last signed
 * fact saying `running` forever, and an age threshold over that fact only
 * decides how long the app repeats it. Project Pulse removed exactly that lie
 * by reading kind 24223 leases; two surfaces in one app answering the same
 * question by different rules is worse than either rule.
 *
 * So this file adds a query rather than avoiding one. "No new kind" still
 * holds — 24223 and the 442xx facts already exist — but the shelf's inputs
 * structurally cannot prove reachability, so a lease read is required.
 *
 * The read is scoped and bounded exactly like Pulse's, and every truncation or
 * failure is recorded: a channel that did not answer must never render as a
 * channel with nothing running.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  foldSessionCoordination,
  sessionLeaseExpiryDelayMs,
} from "@/shared/coordination/sessionCoordinationFold";
import { isStrictMetadataContent } from "@/shared/coordination/sessionCoordinationStrictJson";
import {
  KIND_SESSION_LEASE,
  SESSION_COORDINATION_DURABLE_KINDS,
  SESSION_COORDINATION_KINDS,
  type CoordinatedSession,
  type CoordinationEvent,
  type SessionCoordinationAmbiguity,
} from "@/shared/coordination/sessionCoordinationTypes";
import { hasValidSignature } from "@/shared/lib/authors";

/** Relay hard cap on the aggregate explicit `#h` values in one request. */
export const AGENT_PROGRESS_CHANNELS_PER_QUERY = 128;
/** Upper bound on durable session facts per chunk; reaching it is partial. */
export const AGENT_PROGRESS_SESSION_QUERY_LIMIT = 1000;
/** One-shot cap for current lease keys; reaching it is a partial read. */
export const AGENT_PROGRESS_LEASE_QUERY_LIMIT = 1000;

/** One source query that failed, was truncated, or yielded an unreadable event. */
export type AgentProgressReadError = { scope: string; message: string };

/** What one coordination read returned, and what it could not answer. */
export type AgentProgressCoordinationRead = {
  /** Seconds since the epoch, read once after the last source query returned. */
  asOf: number;
  sessions: CoordinatedSession[];
  channelsBySession: ReadonlyMap<string, string[]>;
  /**
   * True only when every source query answered in full. False means the
   * session list is a floor — "at least this many" — never a census.
   */
  complete: boolean;
  errors: AgentProgressReadError[];
  ambiguities: SessionCoordinationAmbiguity[];
};

function channelChunks(channelIds: readonly string[]): string[][] {
  const sorted = [...new Set(channelIds)].sort();
  const chunks: string[][] = [];
  for (
    let index = 0;
    index < sorted.length;
    index += AGENT_PROGRESS_CHANNELS_PER_QUERY
  ) {
    chunks.push(sorted.slice(index, index + AGENT_PROGRESS_CHANNELS_PER_QUERY));
  }
  return chunks;
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** The one relay read this module needs; injectable so tests fold real bytes. */
export type AgentProgressEventFetcher = (
  filter: RelaySubscriptionFilter,
) => Promise<RelayEvent[]>;

/**
 * Keep only events this client can trust and understand — valid signature, and
 * for 44223 the shared strict metadata decoder.
 *
 * Every drop is recorded. An event the client could not read is an observation
 * it does not have, and dropping it silently is how a viewer with one
 * undecodable live session gets told nothing is running.
 */
function admissibleEvents(events: readonly RelayEvent[]): {
  events: CoordinationEvent[];
  errors: AgentProgressReadError[];
} {
  const admitted: CoordinationEvent[] = [];
  const errors: AgentProgressReadError[] = [];
  for (const event of events) {
    const why = !hasValidSignature(event)
      ? "failed signature validation"
      : event.kind === 44223 && !isStrictMetadataContent(event.content)
        ? "carried undecodable coding-session metadata"
        : null;
    if (why !== null) {
      errors.push({
        scope: "invalid-event",
        message: `event ${event.id} (kind ${event.kind}) ${why} and was excluded`,
      });
      continue;
    }
    admitted.push({
      id: event.id,
      pubkey: event.pubkey,
      created_at: event.created_at,
      kind: event.kind,
      tags: event.tags,
      content: event.content,
    });
  }
  return { events: admitted, errors };
}

/**
 * Read and fold every coding session reachable through `channelIds`.
 *
 * `channelsUnresolved` is the caller admitting its channel list is a floor
 * (the channels query is pending or failed). That is a read that did not
 * happen, not a viewer with no channels, and it is recorded before the queries
 * so it survives even when the partial set answers cleanly.
 */
export async function fetchAgentProgressCoordination(
  channelIds: readonly string[],
  dependencies: {
    fetchEvents?: AgentProgressEventFetcher;
    channelsUnresolved?: boolean;
  } = {},
): Promise<AgentProgressCoordinationRead> {
  const fetchEvents: AgentProgressEventFetcher =
    dependencies.fetchEvents ?? ((filter) => relayClient.fetchEvents(filter));
  const errors: AgentProgressReadError[] = [];
  const events: RelayEvent[] = [];

  if (dependencies.channelsUnresolved) {
    errors.push({
      scope: "channels",
      message:
        channelIds.length === 0
          ? "the readable channel set could not be read; no session facts were queried"
          : "the complete readable channel set could not be read; session facts were queried only for known channels",
    });
  }

  for (const channels of channelChunks(channelIds)) {
    try {
      const durable = await fetchEvents({
        kinds: [...SESSION_COORDINATION_DURABLE_KINDS],
        "#h": channels,
        limit: AGENT_PROGRESS_SESSION_QUERY_LIMIT,
      });
      events.push(...durable);
      if (durable.length >= AGENT_PROGRESS_SESSION_QUERY_LIMIT) {
        errors.push({
          scope: "sessions",
          message: `session read truncated at ${AGENT_PROGRESS_SESSION_QUERY_LIMIT} events`,
        });
      }
    } catch (error) {
      errors.push({ scope: "sessions", message: errorMessage(error) });
    }
  }
  for (const channels of channelChunks(channelIds)) {
    try {
      // Kind 24223 is an ephemeral snapshot of the relay's current lease keys.
      // One REQ obtains them; it must not share the durable limit or be
      // paginated as history.
      const leases = await fetchEvents({
        kinds: [KIND_SESSION_LEASE],
        "#h": channels,
        limit: AGENT_PROGRESS_LEASE_QUERY_LIMIT,
      });
      events.push(...leases);
      if (leases.length >= AGENT_PROGRESS_LEASE_QUERY_LIMIT) {
        errors.push({
          scope: "leases",
          message: `lease snapshot truncated at ${AGENT_PROGRESS_LEASE_QUERY_LIMIT} events`,
        });
      }
    } catch (error) {
      errors.push({ scope: "leases", message: errorMessage(error) });
    }
  }

  // `now` is read once, after the last source query returned — the instant
  // every lease expiry on this screen is measured against.
  const asOf = Math.floor(Date.now() / 1_000);
  const admissible = admissibleEvents(events);
  const sourceErrors = [...errors, ...admissible.errors];
  // No `acceptProjectRef`: this surface is global by design. It answers "what
  // is happening across every session I can read", so a session whose
  // `projectRef` is null, or names a project this client cannot see, still
  // belongs on it.
  const fold = foldSessionCoordination({
    now: asOf,
    events: admissible.events,
    sourceErrors,
  });
  return {
    asOf,
    sessions: fold.sessions,
    channelsBySession: fold.channelsBySession,
    // An unreadable event is a hole in this read exactly as a failed query is:
    // both mean the list below is a floor. Only an untruncated, fully decoded
    // read may present itself as the whole answer.
    complete: fold.complete,
    errors: fold.errors,
    ambiguities: fold.ambiguities,
  };
}

/** React Query key for one viewer's coordination read. */
export function agentProgressCoordinationQueryKey(
  channelIds: readonly string[],
  channelsUnresolved: boolean,
): readonly unknown[] {
  return [
    "agent-progress-coordination",
    [...channelIds].sort().join(","),
    channelsUnresolved,
  ];
}

/** What the panel knows about coordination right now. */
export type AgentProgressCoordinationState = {
  read: AgentProgressCoordinationRead | null;
  isPending: boolean;
};

/**
 * The coordination read, refreshed on live fan-out, on a 60s fallback poll,
 * and — crucially — when the earliest live lease lapses.
 *
 * Without that last timer a row would keep reading `Reachable` after its lease
 * expired simply because no new event arrived to trigger a re-render, which is
 * the same class of lie the freshness window used to tell.
 */
export function useAgentProgressCoordination(
  channelIds: readonly string[],
  channelsUnresolved: boolean,
): AgentProgressCoordinationState {
  const queryClient = useQueryClient();
  const channelKey = [...channelIds].sort().join(",");
  const stableChannelIds = React.useMemo(
    () => (channelKey === "" ? [] : channelKey.split(",")),
    [channelKey],
  );
  const key = React.useMemo(
    () =>
      agentProgressCoordinationQueryKey(stableChannelIds, channelsUnresolved),
    [channelsUnresolved, stableChannelIds],
  );

  React.useEffect(() => {
    let disposed = false;
    const unsubscribes = new Set<() => void>();
    const since = Math.floor(Date.now() / 1_000);
    for (const channels of channelChunks(
      channelKey === "" ? [] : channelKey.split(","),
    )) {
      void relayClient
        .subscribeLive(
          {
            kinds: [...SESSION_COORDINATION_KINDS],
            "#h": channels,
            since,
            limit: 100,
          },
          () => {
            void queryClient.invalidateQueries({ queryKey: key });
          },
        )
        .then((handle) => {
          if (!handle) return;
          if (disposed) handle();
          else unsubscribes.add(handle);
        })
        .catch(() => {
          // The 60-second cold read below is the fallback. A live-subscription
          // transport failure must not manufacture a durable read result.
        });
    }
    return () => {
      disposed = true;
      for (const unsubscribe of unsubscribes) unsubscribe();
      unsubscribes.clear();
    };
  }, [channelKey, key, queryClient]);

  const query = useQuery({
    queryKey: key,
    refetchInterval: 60_000,
    queryFn: () =>
      fetchAgentProgressCoordination(stableChannelIds, { channelsUnresolved }),
  });

  React.useEffect(() => {
    if (!query.data) return;
    const delay = sessionLeaseExpiryDelayMs(query.data.sessions, Date.now());
    if (delay === null) return;
    const timer = window.setTimeout(() => {
      void queryClient.invalidateQueries({ queryKey: key });
    }, delay);
    return () => window.clearTimeout(timer);
  }, [key, query.data, queryClient]);

  return { read: query.data ?? null, isPending: query.isPending };
}
