import type { TimelineMessageDelta } from "@/features/messages/lib/timelineSnapshot";

export function getPinnedCenterDrift({
  contentTop,
  currentContentTop,
}: {
  contentTop: number;
  currentContentTop: number;
}): number | null {
  const drift = currentContentTop - contentTop;
  return Math.abs(drift) > 0.5 ? drift : null;
}

export function shouldIgnorePinnedCenterScroll({
  currentScrollTop,
  expectedScrollTop,
  isWritingScroll,
}: {
  currentScrollTop: number;
  expectedScrollTop: number | null;
  isWritingScroll: boolean;
}): boolean {
  return isWritingScroll || expectedScrollTop === currentScrollTop;
}

// Programmatic bottom pins require the physical floor, not merely the looser
// UI at-bottom threshold used for unread affordances.
const TRUE_BOTTOM_THRESHOLD_PX = 1;

type BottomSettleContainer = Pick<
  HTMLDivElement,
  "scrollHeight" | "clientHeight" | "scrollTop" | "scrollTo"
>;

export function settleProgrammaticBottomPin(
  container: BottomSettleContainer,
): boolean {
  container.scrollTo({ top: container.scrollHeight, behavior: "auto" });
  return (
    container.scrollHeight - container.clientHeight - container.scrollTop <=
    TRUE_BOTTOM_THRESHOLD_PX
  );
}

export function shouldSettleForSplitPanel({
  isAtBottom,
  splitPanelOpen,
}: {
  isAtBottom: boolean;
  splitPanelOpen: boolean;
}): boolean {
  return isAtBottom && splitPanelOpen;
}

export function shouldSettleVirtualizedBottom({
  isAtBottom,
  messageDelta,
  messagesArrived,
  messagesChanged,
}: {
  isAtBottom: boolean;
  messageDelta: TimelineMessageDelta;
  messagesArrived: number;
  messagesChanged: boolean;
}): boolean {
  return (
    isAtBottom &&
    messageDelta !== "prepend" &&
    (messagesArrived > 0 || messagesChanged)
  );
}

// Keys that scroll a focused-or-last-clicked scroller toward older content.
// Space scrolls up only with Shift.
const UPWARD_SCROLL_KEYS = new Set(["ArrowUp", "PageUp", "Home"]);

export function isUpwardScrollKey({
  key,
  shiftKey,
  targetIsEditable,
}: {
  key: string;
  shiftKey: boolean;
  targetIsEditable: boolean;
}): boolean {
  if (targetIsEditable) return false;
  return UPWARD_SCROLL_KEYS.has(key) || (key === " " && shiftKey);
}

// A bottom-pinned view whose next scroll event reads as "left the bottom" is
// released only when the reader scrolled it up: reader input AND a smaller
// scrollTop. Otherwise the gap is content that landed after the event was
// queued (a virtualizer correcting `scrollTop` for freshly measured rows emits
// its scroll event a frame late), and the view should re-pin instead. Input
// alone is not enough: a reader who drags back down to the floor is still
// "scrolling" while that late growth lands.
export function shouldHoldBottomPin({
  holdEnabled,
  nextAtBottom,
  readerScrolledUp,
  wasAtBottom,
}: {
  holdEnabled: boolean;
  nextAtBottom: boolean;
  readerScrolledUp: boolean;
  wasAtBottom: boolean;
}): boolean {
  return holdEnabled && wasAtBottom && !nextAtBottom && !readerScrolledUp;
}
