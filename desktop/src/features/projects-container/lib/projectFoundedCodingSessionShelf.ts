/**
 * Founded-but-unstarted umbrellas as project shelf rows.
 *
 * A team session is founded first (genesis, goal, name) and started later,
 * inside the session. Between those moments the umbrella has no execution,
 * so the metadata-driven shelf cannot see it; this module projects the
 * founded facts into rows of the same shape, marked `founded`, so the
 * sidebar and the project page list them under their project with an honest
 * "Not started" state.
 *
 * Founded rows never enter `applyPendingCodingSessionLifecycle`: it
 * acknowledges a pending create by its `sessionRef` echo, and a founded row
 * carrying the same ref would eat the "Starting" row it is about to be
 * replaced by. The merge below runs *after* the pending overlay instead, and
 * hides a founded row while a live pending create claims its ref.
 */
import {
  type CodingSessionClosure,
  codingSessionClosureIsClosed,
  codingSessionClosureKey,
} from "@/features/coding-sessions/lib/codingSessionClosure";
import {
  type CodingSessionFoundedUmbrella,
  resolveFoundedCodingSessions,
} from "@/features/coding-sessions/lib/codingSessionFoundedModel";
import { codingSessionNameKey } from "@/features/coding-sessions/lib/codingSessionName";
import {
  PENDING_CODING_SESSION_LIFECYCLE_TTL_MS,
  type PendingCodingSessionLifecycle,
} from "@/features/coding-sessions/lib/codingSessionPendingLifecycle";
import {
  foundedCodingSessionRowId,
  parseFoundedCodingSessionRowId,
} from "@/features/coding-sessions/lib/codingSessionRoute";
import type {
  CodingSessionCatalogRecord,
  GlobalCodingSessionCatalogSnapshot,
} from "@/features/coding-sessions/lib/codingSessionTypes";

import {
  type ProjectCodingSessionName,
  type ProjectCodingSessionPlacementIndex,
  type ProjectCodingSessionShelfEntry,
  projectCodingSessionLabelOrigin,
  resolveProjectCodingSessionPlacement,
} from "./projectCodingSessionShelf";

/** The label a founded umbrella nobody named yet shows. */
export const UNTITLED_FOUNDED_CODING_SESSION_LABEL = "Untitled session";

/**
 * Every founded umbrella across the global catalog's channels, newest first
 * within a channel. Per channel because the projection subtracts creates and
 * entries of *that* channel; a snapshot that collected no geneses yields none.
 */
export function resolveGlobalFoundedCodingSessions(
  catalog: Pick<
    GlobalCodingSessionCatalogSnapshot,
    "entries" | "creates" | "geneses" | "authorityErrorMessage"
  >,
): CodingSessionFoundedUmbrella[] {
  if (catalog.authorityErrorMessage) return [];
  const geneses = catalog.geneses ?? [];
  const founded: CodingSessionFoundedUmbrella[] = [];
  for (const channelId of new Set(
    geneses.map((genesis) => genesis.channelId),
  )) {
    founded.push(
      ...resolveFoundedCodingSessions({
        channelId,
        geneses,
        entries: catalog.entries
          .filter((entry) => entry.channelId === channelId)
          .map((entry) => entry.session),
        creates: catalog.creates ?? [],
      }),
    );
  }
  return founded;
}

/**
 * Shelf rows for founded umbrellas.
 *
 * Placement is by channel only — a genesis carries no `projectRef`, so the
 * channel's project owns the row exactly as it owns a workflow. The label is
 * the founder-keyed 44229 name, the same key the started shelf resolves;
 * closure facts are keyed `(channel, ref, genesisRef)` like every other row,
 * so a founded session closed with a 44230 files under Settled.
 */
export function resolveFoundedProjectCodingSessionEntries(input: {
  founded: readonly CodingSessionFoundedUmbrella[];
  index: ProjectCodingSessionPlacementIndex;
  channelLabels?: ReadonlyMap<string, string>;
  names?: ReadonlyMap<string, ProjectCodingSessionName>;
  closures?: ReadonlyMap<string, CodingSessionClosure>;
}): ProjectCodingSessionShelfEntry[] {
  const channelLabels = input.channelLabels ?? new Map<string, string>();
  const names = input.names ?? new Map<string, ProjectCodingSessionName>();
  const closures = input.closures ?? new Map<string, CodingSessionClosure>();
  return input.founded.map((founded) => {
    const placement = resolveProjectCodingSessionPlacement(
      null,
      founded.channelId,
      input.index,
    );
    const wireName = names.get(
      codingSessionNameKey(
        founded.channelId,
        founded.sessionRef,
        founded.founderPubkey,
      ),
    );
    const name = wireName?.content.trim();
    const label = name || UNTITLED_FOUNDED_CODING_SESSION_LABEL;
    // Only a wire name has an origin; "Untitled session" is no one's words.
    const labelOrigin = name ? projectCodingSessionLabelOrigin(wireName) : null;
    const closure =
      closures.get(
        codingSessionClosureKey(
          founded.channelId,
          founded.sessionRef,
          founded.genesisRef,
        ),
      ) ?? null;
    const session = synthesizeFoundedSession(founded, label);
    return {
      placement: placement.projectId ? "project" : "unassigned",
      projectId: placement.projectId,
      placedBy: placement.placedBy,
      channelId: founded.channelId,
      generationId: session.generationId,
      label,
      labelOrigin,
      sourceChannelLabel: channelLabels.get(founded.channelId)?.trim() || null,
      runtimeLabel: null,
      runtimeLabels: [],
      executionCount: 0,
      closure,
      isClosed: codingSessionClosureIsClosed(closure?.action),
      isArchived: closure?.action === "archived",
      sessionRef: founded.sessionRef,
      genesisRef: founded.genesisRef,
      founderPubkey: founded.founderPubkey,
      status: { kind: "founded", label: "Not started" },
      stopTargets: [],
      session,
      founded: true,
    };
  });
}

/**
 * A catalog record with nothing in it, so the row fits the shelf's shape.
 * Every provider-reported field is null: no provider has reported, and a
 * value here would be a claim nobody signed. `lastEventAt` is the founding
 * instant, which is the only activity the umbrella has.
 */
function synthesizeFoundedSession(
  founded: CodingSessionFoundedUmbrella,
  label: string,
): CodingSessionCatalogRecord {
  return {
    generationId: foundedCodingSessionRowId(founded.sessionRef),
    label,
    title: "",
    providerAuthorityPubkey: null,
    metadataAuthorityPubkey: null,
    lastEventAt: new Date(founded.foundedAt * 1000).toISOString(),
    status: "unknown",
    statusAt: null,
    statusEventId: null,
    transcript: [],
    conflictCount: 0,
    commandTarget: null,
    projectRef: null,
    repoRef: null,
    sessionRef: founded.sessionRef,
    provider: null,
    runtime: null,
    model: null,
    agentRef: null,
    role: null,
    turnBudget: null,
    routing: null,
    capabilities: null,
    beeStamp: null,
    packRef: null,
    composeRef: null,
  };
}

/**
 * Add founded rows to the overlaid shelf.
 *
 * A founded row is dropped while a live (within TTL) pending create names its
 * channel and `sessionRef`: the person pressed Start, the "Starting" row is
 * the honest face of that, and both at once would be two rows for one
 * session. An expired pending record hides nothing — the provider never
 * answered, and the founded row is the truth again. A founded row is also
 * dropped if a real row already carries its ref (the projection's own check,
 * repeated here so the merge is safe on any input).
 */
export function mergeFoundedCodingSessionShelfEntries<
  Entry extends Pick<
    ProjectCodingSessionShelfEntry,
    "channelId" | "sessionRef" | "founded"
  >,
>(
  entries: readonly Entry[],
  foundedEntries: readonly Entry[],
  pending: readonly PendingCodingSessionLifecycle[],
  now: number,
): Entry[] {
  const claimed = new Set<string>();
  for (const record of pending) {
    if (record.kind !== "create" || record.sessionRef === null) continue;
    if (now - record.recordedAt > PENDING_CODING_SESSION_LIFECYCLE_TTL_MS) {
      continue;
    }
    claimed.add(`${record.channelId}\u0000${record.sessionRef}`);
  }
  for (const entry of entries) {
    if (entry.sessionRef && entry.founded !== true) {
      claimed.add(`${entry.channelId}\u0000${entry.sessionRef}`);
    }
  }
  const admitted = foundedEntries.filter(
    (entry) =>
      entry.sessionRef !== null &&
      !claimed.has(`${entry.channelId}\u0000${entry.sessionRef}`),
  );
  return [...entries, ...admitted];
}

/** Where opening a shelf row goes: a generation, or a founded umbrella. */
export type ProjectCodingSessionOpenTarget =
  | { kind: "generation"; channelId: string; generationId: string }
  | { kind: "founded"; channelId: string; sessionRef: string };

/**
 * Resolve a row's open coordinates to the route they name.
 *
 * A founded row sits in the generation slot under the founded row id, and
 * every path that opens a row — the click, the hotkey — hands the same
 * `{channelId, generationId}` up. Feeding that id to the generation route
 * would open a session that does not exist; this is the one place the two
 * are told apart.
 */
export function resolveProjectCodingSessionOpenTarget(coordinates: {
  channelId: string;
  generationId: string;
}): ProjectCodingSessionOpenTarget {
  const sessionRef = parseFoundedCodingSessionRowId(coordinates.generationId);
  return sessionRef !== null && sessionRef.length > 0
    ? { kind: "founded", channelId: coordinates.channelId, sessionRef }
    : {
        kind: "generation",
        channelId: coordinates.channelId,
        generationId: coordinates.generationId,
      };
}
