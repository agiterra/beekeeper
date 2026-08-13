// Pure NIP-ST observer-side frame protocol: parse kind:24311 events, track
// (epoch, seq) ordering, and turn frames into terminal actions. No IO here —
// the hook owns subscriptions, timers, and the xterm instance.

import type { RelayEvent } from "@/shared/api/types";
import { KIND_SHELL_FRAME } from "@/shared/constants/kinds";

export type ObserveFrameType = "tail" | "snap" | "diff" | "resize" | "end";

export type ObserveFrame = {
  type: ObserveFrameType;
  seq: number;
  epoch: string;
  dims: { rows: number; cols: number } | null;
  bytes: Uint8Array;
};

function tagValue(event: RelayEvent, name: string): string | null {
  const values = event.tags
    .filter((tag) => tag[0] === name && typeof tag[1] === "string")
    .map((tag) => tag[1] as string);
  return values.length === 1 ? values[0] : null;
}

function parseDims(
  value: string | null,
): { rows: number; cols: number } | null {
  if (!value) return null;
  const match = /^(\d{1,3})x(\d{1,4})$/.exec(value);
  if (!match) return null;
  return { rows: Number(match[1]), cols: Number(match[2]) };
}

function decodeBase64(content: string): Uint8Array | null {
  try {
    const binary = window.atob(content);
    const bytes = new Uint8Array(binary.length);
    for (let i = 0; i < binary.length; i += 1) {
      bytes[i] = binary.charCodeAt(i);
    }
    return bytes;
  } catch {
    return null;
  }
}

const FRAME_TYPES: readonly string[] = [
  "tail",
  "snap",
  "diff",
  "resize",
  "end",
];

/**
 * Parse and validate one relay event as a frame for the expected session.
 * Returns `null` for anything that is not a well-formed frame from the
 * session's owner — the subscription already filters by author and session,
 * but the parser never trusts that.
 */
export function parseShellFrame(
  event: RelayEvent,
  expected: { ownerPubkey: string; sessionId: string },
): ObserveFrame | null {
  if (event.kind !== KIND_SHELL_FRAME) return null;
  if (event.pubkey.toLowerCase() !== expected.ownerPubkey.toLowerCase()) {
    return null;
  }
  if (tagValue(event, "d") !== expected.sessionId) return null;
  const type = tagValue(event, "t");
  if (!type || !FRAME_TYPES.includes(type)) return null;
  const seq = Number(tagValue(event, "seq"));
  const epoch = tagValue(event, "epoch");
  if (!Number.isSafeInteger(seq) || seq < 0 || !epoch) return null;
  const bytes = decodeBase64(event.content);
  if (bytes === null) return null;
  return {
    type: type as ObserveFrameType,
    seq,
    epoch,
    dims: parseDims(tagValue(event, "dims")),
    bytes,
  };
}

/** What the terminal should do with one applied frame. */
export type ObserveAction = {
  /** Bytes to write into the terminal (already prefixed with a clear for
   * snapshots that replace stale content). */
  write: Uint8Array | null;
  /** New grid to apply before writing, when the frame carries one. */
  resize: { rows: number; cols: number } | null;
  /** The observer should publish a `resync` watch event. */
  needsResync: boolean;
  /** The stream ended (session closed / unshared). */
  ended: boolean;
};

const NOOP: ObserveAction = {
  write: null,
  resize: null,
  needsResync: false,
  ended: false,
};

/** `ESC[H ESC[2J` — home + clear, prepended when a snapshot must replace
 * whatever stale content a resync left on screen. */
const CLEAR = new Uint8Array([0x1b, 0x5b, 0x48, 0x1b, 0x5b, 0x32, 0x4a]);

function withClear(bytes: Uint8Array): Uint8Array {
  const out = new Uint8Array(CLEAR.length + bytes.length);
  out.set(CLEAR, 0);
  out.set(bytes, CLEAR.length);
  return out;
}

/**
 * Ordering state machine for one observed session. Frames may arrive after a
 * gap (dropped ephemeral events) or from a restarted broadcaster (new epoch);
 * either way diffs become unsafe until the next snapshot repaints.
 */
export class ObserveStream {
  private epoch: string | null = null;
  private lastSeq = 0;
  private awaitingSnap = false;
  /** True once any snapshot has painted (a resync-snap must clear first). */
  private painted = false;

  apply(frame: ObserveFrame): ObserveAction {
    const epochChanged = this.epoch !== null && frame.epoch !== this.epoch;
    const gap =
      this.epoch !== null && !epochChanged && frame.seq !== this.lastSeq + 1;
    if (this.epoch === null || epochChanged) {
      this.epoch = frame.epoch;
      this.lastSeq = frame.seq;
      if (epochChanged) this.awaitingSnap = true;
    } else if (gap) {
      if (frame.seq <= this.lastSeq) {
        // Stale replay — drop silently.
        return NOOP;
      }
      this.lastSeq = frame.seq;
      this.awaitingSnap = true;
    } else {
      this.lastSeq = frame.seq;
    }

    switch (frame.type) {
      case "end":
        return { ...NOOP, ended: true };
      case "resize":
        // A resize invalidates the diff chain; the owner follows with a snap.
        this.awaitingSnap = true;
        return { ...NOOP, resize: frame.dims };
      case "snap": {
        const mustClear = this.awaitingSnap && this.painted;
        this.awaitingSnap = false;
        this.painted = true;
        return {
          ...NOOP,
          resize: frame.dims,
          write: mustClear ? withClear(frame.bytes) : frame.bytes,
        };
      }
      case "tail":
        // Scrollback replay ahead of the first snapshot; if we're mid-stream
        // awaiting a snap, stale tails would corrupt the screen — skip them.
        return this.awaitingSnap && this.painted
          ? { ...NOOP, needsResync: true }
          : { ...NOOP, write: frame.bytes };
      case "diff":
        if (this.awaitingSnap) {
          return { ...NOOP, needsResync: true };
        }
        this.painted = true;
        return { ...NOOP, write: frame.bytes };
    }
  }
}
