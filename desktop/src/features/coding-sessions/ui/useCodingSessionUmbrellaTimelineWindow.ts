import * as React from "react";

import {
  isBottomAnchorAtLatest,
  recallCodingSessionNarrativeWindow,
  rememberCodingSessionNarrativeWindow,
} from "@/features/coding-sessions/hooks/useCodingSessionBottomAnchor";
import {
  latestUmbrellaTimelineTurnKey,
  loadEarlierUmbrellaTimeline,
  resolveUmbrellaTimelineWindow,
  revealInUmbrellaTimeline,
  settleUmbrellaTimelinePin,
  type UmbrellaTimelineWindow,
  type UmbrellaTimelineWindowEntries,
} from "./CodingSessionUmbrellaTimelineWindow";

export type UmbrellaTimelineWindowControls = {
  window: UmbrellaTimelineWindow;
  /** Render `UMBRELLA_TIMELINE_WINDOW_TURNS` more turns above, holding the view. */
  loadEarlier: () => void;
  /**
   * Bring `key` into the window. Returns true when rows had to mount first;
   * the caller then waits for `onRevealReady` instead of scrolling now.
   */
  reveal: (key: string) => boolean;
};

/**
 * State for the umbrella timeline's render window (see
 * `CodingSessionUmbrellaTimelineWindow.ts` for the model and why it is a
 * window). The pin is remembered per `memoryKey` alongside the bottom
 * anchor's distance, so a remount restores both.
 */
export function useCodingSessionUmbrellaTimelineWindow<T>({
  source,
  memoryKey,
  scrollRef,
  onRevealReady,
}: {
  source: UmbrellaTimelineWindowEntries<T>;
  memoryKey: string;
  scrollRef?: React.RefObject<HTMLElement | null>;
  /** Called once the rows a `reveal` mounted are laid out. */
  onRevealReady: (key: string) => void;
}): UmbrellaTimelineWindowControls {
  const [pinState, setPinState] = React.useState(() => ({
    owner: memoryKey,
    pin: recallCodingSessionNarrativeWindow(memoryKey),
  }));
  const pin =
    pinState.owner === memoryKey
      ? pinState.pin
      : recallCodingSessionNarrativeWindow(memoryKey);
  const setPin = React.useCallback(
    (next: (current: string | null) => string | null) =>
      setPinState((current) => {
        const base =
          current.owner === memoryKey
            ? current.pin
            : recallCodingSessionNarrativeWindow(memoryKey);
        const pinned = next(base);
        return current.owner === memoryKey && current.pin === pinned
          ? current
          : { owner: memoryKey, pin: pinned };
      }),
    [memoryKey],
  );

  const timelineWindow = React.useMemo(
    () => resolveUmbrellaTimelineWindow(source, pin),
    [pin, source],
  );

  // Settle the pin after the list changes. A layout effect, so a trim lands
  // before paint; the anchor's own state says whether the reader was at the
  // latest *before* this commit's rows grew the content.
  const sourceRef = React.useRef(source);
  sourceRef.current = source;
  const latestTurnKey = latestUmbrellaTimelineTurnKey(source);
  const previousLatestTurnKey = React.useRef<string | null>(latestTurnKey);
  // biome-ignore lint/correctness/useExhaustiveDependencies: `source.entries` is the trigger — any list change may need the pin settled; the body reads it through `sourceRef`
  React.useLayoutEffect(() => {
    const newTurnArrived =
      previousLatestTurnKey.current !== null &&
      latestTurnKey !== previousLatestTurnKey.current;
    previousLatestTurnKey.current = latestTurnKey;
    const atLatest = isBottomAnchorAtLatest(scrollRef?.current ?? null);
    setPin((current) =>
      settleUmbrellaTimelinePin(sourceRef.current, {
        pinnedStartKey: current,
        newTurnArrived,
        atLatest,
      }),
    );
  }, [latestTurnKey, scrollRef, setPin, source.entries]);

  React.useEffect(() => {
    rememberCodingSessionNarrativeWindow(memoryKey, timelineWindow.startKey);
  }, [memoryKey, timelineWindow.startKey]);

  // "Load earlier" mounts rows above the reader. Hold the distance from the
  // bottom across that commit, so the row the reader was looking at stays put
  // (WebKit has no CSS scroll anchoring to do it for us).
  const preservedDistance = React.useRef<number | null>(null);
  const loadEarlier = React.useCallback(() => {
    const scroller = scrollRef?.current ?? null;
    preservedDistance.current = scroller
      ? scroller.scrollHeight - scroller.scrollTop
      : null;
    setPin((current) =>
      loadEarlierUmbrellaTimeline(sourceRef.current, current),
    );
  }, [scrollRef, setPin]);
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs on the commit that moved the window's start
  React.useLayoutEffect(() => {
    const distance = preservedDistance.current;
    const scroller = scrollRef?.current ?? null;
    preservedDistance.current = null;
    if (distance === null || !scroller) return;
    scroller.scrollTop = Math.max(0, scroller.scrollHeight - distance);
  }, [scrollRef, timelineWindow.startIndex]);

  // Read through a ref so `reveal` keeps one identity: the turn blocks it
  // reaches (via `onRevealFact`) are memoised on it.
  const pinRef = React.useRef(pin);
  pinRef.current = pin;
  const pendingReveal = React.useRef<string | null>(null);
  const reveal = React.useCallback(
    (key: string) => {
      const pinned = pinRef.current;
      const current = resolveUmbrellaTimelineWindow(sourceRef.current, pinned);
      const next = revealInUmbrellaTimeline(sourceRef.current, pinned, key);
      if (next === current.startKey) return false;
      pendingReveal.current = key;
      setPin(() => next);
      return true;
    },
    [setPin],
  );
  // biome-ignore lint/correctness/useExhaustiveDependencies: runs on the commit that mounted the rows a reveal asked for
  React.useEffect(() => {
    const key = pendingReveal.current;
    if (key === null) return;
    // Two frames: the first lets the bottom anchor's resize callback settle
    // the grown content, so its `scrollTop` write cannot cancel the reveal's
    // smooth scroll.
    let second = 0;
    const first = globalThis.requestAnimationFrame(() => {
      second = globalThis.requestAnimationFrame(() => {
        if (pendingReveal.current !== key) return;
        pendingReveal.current = null;
        onRevealReady(key);
      });
    });
    return () => {
      globalThis.cancelAnimationFrame(first);
      globalThis.cancelAnimationFrame(second);
    };
  }, [onRevealReady, timelineWindow.startIndex]);

  return { window: timelineWindow, loadEarlier, reveal };
}
