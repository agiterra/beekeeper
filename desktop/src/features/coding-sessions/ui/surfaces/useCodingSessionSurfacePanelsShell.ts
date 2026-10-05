import * as React from "react";
import { useQuery } from "@tanstack/react-query";

import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionMissionDecisionInput } from "@/features/coding-sessions/lib/codingSessionMissionDecisions";
import type { CodingSessionMissionOpenHolds } from "@/features/coding-sessions/lib/codingSessionMissionOpenHolds";
import type { CodingSessionSubagentPanel } from "@/features/coding-sessions/lib/codingSessionSubagents";
import type { CodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import type {
  CodingSessionObservedChanges,
  CodingSessionTranscriptModel,
} from "@/features/coding-sessions/lib/codingSessionTranscriptModelTypes";
import type {
  CodingSessionCatalogRecord,
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { CodingSessionUmbrellaTimelineEntry } from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import {
  type CodingSessionTreeQuery,
  type CodingSessionTreeResolution,
  resolveCodingSessionTree,
} from "@/shared/api/tauriCodingSessionTree";
import {
  type CodingSessionProviderStatus,
  getCodingSessionProviderStatus,
} from "@/shared/api/tauriSessionProvider";
import { useCodingSessionProject } from "@/features/projects-container/hooks";

import { codingSessionSurfaceRegistry } from "./codingSessionBuiltinSurfaces";
import type {
  CodingSessionSurfaceBaseCtx,
  CodingSessionSurfaceCtx,
  CodingSessionSurfaceLens,
  CodingSessionSurfaceMissionContent,
  CodingSessionSurfaceObservations,
  CodingSessionSurfaceTree,
} from "./codingSessionSurfaceContext";
import {
  type CodingSessionResolvedSurface,
  resolveCodingSessionSurfaces,
} from "./codingSessionSurfaceRegistry";
import {
  type CodingSessionHeaderPanels,
  type CodingSessionSurfacePanelActions,
  type CodingSessionSurfacePanelState,
  codingSessionSurfacePanelsStorageKey,
  useCodingSessionSurfacePanels,
  useCodingSessionSurfaceShortcuts,
} from "./useCodingSessionSurfacePanels";

/**
 * Everything a workspace needs to host surfaces, in one call: the panel
 * state, the `ctx` every surface reads, the lens's resolved surfaces, the
 * ⌘J / ⌘⌥B shortcuts, and what the header's panel toggles need.
 *
 * Moved out of both workspaces so neither grows (the 1000-line ceiling), and
 * so the two layouts build `ctx` the same way.
 */

const MISSION_SURFACE_IDS = [
  "mission-inspector",
  "mission-context",
  "mission-audit",
] as const;

/**
 * The provider authority that runs `record`, as far as anything says: the
 * trusted transcript authority, else the authority whose metadata enriched
 * the record, else the execution's fact-stream signer. `null` when none is
 * known yet — a session founded here whose provider has published nothing.
 *
 * `providerAuthorityPubkey` alone is null until a generation has a trusted
 * transcript (`codingSessionTypes.ts`), so reading only it would call a
 * brand-new session on this machine "on another computer".
 */
export function codingSessionProviderAuthority(
  record: Pick<
    CodingSessionCatalogRecord,
    "providerAuthorityPubkey" | "metadataAuthorityPubkey"
  > | null,
  execution: Pick<CodingSessionExecution, "signerPubkey"> | null,
): string | null {
  const candidates = [
    record?.providerAuthorityPubkey,
    record?.metadataAuthorityPubkey,
    execution?.signerPubkey,
  ];
  for (const candidate of candidates) {
    const key = candidate?.trim().toLowerCase() ?? "";
    if (key.length > 0) return key;
  }
  return null;
}

/**
 * Whether this machine's provider runs `providerAuthorityPubkey`, from the
 * provider-status read: `true`, `false`, or `null` while that is unknown.
 *
 * Unknown is three cases: the status has not loaded yet (`pending`), it could
 * not be read (`statusUnread`: a host that is restarting, a build with no
 * host to ask), or the session has not said which provider runs it
 * (`authorityUnknown`). None of them is "another computer". A status that
 * was read and names no provider key means this machine has no provider for
 * this community, so it runs nothing here: `false`.
 */
export function codingSessionLocalProviderState(
  status: {
    data: Pick<CodingSessionProviderStatus, "providerPubkey"> | undefined;
    isError: boolean;
  },
  providerAuthorityPubkey: string | null,
): {
  isLocalProvider: boolean | null;
  statusUnread: boolean;
  authorityUnknown: boolean;
} {
  const authority = providerAuthorityPubkey?.trim().toLowerCase() ?? "";
  if (authority.length === 0) {
    return {
      isLocalProvider: null,
      statusUnread: status.isError,
      authorityUnknown: true,
    };
  }
  if (status.data === undefined) {
    return {
      isLocalProvider: null,
      statusUnread: status.isError,
      authorityUnknown: false,
    };
  }
  const local = status.data.providerPubkey?.trim().toLowerCase() ?? "";
  return {
    isLocalProvider: local.length > 0 && authority === local,
    statusUnread: false,
    authorityUnknown: false,
  };
}

function useIsLocalProvider(providerAuthorityPubkey: string | null) {
  const status = useQuery({
    queryKey: ["coding-session-provider-status"],
    queryFn: getCodingSessionProviderStatus,
    retry: false,
    staleTime: 60_000,
  });
  return codingSessionLocalProviderState(
    { data: status.data, isError: status.isError },
    providerAuthorityPubkey,
  );
}

const TREE_LOCALITY_UNREAD =
  "This computer could not say whether it runs this session, so it cannot say where the working tree is.";
const TREE_AUTHORITY_UNKNOWN =
  "This session has not said which provider runs it yet.";

/**
 * The tree `ctx` carries, from the host's resolution and what is known about
 * this machine's provider.
 *
 * While the provider status is loading, the tree is `loading`. When the
 * status could not be read, or the session has not said which provider runs
 * it, the host is still asked with `isLocalProvider: false` — a worktree this
 * host cut for the session is this host's either way — and a `notLocal`
 * answer is reported as the unknown it is, never as "on another computer".
 */
export function codingSessionSurfaceTreeState(input: {
  query: CodingSessionTreeQuery;
  isLocalProvider: boolean | null;
  statusUnread: boolean;
  /** No authority is known for the session yet (see `codingSessionProviderAuthority`). */
  authorityUnknown?: boolean;
  resolution: CodingSessionTreeResolution | undefined;
  resolutionFailed: boolean;
}): CodingSessionSurfaceTree {
  const { query } = input;
  const unavailable = (
    state: "loading" | "error",
    reason: string | null,
  ): CodingSessionSurfaceTree => ({
    state,
    available: false,
    source: null,
    label: "no working tree",
    reason,
    refusal: null,
    query,
  });
  const authorityUnknown = input.authorityUnknown === true;
  if (
    input.isLocalProvider === null &&
    !input.statusUnread &&
    !authorityUnknown
  ) {
    return unavailable("loading", null);
  }
  const resolution = input.resolution;
  if (resolution) {
    if (input.isLocalProvider === null && resolution.refusal === "notLocal") {
      return unavailable(
        "error",
        authorityUnknown ? TREE_AUTHORITY_UNKNOWN : TREE_LOCALITY_UNREAD,
      );
    }
    return {
      state: "resolved",
      available: resolution.available,
      source: resolution.source,
      label: resolution.label,
      reason: resolution.reason,
      refusal: resolution.refusal,
      query,
    };
  }
  if (input.resolutionFailed) {
    return unavailable(
      "error",
      "This computer could not say where the session's working tree is.",
    );
  }
  return unavailable("loading", null);
}

function useCodingSessionSurfaceTree(input: {
  query: CodingSessionTreeQuery;
  isLocalProvider: boolean | null;
  statusUnread: boolean;
  authorityUnknown: boolean;
}): CodingSessionSurfaceTree {
  const { authorityUnknown, isLocalProvider, query, statusUnread } = input;
  const resolution = useQuery({
    queryKey: ["coding-session-tree", query],
    queryFn: () => resolveCodingSessionTree(query),
    enabled: isLocalProvider !== null || statusUnread || authorityUnknown,
    retry: false,
    staleTime: 30_000,
  });
  return React.useMemo(
    () =>
      codingSessionSurfaceTreeState({
        query,
        isLocalProvider,
        statusUnread,
        authorityUnknown,
        resolution: resolution.data,
        resolutionFailed: resolution.isError,
      }),
    [
      authorityUnknown,
      isLocalProvider,
      query,
      resolution.data,
      resolution.isError,
      statusUnread,
    ],
  );
}

/**
 * The extensions object, kept by identity while every surface's extension
 * value is identical, so `ctx` changes only when its content does.
 */
function useStableExtensions(
  next: Record<string, unknown>,
): Readonly<Record<string, unknown>> {
  const ref = React.useRef<Record<string, unknown>>(next);
  const previous = ref.current;
  const keys = Object.keys(next);
  const same =
    keys.length === Object.keys(previous).length &&
    keys.every((key) => Object.is(previous[key], next[key]));
  if (!same) ref.current = next;
  return same ? previous : next;
}

export type CodingSessionSurfaceShell = {
  ctx: CodingSessionSurfaceCtx;
  panels: {
    state: CodingSessionSurfacePanelState;
    actions: CodingSessionSurfacePanelActions;
  };
  /** Every surface the lens lists, launcher order, availability resolved. */
  surfaces: CodingSessionResolvedSurface[];
  /** The drawer-placement subset. */
  drawerSurfaces: CodingSessionResolvedSurface[];
  /** Opens Agents from a transcript row; undefined where Agents cannot open. */
  openAgentsSurface: (() => void) | undefined;
  /** Opens Mission's three surfaces with Inspector active (the person's choice). */
  openMissionSurfaces: () => void;
  /** The same, on the view's own initiative: refused after a stored choice. */
  openMissionSurfacesProactively: () => void;
  /** Closes Mission's three surfaces (leaving the lens). */
  closeMissionSurfaces: () => void;
  headerPanels: CodingSessionHeaderPanels;
  minimapSlotRef: React.RefObject<HTMLDivElement | null>;
};

export function useCodingSessionSurfaceShell(input: {
  layout: "single" | "umbrella";
  channelId: string;
  communityScope: string;
  umbrella: CodingSessionUmbrellaRecord;
  focusedExecution: CodingSessionExecution | null;
  lens: CodingSessionSurfaceLens;
  transcript: CodingSessionSurfaceBaseCtx["transcript"];
  transcriptModel: CodingSessionTranscriptModel | null;
  umbrellaTimeline: readonly CodingSessionUmbrellaTimelineEntry[] | null;
  observedChanges: CodingSessionObservedChanges;
  subagents: CodingSessionSubagentPanel;
  taskModel: CodingSessionTaskModel | null;
  currentUserPubkey: string | null;
  resolveActorName: CodingSessionActorNameResolver;
  resolveReachability: CodingSessionReachabilityResolver;
  sessionClosed: boolean;
  observations: CodingSessionSurfaceObservations;
  openRulings: CodingSessionMissionOpenHolds | null;
  decisions: readonly CodingSessionMissionDecisionInput[] | null;
  /** `ctx.decisionRequests`; absent reads as not read (`null`). */
  decisionRequests?: CodingSessionSurfaceBaseCtx["decisionRequests"];
  /** `ctx.teamTransactions`; absent reads as not read (`null`). */
  teamTransactions?: CodingSessionSurfaceBaseCtx["teamTransactions"];
  mission: CodingSessionSurfaceMissionContent | null;
  onOpenPeople?: () => void;
}): CodingSessionSurfaceShell {
  const registry = codingSessionSurfaceRegistry();
  const lensDefinitions = registry.forLens(input.lens);
  const knownIds = React.useMemo(
    () => new Set(registry.definitions.map((definition) => definition.id)),
    [registry],
  );
  const visibleIds = React.useMemo(
    () => new Set(lensDefinitions.map((definition) => definition.id)),
    [lensDefinitions],
  );
  const drawerIds = React.useMemo(
    () =>
      new Set(
        registry.definitions
          .filter((definition) => definition.placement === "drawer")
          .map((definition) => definition.id),
      ),
    [registry],
  );
  const sessionKey = input.umbrella.sessionRef ?? input.umbrella.umbrellaKey;
  const panels = useCodingSessionSurfacePanels({
    storageKey: codingSessionSurfacePanelsStorageKey({
      relayUrl: input.communityScope,
      channelId: input.channelId,
      sessionKey,
    }),
    knownIds,
    visibleIds,
    drawerIds,
  });
  useCodingSessionSurfaceShortcuts(panels.actions);
  const minimapSlotRef = React.useRef<HTMLDivElement | null>(null);

  const focusedRecord = input.focusedExecution?.activeGeneration ?? null;
  const { authorityUnknown, isLocalProvider, statusUnread } =
    useIsLocalProvider(
      codingSessionProviderAuthority(focusedRecord, input.focusedExecution),
    );
  const projectRef = focusedRecord?.projectRef ?? null;
  // The sidebar's own resolution, as the header crumb uses (cached query).
  const owningProject = useCodingSessionProject(input.channelId, projectRef);
  const project = React.useMemo(
    () =>
      owningProject ? { id: owningProject.id, name: owningProject.name } : null,
    [owningProject],
  );
  // An agent seated on the execution is a hire: it works only in the tree cut
  // for it, never the project's checkout (`hired_seat_cwd_refusal`).
  const isHiredSeat = (focusedRecord?.agentRef ?? null) !== null;
  const treeQuery = React.useMemo<CodingSessionTreeQuery>(
    () => ({
      sessionId: focusedRecord?.commandTarget?.sessionId ?? null,
      channelId: input.channelId,
      projectRef,
      isLocalProvider: isLocalProvider === true,
      isHiredSeat,
    }),
    [
      focusedRecord?.commandTarget?.sessionId,
      input.channelId,
      isHiredSeat,
      isLocalProvider,
      projectRef,
    ],
  );
  const tree = useCodingSessionSurfaceTree({
    query: treeQuery,
    isLocalProvider,
    statusUnread,
    authorityUnknown,
  });
  const { resolveReachability, umbrella } = input;
  const executions = React.useMemo(
    () =>
      umbrella.executions.map((execution) => {
        const record = execution.activeGeneration;
        return {
          execution,
          wireStatus: record.status,
          status: deriveCodingSessionWorkspaceStatus(
            record.transcript,
            record.status,
            record.statusAt,
            resolveReachability(record.commandTarget),
          ),
        };
      }),
    [resolveReachability, umbrella.executions],
  );

  const openRulings = umbrella.genesisRef === null ? null : input.openRulings;
  const decisions = umbrella.genesisRef === null ? null : input.decisions;
  const decisionRequests =
    umbrella.genesisRef === null ? null : (input.decisionRequests ?? null);
  const teamTransactions =
    umbrella.genesisRef === null ? null : (input.teamTransactions ?? null);
  const base = React.useMemo<CodingSessionSurfaceBaseCtx>(
    () => ({
      layout: input.layout,
      channelId: input.channelId,
      communityScope: input.communityScope,
      sessionKey,
      umbrella,
      focusedExecution: input.focusedExecution,
      focusedRecord,
      executions,
      transcript: input.transcript,
      transcriptModel: input.transcriptModel,
      umbrellaTimeline: input.umbrellaTimeline,
      observedChanges: input.observedChanges,
      subagents: input.subagents,
      taskModel: input.taskModel,
      isLocalProvider,
      projectRef,
      repoRef: focusedRecord?.repoRef ?? null,
      project,
      genesisRef: umbrella.genesisRef,
      founderPubkey: umbrella.founderPubkey,
      currentUserPubkey: input.currentUserPubkey,
      sessionClosed: input.sessionClosed,
      lens: input.lens,
      activeSurfaceId: panels.state.rightOpen ? panels.state.active : null,
      panelState: panels.state,
      panels: panels.actions,
      observations: input.observations,
      openRulings,
      decisions,
      decisionRequests,
      teamTransactions,
      tree,
      minimapSlotRef,
      resolveActorName: input.resolveActorName,
      resolveReachability,
      mission: input.mission,
      onOpenPeople: input.onOpenPeople,
    }),
    [
      executions,
      focusedRecord,
      input.channelId,
      input.communityScope,
      input.currentUserPubkey,
      input.focusedExecution,
      input.layout,
      input.lens,
      input.mission,
      input.observations,
      input.observedChanges,
      input.onOpenPeople,
      input.resolveActorName,
      input.sessionClosed,
      input.subagents,
      input.taskModel,
      input.transcript,
      input.transcriptModel,
      input.umbrellaTimeline,
      isLocalProvider,
      openRulings,
      decisions,
      decisionRequests,
      teamTransactions,
      panels.actions,
      panels.state,
      project,
      projectRef,
      resolveReachability,
      sessionKey,
      tree,
      umbrella,
    ],
  );
  // Each surface's own hook, in registry order — stable, because the
  // registry is fixed at load (rules of hooks hold).
  const nextExtensions: Record<string, unknown> = {};
  for (const definition of registry.definitions) {
    const readExtension = definition.readExtension;
    if (readExtension) nextExtensions[definition.id] = readExtension(base);
  }
  const extensions = useStableExtensions(nextExtensions);
  const ctx = React.useMemo<CodingSessionSurfaceCtx>(
    () => ({ ...base, extensions }),
    [base, extensions],
  );
  const surfaces = React.useMemo(
    () => resolveCodingSessionSurfaces(lensDefinitions, ctx),
    [ctx, lensDefinitions],
  );
  const drawerSurfaces = React.useMemo(
    () =>
      surfaces.filter((surface) => surface.definition.placement === "drawer"),
    [surfaces],
  );

  const agentsAvailable = surfaces.some(
    (surface) =>
      surface.definition.id === "agents" && surface.availability.available,
  );
  const { actions } = panels;
  const openAgentsSurface = React.useMemo(
    () => (agentsAvailable ? () => actions.open("agents") : undefined),
    [actions, agentsAvailable],
  );
  const openMissionSurfaces = React.useCallback(() => {
    for (const id of MISSION_SURFACE_IDS) actions.open(id);
    actions.activate("mission-inspector");
  }, [actions]);
  const openMissionSurfacesProactively = React.useCallback(
    () => actions.openProactive(MISSION_SURFACE_IDS, "mission-inspector"),
    [actions],
  );
  const closeMissionSurfaces = React.useCallback(
    () => actions.closeMany(MISSION_SURFACE_IDS),
    [actions],
  );
  const drawerUnavailable = drawerSurfaces.find(
    (surface) => !surface.availability.available,
  )?.availability;
  return {
    ctx,
    panels,
    surfaces,
    drawerSurfaces,
    openAgentsSurface,
    openMissionSurfaces,
    openMissionSurfacesProactively,
    closeMissionSurfaces,
    headerPanels: {
      rightOpen: panels.state.rightOpen,
      onToggleRight: actions.toggleRight,
      bottomOpen: panels.state.bottomOpen,
      onToggleBottom: actions.toggleBottom,
      bottomUnavailableReason:
        drawerUnavailable && !drawerUnavailable.available
          ? drawerUnavailable.reason
          : null,
    },
    minimapSlotRef,
  };
}
