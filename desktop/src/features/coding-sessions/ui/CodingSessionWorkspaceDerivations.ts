import * as React from "react";

import { deriveTranscriptItemBlockIds } from "@/features/agents/ui/agentSessionTranscriptGrouping";
import type { TranscriptItem } from "@/features/agents/ui/agentSessionTypes";
import { deriveCodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import {
  deriveCodingSessionSubagentPanel,
  settleCodingSessionSubagentPanel,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import {
  type CodingSessionTranscriptBlock,
  type CodingSessionTurnRestingStatus,
  deriveCodingSessionObservedChanges,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { resolveCodingSessionTurnSettlementsById } from "@/features/coding-sessions/lib/codingSessionTranscriptModelSettlement";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";

/**
 * The whole-transcript derivations the single-session workspace reads.
 *
 * Injectable only so a test can count how often each one runs; production
 * always uses {@link CODING_SESSION_WORKSPACE_DERIVERS}.
 */
export type CodingSessionWorkspaceDerivers = {
  blockIds: (items: readonly TranscriptItem[]) => string[];
  taskModel: typeof deriveCodingSessionTaskModel;
  contextWindow: typeof deriveCodingSessionContextWindow;
  observedChanges: typeof deriveCodingSessionObservedChanges;
  subagents: (
    items: readonly TranscriptItem[],
  ) => ReturnType<typeof deriveCodingSessionSubagentPanel>;
};

export const CODING_SESSION_WORKSPACE_DERIVERS: CodingSessionWorkspaceDerivers =
  {
    blockIds: deriveTranscriptItemBlockIds,
    taskModel: deriveCodingSessionTaskModel,
    contextWindow: deriveCodingSessionContextWindow,
    observedChanges: deriveCodingSessionObservedChanges,
    subagents: (items) => deriveCodingSessionSubagentPanel([items]),
  };

/** Deeper than any projected item nests; past it, values count as changed. */
const MAX_COMPARE_DEPTH = 32;

/**
 * Structural equality over the plain data a projected transcript item holds.
 *
 * Projection rebuilds every item object on every publish, but the strings and
 * nested payloads inside mostly come straight from the store's retained parse,
 * so the `Object.is` fast path settles nearly every field without walking it.
 * Anything that is not a plain object or array (a function, a class instance)
 * is equal only to itself.
 */
function sameTranscriptValue(
  left: unknown,
  right: unknown,
  depth: number,
): boolean {
  if (Object.is(left, right)) return true;
  if (
    depth > MAX_COMPARE_DEPTH ||
    typeof left !== "object" ||
    typeof right !== "object" ||
    left === null ||
    right === null
  ) {
    return false;
  }
  if (Array.isArray(left)) {
    if (!Array.isArray(right) || left.length !== right.length) return false;
    for (let index = 0; index < left.length; index += 1) {
      if (!sameTranscriptValue(left[index], right[index], depth + 1)) {
        return false;
      }
    }
    return true;
  }
  if (Array.isArray(right)) return false;
  const leftProto = Object.getPrototypeOf(left);
  if (
    leftProto !== Object.getPrototypeOf(right) ||
    (leftProto !== Object.prototype && leftProto !== null)
  ) {
    return false;
  }
  const leftRecord = left as Record<string, unknown>;
  const rightRecord = right as Record<string, unknown>;
  const leftKeys = Object.keys(leftRecord);
  if (leftKeys.length !== Object.keys(rightRecord).length) return false;
  for (const key of leftKeys) {
    if (!Object.hasOwn(rightRecord, key)) return false;
    if (!sameTranscriptValue(leftRecord[key], rightRecord[key], depth + 1)) {
      return false;
    }
  }
  return true;
}

/**
 * Carry forward every item that did not change, and the whole array when none
 * did.
 *
 * The catalog re-projects a session's transcript whenever *anything* in the
 * channel publishes — a status change, another execution's event, a receipt —
 * and each re-projection is a new array of new objects. Without this, every
 * derivation below re-ran over the whole transcript on events that changed
 * nothing in it, and every memoized row downstream saw a "new" item. With it,
 * an unrelated event costs one comparison pass and no derivation; an append
 * keeps every earlier item's identity.
 *
 * Positional on purpose: an item that moved is a changed transcript.
 */
export function reconcileCodingSessionTranscriptItems<T>(
  previous: readonly T[] | null,
  next: readonly T[],
): readonly T[] {
  if (previous === null || previous === next) return next;
  let changed = previous.length !== next.length;
  let reused = 0;
  const merged = next.map((item, index) => {
    const prior = previous[index];
    if (index < previous.length && sameTranscriptValue(prior, item, 0)) {
      reused += 1;
      return prior;
    }
    changed = true;
    return item;
  });
  if (!changed) return previous;
  return reused === 0 ? next : merged;
}

/** {@link reconcileCodingSessionTranscriptItems} across renders. */
export function useStableCodingSessionTranscript<T>(next: T[]): T[] {
  const ref = React.useRef<T[] | null>(null);
  const stable = reconcileCodingSessionTranscriptItems(
    ref.current,
    next,
  ) as T[];
  ref.current = stable;
  return stable;
}

/**
 * Every transcript-wide derivation the workspace needs, each memoized on the
 * reconciled transcript alone — so each recomputes only when the transcript's
 * content changed, never on a re-render or a re-projection that changed
 * nothing.
 */
export function useCodingSessionWorkspaceDerivations<T extends TranscriptItem>(
  rawTranscript: T[],
  derivers: CodingSessionWorkspaceDerivers = CODING_SESSION_WORKSPACE_DERIVERS,
) {
  const transcript = useStableCodingSessionTranscript(rawTranscript);
  const blockIds = React.useMemo(
    () => derivers.blockIds(transcript),
    [derivers, transcript],
  );
  const stableBlockIds = useStableArrayShallow(blockIds);
  const messages = React.useMemo(
    () => stableBlockIds.map((id) => ({ id })),
    [stableBlockIds],
  );
  const taskModel = React.useMemo(
    () => derivers.taskModel(transcript),
    [derivers, transcript],
  );
  const contextWindow = React.useMemo(
    () => derivers.contextWindow(transcript),
    [derivers, transcript],
  );
  const observedChanges = React.useMemo(
    () => derivers.observedChanges(transcript),
    [derivers, transcript],
  );
  const subagents = React.useMemo(
    () => derivers.subagents(transcript),
    [derivers, transcript],
  );
  return {
    contextWindow,
    messages,
    observedChanges,
    subagents,
    taskModel,
    transcript,
  };
}

/**
 * The Agents panel read through each spawn's turn settlement — the one its
 * stream row reads it through (`CodingSessionTurnSettlementContext`) — so an
 * open Task call in a turn the stream shows as stopped or of unknown status
 * is not a spinner one click away. A call outside any turn keeps the
 * stream's default, `live`.
 */
export function useCodingSessionSettledSubagents(
  panel: ReturnType<typeof deriveCodingSessionSubagentPanel>,
  blocks: readonly CodingSessionTranscriptBlock[],
  restingStatus: CodingSessionTurnRestingStatus,
): ReturnType<typeof deriveCodingSessionSubagentPanel> {
  return React.useMemo(() => {
    if (panel.rows.every((row) => row.spawn.status !== "running")) {
      return panel;
    }
    const settlements = resolveCodingSessionTurnSettlementsById(
      blocks,
      restingStatus,
    );
    return settleCodingSessionSubagentPanel(panel, (call) =>
      call.turnId ? (settlements.get(call.turnId) ?? "live") : "live",
    );
  }, [blocks, panel, restingStatus]);
}
