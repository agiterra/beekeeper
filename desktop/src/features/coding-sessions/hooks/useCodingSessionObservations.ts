/**
 * Reads this umbrella's kind-44246 observations, folded by `buzz-core`'s rule.
 *
 * A read with an explicit refresh, kept live by a kind-44246 subscription on
 * the same `h`/`d` scope (SV-41): a burst of new observations triggers one
 * re-read after a 750 ms quiet spell, so a gate start that the provider closes
 * stops reading "running" without anybody asking again. Nothing here polls
 * (§8 I1); the subscription only wakes the read. A live re-read keeps the
 * previous answer on screen until the new one settles, and `readAtMs` says
 * when the answer on screen was read. `live` says whether the subscription is
 * up: when it is not, the answer is a snapshot and the running-gate surfaces
 * say "not live — read at HH:MM". It runs only while a scope is supplied,
 * so a mission nobody is auditing pays nothing for it (REVIEW-A3 F5's rule,
 * applied one level up).
 *
 * `knownAssignmentRefs` comes from the Mission fold this surface already
 * holds. It is not re-fetched: an `assignmentRef` is a **pointer**, and the
 * only thing the fold does with it is disclose the ones that resolve to
 * nothing — so a second relay read would buy one word of disclosure at the
 * price of a round trip.
 */
import * as React from "react";

import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type {
  ConnectionState,
  LiveSubscriptionReadiness,
  RelaySubscriptionFilter,
} from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import { KIND_CODING_SESSION_OBSERVATION } from "@/shared/constants/kinds";
import type { CodingSessionObservationLive } from "@/features/coding-sessions/lib/codingSessionObservationLiveness";
import {
  invokeCodingSessionObservationFold,
  type CodingSessionObservationFoldResult,
} from "@/features/coding-sessions/lib/invokeCodingSessionObservationFold";

const MAX_ERROR_MESSAGE_LENGTH = 4096;

/**
 * How many observations one read retains.
 *
 * Deliberately the fold's own ceiling: asking for more would hand `buzz-core`
 * events it is going to count as `truncated` anyway, and asking for fewer
 * would make this client's window look like the wire's.
 */
export const CODING_SESSION_OBSERVATION_HISTORY_LIMIT = 512;

/** The exact scope one observation read is about. */
export type CodingSessionObservationScope = {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
};

/**
 * The pubkeys whose `observed` claim this session honours (REVIEW-L5 F2).
 *
 * `null` is not `[]`. `null` means the caller could not resolve the session's
 * provider instances, so the fold checks nothing and reports
 * `provenanceChecked: false`; a list is a real answer, and a claim from outside
 * it is folded down to `declared` and disclosed.
 */
export type CodingSessionProviderPubkeys = readonly string[] | null;

/** What a caller needs to fetch observations; `relayClient` satisfies it. */
export type CodingSessionObservationClient = {
  fetchEventsCoalesced(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  /**
   * Live subscription; absent on a read-only client, which then reads once
   * and on `refresh` only. `onReady` reports how the relay answered the REQ:
   * a handle comes back after readiness of any kind, including a terminal
   * `closed` refusal, so the handle alone does not mean the subscription is up.
   */
  subscribeLive?(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
    onReady?: (readiness: LiveSubscriptionReadiness) => void,
    readinessTimeoutMs?: number,
  ): Promise<(() => Promise<void>) | undefined | null>;
  /**
   * Socket state transitions, fired at once with the current state. Absent on
   * a client that cannot report them; then `live` only knows the first EOSE.
   */
  subscribeToConnectionState?(
    listener: (state: ConnectionState) => void,
  ): () => void;
  /** Fired after a dropped socket reconnects and live REQs are replayed. */
  subscribeToReconnects?(listener: () => void): () => void;
};

/** Quiet spell after the last live observation before one re-read. */
export const CODING_SESSION_OBSERVATION_LIVE_DEBOUNCE_MS = 750;

/**
 * How long the live REQ may wait for its EOSE before readiness is reported as
 * `timeout`. The client reports readiness once, so a timeout leaves the line
 * reading "not live" even if the EOSE arrives later; a generous window keeps
 * that under-claim for genuinely slow relays.
 */
export const CODING_SESSION_OBSERVATION_LIVE_READY_TIMEOUT_MS = 10_000;

export type CodingSessionObservationsResult = {
  isLoading: boolean;
  errorMessage: string | null;
  /** The native fold's answer, or null while it is unknown. */
  result: CodingSessionObservationFoldResult | null;
  /** When `result` was read (this machine's clock), or null with no result. */
  readAtMs: number | null;
  /** Whether the live subscription is up; anything else means a snapshot. */
  live: CodingSessionObservationLive;
  refresh: () => void;
};

/**
 * The one filter an observation read needs, with explicit `kinds`.
 *
 * Omitting `kinds` triggers the relay's p-gate (403), so it is never omitted.
 * Scoped by `h` (channel), `d` (umbrella) and `csob-genesis` — the three tags
 * NIP-CSOB's envelope requires agree with the content.
 */
export function buildCodingSessionObservationFilters(
  scope: CodingSessionObservationScope,
  limit: number,
): RelaySubscriptionFilter[] {
  return [
    {
      kinds: [KIND_CODING_SESSION_OBSERVATION],
      "#h": [scope.channelRef],
      "#d": [scope.sessionRef],
      "#csob-genesis": [scope.genesisRef],
      limit,
    },
  ];
}

/**
 * The live filter: new 44246 on this umbrella's channel and `d` tag, from
 * `sinceSeconds` on. Only a wake-up — the re-read applies the full filter.
 * `since` (not `limit: 0`) so an observation signed between the first read
 * and the subscription being ready is replayed and still wakes a re-read.
 */
export function buildCodingSessionObservationLiveFilter(
  scope: Pick<CodingSessionObservationScope, "channelRef" | "sessionRef">,
  sinceSeconds: number,
): RelaySubscriptionFilter {
  return {
    kinds: [KIND_CODING_SESSION_OBSERVATION],
    "#h": [scope.channelRef],
    "#d": [scope.sessionRef],
    since: sinceSeconds,
    limit: 16,
  };
}

/**
 * Calls `onBurst` once per burst of `wake()` calls, `delayMs` after the last.
 * `cancel` drops a pending call.
 */
export function createTrailingDebounce(
  onBurst: () => void,
  delayMs: number,
  timers: {
    set: (callback: () => void, ms: number) => unknown;
    clear: (handle: unknown) => void;
  } = {
    set: (callback, ms) => setTimeout(callback, ms),
    clear: (handle) => clearTimeout(handle as ReturnType<typeof setTimeout>),
  },
): { wake: () => void; cancel: () => void } {
  let pending: unknown = null;
  return {
    wake() {
      if (pending !== null) timers.clear(pending);
      pending = timers.set(() => {
        pending = null;
        onBurst();
      }, delayMs);
    },
    cancel() {
      if (pending !== null) timers.clear(pending);
      pending = null;
    },
  };
}

/**
 * Opens the live wake-up subscription and reports its state through `onLive`:
 * `subscribed` only once the relay answered the REQ with EOSE; `unavailable`
 * when the client has no `subscribeLive`, hands back no handle, rejects, or the
 * relay refused the REQ with CLOSED (an auth, access or filter refusal ends the
 * subscription for good). A readiness `timeout`, or a handle with no readiness
 * reported, stays `connecting`: not confirmed, so not claimed live. A failure
 * manufactures no result; it only stops the answer reading as live. Returns
 * the disposer.
 */
export function openCodingSessionObservationLiveSubscription(input: {
  client: CodingSessionObservationClient;
  scope: Pick<CodingSessionObservationScope, "channelRef" | "sessionRef">;
  sinceSeconds: number;
  onWake: () => void;
  onLive: (live: CodingSessionObservationLive) => void;
}): () => void {
  const { client } = input;
  if (client.subscribeLive === undefined) {
    input.onLive("unavailable");
    return () => {};
  }
  let disposed = false;
  let unsubscribe: (() => Promise<void>) | null = null;
  let readiness: LiveSubscriptionReadiness | null = null;
  input.onLive("connecting");
  let opened: Promise<(() => Promise<void>) | undefined | null>;
  try {
    opened = client.subscribeLive(
      buildCodingSessionObservationLiveFilter(input.scope, input.sinceSeconds),
      () => input.onWake(),
      (reported) => {
        readiness = reported;
      },
      CODING_SESSION_OBSERVATION_LIVE_READY_TIMEOUT_MS,
    );
  } catch (error) {
    opened = Promise.reject(error);
  }
  opened.then(
    (handle) => {
      if (disposed) {
        if (handle) void handle();
        return;
      }
      if (!handle) {
        input.onLive("unavailable");
        return;
      }
      unsubscribe = handle;
      // The handle arrives after readiness of any kind; only EOSE is "up".
      if (readiness === "eose") input.onLive("subscribed");
      else if (readiness === "closed") input.onLive("unavailable");
    },
    () => {
      if (!disposed) input.onLive("unavailable");
    },
  );
  return () => {
    disposed = true;
    if (unsubscribe !== null) void unsubscribe();
  };
}

/**
 * Keeps `live` honest after the first EOSE. The relay registry reports a
 * subscription's readiness once and replays its REQ on reconnect without
 * reporting again, so a dropped socket would otherwise leave every running
 * line claiming to be live while no close can arrive.
 *
 * `onDown` fires when the socket is not up: `connecting` while it is being
 * re-established (connecting, reconnecting, stalled), `unavailable` when the
 * session disconnected for good. `onRestored` fires once it is back — on the
 * client's reconnect signal, or on `connected` after a drop when the client has
 * no reconnect signal — and the caller re-opens its subscription (so `live`
 * returns to `subscribed` only on that REQ's own EOSE) and re-reads once.
 * A client that reports neither is left alone. Returns the disposer.
 */
export function watchCodingSessionObservationConnection(input: {
  client: CodingSessionObservationClient;
  onDown: (live: CodingSessionObservationLive) => void;
  onRestored: () => void;
}): () => void {
  const { client } = input;
  if (client.subscribeToConnectionState === undefined) return () => {};
  const hasReconnectSignal = client.subscribeToReconnects !== undefined;
  let down = false;
  const stopState = client.subscribeToConnectionState((state) => {
    if (state === "connected") {
      if (down && !hasReconnectSignal) {
        down = false;
        input.onRestored();
      }
      return;
    }
    // `idle` is before any connection was asked for: nothing has dropped.
    if (state === "idle") return;
    down = true;
    input.onDown(state === "disconnected" ? "unavailable" : "connecting");
  });
  const stopReconnects =
    client.subscribeToReconnects?.(() => {
      down = false;
      input.onRestored();
    }) ?? (() => {});
  return () => {
    stopState();
    stopReconnects();
  };
}

/**
 * Fetch and fold one umbrella's observations.
 *
 * Exported separately from the hook so a test can drive the whole path with a
 * fake client and a fake invoke, without React.
 */
export async function readCodingSessionObservations(input: {
  scope: CodingSessionObservationScope;
  knownAssignmentRefs: readonly string[];
  providerPubkeys: CodingSessionProviderPubkeys;
  client: CodingSessionObservationClient;
  invoke?: (command: string, args: Record<string, unknown>) => Promise<unknown>;
}): Promise<CodingSessionObservationFoldResult> {
  const [events] = await Promise.all(
    buildCodingSessionObservationFilters(
      input.scope,
      CODING_SESSION_OBSERVATION_HISTORY_LIMIT,
    ).map((filter) => input.client.fetchEventsCoalesced(filter)),
  );
  return invokeCodingSessionObservationFold({
    sessionRef: input.scope.sessionRef,
    genesisRef: input.scope.genesisRef,
    knownAssignmentRefs: input.knownAssignmentRefs,
    providerPubkeys: input.providerPubkeys,
    events,
    invoke: input.invoke,
  });
}

/** Read one umbrella's folded observations. A `null` scope reads nothing. */
export function useCodingSessionObservations(
  scope: CodingSessionObservationScope | null,
  knownAssignmentRefs: readonly string[],
  providerPubkeys: CodingSessionProviderPubkeys,
  client: CodingSessionObservationClient = defaultRelayClient,
): CodingSessionObservationsResult {
  // Three primitives rather than the object: a caller that builds its scope
  // literal inline hands a new reference every render, and a relay read keyed
  // on object identity would restart on each one.
  const channelRef = scope?.channelRef ?? null;
  const sessionRef = scope?.sessionRef ?? null;
  const genesisRef = scope?.genesisRef ?? null;
  // The pointer set is part of the answer (which pointers resolve), so it is
  // part of the read's identity — joined rather than referenced, for the same
  // render-stability reason.
  const pointerIdentity = [...knownAssignmentRefs].sort().join(",");
  // Part of the answer (which claims are honoured), so part of the read's
  // identity. `null` and `[]` are deliberately different strings.
  const providerIdentity =
    providerPubkeys === null
      ? "unresolved"
      : `set:${[...providerPubkeys].sort().join(",")}`;
  const identity =
    channelRef === null || sessionRef === null || genesisRef === null
      ? "no-observation-scope"
      : [channelRef, sessionRef, genesisRef, pointerIdentity].join("\u0000");
  const stableScope = React.useMemo(
    () =>
      channelRef === null || sessionRef === null || genesisRef === null
        ? null
        : { channelRef, sessionRef, genesisRef },
    [channelRef, genesisRef, sessionRef],
  );
  const stablePointers = React.useMemo(
    () => (pointerIdentity.length === 0 ? [] : pointerIdentity.split(",")),
    [pointerIdentity],
  );
  const stableProviders = React.useMemo<CodingSessionProviderPubkeys>(() => {
    if (providerIdentity === "unresolved") return null;
    const joined = providerIdentity.slice("set:".length);
    return joined.length === 0 ? [] : joined.split(",");
  }, [providerIdentity]);
  const [revision, setRevision] = React.useState(0);
  const refresh = React.useCallback(
    () => setRevision((current) => current + 1),
    [],
  );
  const runIdentity = `${identity}\u0000${revision}`;
  // Bumped by the live subscription. Not part of `runIdentity`: a live re-read
  // keeps the answer on screen (with its `readAtMs`) until the new one settles,
  // rather than blanking the view to "loading" on every observation.
  const [liveRevision, setLiveRevision] = React.useState(0);
  const settledIdentityRef = React.useRef<string | null>(null);
  const [state, setState] = React.useState<{
    identity: string;
    isLoading: boolean;
    errorMessage: string | null;
    result: CodingSessionObservationFoldResult | null;
    readAtMs: number | null;
  }>(() => ({
    identity: runIdentity,
    isLoading: stableScope !== null,
    errorMessage: null,
    result: null,
    readAtMs: null,
  }));

  const [live, setLive] =
    React.useState<CodingSessionObservationLive>("connecting");
  // Bumped when the socket comes back after a drop: the live subscription is
  // re-opened so `subscribed` again means its own EOSE was seen.
  const [liveEpoch, setLiveEpoch] = React.useState(0);
  React.useEffect(() => {
    if (stableScope === null) return;
    return watchCodingSessionObservationConnection({
      client,
      onDown: setLive,
      onRestored: () => {
        setLiveEpoch((current) => current + 1);
        // Closes signed while the socket was down never woke a re-read.
        setLiveRevision((current) => current + 1);
      },
    });
  }, [client, stableScope]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: liveEpoch is intentional — a re-open trigger after a reconnect, so `subscribed` again means this REQ's own EOSE; its value is not read
  React.useEffect(() => {
    if (stableScope === null) return;
    const debounce = createTrailingDebounce(
      () => setLiveRevision((current) => current + 1),
      CODING_SESSION_OBSERVATION_LIVE_DEBOUNCE_MS,
    );
    // No live wake-ups (`unavailable`): the read still stands, with its
    // `readAtMs`, and `refresh` still works — but it is a snapshot, and the
    // surfaces that show a running gate say so.
    const dispose = openCodingSessionObservationLiveSubscription({
      client,
      scope: stableScope,
      sinceSeconds: Math.floor(Date.now() / 1_000),
      onWake: () => debounce.wake(),
      onLive: setLive,
    });
    return () => {
      debounce.cancel();
      dispose();
    };
  }, [client, liveEpoch, stableScope]);

  React.useEffect(() => {
    let cancelled = false;
    if (stableScope === null) {
      settledIdentityRef.current = null;
      setState({
        identity: runIdentity,
        isLoading: false,
        errorMessage: null,
        result: null,
        readAtMs: null,
      });
      return () => {
        cancelled = true;
      };
    }
    // A live wake-up for the read already on screen re-reads in the
    // background; a new scope or an explicit refresh starts from loading.
    const background =
      liveRevision > 0 && settledIdentityRef.current === runIdentity;
    if (!background) {
      setState({
        identity: runIdentity,
        isLoading: true,
        errorMessage: null,
        result: null,
        readAtMs: null,
      });
    }
    void (async () => {
      try {
        const result = await readCodingSessionObservations({
          client,
          knownAssignmentRefs: stablePointers,
          providerPubkeys: stableProviders,
          scope: stableScope,
        });
        if (cancelled) return;
        settledIdentityRef.current = runIdentity;
        setState({
          identity: runIdentity,
          isLoading: false,
          errorMessage: null,
          result,
          readAtMs: Date.now(),
        });
      } catch (error) {
        if (cancelled) return;
        // A failed re-read is a failed read: the old answer is no longer
        // known to be current, so it is not left standing as if it were.
        settledIdentityRef.current = null;
        setState({
          identity: runIdentity,
          isLoading: false,
          errorMessage: errorText(error),
          result: null,
          readAtMs: null,
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    client,
    liveRevision,
    runIdentity,
    stablePointers,
    stableProviders,
    stableScope,
  ]);

  // A state left over from a previous scope or refresh is not this read's
  // answer, so it reads as loading rather than as a stale record.
  const current = state.identity === runIdentity;
  return {
    isLoading: current ? state.isLoading : true,
    errorMessage: current ? state.errorMessage : null,
    result: current ? state.result : null,
    readAtMs: current ? state.readAtMs : null,
    live,
    refresh,
  };
}

function errorText(error: unknown): string {
  const message =
    error instanceof Error
      ? error.message
      : "Failed to read this session's observations.";
  return message.length <= MAX_ERROR_MESSAGE_LENGTH
    ? message
    : `${message.slice(0, MAX_ERROR_MESSAGE_LENGTH)}… [truncated]`;
}
