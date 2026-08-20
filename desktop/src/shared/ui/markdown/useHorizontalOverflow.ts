import * as React from "react";

/**
 * Tracks whether a horizontally scrollable element still has content hidden to
 * its right.
 *
 * Platform overlay scrollbars paint nothing until the user is already
 * scrolling, so a wide code block or table ends in a hard cut that is
 * indistinguishable from clipped, truncated text. Callers surface the returned
 * flag as a visible edge fade, and because it is recomputed on resize and on
 * scroll, the affordance retracts honestly once the last column is on screen.
 *
 * @param ref element that owns the horizontal scroll (`overflow-x: auto`)
 * @param deps values that change the scrollable content or its wrapping mode
 * @returns `[hasHiddenOverflow, onScroll]` — spread `onScroll` onto the element
 */
export function useHorizontalOverflow<T extends HTMLElement>(
  ref: React.RefObject<T | null>,
  deps: readonly unknown[] = [],
): [boolean, () => void] {
  const [hasHiddenOverflow, setHasHiddenOverflow] = React.useState(false);

  const measure = React.useCallback(() => {
    const node = ref.current;
    if (!node) return;
    setHasHiddenOverflow(
      node.scrollWidth - node.clientWidth - node.scrollLeft > 1,
    );
  }, [ref]);

  React.useEffect(() => {
    const node = ref.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    for (const child of Array.from(node.children)) observer.observe(child);
    return () => observer.disconnect();
  }, [ref, measure]);

  React.useEffect(() => {
    measure();
    // Content and wrap-mode changes resize nothing the observer watches when the
    // element itself keeps its box, so re-measure explicitly.
  }, [measure, ...deps]);

  return [hasHiddenOverflow, measure];
}
