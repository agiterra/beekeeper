import type {
  CodingSessionCatalogRecord,
  CodingSessionWorkspaceStatus,
  GlobalCodingSessionCatalogSnapshot,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";

import { normalizeProjectRef } from "./projectContainerModel";

export type ProjectCodingSessionShelfEntry = {
  /** `project` once a project owns this session; `unassigned` displays under General. */
  placement: "project" | "unassigned";
  /** Owning project id, or null when nothing claims the session. */
  projectId: string | null;
  /** How the owner was decided — the audit trail behind `projectId`. */
  placedBy: "project-ref" | "channel" | null;
  channelId: string;
  generationId: string;
  label: string;
  sourceChannelLabel: string | null;
  runtimeLabel: string | null;
  status: CodingSessionWorkspaceStatus;
  session: CodingSessionCatalogRecord;
};

export type ProjectCodingSessionShelfState =
  | {
      kind: "loading";
      message: string;
      detail: null;
    }
  | {
      kind: "partial" | "unavailable";
      message: string;
      detail: string;
    }
  | {
      kind: "ready";
      message: string | null;
      detail: null;
    };

export type ProjectCodingSessionShelfModel = {
  entries: ProjectCodingSessionShelfEntry[];
  state: ProjectCodingSessionShelfState;
};

/**
 * Where a session is displayed, resolved in one place.
 *
 * `projectIdByRef` maps a *known* project's canonical address (`30621:owner:
 * dtag`) to its id, so a signed `projectRef` naming a project this client
 * cannot see never places a session anywhere; `projectIdByChannel` is the
 * transitive fallback every other channel-scoped child already uses (a
 * workflow's project is its trigger channel's project — a session's is its
 * `h`-scoped channel's).
 */
export type ProjectCodingSessionPlacementIndex = {
  projectIdByRef: ReadonlyMap<string, string>;
  projectIdByChannel: ReadonlyMap<string, string>;
};

/**
 * Resolve globally discoverable sessions for the Projects surfaces.
 *
 * Placement authority, in order: the session's own signed `projectRef` from
 * its 44223 metadata (or 44221 create) when it names a project this client
 * knows, then the project that owns the session's channel, then nothing —
 * an unplaced session is displayed under General rather than hidden. Labels,
 * channel names, runtime, and transcript content never influence placement.
 */
export function resolveProjectCodingSessionShelf(
  catalog: GlobalCodingSessionCatalogSnapshot,
  index: ProjectCodingSessionPlacementIndex = {
    projectIdByRef: new Map(),
    projectIdByChannel: new Map(),
  },
  sourceChannelLabels: ReadonlyMap<string, string> = new Map(),
): ProjectCodingSessionShelfModel {
  if (catalog.authorityErrorMessage) {
    return {
      entries: [],
      state: {
        kind: "unavailable",
        message: "Sessions unavailable",
        detail: catalog.authorityErrorMessage,
      },
    };
  }

  const entries = catalog.entries
    .map(({ channelId, session }) => {
      const placement = resolveProjectCodingSessionPlacement(
        session.projectRef,
        channelId,
        index,
      );
      return {
        placement: placement.projectId
          ? ("project" as const)
          : ("unassigned" as const),
        projectId: placement.projectId,
        placedBy: placement.placedBy,
        channelId,
        generationId: session.generationId,
        label: buildProjectCodingSessionLabel(session),
        // Presentation provenance only. It is intentionally not passed to any
        // project-placement decision or exact session action.
        sourceChannelLabel: sourceChannelLabels.get(channelId)?.trim() || null,
        runtimeLabel: buildRuntimeLabel(session),
        status: deriveCodingSessionWorkspaceStatus(session.transcript),
        session,
      };
    })
    .sort(compareProjectCodingSessionEntries);

  if (catalog.errorMessage) {
    return {
      entries,
      state: {
        kind: entries.length > 0 ? "partial" : "unavailable",
        message:
          entries.length > 0
            ? "Session history may be incomplete"
            : "Sessions unavailable",
        detail: catalog.errorMessage,
      },
    };
  }
  if (catalog.isLoading) {
    return {
      entries,
      state: {
        kind: "loading",
        message:
          entries.length > 0 ? "Refreshing sessions…" : "Loading sessions…",
        detail: null,
      },
    };
  }
  return {
    entries,
    state: {
      kind: "ready",
      message: entries.length === 0 ? "No trusted sessions yet" : null,
      detail: null,
    },
  };
}

/**
 * The signed `projectRef` beats the channel fallback whenever it names a
 * project this client knows: a session that says which project it belongs to
 * is stating placement, while the channel is only inferring it.
 */
export function resolveProjectCodingSessionPlacement(
  projectRef: string | null,
  channelId: string,
  index: ProjectCodingSessionPlacementIndex,
): { projectId: string | null; placedBy: "project-ref" | "channel" | null } {
  // A `kind:owner:dtag` back-reference and the project's own address name the
  // same project; canonicalize before lookup so only one of them resolving is
  // never mistaken for the project being unknown.
  const canonical = projectRef ? normalizeProjectRef(projectRef) : null;
  const signed = canonical ? index.projectIdByRef.get(canonical) : undefined;
  if (signed !== undefined) {
    return { projectId: signed, placedBy: "project-ref" };
  }
  const owning = index.projectIdByChannel.get(channelId);
  return owning !== undefined
    ? { projectId: owning, placedBy: "channel" }
    : { projectId: null, placedBy: null };
}

/** Split resolved entries into the per-project buckets the sidebar renders. */
export function bucketProjectCodingSessions(
  entries: readonly ProjectCodingSessionShelfEntry[],
): {
  byProject: Map<string, ProjectCodingSessionShelfEntry[]>;
  unclaimed: ProjectCodingSessionShelfEntry[];
} {
  const byProject = new Map<string, ProjectCodingSessionShelfEntry[]>();
  const unclaimed: ProjectCodingSessionShelfEntry[] = [];
  for (const entry of entries) {
    if (!entry.projectId) {
      unclaimed.push(entry);
      continue;
    }
    const bucket = byProject.get(entry.projectId);
    if (bucket) {
      bucket.push(entry);
    } else {
      byProject.set(entry.projectId, [entry]);
    }
  }
  return { byProject, unclaimed };
}

/** Working first, then unknown, then idle; newest activity wins inside a tier. */
export function compareProjectCodingSessionEntries(
  left: ProjectCodingSessionShelfEntry,
  right: ProjectCodingSessionShelfEntry,
): number {
  const byActivity = statusPriority(left.status) - statusPriority(right.status);
  if (byActivity !== 0) return byActivity;
  const byTime = right.session.lastEventAt.localeCompare(
    left.session.lastEventAt,
  );
  if (byTime !== 0) return byTime;
  const byChannel = left.channelId.localeCompare(right.channelId);
  return byChannel !== 0
    ? byChannel
    : left.generationId.localeCompare(right.generationId);
}

function buildProjectCodingSessionLabel(
  session: CodingSessionCatalogRecord,
): string {
  const title =
    session.title.trim().length > 0 ? session.title.trim() : "Coding session";
  const generation = session.commandTarget?.generation;
  return generation && Number.isSafeInteger(generation)
    ? `${title} · generation ${generation}`
    : title;
}

function buildRuntimeLabel(session: CodingSessionCatalogRecord): string | null {
  const runtime =
    session.runtime ?? session.provider ?? session.commandTarget?.driver;
  return runtime ? formatCodingSessionRuntimeLabel(runtime) : null;
}

function statusPriority(status: CodingSessionWorkspaceStatus): number {
  switch (status.kind) {
    case "working":
      return 0;
    case "unknown":
      return 1;
    case "idle":
      return 2;
    case "ended":
      return 3;
  }
}

export type ExactProjectCodingSessionCoordinates = {
  channelId: string;
  generationId: string;
};

/** Resolve the exact active route without interpreting either opaque id. */
export function parseActiveProjectCodingSessionPath(
  pathname: string,
): ExactProjectCodingSessionCoordinates | null {
  const match = /^\/coding-sessions\/([^/]+)\/([^/]+)\/?$/.exec(pathname);
  if (!match) return null;
  try {
    const channelId = decodeURIComponent(match[1]);
    const generationId = decodeURIComponent(match[2]);
    return channelId.length > 0 && generationId.length > 0
      ? { channelId, generationId }
      : null;
  } catch {
    return null;
  }
}
