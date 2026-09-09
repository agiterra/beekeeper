/**
 * The paged Pulse read: declared work across a project's visible sessions.
 *
 * A sibling of `pulseQueries.ts` rather than a section inside it — that module
 * sits at the repository's 1000-line ceiling, and the gate's instruction is to
 * split rather than to squeeze under it. `pulseQueries.ts` re-exports every
 * name below, so the three Pulse reads still share one import path.
 *
 * The digest answers "which sessions exist"; this read opens them eight at a
 * time, newest observation first, **including closed ones** — closing an
 * execution settles no assignment, so a read that started from open sessions
 * alone would render unresolved work as work that does not exist.
 *
 * Each page is one gather plus one native invoke, and pages are disjoint
 * slices of one sorted list. Nothing is folded here: `buzz-core` decides what
 * was canonically included and settled, `pulseDeclaredWork.ts` decides what
 * the section says, and this module decides only when to ask.
 */
import * as React from "react";
import { useInfiniteQuery } from "@tanstack/react-query";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { useIdentityQuery } from "@/shared/api/hooks";
import { relayClient } from "@/shared/api/relayClient";

import {
  invokePulseDeclaredWork,
  type PulseDeclaredWorkInvoker,
} from "./invokePulseDeclaredWork";
import type { PulseMissionReadError } from "./invokePulseMissionRows";
import { readProjectPulseDigest } from "./projectPulseCache";
import type { PulseDeclaredWorkPageError } from "./pulseDeclaredWork";
import type {
  PulseDeclaredWorkResponse,
  PulseDeclaredWorkSessionInput,
} from "./pulseDeclaredWorkWire";
import type { ProjectPulseDigest } from "./pulseFold.ts";
import {
  buildPulseDeclaredWorkSessionInput,
  PULSE_DECLARED_WORK_MAX_PAGES,
  PULSE_DECLARED_WORK_PAGE_SIZE,
  pulseDeclaredWorkPage,
  pulseDeclaredWorkSessions,
  readPulseMissionSessions,
} from "./pulseMissionSessionRead";
import type { PulseEventFetcher } from "./pulseQueries";

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Frozen empties, returned by identity.
 *
 * The state object is rebuilt on every render; a fresh `[]` in it defeats the
 * section's `React.memo` on every paint, and the projection below it is not
 * cheap. One shared reference per empty case keeps the common state — no page
 * errors, no pages yet — reference-stable.
 */
const NO_PAGE_ERRORS: readonly PulseDeclaredWorkPageError[] = Object.freeze([]);
const NO_PAGES: readonly PulseDeclaredWorkPage[] = Object.freeze([]);

/**
 * React Query key for one project's declared work.
 *
 * Order-insensitive on the channel set, like the digest and mission keys:
 * the caller hands back a fresh array every render.
 */
export function pulseDeclaredWorkQueryKey(
  coordinate: string,
  channelIds: readonly string[],
): readonly unknown[] {
  return [
    "project-pulse-declared",
    coordinate,
    [...channelIds].sort().join(","),
  ];
}

/** One loaded page: which page it is, what came back, and what it asked about. */
export type PulseDeclaredWorkPage = {
  pageIndex: number;
  response: PulseDeclaredWorkResponse;
  /** The session keys this page asked about, in the order it asked. */
  sessionKeys: string[];
};

/**
 * The next page's index, or `undefined` when this read is done asking.
 *
 * Two bounds, both required: there must be an unread visible session *and*
 * the scan's own page cap must allow another request. Exported because the
 * scan sentence's `capped` claim and this predicate must agree — a control
 * offering a page the read will not fetch is the same lie in a button.
 */
export function pulseDeclaredWorkNextPageParam(
  pageIndex: number,
  visibleSessionCount: number,
  pageSize: number = PULSE_DECLARED_WORK_PAGE_SIZE,
  maxPages: number = PULSE_DECLARED_WORK_MAX_PAGES,
): number | undefined {
  const next = pageIndex + 1;
  if (next >= maxPages) return undefined;
  if (next * pageSize >= visibleSessionCount) return undefined;
  return next;
}

/** What a declared-work page read is about, beyond coordinate and channels. */
export type PulseDeclaredWorkPageInput = {
  /**
   * The digest whose sessions this page opens, and the **only** source of each
   * session's lifecycle. A `team-only` gather does not read the lifecycle
   * kinds, so a session whose lifecycle this digest does not carry is dropped
   * and disclosed rather than sent as open.
   */
  digest: ProjectPulseDigest | null;
  /** Which slice of the visible sessions to read: `[8n, 8n + 8)`. */
  pageIndex: number;
  /** The viewer's own pubkey, or null when this surface has no identity. */
  viewerPubkey?: string | null;
};

/** Read and decode one page of declared work. Exported for tests. */
export async function fetchPulseDeclaredWorkPage(
  coordinate: string,
  channelIds: readonly string[],
  input: PulseDeclaredWorkPageInput,
  dependencies: {
    invoke?: PulseDeclaredWorkInvoker;
    fetchEvents?: PulseEventFetcher;
    relaySelf?: () => Promise<string | null>;
  } = {},
): Promise<PulseDeclaredWorkPage> {
  const visible = pulseDeclaredWorkSessions(input.digest);
  const page = pulseDeclaredWorkPage(visible, input.pageIndex);
  const lifecycles = new Map(
    page.map((session) => [session.sessionKey, session.lifecycle] as const),
  );
  const read = await readPulseMissionSessions(
    { channelIds, openSessions: page },
    {
      fetchEvents:
        dependencies.fetchEvents ??
        ((filter) => relayClient.fetchEventsCoalesced(filter)),
      relaySelf: dependencies.relaySelf ?? getRelaySelf,
    },
    { records: "team-only" },
  );
  const readErrors: PulseMissionReadError[] = [...read.readErrors];
  const sessions: PulseDeclaredWorkSessionInput[] = [];
  for (const session of read.sessions) {
    const lifecycle = lifecycles.get(session.sessionKey);
    if (!lifecycle) {
      // Unreachable while the page is sliced from the same digest, and named
      // rather than defaulted because "open" is a claim about a session this
      // read cannot make from the records it asked for.
      readErrors.push({
        scope: `declared:${session.sessionKey}`,
        message:
          "this session's lifecycle was not in the digest this page was sliced from, so its records were not sent",
      });
      continue;
    }
    sessions.push(buildPulseDeclaredWorkSessionInput(session, lifecycle));
  }
  const response = await invokePulseDeclaredWork(
    {
      project: coordinate,
      channelIds,
      sessions,
      readErrors,
      viewerPubkey: input.viewerPubkey ?? null,
    },
    dependencies,
  );
  return {
    pageIndex: input.pageIndex,
    response,
    sessionKeys: page.map((session) => session.sessionKey),
  };
}

/** What the surface knows about a project's declared work right now. */
export type PulseDeclaredWorkState = {
  kind: "loading" | "ready" | "unreadable";
  /**
   * The pages that came back, in load order.
   *
   * Whole pages, not bare responses: each one carries the session keys it
   * *asked* about, and the projection needs that to tell a session whose
   * records could not be read from one no page has reached yet.
   */
  pages: readonly PulseDeclaredWorkPage[];
  /** The pages that did not. Disclosed; never rendered as an empty page. */
  pageErrors: readonly PulseDeclaredWorkPageError[];
  loadedPageCount: number;
  visibleSessionCount: number;
  hasNextPage: boolean;
  isFetchingNextPage: boolean;
  refreshing: boolean;
  fetchNextPage: () => void;
  refetch: () => void;
  message: string | null;
};

/** The infinite-query surface this reducer reads. Narrow on purpose. */
export type PulseDeclaredWorkQuery = {
  data: { pages: readonly PulseDeclaredWorkPage[] } | undefined;
  error: unknown;
  isPending: boolean;
  isFetching: boolean;
  isFetchingNextPage: boolean;
  /** Whether the error, if any, came from the attempt to add a page. */
  isFetchNextPageError: boolean;
  hasNextPage: boolean;
  fetchNextPage: () => void;
  refetch: () => void;
};

/**
 * Map one infinite query onto the three answers this section distinguishes.
 *
 * A failed *later* page never discards the earlier ones: those pages were
 * read, their sessions were scanned, and dropping them would hide declared
 * work behind an unrelated network failure. The failure becomes a
 * `pageErrors` row instead, which the projection turns into a limitation and
 * the scan sentence counts as unscanned.
 *
 * Only a read with **no** page at all is `unreadable`, and a read with no page
 * and no error is `loading`. Neither is ever "no declared work".
 */
export function pulseDeclaredWorkState(
  query: PulseDeclaredWorkQuery,
  visibleSessionCount: number,
): PulseDeclaredWorkState {
  // The array React Query holds, by reference: it changes only when the data
  // does, so the section's memo survives a render that changed nothing.
  const pages = query.data?.pages ?? NO_PAGES;
  const message = query.error ? errorMessage(query.error) : null;
  // Only a failed *next page* names a page: that page's sessions were never
  // read and the scan sentence must count them as unscanned. A failed refresh
  // of already-loaded pages is a different fact — those pages were read, they
  // are simply not current — and numbering it would name a page that did not
  // fail. It travels in `message` with `refreshing`, and never as a page that
  // could not be read.
  const pageErrors: readonly PulseDeclaredWorkPageError[] =
    message !== null && pages.length > 0 && query.isFetchNextPageError
      ? [{ pageIndex: pages.length, message }]
      : NO_PAGE_ERRORS;
  const shared = {
    pages,
    pageErrors,
    loadedPageCount: pages.length,
    visibleSessionCount,
    hasNextPage: query.hasNextPage,
    isFetchingNextPage: query.isFetchingNextPage,
    fetchNextPage: query.fetchNextPage,
    refetch: query.refetch,
  };
  if (pages.length > 0) {
    return {
      ...shared,
      kind: "ready",
      refreshing: query.isFetching,
      message,
    };
  }
  if (message !== null) {
    return { ...shared, kind: "unreadable", refreshing: false, message };
  }
  return { ...shared, kind: "loading", refreshing: false, message: null };
}

/**
 * One project's declared work, paged over its visible sessions.
 *
 * The visible-session list is part of the key for the same reason the mission
 * read keys on its open sessions: a session appearing, or closing, changes
 * what this read is about. Closing in particular changes nothing about the
 * assignment inside it, which is exactly why the page must be re-read rather
 * than kept.
 *
 * No live subscription of its own — the digest already subscribes to the same
 * signed facts — and no module-level cache: a paged read has no single "last
 * good answer" to bank, and banking one page would paint a scan sentence over
 * pages that were never re-read.
 */
export function usePulseDeclaredWork(
  coordinate: string | null,
  channelIds: readonly string[],
  options: {
    /** The digest this read pages over. Defaults to the last complete fold. */
    digest?: ProjectPulseDigest | null;
  } = {},
): PulseDeclaredWorkState {
  const channelKey = [...channelIds].sort().join(",");
  const stableChannelIds = React.useMemo(
    () => (channelKey === "" ? [] : channelKey.split(",")),
    [channelKey],
  );
  const viewerPubkey = useIdentityQuery().data?.pubkey ?? null;
  const digest =
    options.digest ?? (coordinate ? readProjectPulseDigest(coordinate) : null);
  const visibleSessions = React.useMemo(
    () => pulseDeclaredWorkSessions(digest),
    [digest],
  );
  const visibleSessionCount = visibleSessions.length;
  const visibleSessionKey = visibleSessions
    .map((session) => session.sessionKey)
    .sort()
    .join(",");
  const key = React.useMemo(
    () => [
      ...pulseDeclaredWorkQueryKey(coordinate ?? "none", stableChannelIds),
      visibleSessionKey,
      viewerPubkey,
    ],
    [coordinate, stableChannelIds, viewerPubkey, visibleSessionKey],
  );
  const query = useInfiniteQuery<PulseDeclaredWorkPage>({
    queryKey: key,
    enabled: coordinate !== null,
    refetchInterval: 60_000,
    // A contract disagreement does not heal by asking again, and a retry hides
    // it behind three more seconds of "loading" before it is disclosed.
    retry: false,
    initialPageParam: 0,
    queryFn: ({ pageParam }) =>
      fetchPulseDeclaredWorkPage(coordinate ?? "", stableChannelIds, {
        digest,
        pageIndex: typeof pageParam === "number" ? pageParam : 0,
        viewerPubkey,
      }),
    getNextPageParam: (lastPage) =>
      pulseDeclaredWorkNextPageParam(lastPage.pageIndex, visibleSessionCount),
  });
  return pulseDeclaredWorkState(
    {
      data: query.data,
      error: query.error,
      isPending: query.isPending,
      isFetching: query.isFetching,
      isFetchingNextPage: query.isFetchingNextPage,
      isFetchNextPageError: query.isFetchNextPageError,
      hasNextPage: query.hasNextPage,
      fetchNextPage: () => {
        void query.fetchNextPage();
      },
      refetch: () => {
        void query.refetch();
      },
    },
    visibleSessionCount,
  );
}
