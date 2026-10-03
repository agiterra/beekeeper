import * as React from "react";

/**
 * Hold a scroller's distance from the bottom, not its `scrollTop`.
 *
 * Andy, 2026-09-29: back on a session tab, the umbrella stream stopped ~200 px
 * short of the bottom. The router restores every scroller's absolute
 * `scrollTop` after a navigation, and on a remount the stream's reserve and
 * rows are not yet the height they were when that number was taken, so the
 * restored number points somewhere else. The stream also had no notion of
 * "at the latest" at all: a reply growing under a reader at the bottom, or the
 * dock growing under the last row, left the view where it was.
 *
 * One number drives all of it: the distance from the bottom, 0 when the reader
 * is at the latest. While it is 0, any resize of the scroller or its content —
 * a streaming reply, the dock's reserve, a row measuring itself — puts the
 * view back at the bottom, and so does any scroll the reader did not make: the
 * browser's own scroll anchoring moves `scrollTop` when rows grow, and its
 * scroll event lands *before* the resize callback for the same frame, so read
 * as a position it would switch following off (measured: a 518 px burst read
 * as the reader 553 px up). Only reader input — wheel, scroll keys, a pointer
 * held on the scroller or pressed anywhere, which is how a click on a reveal
 * control moves the stream — releases the bottom. When the reader scrolls up,
 * the number follows what they did and resizes leave the view alone. It is kept per `key` across
 * unmounts, and for a short window after a mount it is re-applied over any
 * scroll the reader did not make, which is where the router's restore lands.
 */

/** Within this many px of the bottom counts as at the latest. */
export const BOTTOM_ANCHOR_SLOP_PX = 4;

/** How long after a mount a scroll the reader did not make is overridden. */
export const BOTTOM_ANCHOR_RESTORE_MS = 1_000;

/** How long one wheel tick or scroll key counts as the reader scrolling. */
const READER_INPUT_MS = 300;

/** Sessions remembered; the oldest is dropped past this. */
const MAX_REMEMBERED = 256;

export type BottomAnchorState = {
  /** Distance from the bottom to hold; 0 means at the latest. */
  distance: number;
  /** Inside the window after a mount that overrides non-reader scrolls. */
  restoring: boolean;
};

export type BottomAnchorStep = {
  state: BottomAnchorState;
  /** Scroll to `state.distance` from the bottom. */
  apply: boolean;
};

type ScrollMetrics = Pick<
  HTMLElement,
  "clientHeight" | "scrollHeight" | "scrollTop"
>;

export function distanceFromBottom(metrics: ScrollMetrics): number {
  return Math.max(
    0,
    metrics.scrollHeight - metrics.clientHeight - metrics.scrollTop,
  );
}

export function scrollTopForDistance(
  metrics: Pick<HTMLElement, "clientHeight" | "scrollHeight">,
  distance: number,
): number {
  return Math.max(0, metrics.scrollHeight - metrics.clientHeight - distance);
}

function settle(distance: number): number {
  return distance <= BOTTOM_ANCHOR_SLOP_PX ? 0 : distance;
}

/**
 * A scroll event: the reader's moves are recorded. Anything else is undone at
 * the latest and while restoring, and recorded otherwise.
 */
export function bottomAnchorOnScroll(
  state: BottomAnchorState,
  input: { distance: number; readerActive: boolean },
): BottomAnchorStep {
  if (input.readerActive) {
    return {
      state: { distance: settle(input.distance), restoring: false },
      apply: false,
    };
  }
  if (state.restoring || state.distance === 0) {
    return { state, apply: settle(input.distance) !== state.distance };
  }
  return {
    state: { ...state, distance: settle(input.distance) },
    apply: false,
  };
}

/** A resize: at the latest (or restoring) holds the number; else follow it. */
export function bottomAnchorOnResize(
  state: BottomAnchorState,
  input: { distance: number },
): BottomAnchorStep {
  if (state.restoring || state.distance === 0) {
    return { state, apply: settle(input.distance) !== state.distance };
  }
  return {
    state: { ...state, distance: settle(input.distance) },
    apply: false,
  };
}

/**
 * Per-key scroll memory: the held distance, and the first entry the umbrella
 * timeline's render window started at (`CodingSessionUmbrellaTimelineWindow`).
 * One map so a remount restores both together — a distance measured against
 * thirty loaded turns means nothing against the default ten.
 */
type RememberedScroll = { distance: number; windowStartKey: string | null };

const remembered = new Map<string, RememberedScroll>();

function rememberPatch(key: string, patch: Partial<RememberedScroll>) {
  const previous = remembered.get(key) ?? { distance: 0, windowStartKey: null };
  remembered.delete(key);
  remembered.set(key, { ...previous, ...patch });
  if (remembered.size > MAX_REMEMBERED) {
    const oldest = remembered.keys().next().value;
    if (oldest !== undefined) remembered.delete(oldest);
  }
}

function remember(key: string, distance: number) {
  rememberPatch(key, { distance });
}

/** Record where the umbrella timeline's render window starts for `key`. */
export function rememberCodingSessionNarrativeWindow(
  key: string,
  windowStartKey: string | null,
): void {
  if (remembered.get(key)?.windowStartKey === windowStartKey) return;
  rememberPatch(key, { windowStartKey });
}

/** The render-window start remembered for `key`, or null. */
export function recallCodingSessionNarrativeWindow(key: string): string | null {
  return remembered.get(key)?.windowStartKey ?? null;
}

/** Forget every remembered distance and window (a community switch). */
export function resetCodingSessionNarrativeMemory(): void {
  remembered.clear();
}

/** The live anchor state of each bound scroller, for `isBottomAnchorAtLatest`. */
const liveStates = new WeakMap<HTMLElement, () => BottomAnchorState>();

/**
 * Whether the anchor bound to `scroller` is holding the bottom — the anchor's
 * own number, not geometry. Read during a commit, geometry already includes
 * rows that just mounted and reports the reader as scrolled up; the anchor's
 * state does not move until its resize callback runs. `false` when unbound.
 */
export function isBottomAnchorAtLatest(scroller: HTMLElement | null): boolean {
  if (!scroller) return false;
  return liveStates.get(scroller)?.().distance === 0;
}

function isScrollKey(event: KeyboardEvent): boolean {
  const target = event.target as HTMLElement | null;
  if (
    target?.isContentEditable === true ||
    /^(INPUT|TEXTAREA|SELECT)$/.test(target?.tagName ?? "")
  ) {
    return false;
  }
  return [
    "ArrowDown",
    "ArrowUp",
    "End",
    "Home",
    "PageDown",
    "PageUp",
    " ",
  ].includes(event.key);
}

/**
 * Keep a scroller's distance from the bottom across content and reserve
 * resizes and across remounts under the same `key`. A session opened for the
 * first time opens at its latest content.
 *
 * Returns the ref to put on the scroller; it also fills `scrollRef` for code
 * that reads the element. A callback ref rather than an effect over
 * `scrollRef.current`, so the binding follows whichever element React
 * attaches instead of the one that existed when an effect happened to run.
 */
export function useCodingSessionBottomAnchor<T extends HTMLElement>(
  scrollRef: React.RefObject<T | null>,
  key: string,
): React.RefCallback<T> {
  return React.useCallback(
    (scroller: T | null) => {
      scrollRef.current = scroller;
      if (!scroller) return;
      return bindBottomAnchor(scroller, key, () => {
        if (scrollRef.current === scroller) scrollRef.current = null;
      });
    },
    [key, scrollRef],
  );
}

function bindBottomAnchor(
  scroller: HTMLElement,
  key: string,
  onUnbind: () => void,
): () => void {
  const doc = scroller.ownerDocument;
  const view = doc.defaultView ?? window;
  let state: BottomAnchorState = {
    distance: remembered.get(key)?.distance ?? 0,
    restoring: true,
  };
  let readerUntil = 0;
  let pointerHeld = false;
  const readerActive = () =>
    pointerHeld || view.performance.now() < readerUntil;
  const markReader = () => {
    readerUntil = view.performance.now() + READER_INPUT_MS;
  };
  const apply = () => {
    const top = scrollTopForDistance(scroller, state.distance);
    if (Math.abs(scroller.scrollTop - top) > 1) scroller.scrollTop = top;
  };
  const run = (step: BottomAnchorStep) => {
    state = step.state;
    if (step.apply) apply();
  };
  const handleScroll = () =>
    run(
      bottomAnchorOnScroll(state, {
        distance: distanceFromBottom(scroller),
        readerActive: readerActive(),
      }),
    );
  const handleResize = () =>
    run(
      bottomAnchorOnResize(state, {
        distance: distanceFromBottom(scroller),
      }),
    );
  const handlePointerDown = () => {
    pointerHeld = true;
  };
  // A press anywhere may start a scroll of this stream (a reveal control,
  // a link to a row); its first scroll event lands within the window.
  const handleDocumentPointerDown = () => markReader();
  const handlePointerUp = () => {
    if (!pointerHeld) return;
    pointerHeld = false;
    // The scroll event for the last drag step can land after the release.
    markReader();
  };
  const handleKeyDown = (event: KeyboardEvent) => {
    if (isScrollKey(event)) markReader();
  };

  liveStates.set(scroller, () => state);
  apply();
  const restoreTimer = view.setTimeout(() => {
    state = { ...state, restoring: false };
  }, BOTTOM_ANCHOR_RESTORE_MS);
  scroller.addEventListener("scroll", handleScroll, { passive: true });
  scroller.addEventListener("wheel", markReader, { passive: true });
  scroller.addEventListener("pointerdown", handlePointerDown);
  doc.addEventListener("pointerdown", handleDocumentPointerDown);
  doc.addEventListener("pointerup", handlePointerUp);
  doc.addEventListener("pointercancel", handlePointerUp);
  doc.addEventListener("keydown", handleKeyDown);
  // The content's border box carries the dock reserve as padding, so a
  // reserve change is a resize here even when no row moved.
  let observer: ResizeObserver | null = null;
  if (typeof ResizeObserver !== "undefined") {
    observer = new ResizeObserver(handleResize);
    observer.observe(scroller);
    const content = scroller.firstElementChild;
    if (content) observer.observe(content, { box: "border-box" });
  } else {
    view.addEventListener("resize", handleResize);
  }
  return () => {
    remember(key, state.distance);
    liveStates.delete(scroller);
    view.clearTimeout(restoreTimer);
    scroller.removeEventListener("scroll", handleScroll);
    scroller.removeEventListener("wheel", markReader);
    scroller.removeEventListener("pointerdown", handlePointerDown);
    doc.removeEventListener("pointerdown", handleDocumentPointerDown);
    doc.removeEventListener("pointerup", handlePointerUp);
    doc.removeEventListener("pointercancel", handlePointerUp);
    doc.removeEventListener("keydown", handleKeyDown);
    if (observer) observer.disconnect();
    else view.removeEventListener("resize", handleResize);
    onUnbind();
  };
}
