import * as React from "react";

import type { CodingSessionMissionOpenHolds } from "@/features/coding-sessions/lib/codingSessionMissionOpenHolds";

import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  codingSessionUmbrellaEntryKey,
  type CodingSessionUmbrellaTimelineEntry,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import type {
  CodingSessionCatalogRecord,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import { codingSessionWireWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type { CodingSessionAgentFocusItem } from "./CodingSessionAgentFocus";

export function scrollCodingSessionNarrativeToLatest(
  viewport: Pick<HTMLElement, "scrollHeight" | "scrollTo"> | null,
): void {
  if (!viewport) return;
  viewport.scrollTo({ behavior: "smooth", top: viewport.scrollHeight });
}

/** Focusing a seat (the live-activity bar, the roster) jumps to the latest. */
export function useScrollNarrativeToLatestOnFocus(
  narrativeScrollRef: React.RefObject<HTMLElement | null>,
  focusedExecutionKey: string | null,
): void {
  React.useLayoutEffect(() => {
    if (focusedExecutionKey === null) return;
    const frame = window.requestAnimationFrame(() => {
      scrollCodingSessionNarrativeToLatest(narrativeScrollRef.current);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusedExecutionKey, narrativeScrollRef]);
}

/**
 * Provenance labels a contiguous execution run, not every turn. A generation
 * lifecycle row already identifies the execution and generation, so the first
 * block after that row does not repeat the same chrome either.
 */
export function shouldShowTurnBlockProvenance(
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
  index: number,
): boolean {
  const entry = entries[index];
  if (entry?.kind !== "turn-block") return false;
  const previous = entries[index - 1];
  if (!previous) return true;
  if (previous.kind === "conversation") return true;
  if (previous.executionKey !== entry.executionKey) return true;
  if (previous.kind === "lifecycle") {
    return previous.generation !== entry.generation;
  }
  return (
    previous.kind !== "turn-block" || previous.generation !== entry.generation
  );
}

/** Map the umbrella's derived status onto the header's three honest states. */
export function umbrellaWorkspaceStatus(
  umbrella: Pick<CodingSessionUmbrellaRecord, "status">,
): CodingSessionWorkspaceStatus {
  return codingSessionWireWorkspaceStatus(umbrella.status);
}

/**
 * Mission's word for the umbrella's own liveness.
 *
 * A4: one screen said `WORKING` in the header pill, `idle`/`live` on the seat
 * chips, `Running (signed) · 1 seat live` in the Inspector and `Stop 2 live
 * seats` in an aria label — four vocabularies for one fact. DESIGN-SPEC fixes
 * two: A3 gives the *umbrella* `Running`, B1 gives the *seat* `live`. They are
 * different subjects and neither borrows the other's word.
 *
 * Returns null for every other state, which means "the status' own label is
 * already the right word" — the header falls back to it. Mission-gated on
 * purpose: `codingSessionWireWorkspaceStatus` is Conversation's mapping too,
 * and I8 freezes that DOM.
 */
export function codingSessionMissionUmbrellaWord(
  status: CodingSessionWorkspaceStatus,
): string | null {
  return status.kind === "working" ? "Running" : null;
}

export function umbrellaAgentStatusSummary(
  agents: readonly CodingSessionAgentFocusItem[],
): string | null {
  if (agents.length <= 1) return null;
  const working = agents.filter(
    (agent) => agent.status.kind === "working",
  ).length;
  if (working > 0) {
    return `${agents.length} agents · ${working} working`;
  }
  const attention = agents.filter(
    (agent) => agent.status.kind === "unknown" && agent.status.attention,
  ).length;
  if (attention > 0) {
    return `${agents.length} agents · ${attention} need attention`;
  }
  return `${agents.length} agents · idle`;
}

export function shouldAutoOpenAgentsSurface({
  bodyWidthPx,
  isMultiExecution,
}: {
  bodyWidthPx: number;
  isMultiExecution: boolean;
}): boolean {
  return isMultiExecution && bodyWidthPx >= 1920;
}

/** The exact `cs-target` key of a block's stream, when the record has one. */
export function blockTargetKey(
  record: CodingSessionCatalogRecord | null,
): string | null {
  return record?.commandTarget
    ? buildCodingSessionTargetKey(record.commandTarget)
    : null;
}

/**
 * The keys of blocks that are visibly streaming: the last block of each
 * execution whose active generation reports a working status.
 */
export function resolveWorkingBlockKeys(
  umbrella: CodingSessionUmbrellaRecord,
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
): ReadonlySet<string> {
  const runningExecutions = new Set(
    umbrella.executions
      .filter((execution) => execution.activeGeneration.status === "running")
      .map((execution) => execution.executionKey),
  );
  const lastBlockKeyByExecution = new Map<string, string>();
  for (const entry of entries) {
    if (
      entry.kind === "turn-block" &&
      runningExecutions.has(entry.executionKey)
    ) {
      lastBlockKeyByExecution.set(
        entry.executionKey,
        codingSessionUmbrellaEntryKey(entry),
      );
    }
  }
  return new Set(lastBlockKeyByExecution.values());
}

/**
 * Which lens the workspace below this point is rendering.
 *
 * A context and not a prop because the two components that need the answer —
 * the composer's editor and its control deck — sit three levels below the
 * workspace behind `CodingSessionUmbrellaComposer` and `CodingSessionComposer`,
 * neither of which is this lane's file. Threading a `mission` boolean through
 * them would move DOM that I8 freezes for Conversation; a context reaches the
 * two leaves that must differ and moves nothing in between.
 *
 * The default is `false`: every existing caller — Conversation, the pop-out,
 * a single-execution session — renders exactly what it rendered before.
 */
export const CodingSessionMissionLensContext = React.createContext(false);

/** Is the surface under this component the Mission lens? */
export function useCodingSessionMissionLens(): boolean {
  return React.useContext(CodingSessionMissionLensContext);
}

/**
 * The measured height of one element, in px, kept current by a ResizeObserver.
 *
 * B4's fix needs a number, not a constant: the stream reserved a literal
 * `pb-48` (192 px) under its last row while the dock's real height moved with
 * the unreachable notice, the live-activity strip and the task rail. When the
 * dock grew, the reserve did not, and the `Add provider` button landed on top
 * of a turn block. A measurement cannot drift out of register with the thing
 * it measures.
 *
 * Returns 0 until the element mounts and until a browser without
 * `ResizeObserver` fires its first resize — callers must render correctly at
 * 0, which for the reserve means "no extra padding yet", never a guess.
 */
export function useCodingSessionElementHeight<T extends HTMLElement>(): [
  React.RefObject<T | null>,
  number,
] {
  const ref = React.useRef<T>(null);
  const [heightPx, setHeightPx] = React.useState(0);

  React.useEffect(() => {
    let frameId: number | null = null;
    let cleanup: (() => void) | null = null;

    const attach = () => {
      const element = ref.current;
      if (!element) {
        frameId = window.requestAnimationFrame(attach);
        return;
      }
      const update = () => {
        setHeightPx(element.getBoundingClientRect().height);
      };
      update();
      if (typeof ResizeObserver === "undefined") {
        window.addEventListener("resize", update);
        cleanup = () => window.removeEventListener("resize", update);
        return;
      }
      const observer = new ResizeObserver(update);
      observer.observe(element);
      cleanup = () => observer.disconnect();
    };

    attach();
    return () => {
      if (frameId !== null) window.cancelAnimationFrame(frameId);
      cleanup?.();
    };
  }, []);

  return [ref, heightPx];
}

/**
 * The seat the composer is currently addressed to, as the deck names it.
 *
 * A7: the unreachable notice said "this execution" while the live strip
 * directly above it showed a *different* seat working. Both sentences were
 * true and the reader had no way to tell them apart. The name the notice
 * needs is resolved in the workspace — it is the same participant the deck's
 * `Send to …` footer reads — and the notice lives three levels down behind two
 * files this lane does not own, so it travels as context rather than as a
 * prop chain through them.
 *
 * `null` means no seat resolved, and the notice then keeps its old sentence
 * rather than inventing a name.
 */
export const CodingSessionComposerRecipientContext = React.createContext<
  string | null
>(null);

/** The seat label the composer is addressed to, or null if none resolved. */
export function useCodingSessionComposerRecipient(): string | null {
  return React.useContext(CodingSessionComposerRecipientContext);
}

/**
 * The session's open assignments and reports, for the Inspector's Team rows.
 *
 * A context and not a prop, for a reason worth stating: the holds are derived
 * from the transactions `useCodingSessionMissionSurface` itself subscribes to,
 * and that hook is what builds the Inspector element. A prop would have to be
 * computed after the hook returned and passed in on the *next* render — so
 * every time the holds changed, the Inspector would render the previous
 * answer for one frame. A stale hold is exactly the fact A5 is about. Context
 * is read at the consumer's own render and cannot lag.
 *
 * Empty by default: any surface that does not provide one renders no hold
 * lines rather than a claim it has not derived.
 */
/** Shared frozen empty — a fresh object per render defeats every memo below it. */
const EMPTY_OPEN_HOLDS: CodingSessionMissionOpenHolds = Object.freeze({
  holds: Object.freeze([]),
  omitted: 0,
});

export const CodingSessionOpenHoldsContext =
  React.createContext<CodingSessionMissionOpenHolds>(EMPTY_OPEN_HOLDS);

/** The open holds in scope, or an empty result outside a provider. */
export function useCodingSessionOpenHolds(): CodingSessionMissionOpenHolds {
  return React.useContext(CodingSessionOpenHoldsContext);
}
