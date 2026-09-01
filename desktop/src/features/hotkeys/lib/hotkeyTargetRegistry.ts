/**
 * The numbered rows a positional hotkey can reach, published by whatever
 * renders them.
 *
 * The sidebar is the only place that knows the true displayed order: a
 * project's children are filtered, type-ranked, paginated ten at a time and
 * then emitted in four separate blocks. Re-deriving that order inside a
 * keyboard handler would agree with the screen right up until either side
 * changed, and then quietly send ⌘3 somewhere other than the row wearing "3".
 *
 * So the renderer publishes its ordered, already-bound `activate` callbacks
 * here and the dispatcher indexes into them. The badge number and the chord
 * cannot disagree, because they are the same array.
 */

import * as React from "react";

export type HotkeyScope = "projects" | `project:${string}` | "dms";

type Listener = () => void;

export type HotkeyTarget = {
  /** Stable identity of the row, for debugging and test assertions. */
  key: string;
  label: string;
  activate: () => void;
};

const targetsByScope = new Map<HotkeyScope, HotkeyTarget[]>();

/**
 * Publish `targets` as the numbered rows of `scope`, replacing whatever was
 * there. Returns an unregister that only clears the scope if it still holds
 * *this* list — a remount that registers before the old effect cleans up must
 * not be undone by its predecessor.
 */
export function registerHotkeyTargets(
  scope: HotkeyScope,
  targets: HotkeyTarget[],
): () => void {
  targetsByScope.set(scope, targets);
  return () => {
    if (targetsByScope.get(scope) === targets) {
      targetsByScope.delete(scope);
    }
  };
}

export function getHotkeyTargets(
  scope: HotkeyScope | null,
): readonly HotkeyTarget[] {
  if (!scope) return [];
  return targetsByScope.get(scope) ?? [];
}

/** Activate the row at a zero-based position; returns whether one was there. */
export function activateHotkeyTarget(
  scope: HotkeyScope | null,
  index: number,
): boolean {
  const target = getHotkeyTargets(scope)[index];
  if (!target) return false;
  target.activate();
  return true;
}

/** Test-only: drop every scope between cases. */
export function __resetHotkeyTargetsForTests(): void {
  targetsByScope.clear();
  activeScope = null;
}

/**
 * Which scope the item modifier currently addresses.
 *
 * Published by the dispatcher (which owns the route → project resolution) and
 * read by the sidebar, so exactly one project group numbers its rows. Without
 * a single answer, every visible group would badge a "3" and only one of them
 * would be the one ⌘3 actually opens.
 */
let activeScope: HotkeyScope | null = null;
const scopeListeners = new Set<Listener>();

function subscribeActiveScope(listener: Listener): () => void {
  scopeListeners.add(listener);
  return () => {
    scopeListeners.delete(listener);
  };
}

function getActiveScopeSnapshot(): HotkeyScope | null {
  return activeScope;
}

export function setActiveHotkeyScope(scope: HotkeyScope | null): void {
  if (activeScope === scope) return;
  activeScope = scope;
  for (const listener of scopeListeners) listener();
}

export function useActiveHotkeyScope(): HotkeyScope | null {
  return React.useSyncExternalStore(
    subscribeActiveScope,
    getActiveScopeSnapshot,
    getActiveScopeSnapshot,
  );
}
