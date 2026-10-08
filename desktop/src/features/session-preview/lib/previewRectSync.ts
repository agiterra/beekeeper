import type { SessionPreviewRect } from "@/shared/api/tauriSessionPreview";

/**
 * Last-write-wins rect sync for the native preview slot (WIRE-C4 § 3,
 * `session_preview_set_rect`).
 *
 * Every layout change calls `schedule(rect)`. At most one frame later the
 * newest rect is sent, and only if it differs from the last one sent. While a
 * send is in flight, newer rects overwrite one pending slot instead of
 * queueing, so a drag never builds a backlog: whatever arrives last is what
 * Rust applies. Each send carries a rising `seq`, which Rust also uses to drop
 * anything that arrives out of order.
 *
 * Pure (the frame scheduler is injected), so the unit test drives it with a
 * fake clock.
 */
export type PreviewRectSync = {
  /** Record the slot's newest rect (or `null`: unmounted). */
  schedule(rect: SessionPreviewRect | null): void;
  /** Forget the last sent rect, so the next schedule always sends. */
  invalidate(): void;
  /** Stop: cancels the pending frame; nothing more is sent. */
  dispose(): void;
};

export type PreviewRectSyncOptions = {
  send: (rect: SessionPreviewRect | null, seq: number) => Promise<unknown>;
  requestFrame: (callback: () => void) => number;
  cancelFrame: (handle: number) => void;
  /** The next `seq`; must rise across syncs (see {@link nextPreviewRectSeq}). */
  nextSeq?: () => number;
  /** Called when a send rejects; the sync keeps going. */
  onError?: (error: unknown) => void;
};

const UNSET = Symbol("unset");

// Per-webview sequence for `set_rect`. Rust drops a `seq` older than the last
// it applied, and a slot remounts (tab switch, dock, pop-out window) many
// times per preview, so the counter outlives each sync. Seeded from the clock
// so a reloaded webview keeps rising. UI bookkeeping, not community data:
// `resetCommunityState()` has nothing to reset here.
let lastSeq = 0;

/** A `seq` greater than every one this webview has sent. */
export function nextPreviewRectSeq(now: number = Date.now()): number {
  lastSeq = Math.max(lastSeq + 1, now);
  return lastSeq;
}

/** Round to the device's logical point grid so sub-pixel jitter never sends. */
export function roundPreviewRect(rect: SessionPreviewRect): SessionPreviewRect {
  return {
    x: Math.round(rect.x),
    y: Math.round(rect.y),
    width: Math.max(0, Math.round(rect.width)),
    height: Math.max(0, Math.round(rect.height)),
  };
}

export function samePreviewRect(
  left: SessionPreviewRect | null,
  right: SessionPreviewRect | null,
): boolean {
  if (left === null || right === null) return left === right;
  return (
    left.x === right.x &&
    left.y === right.y &&
    left.width === right.width &&
    left.height === right.height
  );
}

export function createPreviewRectSync(
  options: PreviewRectSyncOptions,
): PreviewRectSync {
  let pending: SessionPreviewRect | null | typeof UNSET = UNSET;
  let lastSent: SessionPreviewRect | null | typeof UNSET = UNSET;
  let frame: number | null = null;
  let inFlight = false;
  let counter = 0;
  const nextSeq =
    options.nextSeq ??
    (() => {
      counter += 1;
      return counter;
    });
  let disposed = false;

  const flush = () => {
    frame = null;
    if (disposed || inFlight || pending === UNSET) return;
    const next = pending;
    pending = UNSET;
    if (lastSent !== UNSET && samePreviewRect(lastSent, next)) return;
    lastSent = next;
    const seq = nextSeq();
    inFlight = true;
    options
      .send(next, seq)
      .catch((error) => {
        // A failed send says nothing about where the view is: send again.
        lastSent = UNSET;
        options.onError?.(error);
      })
      .finally(() => {
        inFlight = false;
        if (!disposed && pending !== UNSET) arm();
      });
  };

  const arm = () => {
    if (frame === null) frame = options.requestFrame(flush);
  };

  return {
    schedule(rect) {
      if (disposed) return;
      pending = rect === null ? null : roundPreviewRect(rect);
      if (!inFlight) arm();
    },
    invalidate() {
      lastSent = UNSET;
    },
    dispose() {
      disposed = true;
      if (frame !== null) options.cancelFrame(frame);
      frame = null;
    },
  };
}
