/**
 * The umbrella timeline's render window: the most recent turns, with an
 * explicit "Load earlier" control for the rest.
 *
 * Why a window and not a virtualizer: every turn block carries its own
 * transcript, folds and expands in place, and its height changes while it
 * streams. A virtualizer over rows like that rewrites `scrollTop` as rows
 * measure, which fights the bottom anchor (`useCodingSessionBottomAnchor`),
 * the Route rail's `scrollIntoView`, and the IntersectionObserver that feeds
 * the rail's `You are here` band — and unmounting an off-screen block drops
 * its in-DOM disclosure state. A window keeps every rendered block an
 * ordinary flow child, so all three keep working unchanged, and bounds the
 * cost of a long multi-generation session to the last few turns.
 *
 * The window is described by one key — the entry it starts at — so turns that
 * arrive while the reader is scrolled up grow the window at the bottom and
 * never unmount a row above the reader. The window slides forward again (the
 * top trimmed back to the last `size` turns) only when a new turn arrives
 * while the reader is at the latest, where the bottom anchor holds the view.
 *
 * Pure functions over the entry list; the component owns the state.
 */

import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { isCompletedCodingSessionTurnBlock } from "@/features/coding-sessions/lib/codingSessionHandoff";
import type { CodingSessionUmbrellaTurnBlock } from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";

/** Turns rendered before "Load earlier" — and how many one press adds. */
export const UMBRELLA_TIMELINE_WINDOW_TURNS = 10;

export type UmbrellaTimelineWindowEntries<T> = {
  entries: readonly T[];
  keyOf: (entry: T) => string;
  /**
   * Whether an entry is a turn that counts toward the window. Light rows
   * (conversation, lifecycle, transactions) ride along with the turn they
   * precede and never count.
   */
  isTurn: (entry: T) => boolean;
  /**
   * Whether an entry must render whatever the window size: a turn still
   * running, or one waiting on the person (an unanswered permission). An open
   * turn sorts at its newest item, so a lead parked on an approval produces no
   * items and would otherwise sink above the control while other seats finish
   * ten turns — hiding the one row that needs an answer behind a click.
   */
  isLive?: (entry: T) => boolean;
};

export type UmbrellaTimelineWindow = {
  /** First rendered entry index; `0` renders everything. */
  startIndex: number;
  /** Key of the entry at `startIndex`, or null for an empty list. */
  startKey: string | null;
  /** Counted turns before the window. */
  hiddenTurnCount: number;
  /** Entries of any kind before the window. */
  hiddenEntryCount: number;
};

/**
 * Where a window holding the last `turns` counted turns starts.
 *
 * It starts just after the turn before them, so the user message and any
 * lifecycle or transaction rows that led into the window's first turn are
 * inside it rather than orphaned above the control.
 */
export function umbrellaTimelineFollowStart<T>(
  source: UmbrellaTimelineWindowEntries<T>,
  turns: number = UMBRELLA_TIMELINE_WINDOW_TURNS,
  before: number = source.entries.length,
): number {
  let seen = 0;
  for (let index = before - 1; index >= 0; index -= 1) {
    const entry = source.entries[index];
    if (entry === undefined || !source.isTurn(entry)) continue;
    seen += 1;
    if (seen > turns) return index + 1;
  }
  return 0;
}

function countTurns<T>(
  source: UmbrellaTimelineWindowEntries<T>,
  end: number,
): number {
  let count = 0;
  for (let index = 0; index < end; index += 1) {
    const entry = source.entries[index];
    if (entry !== undefined && source.isTurn(entry)) count += 1;
  }
  return count;
}

/** The earliest index the window must reach to keep every live entry in it. */
function liveFloor<T>(source: UmbrellaTimelineWindowEntries<T>): number {
  if (source.isLive === undefined) return source.entries.length;
  const index = source.entries.findIndex((entry) => source.isLive?.(entry));
  if (index === -1) return source.entries.length;
  // Open at the live entry's group, so the message that prompted it shows too.
  return source.isTurn(source.entries[index] as T)
    ? umbrellaTimelineFollowStart(source, 0, index)
    : index;
}

function indexOfKey<T>(
  source: UmbrellaTimelineWindowEntries<T>,
  key: string,
): number {
  return source.entries.findIndex((entry) => source.keyOf(entry) === key);
}

/**
 * Resolve the rendered window from the pinned start key.
 *
 * `null` (nothing pinned yet) and a key that is no longer in the list (a
 * density change filtered it out) both fall back to the last `size` turns.
 * A pin never shrinks the window below `size` turns, and no window starts
 * below a live entry (`isLive`).
 */
export function resolveUmbrellaTimelineWindow<T>(
  source: UmbrellaTimelineWindowEntries<T>,
  pinnedStartKey: string | null,
  size: number = UMBRELLA_TIMELINE_WINDOW_TURNS,
): UmbrellaTimelineWindow {
  const follow = Math.min(
    umbrellaTimelineFollowStart(source, size),
    liveFloor(source),
  );
  const pinned =
    pinnedStartKey === null ? -1 : indexOfKey(source, pinnedStartKey);
  const startIndex = pinned === -1 ? follow : Math.min(pinned, follow);
  const startEntry = source.entries[startIndex];
  return {
    startIndex,
    startKey: startEntry === undefined ? null : source.keyOf(startEntry),
    hiddenTurnCount: countTurns(source, startIndex),
    hiddenEntryCount: startIndex,
  };
}

/** The pin after "Load earlier": `size` more turns above the current start. */
export function loadEarlierUmbrellaTimeline<T>(
  source: UmbrellaTimelineWindowEntries<T>,
  pinnedStartKey: string | null,
  size: number = UMBRELLA_TIMELINE_WINDOW_TURNS,
): string | null {
  const current = resolveUmbrellaTimelineWindow(source, pinnedStartKey, size);
  if (current.startIndex === 0) return current.startKey;
  const next = umbrellaTimelineFollowStart(source, size, current.startIndex);
  const entry = source.entries[next];
  return entry === undefined ? current.startKey : source.keyOf(entry);
}

/**
 * The pin that brings `targetKey` into the window, or the current pin when it
 * is already inside (or not in the list at all — nothing to reach).
 *
 * The window opens at the start of the target's group — just after the turn
 * before it — so a jump to a turn also shows the message that prompted it.
 */
export function revealInUmbrellaTimeline<T>(
  source: UmbrellaTimelineWindowEntries<T>,
  pinnedStartKey: string | null,
  targetKey: string,
  size: number = UMBRELLA_TIMELINE_WINDOW_TURNS,
): string | null {
  const current = resolveUmbrellaTimelineWindow(source, pinnedStartKey, size);
  const target = indexOfKey(source, targetKey);
  if (target === -1 || target >= current.startIndex) return current.startKey;
  const groupStart = source.isTurn(source.entries[target] as T)
    ? umbrellaTimelineFollowStart(source, 0, target)
    : target;
  const entry = source.entries[groupStart];
  return entry === undefined ? current.startKey : source.keyOf(entry);
}

/** Key of the newest counted turn, or null. A change means a new turn arrived. */
export function latestUmbrellaTimelineTurnKey<T>(
  source: UmbrellaTimelineWindowEntries<T>,
): string | null {
  for (let index = source.entries.length - 1; index >= 0; index -= 1) {
    const entry = source.entries[index];
    if (entry !== undefined && source.isTurn(entry)) return source.keyOf(entry);
  }
  return null;
}

/**
 * The pin after the entry list changed.
 *
 * - Nothing pinned yet: pin where the window renders now, so later appends
 *   grow it instead of sliding it.
 * - A new turn arrived while the reader is at the latest: slide back to the
 *   last `size` turns. The bottom anchor holds the view, so trimming rows
 *   above it moves nothing the reader can see.
 * - Otherwise — the reader is scrolled up, or the change was a turn growing —
 *   keep the pin: nothing above the reader unmounts.
 */
export function settleUmbrellaTimelinePin<T>(
  source: UmbrellaTimelineWindowEntries<T>,
  input: {
    pinnedStartKey: string | null;
    newTurnArrived: boolean;
    atLatest: boolean;
  },
  size: number = UMBRELLA_TIMELINE_WINDOW_TURNS,
): string | null {
  if (input.pinnedStartKey !== null && input.newTurnArrived && input.atLatest) {
    return resolveUmbrellaTimelineWindow(source, null, size).startKey;
  }
  return resolveUmbrellaTimelineWindow(source, input.pinnedStartKey, size)
    .startKey;
}

function isUnansweredPermission(item: TranscriptItem): boolean {
  return (
    item.type === "lifecycle" &&
    item.renderClass === "permission" &&
    !item.outcome
  );
}

/**
 * Keys of the turn blocks the window must always render.
 *
 * - An unterminated block that is its execution's newest: the seat's live
 *   turn, including one parked on an approval that emits nothing.
 * - Any unterminated block holding an unanswered permission, wherever it is.
 *
 * Deliberately not every unterminated block: a turn cut off by a crash or a
 * superseded generation never writes its terminator, and pinning those would
 * hold the window open at the oldest such block forever. A *terminated* turn's
 * unanswered permission is moot — the turn ended — and does not pin either.
 */
export function umbrellaTimelineLiveBlockKeys<
  B extends Pick<CodingSessionUmbrellaTurnBlock, "executionKey" | "items">,
>(blocks: readonly B[], keyOf: (block: B) => string): ReadonlySet<string> {
  const newestByExecution = new Map<string, number>();
  blocks.forEach((block, index) => {
    newestByExecution.set(block.executionKey, index);
  });
  const live = new Set<string>();
  blocks.forEach((block, index) => {
    if (isCompletedCodingSessionTurnBlock(block)) return;
    if (
      newestByExecution.get(block.executionKey) === index ||
      block.items.some(isUnansweredPermission)
    ) {
      live.add(keyOf(block));
    }
  });
  return live;
}

/** Document event asking a mounted umbrella timeline to reveal a transcript item. */
export const UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT =
  "coding-session:umbrella-reveal-item";

/**
 * Ask the umbrella timeline to bring the turn holding `itemId` into its window
 * and scroll to it — the reach a plain DOM query cannot have for a row above
 * "Load earlier". Returns true when a mounted timeline owned the item.
 *
 * For `revealCodingSessionSubagentInStream` to fall back on when its query
 * finds nothing. A document event rather than a module-level registry, so
 * there is no singleton to reset on a community switch.
 */
export function requestCodingSessionUmbrellaItemReveal(
  itemId: string,
): boolean {
  if (typeof document === "undefined") return false;
  const event = new CustomEvent(UMBRELLA_TIMELINE_REVEAL_ITEM_EVENT, {
    cancelable: true,
    detail: { itemId },
  });
  return !document.dispatchEvent(event);
}
