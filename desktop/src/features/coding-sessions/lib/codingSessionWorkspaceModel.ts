import {
  type CodingSessionUmbrellaCreateObservation,
  groupCodingSessionCatalog,
} from "./codingSessionUmbrellaModel";
import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";

export type CodingSessionWorkspaceResolution =
  | { kind: "loading" }
  | {
      kind: "ready";
      session: CodingSessionCatalogRecord;
      /**
       * The umbrella the routed generation belongs to. An umbrella of one is
       * the pre-Step-4 shape; the workspace renders exactly today's
       * single-session tree for it.
       */
      umbrella: CodingSessionUmbrellaRecord;
      /** The member execution whose generation the route addresses. */
      focusedExecution: CodingSessionExecution;
    }
  | {
      kind: "untrusted";
      description: string;
    }
  | {
      kind: "missing";
      description: string;
    };

export function resolveCodingSessionWorkspace(input: {
  catalog: CodingSessionCatalogSnapshot;
  generationId: string;
}): CodingSessionWorkspaceResolution {
  const exact = input.catalog.entries.find(
    (entry) => entry.generationId === input.generationId,
  );
  // The catalog carries the channel's receipt-joined create observations when
  // it collected any; without them founder and operator resolve to null and
  // every surface stays observer-grade rather than gated.
  const creates = input.catalog.creates ?? [];
  if (exact) {
    // Every catalog record groups into exactly one umbrella, so the routed
    // record always resolves; the single-record fallback only guards against
    // a grouping bug ever hiding a session the flat lookup already found.
    const grouped =
      resolveUmbrellaForGeneration(
        input.catalog.entries,
        input.generationId,
        creates,
      ) ?? resolveUmbrellaForGeneration([exact], input.generationId, creates);
    if (grouped) {
      return {
        kind: "ready",
        session: exact,
        umbrella: grouped.umbrella,
        focusedExecution: grouped.execution,
      };
    }
  }
  if (input.catalog.isLoading) {
    return { kind: "loading" };
  }
  if (input.catalog.authorityErrorMessage) {
    return {
      kind: "untrusted",
      description: input.catalog.authorityErrorMessage,
    };
  }
  return {
    kind: "missing",
    description:
      input.catalog.errorMessage ??
      "This exact coding-session generation is not available in the relay catalog.",
  };
}

/**
 * Locate the umbrella and member execution containing a routed generation.
 *
 * Prior generations count as membership too: routing to a collapsed earlier
 * generation still resolves the same umbrella, so history deep links keep the
 * surrounding session context.
 */
export function resolveUmbrellaForGeneration(
  entries: readonly CodingSessionCatalogRecord[],
  generationId: string,
  creates: readonly CodingSessionUmbrellaCreateObservation[] = [],
): {
  umbrella: CodingSessionUmbrellaRecord;
  execution: CodingSessionExecution;
} | null {
  for (const umbrella of groupCodingSessionCatalog(entries, creates)) {
    for (const execution of umbrella.executions) {
      const owns =
        execution.activeGeneration.generationId === generationId ||
        execution.priorGenerations.some(
          (record) => record.generationId === generationId,
        );
      if (owns) return { umbrella, execution };
    }
  }
  return null;
}

const WORKING_STATUSES = new Set([
  "busy",
  "running",
  "streaming",
  "thinking",
  "working",
]);
const IDLE_STATUSES = new Set([
  "cancelled",
  "completed",
  "done",
  "failed",
  "idle",
  "interrupted",
  "stopped",
]);

export function deriveCodingSessionWorkspaceStatus(
  transcript: CodingSessionCatalogRecord["transcript"],
  lifecycleStatus?: CodingSessionCatalogRecord["status"],
): CodingSessionWorkspaceStatus {
  if (lifecycleStatus === "stopped") {
    return { kind: "ended", label: "Ended" };
  }
  for (let index = transcript.length - 1; index >= 0; index -= 1) {
    const item = transcript[index];
    if (item.type !== "lifecycle") continue;

    if (item.title === "Turn result" || item.title === "Interrupted") {
      return { kind: "idle", label: "Idle" };
    }
    if (item.title !== "Status") continue;

    const normalized = item.text.trim().toLowerCase();
    if (WORKING_STATUSES.has(normalized)) {
      return { kind: "working", label: "Working" };
    }
    if (IDLE_STATUSES.has(normalized)) {
      return { kind: "idle", label: "Idle" };
    }
  }
  return { kind: "unknown", label: "Status unknown" };
}
