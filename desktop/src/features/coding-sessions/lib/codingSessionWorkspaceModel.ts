import type {
  CodingSessionCatalogRecord,
  CodingSessionCatalogSnapshot,
  CodingSessionWorkspaceStatus,
} from "./codingSessionTypes";

export type CodingSessionWorkspaceResolution =
  | { kind: "loading" }
  | { kind: "ready"; session: CodingSessionCatalogRecord }
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
  if (exact) {
    return { kind: "ready", session: exact };
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
): CodingSessionWorkspaceStatus {
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
