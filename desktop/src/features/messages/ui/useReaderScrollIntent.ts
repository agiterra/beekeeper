import * as React from "react";

import { isUpwardScrollKey } from "./anchoredScrollPolicy";

/** How long one upward wheel tick or scroll key counts as the reader
 *  scrolling. Inertial trackpad scrolling keeps emitting wheel events, so the
 *  window only has to bridge the gap to the scroll event each one produces. */
const READER_SCROLL_INTENT_MS = 300;

/**
 * Tracks whether the reader scrolled the container up: `scrollTop` fell since
 * the previous call, while there was reader input — an upward wheel tick or
 * scroll key within the last few hundred ms, or a pointer held on the scroller
 * (scrollbar drag, drag-select autoscroll, touch). Keys are read at the
 * document: a scroller the reader last clicked scrolls on PageUp even while
 * focus sits on the body.
 *
 * Returns a stable predicate to call with each scroll event's `scrollTop`; it
 * is always false while `enabled` is false.
 */
export function useReaderScrollIntent(
  scrollContainerRef: React.RefObject<HTMLDivElement | null>,
  enabled: boolean,
  resetKey: unknown,
): (scrollTop: number) => boolean {
  const scrollUntilRef = React.useRef(0);
  const pointerHeldRef = React.useRef(false);
  const lastScrollTopRef = React.useRef(0);

  // biome-ignore lint/correctness/useExhaustiveDependencies: resetKey deliberately re-subscribes after a keyed or conditional scroll-container mount replaces ref.current.
  React.useEffect(() => {
    if (!enabled) return;
    const container = scrollContainerRef.current;
    if (!container) return;
    const doc = container.ownerDocument;
    lastScrollTopRef.current = container.scrollTop;
    const markReaderScroll = () => {
      scrollUntilRef.current = performance.now() + READER_SCROLL_INTENT_MS;
    };
    const handleWheel = (event: WheelEvent) => {
      if (event.deltaY < 0) markReaderScroll();
    };
    const handlePointerDown = () => {
      pointerHeldRef.current = true;
    };
    const handlePointerUp = () => {
      if (!pointerHeldRef.current) return;
      pointerHeldRef.current = false;
      // The scroll event for the last drag step can land after the release.
      markReaderScroll();
    };
    const handleKeyDown = (event: KeyboardEvent) => {
      const target = event.target as HTMLElement | null;
      if (
        isUpwardScrollKey({
          key: event.key,
          shiftKey: event.shiftKey,
          targetIsEditable:
            target?.isContentEditable === true ||
            /^(INPUT|TEXTAREA|SELECT)$/.test(target?.tagName ?? ""),
        })
      ) {
        markReaderScroll();
      }
    };
    container.addEventListener("wheel", handleWheel, { passive: true });
    container.addEventListener("pointerdown", handlePointerDown);
    doc.addEventListener("pointerup", handlePointerUp);
    doc.addEventListener("pointercancel", handlePointerUp);
    doc.addEventListener("keydown", handleKeyDown);
    return () => {
      container.removeEventListener("wheel", handleWheel);
      container.removeEventListener("pointerdown", handlePointerDown);
      doc.removeEventListener("pointerup", handlePointerUp);
      doc.removeEventListener("pointercancel", handlePointerUp);
      doc.removeEventListener("keydown", handleKeyDown);
      pointerHeldRef.current = false;
      scrollUntilRef.current = 0;
    };
  }, [enabled, resetKey, scrollContainerRef]);

  return React.useCallback((scrollTop: number) => {
    const movedUp = scrollTop < lastScrollTopRef.current;
    lastScrollTopRef.current = scrollTop;
    return (
      movedUp &&
      (pointerHeldRef.current || performance.now() < scrollUntilRef.current)
    );
  }, []);
}
