import * as React from "react";

/**
 * Whether a height-capped scroller is hiding lines below its cap.
 *
 * Measured only while the cap is on (`expanded` false): once the reader lifts
 * it, the last answer stands, so the control that lifted it stays to put it
 * back. Where an ancestor removes the cap (the coding-session column), the
 * element is as tall as its content and this reads `false`, so no control is
 * drawn for a cap that is not there.
 *
 * @param ref the capped element (`max-height` + `overflow-y: auto`)
 * @param expanded whether the reader has lifted the cap
 * @param deps values that change the element's content or wrapping
 */
export function useExceedsHeightCap<T extends HTMLElement>(
  ref: React.RefObject<T | null>,
  expanded: boolean,
  deps: readonly unknown[] = [],
): boolean {
  const [exceeds, setExceeds] = React.useState(false);

  const measure = React.useCallback(() => {
    const node = ref.current;
    if (!node || expanded) return;
    setExceeds(node.scrollHeight - node.clientHeight > 1);
  }, [ref, expanded]);

  React.useEffect(() => {
    const node = ref.current;
    if (!node || typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(node);
    for (const child of Array.from(node.children)) observer.observe(child);
    return () => observer.disconnect();
  }, [ref, measure]);

  React.useEffect(() => {
    measure();
  }, [measure, ...deps]);

  return exceeds;
}
