/**
 * The observer's timing as a pure state machine (lane V contract § Observer
 * timing; WIRE-C5 §§ 3–4). `useSurfaceObserver` and its tests drive it; the
 * CLI's `watch` agrees with it.
 *
 * - `watch` on mount and every 15 s while the document is visible; a beat
 *   with no frame since the previous one sends `resync` instead.
 * - No frame within 10 s of the first watch → `not-streaming`.
 * - Last frame older than 30 s → `stalled`; a stalled frame is never Live.
 * - `t=paused` → `paused`; `t=end` → `ended`.
 * - Within an epoch a seq at or below the last one is ignored; a newer epoch
 *   resets the order; an older epoch is a late frame from before a restart.
 * - A `snapshot` request at most once per 10 s; no 44253 naming me as
 *   `requested-by` within 15 s → "The host did not answer."
 *
 * A frame's time is its `captured-at`, clamped to when it arrived: a
 * producer clock running ahead cannot keep a frame "live", and one running
 * behind errs toward stalled — never toward a stale frame labelled Live.
 */
import type { RelayEvent } from "@/shared/api/types";
import { KIND_SURFACE_FRAME } from "@/shared/constants/kinds";

import {
  isSurfaceHex,
  matchSurfaceTags,
  parseSurfaceDecimal,
  parseSurfaceDim,
  type SurfaceKind,
  surfaceOptional,
  surfaceRequired,
} from "./codingSessionSurfaceSnapshot";

export const SURFACE_WATCH_KEEPALIVE_MS = 15_000;
export const SURFACE_HANDSHAKE_TIMEOUT_MS = 10_000;
export const SURFACE_STALL_MS = 30_000;
export const SURFACE_SNAPSHOT_MIN_INTERVAL_MS = 10_000;
export const SURFACE_SNAPSHOT_TIMEOUT_MS = 15_000;
const MAX_FRAME_CONTENT_BYTES = 200 * 1024;

export type SurfaceObserverStatus =
  | "connecting"
  | "live"
  | "not-streaming"
  | "stalled"
  | "paused"
  | "ended";

/** One parsed 24321. */
export type SurfaceFrame = {
  eventId: string;
  author: string;
  type: "frame" | "paused" | "end";
  seq: number;
  epoch: number;
  cadenceMs: number;
  dim: { width: number; height: number };
  /** Unix ms, the producer's clock. */
  capturedAt: number;
  actor: string | null;
  commit: string | null;
  /** `data:image/jpeg;base64,…` for `t=frame`, else null. */
  dataUrl: string | null;
};

const FRAME_LAYOUT = [
  surfaceRequired("h"),
  surfaceRequired("surface"),
  surfaceRequired("d"),
  surfaceRequired("t"),
  surfaceRequired("seq"),
  surfaceRequired("epoch"),
  surfaceRequired("cadence-ms"),
  surfaceRequired("dim"),
  surfaceRequired("captured-at"),
  surfaceOptional("actor"),
  surfaceOptional("commit"),
];

/**
 * Parse one 24321 for this observer, or `null`: the tags in order, the
 * expected channel/surface/key, and the producer as author (frames from
 * anyone else are dropped even if the relay delivered them).
 */
export function parseSurfaceFrame(
  event: RelayEvent,
  expect: {
    channelId: string;
    surface: SurfaceKind;
    key: string;
    producerPubkey: string;
  },
): SurfaceFrame | null {
  if (event.kind !== KIND_SURFACE_FRAME) return null;
  if (event.pubkey !== expect.producerPubkey) return null;
  const slots = matchSurfaceTags(event.tags, FRAME_LAYOUT);
  if (!slots) return null;
  const value = (index: number) => slots[index]?.[1] ?? "";
  if (value(0) !== expect.channelId) return null;
  if (value(1) !== expect.surface || value(2) !== expect.key) return null;
  const type = value(3);
  if (type !== "frame" && type !== "paused" && type !== "end") return null;
  const seq = parseSurfaceDecimal(value(4));
  const epoch = parseSurfaceDecimal(value(5));
  const cadenceMs = parseSurfaceDecimal(value(6));
  const dim = parseSurfaceDim(value(7));
  const capturedAt = parseSurfaceDecimal(value(8));
  if (seq === null || epoch === null || capturedAt === null || !dim) {
    return null;
  }
  if (cadenceMs === null || cadenceMs < 500 || cadenceMs > 60_000) return null;
  const actor = slots[9]?.[1] ?? null;
  if (actor !== null && !isSurfaceHex(actor, 64)) return null;
  const commit = slots[10]?.[1] ?? null;
  if (commit !== null && !isSurfaceHex(commit, 40)) return null;
  let dataUrl: string | null = null;
  if (type === "frame") {
    const content = event.content;
    if (
      content.length === 0 ||
      content.length > MAX_FRAME_CONTENT_BYTES ||
      !content.startsWith("/9j/") ||
      content.length % 4 !== 0 ||
      !/^[A-Za-z0-9+/]+={0,2}$/.test(content)
    ) {
      return null;
    }
    dataUrl = `data:image/jpeg;base64,${content}`;
  } else if (event.content !== "") {
    return null;
  }
  return {
    eventId: event.id,
    author: event.pubkey,
    type,
    seq,
    epoch,
    cadenceMs,
    dim,
    capturedAt,
    actor,
    commit,
    dataUrl,
  };
}

export type SurfaceObserverState = {
  status: SurfaceObserverStatus;
  /** Local ms of the first watch, or null before it was sent. */
  watchSentAt: number | null;
  /** The newest `t=frame` shown. Kept under `stalled`, `paused`, `ended`. */
  frame: SurfaceFrame | null;
  /** The shown frame's time: `captured-at` clamped to its arrival. */
  frameAt: number | null;
  cadenceMs: number | null;
  epoch: number | null;
  seq: number | null;
  /** Anything (frame, paused, end) arrived since the last beat. */
  arrivedSinceBeat: boolean;
  snapshot: {
    lastRequestAt: number | null;
    pendingSince: number | null;
    timedOut: boolean;
  };
};

export function initialSurfaceObserverState(): SurfaceObserverState {
  return {
    status: "connecting",
    watchSentAt: null,
    frame: null,
    frameAt: null,
    cadenceMs: null,
    epoch: null,
    seq: null,
    arrivedSinceBeat: false,
    snapshot: { lastRequestAt: null, pendingSince: null, timedOut: false },
  };
}

/** The first watch went out at `now`; later ones do not move the handshake. */
export function surfaceObserverWatchSent(
  state: SurfaceObserverState,
  now: number,
): SurfaceObserverState {
  return state.watchSentAt === null ? { ...state, watchSentAt: now } : state;
}

/**
 * Apply one parsed frame received at `now`. Returns the same object when the
 * frame is out of order (seq regression in its epoch, or an older epoch).
 */
export function surfaceObserverApplyFrame(
  state: SurfaceObserverState,
  frame: SurfaceFrame,
  now: number,
): SurfaceObserverState {
  if (state.epoch !== null) {
    if (frame.epoch < state.epoch) return state;
    if (
      frame.epoch === state.epoch &&
      state.seq !== null &&
      frame.seq <= state.seq
    ) {
      return state;
    }
  }
  const ordered = {
    ...state,
    epoch: frame.epoch,
    seq: frame.seq,
    cadenceMs: frame.cadenceMs,
    arrivedSinceBeat: true,
  };
  if (frame.type === "paused") return { ...ordered, status: "paused" };
  if (frame.type === "end") return { ...ordered, status: "ended" };
  const frameAt = Math.min(frame.capturedAt, now);
  return surfaceObserverTick(
    { ...ordered, status: "live", frame, frameAt },
    now,
  );
}

/** Re-derive time-based status at `now` (the 1 s tick). */
export function surfaceObserverTick(
  state: SurfaceObserverState,
  now: number,
): SurfaceObserverState {
  let next = state;
  const pending = state.snapshot.pendingSince;
  if (
    pending !== null &&
    !state.snapshot.timedOut &&
    now - pending >= SURFACE_SNAPSHOT_TIMEOUT_MS
  ) {
    next = {
      ...next,
      snapshot: { ...next.snapshot, pendingSince: null, timedOut: true },
    };
  }
  let status: SurfaceObserverStatus = next.status;
  if (status === "paused" || status === "ended") {
    // Held until the producer sends a frame again.
  } else if (next.frameAt === null) {
    status =
      next.watchSentAt !== null &&
      now - next.watchSentAt >= SURFACE_HANDSHAKE_TIMEOUT_MS
        ? "not-streaming"
        : "connecting";
  } else {
    status = now - next.frameAt > SURFACE_STALL_MS ? "stalled" : "live";
  }
  return status === next.status ? next : { ...next, status };
}

/**
 * A keepalive beat: `resync` when nothing arrived since the previous beat
 * (the producer then sends a full frame), else `watch`.
 */
export function surfaceObserverBeat(state: SurfaceObserverState): {
  action: "watch" | "resync";
  state: SurfaceObserverState;
} {
  const action = state.arrivedSinceBeat ? "watch" : "resync";
  return {
    action,
    state: state.arrivedSinceBeat
      ? { ...state, arrivedSinceBeat: false }
      : state,
  };
}

/** Whether a `snapshot` request may go out at `now` (≤ 1 per 10 s). */
export function surfaceObserverCanRequestSnapshot(
  state: SurfaceObserverState,
  now: number,
): boolean {
  const last = state.snapshot.lastRequestAt;
  return last === null || now - last >= SURFACE_SNAPSHOT_MIN_INTERVAL_MS;
}

/** A `snapshot` request went out at `now`. */
export function surfaceObserverSnapshotRequested(
  state: SurfaceObserverState,
  now: number,
): SurfaceObserverState {
  return {
    ...state,
    snapshot: { lastRequestAt: now, pendingSince: now, timedOut: false },
  };
}

/**
 * A 44253 arrived that names me as `requested-by`, taken at `takenAt` (ms).
 * It answers the pending request when taken no earlier than 5 s before it
 * (clock slack between the two machines).
 */
export function surfaceObserverSnapshotAnswered(
  state: SurfaceObserverState,
  takenAt: number,
): SurfaceObserverState {
  const pending = state.snapshot.pendingSince;
  const last = state.snapshot.lastRequestAt;
  const since = pending ?? (state.snapshot.timedOut ? last : null);
  if (since === null || takenAt < since - 5_000) return state;
  return {
    ...state,
    snapshot: { ...state.snapshot, pendingSince: null, timedOut: false },
  };
}
