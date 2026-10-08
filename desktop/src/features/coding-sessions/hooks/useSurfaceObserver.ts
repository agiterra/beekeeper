/**
 * Watch one shared surface (a session's Browser, or one device slot) as a
 * read-only observer (WIRE-C5 §§ 3–4), the `useShellObserver` shape for
 * kinds 24320/24321. The timing is the pure machine in
 * `surfaceObserverState.ts`; this hook owns the subscription, the timers
 * and the publishes, and nothing outlives the component (no module cache,
 * so `resetCommunityState()` has nothing to reset).
 *
 * Also here: `useSurfaceStoredEvents`, the bounded read + live merge of a
 * channel's stored surface records that the Device and Browser surfaces
 * share (React Query, keyed by channel and filter).
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import { isRateLimited } from "@/shared/api/relayRateLimitGate";
import { signRelayEvent } from "@/shared/api/tauri";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_SURFACE_FRAME,
  KIND_SURFACE_SNAPSHOT,
  KIND_SURFACE_WATCH,
} from "@/shared/constants/kinds";

import {
  parseSurfaceSnapshot,
  type SurfaceKind,
} from "../lib/codingSessionSurfaceSnapshot";
import {
  initialSurfaceObserverState,
  parseSurfaceFrame,
  SURFACE_WATCH_KEEPALIVE_MS,
  type SurfaceFrame,
  type SurfaceObserverState,
  type SurfaceObserverStatus,
  surfaceObserverApplyFrame,
  surfaceObserverBeat,
  surfaceObserverCanRequestSnapshot,
  surfaceObserverSnapshotAnswered,
  surfaceObserverSnapshotRequested,
  surfaceObserverTick,
  surfaceObserverWatchSent,
} from "../lib/surfaceObserverState";

export type SurfaceWatchAction = "watch" | "stop" | "resync" | "snapshot";

/** What to watch: one surface key, from its announced producer. */
export type SurfaceObserverTarget = {
  channelId: string;
  surface: SurfaceKind;
  /** preview: sessionRef; device: slot. */
  key: string;
  producerPubkey: string;
  /** This viewer's pubkey, to recognise the 44253 that answers a request. */
  viewerPubkey?: string | null;
};

/** The 24320 tags, in the wire's exact order. */
export function buildSurfaceWatchTags(
  target: Pick<
    SurfaceObserverTarget,
    "channelId" | "surface" | "key" | "producerPubkey"
  >,
): string[][] {
  return [
    ["h", target.channelId],
    ["surface", target.surface],
    ["d", target.key],
    ["p", target.producerPubkey],
  ];
}

async function publishSurfaceWatch(
  target: SurfaceObserverTarget,
  action: SurfaceWatchAction,
): Promise<void> {
  const event = await signRelayEvent({
    kind: KIND_SURFACE_WATCH,
    content: JSON.stringify({ action }),
    tags: buildSurfaceWatchTags(target),
  });
  await relayClient.publishEvent(
    event,
    "Timed out while contacting the host.",
    "Failed to contact the host.",
  );
}

type Action =
  | { type: "reset" }
  | { type: "watch-sent"; now: number }
  | { type: "frame"; frame: SurfaceFrame; now: number }
  | { type: "tick"; now: number }
  | { type: "beat" }
  | { type: "snapshot-requested"; now: number }
  | { type: "snapshot-answered"; takenAt: number };

function reduce(
  state: SurfaceObserverState,
  action: Action,
): SurfaceObserverState {
  switch (action.type) {
    case "reset":
      return initialSurfaceObserverState();
    case "watch-sent":
      return surfaceObserverWatchSent(state, action.now);
    case "frame":
      return surfaceObserverApplyFrame(state, action.frame, action.now);
    case "tick":
      return surfaceObserverTick(state, action.now);
    case "beat":
      return surfaceObserverBeat(state).state;
    case "snapshot-requested":
      return surfaceObserverSnapshotRequested(state, action.now);
    case "snapshot-answered":
      return surfaceObserverSnapshotAnswered(state, action.takenAt);
  }
}

export type SurfaceObserverResult = {
  status: SurfaceObserverStatus;
  frame: SurfaceFrame | null;
  /** The shown frame's time (ms): `captured-at`, never later than arrival. */
  lastFrameAt: number | null;
  cadenceMs: number | null;
  /** A 1 s clock for ages. */
  now: number;
  /** Ask the producer for one 44253 (≤ 1 per 10 s); false when refused here. */
  requestSnapshot: () => Promise<boolean>;
  snapshotPending: boolean;
  /** No answering 44253 within 15 s: "The host did not answer." */
  snapshotTimedOut: boolean;
  /** The last request could not be sent (a sentence), or null. */
  snapshotError: string | null;
};

/**
 * One observer for either surface. `target` must be memoized on its fields
 * by the caller; `null` watches nothing (and sends nothing).
 */
export function useSurfaceObserver(
  target: SurfaceObserverTarget | null,
): SurfaceObserverResult {
  const [state, dispatch] = React.useReducer(
    reduce,
    undefined,
    initialSurfaceObserverState,
  );
  const [now, setNow] = React.useState(() => Date.now());
  const [snapshotError, setSnapshotError] = React.useState<string | null>(null);
  const stateRef = React.useRef(state);
  stateRef.current = state;

  React.useEffect(() => {
    const clock = window.setInterval(() => {
      const at = Date.now();
      setNow(at);
      dispatch({ type: "tick", now: at });
    }, 1_000);
    return () => window.clearInterval(clock);
  }, []);

  React.useEffect(() => {
    dispatch({ type: "reset" });
    setSnapshotError(null);
    if (!target) return;
    let disposed = false;
    let leave: (() => Promise<void>) | null = null;
    const send = (action: SurfaceWatchAction) =>
      publishSurfaceWatch(target, action).catch((error) => {
        if (action !== "stop") {
          console.warn("surface-observe: watch publish failed", error);
        }
      });
    const visible = () =>
      typeof document === "undefined" || document.visibilityState === "visible";

    const onEvent = (event: RelayEvent) => {
      if (disposed) return;
      if (event.kind === KIND_SURFACE_SNAPSHOT) {
        const card = parseSurfaceSnapshot(event);
        if (
          card &&
          card.surface === target.surface &&
          card.key === target.key &&
          card.channelId === target.channelId &&
          card.signer === target.producerPubkey &&
          target.viewerPubkey &&
          card.requestedBy === target.viewerPubkey
        ) {
          dispatch({ type: "snapshot-answered", takenAt: card.takenAt });
        }
        return;
      }
      const frame = parseSurfaceFrame(event, target);
      if (frame) dispatch({ type: "frame", frame, now: Date.now() });
    };

    const since = Math.floor(Date.now() / 1_000) - 30;
    const filters: RelaySubscriptionFilter[] = [
      {
        kinds: [KIND_SURFACE_FRAME],
        authors: [target.producerPubkey],
        "#d": [target.key],
        "#h": [target.channelId],
        since,
        limit: 500,
      },
      {
        kinds: [KIND_SURFACE_SNAPSHOT],
        authors: [target.producerPubkey],
        "#d": [target.key],
        "#h": [target.channelId],
        since,
        limit: 50,
      },
    ];
    void (async () => {
      try {
        const unsubscribe = await relayClient.subscribeLiveMany(
          filters,
          onEvent,
        );
        if (disposed) {
          void unsubscribe();
          return;
        }
        leave = unsubscribe;
      } catch (error) {
        console.warn("surface-observe: subscribe failed", error);
      }
      if (disposed || !visible()) return;
      dispatch({ type: "watch-sent", now: Date.now() });
      await send("watch");
    })();

    const keepalive = window.setInterval(() => {
      // Droppable: while the relay's rate-limit gate is armed a beat would
      // only queue; the producer tolerates two missed beats (45 s expiry).
      if (disposed || !visible() || isRateLimited()) return;
      // Decide from the last rendered state; the reducer clears the flag.
      const { action } = surfaceObserverBeat(stateRef.current);
      dispatch({ type: "beat" });
      void send(action);
    }, SURFACE_WATCH_KEEPALIVE_MS);

    const onVisibility = () => {
      if (disposed || !visible()) return;
      dispatch({ type: "watch-sent", now: Date.now() });
      void send("watch");
    };
    document.addEventListener("visibilitychange", onVisibility);

    return () => {
      disposed = true;
      window.clearInterval(keepalive);
      document.removeEventListener("visibilitychange", onVisibility);
      void leave?.();
      void send("stop");
    };
  }, [target]);

  const requestSnapshot = React.useCallback(async () => {
    if (!target) return false;
    const at = Date.now();
    if (!surfaceObserverCanRequestSnapshot(stateRef.current, at)) return false;
    dispatch({ type: "snapshot-requested", now: at });
    setSnapshotError(null);
    try {
      await publishSurfaceWatch(target, "snapshot");
      return true;
    } catch (error) {
      setSnapshotError(
        error instanceof Error ? error.message : "Failed to contact the host.",
      );
      return false;
    }
  }, [target]);

  return {
    status: state.status,
    frame: state.frame,
    lastFrameAt: state.frameAt,
    cadenceMs: state.cadenceMs,
    now,
    requestSnapshot,
    snapshotPending: state.snapshot.pendingSince !== null,
    snapshotTimedOut: state.snapshot.timedOut,
    snapshotError,
  };
}

// ---------------------------------------------------------------------------
// Stored surface records: one bounded read, then a live merge.
// ---------------------------------------------------------------------------

/** How many stored records one read keeps. */
export const SURFACE_STORED_EVENTS_LIMIT = 500;

/** Merge one live event by id; unchanged data stays identical. */
export function mergeSurfaceStoredEvent(
  data: readonly RelayEvent[] | undefined,
  event: RelayEvent,
): readonly RelayEvent[] | undefined {
  if (!data || data.some((held) => held.id === event.id)) return data;
  return [...data, event];
}

/**
 * A channel's stored surface records of `kinds` (optionally narrowed by
 * `#d`), read once and kept current by a live REQ from the moment it opened.
 */
export function useSurfaceStoredEvents(input: {
  channelId: string | null;
  kinds: readonly number[];
  dTags?: readonly string[] | null;
  enabled?: boolean;
}): {
  events: readonly RelayEvent[];
  isLoading: boolean;
  errorMessage: string | null;
} {
  const queryClient = useQueryClient();
  const { channelId } = input;
  const kindsKey = input.kinds.join(",");
  const dKey = input.dTags ? input.dTags.join(",") : null;
  const enabled =
    (input.enabled ?? true) &&
    channelId !== null &&
    channelId !== "" &&
    (dKey === null || dKey !== "");
  const key = React.useMemo(
    () =>
      ["coding-session-surface-records", channelId, kindsKey, dKey] as const,
    [channelId, dKey, kindsKey],
  );
  const filterFor = React.useCallback(
    (extra: { since?: number; limit: number }): RelaySubscriptionFilter => ({
      kinds: kindsKey.split(",").map(Number),
      "#h": [channelId ?? ""],
      ...(dKey !== null ? { "#d": dKey.split(",") } : {}),
      ...extra,
    }),
    [channelId, dKey, kindsKey],
  );
  const query = useQuery({
    queryKey: key,
    enabled,
    retry: false,
    queryFn: () =>
      relayClient.fetchEvents(
        filterFor({ limit: SURFACE_STORED_EVENTS_LIMIT }),
      ),
  });
  React.useEffect(() => {
    if (!enabled) return;
    let disposed = false;
    let leave: (() => Promise<void>) | null = null;
    const since = Math.floor(Date.now() / 1_000);
    void (async () => {
      try {
        const unsubscribe = await relayClient.subscribeLiveMany(
          [filterFor({ since, limit: 0 })],
          (event) => {
            queryClient.setQueryData<readonly RelayEvent[]>(key, (data) =>
              mergeSurfaceStoredEvent(data ?? [], event),
            );
          },
        );
        if (disposed) {
          void unsubscribe();
          return;
        }
        leave = unsubscribe;
      } catch {
        // A refused live REQ leaves the one-shot read as the only source.
      }
    })();
    return () => {
      disposed = true;
      void leave?.();
    };
  }, [enabled, filterFor, key, queryClient]);
  const error = query.error;
  return React.useMemo(
    () => ({
      events: query.data ?? EMPTY_EVENTS,
      isLoading: enabled && query.isLoading,
      errorMessage:
        error instanceof Error ? error.message : error ? String(error) : null,
    }),
    [enabled, error, query.data, query.isLoading],
  );
}

const EMPTY_EVENTS: readonly RelayEvent[] = Object.freeze([]);
