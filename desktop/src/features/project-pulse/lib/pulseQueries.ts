/**
 * Reads behind Project Pulse: two filters, one fold.
 *
 * Entries come back by `#a` on the project coordinate; coding-session facts
 * come back by `#h` on the project's own channels. There is no queryable
 * "sessions of this project" relation — 44223 carries no `a` tag — and a
 * community-wide 44223 scan is forbidden because it is unbounded and it leaks.
 * A session running in a channel outside the project's channel set is
 * therefore not discoverable in v1, which is why the digest carries
 * `sessionsScope: "project channels"` and no surface may present Active work
 * as exhaustive.
 *
 * Every channel that fails records a `{scope, message}` in `errors[]` and
 * flips `complete` to false. A read error must never render as a quiet
 * project.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { parseBuzzCodingSessionMetadata } from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_CLOSURE,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_NAME,
  KIND_PULSE_ENTRY,
} from "@/shared/constants/kinds";
import { hasValidSignature } from "@/shared/lib/authors";

import {
  readProjectPulseDigest,
  rememberProjectPulseDigest,
} from "./projectPulseCache";
import {
  foldProjectPulseDigest,
  type ProjectPulseDigest,
  type PulseDigestError,
} from "./pulseFold.ts";
import type { PulseEvent } from "./pulseEntry.ts";

/** Upper bound on entries fetched in one read; a truncated page is a partial read. */
export const PULSE_ENTRY_QUERY_LIMIT = 500;
/** Upper bound on session facts fetched in one read. */
export const PULSE_SESSION_QUERY_LIMIT = 1000;

const SESSION_FACT_KINDS = [
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_GOAL,
  KIND_CODING_SESSION_NAME,
  KIND_CODING_SESSION_CLOSURE,
];

/** The one relay read this module needs; injectable so tests fold real bytes. */
export type PulseEventFetcher = (
  filter: RelaySubscriptionFilter,
) => Promise<RelayEvent[]>;

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
      parseBuzzCodingSessionMetadata(event.content) === null
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
    fetchEvents?: PulseEventFetcher;
    /**
     * True when the caller could not resolve the project's channel set (the
     * channels query is pending or failed). The set below is then a floor, not
     * the project's channels, and the digest is reported partial.
     */
    channelsUnresolved?: boolean;
  } = {},
): Promise<ProjectPulseDigest> {
  const fetchEvents: PulseEventFetcher =
    dependencies.fetchEvents ?? ((filter) => relayClient.fetchEvents(filter));
  const sourceErrors: PulseDigestError[] = [];
  const events: RelayEvent[] = [];

  try {
    const entries = await fetchEvents({
      kinds: [KIND_PULSE_ENTRY],
      "#a": [coordinate],
      limit: PULSE_ENTRY_QUERY_LIMIT,
    });
    events.push(...entries);
    if (entries.length >= PULSE_ENTRY_QUERY_LIMIT) {
      sourceErrors.push({
        scope: "entries",
        message: `entry read truncated at ${PULSE_ENTRY_QUERY_LIMIT} events`,
      });
    }
  } catch (error) {
    sourceErrors.push({ scope: "entries", message: errorMessage(error) });
  }

  // An unresolved channel set is a read that did not happen, not a project
  // with no channels. Recorded before the guard below so it lands in `errors[]`
  // whether or not a partial set came back — mirroring the CLI's
  // `scan_project_sessions`, which pushes a `{scope:"channels"}` error and
  // flips `complete` on both failure and truncation.
  if (dependencies.channelsUnresolved) {
    sourceErrors.push({
      scope: "channels",
      message:
        "the project's channel set could not be read; session facts were not queried",
    });
  }

  if (channelIds.length > 0) {
    try {
      const sessions = await fetchEvents({
        kinds: SESSION_FACT_KINDS,
        "#h": [...channelIds],
        limit: PULSE_SESSION_QUERY_LIMIT,
      });
      events.push(...sessions);
      if (sessions.length >= PULSE_SESSION_QUERY_LIMIT) {
        sourceErrors.push({
          scope: "sessions",
          message: `session read truncated at ${PULSE_SESSION_QUERY_LIMIT} events`,
        });
      }
    } catch (error) {
      sourceErrors.push({ scope: "sessions", message: errorMessage(error) });
    }
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
  | { kind: "ready"; digest: ProjectPulseDigest }
  | { kind: "partial"; digest: ProjectPulseDigest };

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
  const key = React.useMemo(
    () =>
      projectPulseQueryKey(
        coordinate ?? "none",
        channelKey.split(","),
        channelsUnresolved,
      ),
    [channelKey, channelsUnresolved, coordinate],
  );

  React.useEffect(() => {
    if (coordinate === null) return;
    let disposed = false;
    let unsubscribe: (() => void) | null = null;
    void relayClient
      .subscribeLive(
        {
          kinds: [KIND_PULSE_ENTRY],
          "#a": [coordinate],
          since: Math.floor(Date.now() / 1_000),
          limit: 100,
        },
        () => {
          void queryClient.invalidateQueries({ queryKey: key });
        },
      )
      .then((handle) => {
        if (disposed) handle?.();
        else unsubscribe = handle ?? null;
      })
      .catch(() => {
        // The poll below is the fallback; a failed live subscription is not a
        // read failure and must not be reported as one.
      });
    return () => {
      disposed = true;
      unsubscribe?.();
    };
  }, [coordinate, key, queryClient]);

  const query = useQuery({
    queryKey: key,
    enabled: coordinate !== null,
    refetchInterval: 60_000,
    queryFn: () =>
      fetchProjectPulseDigest(coordinate ?? "", channelIds, {
        channelsUnresolved,
      }),
  });

  const digest =
    query.data ?? (coordinate ? readProjectPulseDigest(coordinate) : null);
  if (!query.data || query.isPending) {
    return { kind: "loading", digest };
  }
  return query.data.complete
    ? { kind: "ready", digest: query.data }
    : { kind: "partial", digest: query.data };
}
