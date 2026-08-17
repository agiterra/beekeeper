import type { CodingSessionCommandTarget } from "@/features/coding-sessions/lib/codingSessionCommand";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import type {
  CodingSessionCatalogRecord,
  CodingSessionWorkspaceStatus,
  GlobalCodingSessionCatalogSnapshot,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";

import { normalizeProjectRef } from "./projectContainerModel";

/** Exact coordinates for durably stopping one provider execution. */
export type ProjectCodingSessionStopTarget = {
  target: CodingSessionCommandTarget;
  providerAuthorityPubkey: string;
};

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
  /** Distinct provider/runtime labels participating in this durable session. */
  runtimeLabels: string[];
  /** Number of provider executions represented by this one session row. */
  executionCount: number;
  status: CodingSessionWorkspaceStatus;
  /** Stop coordinates for every non-ended execution this row stands for —
   * what "End session" publishes, one durable stop per execution. */
  stopTargets: ProjectCodingSessionStopTarget[];
  session: CodingSessionCatalogRecord;
  /** True only on optimistic rows synthesized for a published-but-not-yet-
   * acknowledged create (see codingSessionPendingLifecycle). */
  pending?: boolean;
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

  const executionEntries = catalog.entries.map(({ channelId, session }) => {
    const placement = resolveProjectCodingSessionPlacement(
      session.projectRef,
      channelId,
      index,
    );
    const runtimeLabel = buildRuntimeLabel(session);
    // Lifecycle metadata is required here: a durable stop can otherwise
    // look like an ordinary idle transcript forever.
    const status = deriveCodingSessionWorkspaceStatus(
      session.transcript,
      session.status,
      session.statusAt,
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
      runtimeLabel,
      runtimeLabels: runtimeLabel ? [runtimeLabel] : [],
      executionCount: 1,
      status,
      stopTargets:
        status.kind !== "ended" &&
        session.commandTarget &&
        session.providerAuthorityPubkey
          ? [
              {
                target: session.commandTarget,
                providerAuthorityPubkey: session.providerAuthorityPubkey,
              },
            ]
          : [],
      session,
    };
  });
  const entries = groupProjectCodingSessionEntries(executionEntries).sort(
    compareProjectCodingSessionEntries,
  );

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

/**
 * One shelf row per umbrella session, not per provider execution.
 *
 * A single create can leave several executions behind (the provider opens an
 * ACP session, abandons it during the handshake, and opens the real one);
 * they share a `sessionRef` and the workspace already presents them as one
 * session, so listing each execution shows N identical-looking rows that all
 * open the same place. Records without a sessionRef predate umbrellas and
 * stay one row each.
 */
export function collapseProjectCodingSessionUmbrellas(
  entries: readonly ProjectCodingSessionShelfEntry[],
): ProjectCodingSessionShelfEntry[] {
  const byUmbrella = new Map<string, ProjectCodingSessionShelfEntry>();
  for (const entry of entries) {
    const key = `${entry.channelId}|${
      entry.session.sessionRef ?? `implicit:${entry.generationId}`
    }`;
    const current = byUmbrella.get(key);
    if (!current || representsUmbrellaBetter(entry, current)) {
      byUmbrella.set(key, entry);
    }
  }
  return [...byUmbrella.values()];
}

/**
 * The execution a collapsed row stands for: one with an actual transcript
 * beats a metadata-only phantom, an active one beats a finished one, and
 * newer activity breaks ties.
 */
function representsUmbrellaBetter(
  candidate: ProjectCodingSessionShelfEntry,
  current: ProjectCodingSessionShelfEntry,
): boolean {
  const candidateHasTranscript = candidate.session.transcript.length > 0;
  const currentHasTranscript = current.session.transcript.length > 0;
  if (candidateHasTranscript !== currentHasTranscript) {
    return candidateHasTranscript;
  }
  const byStatus =
    umbrellaStatusPriority(candidate.status) -
    umbrellaStatusPriority(current.status);
  if (byStatus !== 0) return byStatus < 0;
  return (
    candidate.session.lastEventAt.localeCompare(current.session.lastEventAt) > 0
  );
}

/** Unlike display ordering, a known-finished state beats "unknown" here: a
 * transcriptless record is a phantom, not the session's face. */
function umbrellaStatusPriority(status: CodingSessionWorkspaceStatus): number {
  switch (status.kind) {
    case "working":
      return 0;
    case "idle":
      return 1;
    case "ended":
      return 2;
    case "unknown":
      return 3;
  }
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
  return session.title.trim().length > 0
    ? session.title.trim()
    : "Coding session";
}

/**
 * The label an umbrella row shows for its whole group.
 *
 * The representative is picked for coordinates (transcript-bearing wins,
 * see `representsUmbrellaBetter`), but a titled metadata-only member must
 * not lose its name to an untitled representative — the person named the
 * session, and the name is how they find it. When the representative has
 * no title of its own, borrow the label of the newest titled member.
 */
function resolveUmbrellaLabel(
  representative: ProjectCodingSessionShelfEntry,
  group: ProjectCodingSessionShelfEntry[],
): string {
  if (representative.session.title.trim().length > 0) {
    return representative.label;
  }
  const titled = group
    .filter((entry) => entry.session.title.trim().length > 0)
    .sort((left, right) => {
      const byTime = right.session.lastEventAt.localeCompare(
        left.session.lastEventAt,
      );
      return byTime !== 0
        ? byTime
        : left.generationId.localeCompare(right.generationId);
    });
  return titled[0]?.label ?? representative.label;
}

/**
 * Collapse provider executions sharing one signed umbrella reference into one
 * project-navigation row. The newest execution supplies the compatibility
 * route; opening it resolves the complete umbrella in the session workspace.
 */
function groupProjectCodingSessionEntries(
  entries: ProjectCodingSessionShelfEntry[],
): ProjectCodingSessionShelfEntry[] {
  const groups = new Map<string, ProjectCodingSessionShelfEntry[]>();
  for (const entry of entries) {
    const key = entry.session.sessionRef
      ? `umbrella:${entry.channelId}:${entry.session.sessionRef}`
      : `execution:${entry.channelId}:${entry.generationId}`;
    const group = groups.get(key);
    if (group) group.push(entry);
    else groups.set(key, [entry]);
  }
  return [...groups.values()].map((group) => {
    const representative = group.reduce((current, candidate) =>
      representsUmbrellaBetter(candidate, current) ? candidate : current,
    );
    const runtimeLabels = [
      ...new Set(group.flatMap((entry) => entry.runtimeLabels)),
    ];
    // Live work always shows; otherwise an ended execution that is the
    // group's most recent activity settles the whole row — the stop is the
    // user's last word, and a stale idle prior generation must not
    // resurrect a session they ended. Only then does the ordinary priority
    // collapse (idle beats ended beats unknown) apply.
    const working = group.find((entry) => entry.status.kind === "working");
    const newest = [...group].sort(
      (left, right) =>
        right.session.lastEventAt.localeCompare(left.session.lastEventAt) ||
        left.generationId.localeCompare(right.generationId),
    )[0];
    const status =
      working?.status ??
      (newest?.status.kind === "ended"
        ? newest.status
        : ([...group].sort(
            (left, right) =>
              umbrellaStatusPriority(left.status) -
              umbrellaStatusPriority(right.status),
          )[0]?.status ?? representative.status));
    const stopTargets = [
      ...new Map(
        group
          .filter((entry) => entry.status.kind !== "ended")
          .flatMap((entry) => entry.stopTargets)
          .map((stop): [string, ProjectCodingSessionStopTarget] => [
            `${buildCodingSessionTargetKey(stop.target)}\u0000${stop.providerAuthorityPubkey}`,
            stop,
          ]),
      ).values(),
    ];
    return {
      ...representative,
      label: resolveUmbrellaLabel(representative, group),
      runtimeLabel: runtimeLabels.join(" + ") || null,
      runtimeLabels,
      executionCount: group.length,
      status,
      stopTargets,
    };
  });
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
