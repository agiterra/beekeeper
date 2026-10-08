import { emit } from "@tauri-apps/api/event";

import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_SESSION_DEVICE_COMMAND,
  KIND_SESSION_DEVICE_RECORD,
  KIND_SESSION_PREVIEW_ANNOUNCE,
  KIND_SURFACE_FRAME,
  KIND_SURFACE_SNAPSHOT,
  KIND_SURFACE_WATCH,
} from "@/shared/constants/kinds";

import type { MockFilter } from "./e2eBridgeSessionFacts";
import type {
  WaveBMockCommandConfig,
  WaveBMockCommandResult,
} from "./e2eBridgeWaveBRegistry";

/**
 * Shared surface observation in the E2E bridge (C5, SV-34 S1/S2 and SV-33
 * S3/S4): the mock `session_preview_share_*` commands, and the mock relay's
 * handling of the surface kinds (24320/24321/30626/44253/44254/44255).
 *
 * **What the relay mock does for these kinds** (it did nothing special
 * before: the channel branch ignored `authors`, `#d` and `#p`, and live
 * fan-out matched only channel and kind):
 * - A one-shot REQ (or `POST /query`) for stored surface kinds honours
 *   `#h`, `kinds`, `authors`, `ids`, `#d`, `#p`, `#e`, `since`, `until` and
 *   `limit`, and folds the addressable 30626 to each (signer, `d`)'s newest,
 *   the way the relay does. Ephemeral kinds are never served from history.
 * - Live fan-out of a surface kind honours the same filter fields, so a
 *   client that subscribes with `authors=[authority]` (the contract's dual
 *   enforcement) never receives another key's frame — and a client that
 *   forgot to, would.
 * - Ephemeral 24320/24321 that specs inject are delivered live only, never
 *   stored; the app's own published 24320/44254 are recorded on
 *   `window.__BEEKEEPER_E2E_SURFACE_PUBLISHED__` for assertions.
 *
 * Signatures are the spec's: specs sign with `nostr-tools` in Node and pass
 * whole events, because consumers verify signature-first. The mock checks no
 * signature and no producer authority; the relay's ingest refusal of a
 * non-authority frame is the relay e2e's job (`e2e_surface_watch.rs`).
 */

/** The share state shape of `tauriSessionPreviewShare.ts` (V-CONTRACT). */
export type SurfaceObservationShareState = {
  channelId: string;
  sessionRef: string | null;
  share: boolean;
  announced: "open" | "closed" | "none";
  stream: "frames" | "none";
  watchers: string[];
  cadenceMs: number;
  framesLastMinute: number;
  lastFrameAt: number | null;
  unavailable: { code: string; sentence: string } | null;
};

/** One recorded `session_preview_share_*` call. */
export type SurfaceObservationShareCall = {
  command: string;
  payload: Record<string, unknown>;
};

/**
 * Declared by a spec (before the app loads) as
 * `window.__BEEKEEPER_E2E_SURFACE_OBSERVATION__`. Live state: a spec may
 * change it mid-test, or call `__BEEKEEPER_E2E_SURFACE_SET_SHARE_STATE__`
 * to change a channel's state and emit it.
 */
export type SurfaceObservationMockState = {
  /** Per channel id: the share state's starting values. */
  share?: Record<string, Partial<SurfaceObservationShareState>>;
  /** What the camera button resolves to. */
  snapshotResult?: { eventId: string; url: string; sha256: string };
  /** Make the camera button reject with this `{code, message}`. */
  snapshotError?: { code: string; message: string };
  /** Every share command the app called, in order. */
  calls?: SurfaceObservationShareCall[];
};

declare global {
  interface Window {
    __BEEKEEPER_E2E_SURFACE_OBSERVATION__?: SurfaceObservationMockState;
    /** The app's own published 24320 watches and 44254 commands. */
    __BEEKEEPER_E2E_SURFACE_PUBLISHED__?: RelayEvent[];
    /**
     * Inject one signed surface event. Ephemeral kinds (24320/24321) are
     * delivered live only; stored kinds enter channel history and fan out.
     */
    __BEEKEEPER_E2E_SURFACE_SEED__?: (input: {
      channelName: string;
      event: RelayEvent;
    }) => void;
    /**
     * Deliver pre-signed live events (frames, `t=paused`, `t=end`) one per
     * `intervalMs`, starting now, then stop. Replaces a running pump.
     */
    __BEEKEEPER_E2E_SURFACE_PUMP__?: (input: {
      channelName: string;
      events: RelayEvent[];
      intervalMs: number;
    }) => void;
    /** Stop the pump: the producer goes quiet (a stall, if nothing follows). */
    __BEEKEEPER_E2E_SURFACE_PUMP_STOP__?: () => void;
    /** Patch a channel's share state and emit `session-preview://share-state`. */
    __BEEKEEPER_E2E_SURFACE_SET_SHARE_STATE__?: (
      channelId: string,
      patch: Partial<SurfaceObservationShareState>,
    ) => Promise<SurfaceObservationShareState>;
  }
}

const SHARE_STATE_EVENT = "session-preview://share-state";

const EPHEMERAL_SURFACE_KINDS: ReadonlySet<number> = new Set([
  KIND_SURFACE_WATCH,
  KIND_SURFACE_FRAME,
]);
const STORED_SURFACE_KINDS: ReadonlySet<number> = new Set([
  KIND_SESSION_PREVIEW_ANNOUNCE,
  KIND_SURFACE_SNAPSHOT,
  KIND_SESSION_DEVICE_COMMAND,
  KIND_SESSION_DEVICE_RECORD,
]);
const RECORDED_PUBLISHED_KINDS: ReadonlySet<number> = new Set([
  KIND_SURFACE_WATCH,
  KIND_SESSION_DEVICE_COMMAND,
]);

function isSurfaceKind(kind: number): boolean {
  return EPHEMERAL_SURFACE_KINDS.has(kind) || STORED_SURFACE_KINDS.has(kind);
}

function tagValues(event: RelayEvent, name: string): string[] {
  return event.tags
    .filter((tag) => tag[0] === name && typeof tag[1] === "string")
    .map((tag) => tag[1] as string);
}

function anyTagIn(
  event: RelayEvent,
  name: string,
  wanted: readonly string[] | undefined,
): boolean {
  if (!wanted) return true;
  return tagValues(event, name).some((value) => wanted.includes(value));
}

/** NIP-01 filter match over the fields the surface readers use. */
function surfaceFilterMatches(filter: MockFilter, event: RelayEvent): boolean {
  if (filter.kinds && !filter.kinds.includes(event.kind)) return false;
  if (filter.ids && !filter.ids.includes(event.id)) return false;
  if (
    filter.authors &&
    !filter.authors
      .map((author) => author.toLowerCase())
      .includes(event.pubkey.toLowerCase())
  ) {
    return false;
  }
  if (filter.since !== undefined && event.created_at < filter.since) {
    return false;
  }
  if (filter.until !== undefined && event.created_at > filter.until) {
    return false;
  }
  return (
    anyTagIn(event, "h", filter["#h"]) &&
    anyTagIn(event, "d", filter["#d"]) &&
    anyTagIn(event, "p", filter["#p"]) &&
    anyTagIn(event, "e", filter["#e"])
  );
}

/**
 * Whether a live subscription's filters admit `event`. Only surface kinds are
 * narrowed here; every other kind keeps the mock's channel-and-kind rule.
 */
export function mockSurfaceLiveFilterAllows(
  filters: readonly MockFilter[],
  event: RelayEvent,
): boolean {
  if (!isSurfaceKind(event.kind)) return true;
  return filters.some((filter) => surfaceFilterMatches(filter, event));
}

/** Record the app's own surface publications (called on every live fan-out). */
export function noteMockSurfaceLiveEvent(event: RelayEvent): void {
  if (!RECORDED_PUBLISHED_KINDS.has(event.kind)) return;
  if (injectedEventIds.has(event.id)) return;
  window.__BEEKEEPER_E2E_SURFACE_PUBLISHED__ ??= [];
  if (window.__BEEKEEPER_E2E_SURFACE_PUBLISHED__.some((e) => e.id === event.id))
    return;
  window.__BEEKEEPER_E2E_SURFACE_PUBLISHED__.push(event);
}

/**
 * Answer a one-shot REQ whose kinds are all surface kinds. `getEvents(null)`
 * means every channel (a filter with no `#h`, e.g. a card resolving a
 * snapshot by id). Returns `true` when this seam answered.
 */
export function respondToMockSurfaceObservationQuery(
  filter: MockFilter,
  subId: string,
  getEvents: (channelId: string | null) => readonly RelayEvent[],
  send: (message: unknown[]) => void,
): boolean {
  const kinds = filter.kinds ?? [];
  if (kinds.length === 0 || !kinds.every(isSurfaceKind)) return false;
  const channels = filter["#h"];
  const pool = channels?.length
    ? channels.flatMap((channelId) => getEvents(channelId))
    : getEvents(null);
  const matched = pool.filter(
    (event) =>
      STORED_SURFACE_KINDS.has(event.kind) &&
      surfaceFilterMatches(filter, event),
  );
  // Addressable 30626: the relay keeps each (signer, d)'s newest only.
  const newestAnnounce = new Map<string, RelayEvent>();
  const rest: RelayEvent[] = [];
  for (const event of matched) {
    if (event.kind !== KIND_SESSION_PREVIEW_ANNOUNCE) {
      rest.push(event);
      continue;
    }
    const key = `${event.pubkey}:${tagValues(event, "d")[0] ?? ""}`;
    const incumbent = newestAnnounce.get(key);
    if (
      !incumbent ||
      event.created_at > incumbent.created_at ||
      (event.created_at === incumbent.created_at && event.id < incumbent.id)
    ) {
      newestAnnounce.set(key, event);
    }
  }
  const unique = new Map<string, RelayEvent>();
  for (const event of [...rest, ...newestAnnounce.values()]) {
    unique.set(event.id, event);
  }
  const page = [...unique.values()]
    .sort(
      (left, right) =>
        right.created_at - left.created_at || left.id.localeCompare(right.id),
    )
    .slice(0, filter.limit ?? 500)
    .sort(
      (left, right) =>
        left.created_at - right.created_at || left.id.localeCompare(right.id),
    );
  for (const event of page) send(["EVENT", subId, event]);
  send(["EOSE", subId]);
  return true;
}

const injectedEventIds = new Set<string>();
let pumpTimer: number | null = null;

/** Wire the spec hooks to the relay mock. Called once per bridge install. */
export function installMockSurfaceObservationRelay(relay: {
  resolveChannelId: (channelName: string) => string | null;
  emitLive: (channelId: string, event: RelayEvent) => void;
  seedStored: (channelName: string, event: RelayEvent) => void;
}): void {
  const channelIdOf = (channelName: string): string => {
    const channelId = relay.resolveChannelId(channelName);
    if (!channelId) throw new Error(`Mock channel ${channelName} not found.`);
    return channelId;
  };
  const deliver = (channelName: string, event: RelayEvent) => {
    injectedEventIds.add(event.id);
    if (EPHEMERAL_SURFACE_KINDS.has(event.kind)) {
      relay.emitLive(channelIdOf(channelName), event);
    } else {
      relay.seedStored(channelName, event);
    }
  };
  const stop = () => {
    if (pumpTimer !== null) window.clearInterval(pumpTimer);
    pumpTimer = null;
  };
  window.__BEEKEEPER_E2E_SURFACE_SEED__ = ({ channelName, event }) =>
    deliver(channelName, event);
  window.__BEEKEEPER_E2E_SURFACE_PUMP__ = ({
    channelName,
    events,
    intervalMs,
  }) => {
    stop();
    const queue = [...events];
    const next = () => {
      const event = queue.shift();
      if (!event) {
        stop();
        return;
      }
      deliver(channelName, event);
    };
    next();
    if (queue.length > 0) pumpTimer = window.setInterval(next, intervalMs);
  };
  window.__BEEKEEPER_E2E_SURFACE_PUMP_STOP__ = stop;
  window.__BEEKEEPER_E2E_SURFACE_SET_SHARE_STATE__ = async (channelId, patch) =>
    publishShareState({ ...shareStateFor(channelId), ...patch });
}

// ---- Mock `session_preview_share_*` commands ------------------------------

const shareStates = new Map<string, SurfaceObservationShareState>();

function mockState(): SurfaceObservationMockState {
  window.__BEEKEEPER_E2E_SURFACE_OBSERVATION__ ??= {};
  const state = window.__BEEKEEPER_E2E_SURFACE_OBSERVATION__;
  state.calls ??= [];
  return state;
}

/** A fresh Rust-side state: nothing announced, Share on (D5), no watchers. */
function freshShareState(channelId: string): SurfaceObservationShareState {
  return {
    channelId,
    sessionRef: null,
    share: true,
    announced: "none",
    stream: "frames",
    watchers: [],
    cadenceMs: 2_000,
    framesLastMinute: 0,
    lastFrameAt: null,
    unavailable: null,
  };
}

function shareStateFor(channelId: string): SurfaceObservationShareState {
  const known = shareStates.get(channelId);
  if (known) return known;
  const declared = mockState().share?.[channelId] ?? {};
  const state = { ...freshShareState(channelId), ...declared, channelId };
  shareStates.set(channelId, state);
  return state;
}

async function publishShareState(
  next: SurfaceObservationShareState,
): Promise<SurfaceObservationShareState> {
  shareStates.set(next.channelId, next);
  await emit(SHARE_STATE_EVENT, structuredClone(next));
  return structuredClone(next);
}

function refused(code: string, message: string): never {
  throw { code, message };
}

/** Mock the five share commands. Always answers, declared or not. */
export async function handleSurfaceObservationMockCommand(
  command: string,
  payload: unknown,
  _config: WaveBMockCommandConfig,
): Promise<WaveBMockCommandResult> {
  if (!command.startsWith("session_preview_share_")) return null;
  const mock = mockState();
  const args = (payload ?? {}) as Record<string, unknown>;
  mock.calls?.push({ command, payload: structuredClone(args) });
  const channelId = String(args.channelId ?? "");
  const current = shareStateFor(channelId);
  const handled = (value: unknown): WaveBMockCommandResult => ({
    handled: true,
    value,
  });

  switch (command) {
    case "session_preview_share_state":
      return handled(structuredClone(current));
    case "session_preview_share_configure": {
      const share = args.share !== false;
      const sessionRef =
        typeof args.sessionRef === "string" ? args.sessionRef : null;
      return handled(
        await publishShareState({
          ...current,
          sessionRef,
          share,
          // Share off publishes nothing: an open announce becomes one close.
          stream: share ? "frames" : "none",
          watchers: share ? current.watchers : [],
          announced:
            sessionRef === null || current.unavailable
              ? current.announced
              : share
                ? "open"
                : current.announced === "open"
                  ? "closed"
                  : current.announced,
          unavailable:
            current.unavailable ??
            (sessionRef === null
              ? {
                  code: "no_session_ref",
                  sentence:
                    "This session has no session reference, so its Browser cannot be shared.",
                }
              : null),
        }),
      );
    }
    case "session_preview_share_snapshot": {
      if (mock.snapshotError) {
        refused(mock.snapshotError.code, mock.snapshotError.message);
      }
      if (current.unavailable) {
        refused("preview_share_unavailable", current.unavailable.sentence);
      }
      if (!current.share) {
        refused(
          "preview_share_unavailable",
          "Sharing is off for this Browser, so nothing from it is published.",
        );
      }
      return handled(
        mock.snapshotResult ?? {
          eventId: "5".repeat(64),
          url: `https://example.com/e2e/surface/${"6".repeat(64)}.png`,
          sha256: "6".repeat(64),
        },
      );
    }
    case "session_preview_share_watch": {
      const watcher = String(args.watcherPubkey ?? "");
      const action = String(args.action ?? "");
      const others = current.watchers.filter((key) => key !== watcher);
      const watchers =
        action === "stop" || watcher === "" ? others : [...others, watcher];
      await publishShareState({ ...current, watchers });
      return handled(null);
    }
    case "session_preview_share_note_publish":
      return handled(null);
    default:
      return handled(structuredClone(current));
  }
}
