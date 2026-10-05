import * as React from "react";

import type { CodingSessionMinimapItem } from "@/features/coding-sessions/lib/codingSessionTranscriptMinimapItems";
import type { ListVirtualizer } from "@/shared/ui/VirtualizedList";

/** T3's `viewOffset`: a jumped-to turn lands just below the pane's top. */
export const CODING_SESSION_MINIMAP_JUMP_OFFSET = 24;

const TURN_KEY_PREFIX = "turn:";

/** The single layout's rendered turn for a minimap item, if mounted. */
export function findCodingSessionTranscriptTurnElement(
  scroller: HTMLElement | null,
  item: Pick<CodingSessionMinimapItem, "key">,
): HTMLElement | null {
  if (scroller === null || !item.key.startsWith(TURN_KEY_PREFIX)) return null;
  const turnId = item.key.slice(TURN_KEY_PREFIX.length);
  const escaped =
    typeof CSS !== "undefined" && typeof CSS.escape === "function"
      ? CSS.escape(turnId)
      : turnId.replace(/["\\]/g, "\\$&");
  return scroller.querySelector<HTMLElement>(`[data-turn-id="${escaped}"]`);
}

/** Scroll `element` to the scroller's top, less the jump offset. */
export function scrollCodingSessionElementToTop(
  scroller: HTMLElement,
  element: HTMLElement,
): void {
  const top =
    element.getBoundingClientRect().top -
    scroller.getBoundingClientRect().top +
    scroller.scrollTop -
    CODING_SESSION_MINIMAP_JUMP_OFFSET;
  scroller.scrollTo({ top: Math.max(0, top), behavior: "smooth" });
}

/**
 * How the single layout's minimap finds and reaches a turn: through the
 * virtualizer when the transcript is virtualized (past 40 rows), where an
 * off-screen turn has no element to scroll to, and by scrolling the mounted
 * element otherwise.
 */
export function useCodingSessionTranscriptMinimapNavigation(input: {
  scrollRef: React.RefObject<HTMLElement | null> | undefined;
  virtualizerRef: React.RefObject<ListVirtualizer | null> | null;
}): {
  select: (item: CodingSessionMinimapItem) => void;
  resolveElement: (item: CodingSessionMinimapItem) => HTMLElement | null;
} {
  const { scrollRef, virtualizerRef } = input;
  const resolveElement = React.useCallback(
    (item: CodingSessionMinimapItem) =>
      findCodingSessionTranscriptTurnElement(scrollRef?.current ?? null, item),
    [scrollRef],
  );
  const select = React.useCallback(
    (item: CodingSessionMinimapItem) => {
      const virtualizer = virtualizerRef?.current ?? null;
      if (virtualizer !== null) {
        virtualizer.scrollToIndex(item.rowIndex, { align: "start" });
        return;
      }
      const scroller = scrollRef?.current ?? null;
      const element = resolveElement(item);
      if (scroller !== null && element !== null) {
        scrollCodingSessionElementToTop(scroller, element);
      }
    },
    [resolveElement, scrollRef, virtualizerRef],
  );
  return { select, resolveElement };
}
