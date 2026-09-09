/**
 * Reads behind Project Pulse: durable facts, current leases, and one fold.
 *
 * Entries come back by `#a` on the project coordinate; coding-session facts
 * come back by `#h` on the project's own channels. There is no queryable
 * "sessions of this project" relation — 44223 carries no `a` tag — and a
 * community-wide 44223 scan is forbidden because it is unbounded and it leaks.
 * A session running in a channel outside the project's channel set is
 * therefore not discoverable, which is why the digest carries
 * `sessionsScope: "project channels"` and no surface may present provider
 * reachability as exhaustive.
 *
 * Every channel that fails records a `{scope, message}` in `errors[]` and
 * flips `complete` to false. A read error must never render as a quiet
 * project.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";
import {
  MAX_FILTERS_PER_REQ,
  type RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import { chunkFiltersForRequest } from "@/shared/api/relayQueryCoalescer";
import { phaseJitteredPeriodMs } from "@/shared/lib/pollSchedule";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_LEASE,
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_LIFECYCLE_RECEIPT,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_PULSE_ENTRY,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";

import {
  readProjectPulseDigest,
  readPulseMissionRows,
  rememberProjectPulseDigest,
  rememberPulseMissionRows,
} from "./projectPulseCache";
import {
  invokePulseMissionRows,
  type PulseMissionRowsInvoker,
} from "./invokePulseMissionRows";
import type { PulseMissionRowsResponse } from "./pulseMissionWire";
import {
  pulseMissionOpenSessions,
  readPulseMissionSessions,
} from "./pulseMissionSessionRead";
import {
  foldProjectPulseDigest,
  type ProjectPulseDigest,
  type PulseDigestError,
} from "./pulseFold.ts";
import type { PulseEvent } from "./pulseEntry.ts";
import { isStrictMetadataContent } from "@/shared/coordination/sessionCoordinationStrictJson";
import { sessionLeaseExpiryDelayMs } from "@/shared/coordination/sessionCoordinationFold";

/** Upper bound on entries fetched in one read; a truncated page is a partial read. */
export const PULSE_ENTRY_QUERY_LIMIT = 500;
/** Upper bound on session facts fetched in one read. */
export const PULSE_SESSION_QUERY_LIMIT = 1000;
/** One-shot cap for current Redis lease keys; reaching it is a partial read. */
export const PULSE_LEASE_QUERY_LIMIT = 1000;
/** Relay hard cap for the aggregate explicit `#h` values in one request. */
export const PULSE_CHANNELS_PER_QUERY = 128;
/** Fallback poll for a missed live event or a reconnect; jittered per identity. */
export const PULSE_DIGEST_REFETCH_INTERVAL_MS = 60_000;

function channelChunks(channelIds: readonly string[]): string[][] {
  const sorted = [...new Set(channelIds)].sort();
  const chunks: string[][] = [];
  for (
    let index = 0;
    index < sorted.length;
    index += PULSE_CHANNELS_PER_QUERY
  ) {
    chunks.push(sorted.slice(index, index + PULSE_CHANNELS_PER_QUERY));
  }
  return chunks;
}

/**
 * Milliseconds until the earliest currently reachable lease expires.
 *
 * Delegated to the shared coordination module: Agent Progress schedules its
 * own re-read on the same instant, and two surfaces disagreeing about when a
 * lease lapses is two surfaces disagreeing about liveness.
 */
export function pulseLeaseExpiryDelayMs(
  digest: Pick<ProjectPulseDigest, "sessions">,
  nowMs = Date.now(),
): number | null {
  return sessionLeaseExpiryDelayMs(digest.sessions, nowMs);
}

/** Whether a channel list is only a floor rather than an authoritative set. */
export function projectPulseChannelSetUnresolved(query: {
  isPending: boolean;
  isError: boolean;
  isFetching: boolean;
}): boolean {
  return query.isPending || query.isError || query.isFetching;
}

/**
 * Per-generation durable facts: each one is published once per create, resume,
 * stop, or rename, so this window stays bounded by generation count.
 */
const GENERATION_FACT_KINDS = [
  KIND_CODING_SESSION_LIFECYCLE_COMMAND,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_CLOSURE,
];

/**
 * Receipts get their own read and their own budget.
 *
 * Kind 44224 is no longer bounded by generation count: a turn publishes at
 * least `turn_queued` and `turn_started`, so receipts outrun creates by orders
 * of magnitude in any channel doing work. A relay filter returns its newest
 * `limit` rows across every kind it names, so sharing one budget with the
 * facts above let turn volume push the 44221 commands and 44223 metadata the
 * coordination fold needs to *prove* a generation off the end of the read.
 *
 * The split closes only that half. A generation is proven from the 44221
 * command *and* the 44224 receipt answering it, and a create receipt is itself
 * a kind 44224 sharing this budget with unbounded turn receipts — so enough
 * turn traffic still evicts a create receipt and its proven session still
 * vanishes from the digest behind nothing but a generic truncation note.
 * Narrowing the receipt read is owed.
 */
const RECEIPT_FACT_KINDS = [KIND_CODING_SESSION_LIFECYCLE_RECEIPT];

const DURABLE_SESSION_FACT_KINDS = [
  ...GENERATION_FACT_KINDS,
  ...RECEIPT_FACT_KINDS,
];

const LIVE_SESSION_FACT_KINDS = [
  KIND_CODING_SESSION_LEASE,
  ...DURABLE_SESSION_FACT_KINDS,
];

/** One relay read; injectable so tests fold real bytes. */
export type PulseEventFetcher = (
  filter: RelaySubscriptionFilter,
) => Promise<RelayEvent[]>;

/**
 * A bundle of filters as one `POST /query`, each keeping its own `limit`,
 * resolved as the deduplicated union; injectable so tests see the bundles.
 */
export type PulseBatchFetcher = (
  filters: RelaySubscriptionFilter[],
) => Promise<RelayEvent[]>;

/** One source read inside a bundle, and how it reports truncation. */
type PulseSourceRead = {
  scope: string;
  filter: RelaySubscriptionFilter & { kinds: number[]; limit: number };
  truncation: string;
};

/** The three reads one 128-channel chunk needs, each with its own budget. */
function pulseChunkReads(channels: string[]): PulseSourceRead[] {
  const sessions = (kinds: number[]): PulseSourceRead => ({
    scope: "sessions",
    filter: { kinds, "#h": channels, limit: PULSE_SESSION_QUERY_LIMIT },
    truncation: `session read truncated at ${PULSE_SESSION_QUERY_LIMIT} events`,
  });
  return [
    // Two reads, two budgets, so per-turn receipt volume cannot evict the
    // 44221 commands and 44223 metadata a session is proven from. That is
    // half the problem: a generation is proven from the command *and* the
    // 44224 receipt answering it, and a create receipt is itself a 44224
    // sharing this budget with unbounded turn receipts, so enough turn
    // traffic still drops a proven session from the digest. Narrowing the
    // receipt read is owed.
    sessions(GENERATION_FACT_KINDS),
    sessions(RECEIPT_FACT_KINDS),
    // Kind 24223 is an ephemeral Redis snapshot. Its own filter obtains the
    // current keys; it must not share the durable limit or be paginated as
    // history.
    {
      scope: "leases",
      filter: {
        kinds: [KIND_CODING_SESSION_LEASE],
        "#h": channels,
        limit: PULSE_LEASE_QUERY_LIMIT,
      },
      truncation: `lease snapshot truncated at ${PULSE_LEASE_QUERY_LIMIT} events`,
    },
  ];
}

/**
 * Run one bundle and attribute its rows back to each read by kind — exact,
 * because the reads in a bundle name disjoint kinds. A bundle that failed is
 * every read in it not happening, so each of its scopes records the failure.
 */
async function readPulseBundle(
  reads: PulseSourceRead[],
  fetchEventsBatch: PulseBatchFetcher,
  events: RelayEvent[],
  sourceErrors: PulseDigestError[],
): Promise<void> {
  let bundle: RelayEvent[];
  try {
    bundle = await fetchEventsBatch(reads.map((read) => read.filter));
  } catch (error) {
    for (const scope of new Set(reads.map((read) => read.scope))) {
      sourceErrors.push({ scope, message: errorMessage(error) });
    }
    return;
  }
  events.push(...bundle);
  for (const read of reads) {
    const rows = bundle.filter((event) =>
      read.filter.kinds.includes(event.kind),
    ).length;
    if (rows >= read.filter.limit) {
      sourceErrors.push({ scope: read.scope, message: read.truncation });
    }
  }
}

/**
 * React Query key for one project's folded Pulse.
 *
 * `channelsUnresolved` is part of the key on purpose: a digest folded while
 * the channel set was still unknown is a *different* answer from one folded
 * against a resolved (even if empty) set, and for a genuinely channel-less
 * project both produce the same `channelIds` string. Without this segment the
 * partial digest would be served from cache as the settled one.
 */
export function projectPulseQueryKey(
  coordinate: string,
  channelIds: readonly string[],
  channelsUnresolved = false,
): readonly unknown[] {
  return [
    "project-pulse",
    coordinate,
    [...channelIds].sort().join(","),
    channelsUnresolved,
  ];
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Keep only events this client can actually trust and understand: a valid
 * signature (the same fail-closed rule the coding-session goal reader uses)
 * and, for 44223, the shared strict metadata decoder, so the all-four-or-none
 * observation discipline and the `relayReachable`/`verifiedAt` null coupling
 * are enforced once, by the decoder that owns them.
 *
 * Every drop is *recorded*, never silent. An event the client could not read
 * is an observation it does not have; dropping it without a trace turns a
 * project with one undecodable live session into a completed "the project is
 * quiet" verdict — a null rendered as a confirmed negative.
 */
function admissibleEvents(events: readonly RelayEvent[]): {
  events: PulseEvent[];
  errors: PulseDigestError[];
} {
  const admitted: PulseEvent[] = [];
  const errors: PulseDigestError[] = [];
  const excluded = (event: RelayEvent, why: string) => {
    errors.push({
      scope: "invalid-event",
      message: `event ${event.id} (kind ${event.kind}) ${why} and was excluded`,
    });
  };
  for (const event of events) {
    if (!hasValidSignature(event)) {
      excluded(event, "failed signature validation");
      continue;
    }
    if (
      event.kind === KIND_CODING_SESSION_METADATA &&
      !isStrictMetadataContent(event.content)
    ) {
      excluded(event, "carried undecodable coding-session metadata");
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
 * Fetch and fold one project's Pulse. Exported for tests and for any caller
 * that already has the coordinate and channel set; components use
 * {@link useProjectPulseDigest}.
 */
export async function fetchProjectPulseDigest(
  coordinate: string,
  channelIds: readonly string[],
  dependencies: {
    fetchEventsBatch?: PulseBatchFetcher;
    /**
     * True when the caller could not resolve the project's channel set (the
     * channels query is pending or failed). The set below is then a floor, not
     * the project's channels, and the digest is reported partial.
     */
    channelsUnresolved?: boolean;
  } = {},
): Promise<ProjectPulseDigest> {
  const fetchEventsBatch: PulseBatchFetcher =
    dependencies.fetchEventsBatch ??
    ((filters) => relayClient.fetchEventsBatch(filters));
  const sourceErrors: PulseDigestError[] = [];
  const events: RelayEvent[] = [];

  // An unresolved channel set is a read that did not happen, not a project
  // with no channels. Recorded before the reads so it lands in `errors[]`
  // whether or not a partial set came back — mirroring the CLI's
  // `scan_project_sessions`, which pushes a `{scope:"channels"}` error and
  // flips `complete` on both failure and truncation.
  if (dependencies.channelsUnresolved) {
    sourceErrors.push({
      scope: "channels",
      message:
        channelIds.length === 0
          ? "the project's channel set could not be read; session facts were not queried"
          : "the complete project channel set could not be read; session facts were queried only for known channels",
    });
  }

  const entries: PulseSourceRead = {
    scope: "entries",
    filter: {
      kinds: [KIND_PULSE_ENTRY],
      "#a": [coordinate],
      limit: PULSE_ENTRY_QUERY_LIMIT,
    },
    truncation: `entry read truncated at ${PULSE_ENTRY_QUERY_LIMIT} events`,
  };
  // One bundled `POST /query` per 128-channel chunk — every filter keeps its
  // own limit — with the entry read riding the first bundle, so a project of
  // up to 128 channels is one call instead of four REQs against the per-key
  // burst the paired phone shares.
  const chunks = channelChunks(channelIds);
  const bundles =
    chunks.length === 0
      ? [[entries]]
      : chunks.map((channels, index) =>
          index === 0
            ? [entries, ...pulseChunkReads(channels)]
            : pulseChunkReads(channels),
        );
  for (const reads of bundles) {
    await readPulseBundle(reads, fetchEventsBatch, events, sourceErrors);
  }

  // `now` is read once, after the last source query returned — the digest's
  // `asOf`, and the clock every age on the screen is measured against.
  const now = Math.floor(Date.now() / 1_000);
  const admissible = admissibleEvents(events);
  const digest = foldProjectPulseDigest({
    project: coordinate,
    now,
    events: admissible.events,
    sourceErrors: [...sourceErrors, ...admissible.errors],
  });
  rememberProjectPulseDigest(coordinate, digest);
  return digest;
}

/** What the surface knows about a project's Pulse right now. */
export type ProjectPulseState =
  | { kind: "loading"; digest: ProjectPulseDigest | null }
  | { kind: "ready"; digest: ProjectPulseDigest; refreshing?: boolean }
  | { kind: "partial"; digest: ProjectPulseDigest; refreshing?: boolean };

/**
 * One project's folded Pulse, refreshed on live 44240 fan-out and on a 60s
 * fallback poll (a missed event or a reconnect must not freeze the view).
 *
 * `coordinate` is null for a project with no head to query — the local
 * General placeholder — and the hook then stays disabled rather than issuing
 * a query that can never match.
 */
export function useProjectPulseDigest(
  coordinate: string | null,
  channelIds: readonly string[],
  channelsUnresolved = false,
): ProjectPulseState {
  const queryClient = useQueryClient();
  // Stable across renders that hand back a fresh channel array with the same
  // contents — a new key every render would re-subscribe on every paint.
  const channelKey = [...channelIds].sort().join(",");
  const stableChannelIds = React.useMemo(
    () => (channelKey === "" ? [] : channelKey.split(",")),
    [channelKey],
  );
  const key = React.useMemo(
    () =>
      projectPulseQueryKey(
        coordinate ?? "none",
        stableChannelIds,
        channelsUnresolved,
      ),
    [channelsUnresolved, coordinate, stableChannelIds],
  );

  React.useEffect(() => {
    if (coordinate === null) return;
    let disposed = false;
    const unsubscribes = new Set<() => void>();
    const since = Math.floor(Date.now() / 1_000);
    const subscribedChannelIds = channelKey === "" ? [] : channelKey.split(",");
    const filters: RelaySubscriptionFilter[] = [
      { kinds: [KIND_PULSE_ENTRY], "#a": [coordinate], since, limit: 100 },
      ...channelChunks(subscribedChannelIds).map((channels) => ({
        kinds: LIVE_SESSION_FACT_KINDS,
        "#h": channels,
        since,
        limit: 100,
      })),
    ];
    // The entry fan-out and every 128-channel chunk share one REQ (one
    // admission unit) up to the relay's ten filters per REQ.
    // Ten filters per REQ *and* 128 channels per REQ: the relay counts `#h`
    // across every filter in the frame.
    const groups = chunkFiltersForRequest(
      filters.map((filter) => ({ filter })),
      { maxFilters: MAX_FILTERS_PER_REQ },
    ).map((group) => group.map((entry) => entry.filter));
    for (const group of groups) {
      void relayClient
        .subscribeLiveMany(group, () => {
          void queryClient.invalidateQueries({ queryKey: key });
        })
        .then((handle) => {
          if (!handle) return;
          if (disposed) void handle();
          else unsubscribes.add(handle);
        })
        .catch(() => {
          // The 60-second cold read below is the fallback. Its failures enter
          // digest.errors; a live-subscription transport failure does not
          // manufacture a durable read result.
        });
    }
    return () => {
      disposed = true;
      for (const unsubscribe of unsubscribes) unsubscribe();
      unsubscribes.clear();
    };
  }, [channelKey, coordinate, key, queryClient]);

  // Nudged ±10 % per (identity, project) so this poll drifts apart from every
  // other 60 s timer in the app and on the other device sharing this key.
  const identity = useIdentityQuery();
  const query = useQuery({
    queryKey: key,
    enabled: coordinate !== null,
    refetchInterval: phaseJitteredPeriodMs(
      `project-pulse:${coordinate ?? "none"}`,
      PULSE_DIGEST_REFETCH_INTERVAL_MS,
      identity.data?.pubkey,
    ),
    queryFn: () =>
      fetchProjectPulseDigest(coordinate ?? "", stableChannelIds, {
        channelsUnresolved,
      }),
  });

  React.useEffect(() => {
    if (!query.data) return;
    const delay = pulseLeaseExpiryDelayMs(query.data);
    if (delay === null) return;
    const timer = window.setTimeout(() => {
      void queryClient.invalidateQueries({ queryKey: key });
    }, delay);
    return () => window.clearTimeout(timer);
  }, [key, query.data, queryClient]);

  const digest =
    query.data ?? (coordinate ? readProjectPulseDigest(coordinate) : null);
  if (!query.data || query.isPending) {
    return { kind: "loading", digest };
  }
  return query.data.complete
    ? { kind: "ready", digest: query.data, refreshing: query.isFetching }
    : { kind: "partial", digest: query.data, refreshing: query.isFetching };
}

// ── Mission rows ─────────────────────────────────────────────────────────────
//
// The Missions half of Pulse is a **sibling** read: `pulse_mission_rows` is a
// native command whose response adds eight keys beside the digest above and
// retypes none of it. Nothing here folds — `buzz-core` writes every sentence,
// and this module only decides when to ask and what a failure means.

/**
 * React Query key for one project's mission rows.
 *
 * Order-insensitive on the channel set for the same reason the digest key is:
 * the caller hands back a fresh array every render and a key that moved with
 * it would re-read on every paint.
 */
export function pulseMissionRowsQueryKey(
  coordinate: string,
  channelIds: readonly string[],
): readonly unknown[] {
  return [
    "project-pulse-missions",
    coordinate,
    [...channelIds].sort().join(","),
  ];
}

/** What the surface knows about a project's mission rows right now. */
export type PulseMissionRowsState =
  | { kind: "loading"; rows: null; refreshing: false; message: null }
  | {
      kind: "ready";
      rows: PulseMissionRowsResponse;
      refreshing: boolean;
      message: null;
    }
  | {
      kind: "unreadable";
      rows: PulseMissionRowsResponse | null;
      refreshing: false;
      message: string;
    };

/**
 * Map one query result onto the three answers this surface distinguishes.
 *
 * `unreadable` exists so a failed or refused read can never render as "no
 * missions". A decode rejection means the producer and this reader disagree
 * about the contract; a project with no missions and a project whose missions
 * could not be read are different claims and never share a rendering.
 *
 * Exported as a pure function so the distinction is testable without a
 * QueryClient — the component tree that renders it takes rows as a prop and
 * fetches nothing.
 */
export function pulseMissionRowsState(
  query: {
    data: PulseMissionRowsResponse | undefined;
    error: unknown;
    isPending: boolean;
    isFetching: boolean;
  },
  cached: PulseMissionRowsResponse | null,
): PulseMissionRowsState {
  if (query.error) {
    return {
      kind: "unreadable",
      rows: cached,
      refreshing: false,
      message: errorMessage(query.error),
    };
  }
  if (query.data) {
    return {
      kind: "ready",
      rows: query.data,
      refreshing: query.isFetching,
      message: null,
    };
  }
  // A cached answer is the *last complete read*, not the current one. It paints
  // (a surface that blanks on every refetch is worse) but is marked, because a
  // seat that reported since that read is simply absent from it.
  if (cached) {
    return { kind: "ready", rows: cached, refreshing: true, message: null };
  }
  return { kind: "loading", rows: null, refreshing: false, message: null };
}

/** What a mission read is about, beyond the coordinate and the channel floor. */
export type PulseMissionRowsInput = {
  /**
   * The digest whose open sessions this read opens.
   *
   * The count that reaches the native command is the number of open sessions
   * **this digest proved**, never the number the gather managed to read: the
   * command subtracts one from the other and discloses the difference, so a
   * count taken after the gather would silence exactly the sentence that
   * admits a session went unread.
   */
  digest: ProjectPulseDigest | null;
  /** The viewer's own pubkey, so `{Who}` can say "You". */
  viewerPubkey?: string | null;
  /** Display names by lowercase-hex pubkey; unresolved ones degrade to hex. */
  displayNames?: Readonly<Record<string, string>>;
};

/** Read and decode one project's mission rows. Exported for tests. */
export async function fetchPulseMissionRows(
  coordinate: string,
  channelIds: readonly string[],
  input: PulseMissionRowsInput,
  dependencies: {
    invoke?: PulseMissionRowsInvoker;
    fetchEvents?: PulseEventFetcher;
    relaySelf?: () => Promise<string | null>;
  } = {},
): Promise<PulseMissionRowsResponse> {
  const openSessions = pulseMissionOpenSessions(input.digest);
  const read = await readPulseMissionSessions(
    { channelIds, openSessions },
    {
      fetchEvents:
        dependencies.fetchEvents ??
        ((filter) => relayClient.fetchEventsCoalesced(filter)),
      relaySelf: dependencies.relaySelf ?? getRelaySelf,
    },
  );
  const rows = await invokePulseMissionRows(
    {
      project: coordinate,
      channelIds,
      openSessionCount: openSessions.length,
      sessions: read.sessions,
      readErrors: read.readErrors,
      viewerPubkey: input.viewerPubkey ?? null,
      displayNames: input.displayNames,
    },
    dependencies,
  );
  rememberPulseMissionRows(coordinate, rows);
  return rows;
}

/**
 * One project's mission rows, on the same 60s fallback cadence as the digest.
 *
 * No live subscription of its own: the rows are folded from the same signed
 * facts the digest already subscribes to, and a second subscription over the
 * same kinds would double the fan-out to say the same thing twice.
 *
 * The digest is what this read is *about*: its open sessions are the ones
 * opened, and its open-session count is what the native command measures the
 * gather against. Without one the read has no umbrella to open, which is
 * exactly the state the command's own unread disclosure names.
 */
export function usePulseMissionRows(
  coordinate: string | null,
  channelIds: readonly string[],
  options: {
    /**
     * The digest this read is a sibling of. Defaults to the last complete
     * digest folded for this coordinate, so the hook is honest at the call
     * site it already has; a caller holding the *live* digest should pass it,
     * because a cached one is one read behind.
     */
    digest?: ProjectPulseDigest | null;
    /** Display names by lowercase-hex pubkey. Unresolved ones read as hex. */
    displayNames?: Readonly<Record<string, string>>;
  } = {},
): PulseMissionRowsState {
  const channelKey = [...channelIds].sort().join(",");
  const stableChannelIds = React.useMemo(
    () => (channelKey === "" ? [] : channelKey.split(",")),
    [channelKey],
  );
  const viewerPubkey = useIdentityQuery().data?.pubkey ?? null;
  const digest =
    options.digest ?? (coordinate ? readProjectPulseDigest(coordinate) : null);
  const openSessions = React.useMemo(
    () => pulseMissionOpenSessions(digest),
    [digest],
  );
  // The open-session set is part of the key: a session opening or closing
  // changes what this read is about, and without it the rows would keep
  // answering about the umbrellas that were open a minute ago.
  const openSessionKey = openSessions
    .map((session) => session.sessionKey)
    .sort()
    .join(",");
  const key = React.useMemo(
    () => [
      ...pulseMissionRowsQueryKey(coordinate ?? "none", stableChannelIds),
      openSessionKey,
      viewerPubkey,
    ],
    [coordinate, openSessionKey, stableChannelIds, viewerPubkey],
  );
  const displayNames = options.displayNames;
  const query = useQuery({
    queryKey: key,
    enabled: coordinate !== null,
    refetchInterval: 60_000,
    // A contract disagreement does not heal by asking again, and retrying one
    // hides it behind three more seconds of "loading" before it is disclosed.
    retry: false,
    queryFn: () =>
      fetchPulseMissionRows(coordinate ?? "", stableChannelIds, {
        digest,
        viewerPubkey,
        displayNames,
      }),
  });
  return pulseMissionRowsState(
    {
      data: query.data,
      error: query.error,
      isPending: query.isPending,
      isFetching: query.isFetching,
    },
    coordinate ? readPulseMissionRows(coordinate) : null,
  );
}

// ── Declared work ────────────────────────────────────────────────────────────
//
// The third read is paged, and it lives in `pulseDeclaredWorkQueries.ts` —
// this module was at the repository's 1000-line ceiling and a gate that says
// "split the file" is not a gate to squeeze under. Re-exported here so the
// three Pulse reads still have one import path.

export {
  fetchPulseDeclaredWorkPage,
  type PulseDeclaredWorkPage,
  type PulseDeclaredWorkPageInput,
  pulseDeclaredWorkNextPageParam,
  pulseDeclaredWorkQueryKey,
  type PulseDeclaredWorkQuery,
  pulseDeclaredWorkState,
  type PulseDeclaredWorkState,
  usePulseDeclaredWork,
} from "./pulseDeclaredWorkQueries";
