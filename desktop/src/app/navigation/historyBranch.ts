/**
 * How the forward stack survives a navigation.
 *
 * TanStack's hash history exposes an index (`__TSR_index`) and a per-entry key
 * (`__TSR_key`). Forward-ness is not something the browser will tell us, so the
 * shell tracks the highest index it has seen and treats anything below it as
 * forward history.
 *
 * The subtlety is telling a *new* branch from a *replace in place*. Both change
 * the key stored for an index. Only the first should discard what was ahead:
 *
 * - a push arrives at a higher index with a key we have not stored — everything
 *   above it is now unreachable, so it goes;
 * - `history.forward()` arrives at a higher index with the key we already had —
 *   the entries above are still there;
 * - a `navigate({ replace: true })` stays at the same index and mints a new
 *   key — nothing ahead of it moved;
 * - `history.back()` arrives at a lower index and touches nothing.
 *
 * Reading the third case as a branch is the bug this exists to prevent: it
 * killed the forward arrow every time someone picked a person in the New
 * message screen (which rewrites `/messages/new` into the DM in place), left a
 * huddle, or switched community — the arrow went grey over history that was
 * still sitting there.
 *
 * Pure so the table can be unit tested without a router.
 */

export type HistoryBranchInput = {
  /** Index of the entry we were on, or null on the first observation. */
  previousIndex: number | null;
  /** Index of the entry we are on now. */
  index: number;
  /** Key we had recorded for `index`, if any. */
  storedKey: string | undefined;
  /** Key the entry actually carries now. */
  key: string;
  /** Highest index reached so far. */
  maxIndex: number;
};

export type HistoryBranchResult = {
  /** New high-water mark. */
  maxIndex: number;
  /** Whether every recorded entry at or above `index` is now unreachable. */
  truncateAbove: boolean;
};

export function resolveHistoryBranch({
  previousIndex,
  index,
  storedKey,
  key,
  maxIndex,
}: HistoryBranchInput): HistoryBranchResult {
  // Replace in place, or a re-render on the same entry. The key may be new;
  // the entries ahead are untouched either way.
  if (previousIndex !== null && index === previousIndex) {
    return { maxIndex: Math.max(maxIndex, index), truncateAbove: false };
  }

  // Went back. Nothing was destroyed.
  if (previousIndex !== null && index < previousIndex) {
    return { maxIndex: Math.max(maxIndex, index), truncateAbove: false };
  }

  // Arrived at a higher index (or made the first observation). A key we have
  // already recorded means we walked forward into history we still hold; a new
  // key means this is a fresh entry and it overwrote whatever was ahead.
  const isKnownEntry = storedKey !== undefined && storedKey === key;
  if (isKnownEntry) {
    return { maxIndex: Math.max(maxIndex, index), truncateAbove: false };
  }

  return { maxIndex: index, truncateAbove: storedKey !== undefined };
}
