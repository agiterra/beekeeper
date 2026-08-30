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
