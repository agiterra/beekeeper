import * as React from "react";
import { toast } from "sonner";

import { useAppNavigation } from "@/app/navigation/useAppNavigation";

import type { CodingSessionChannelAccess } from "@/features/coding-sessions/lib/codingSessionChannelAccess";
import {
  readCodingSessionLensPreference,
  type CodingSessionLens,
  writeCodingSessionLensPreference,
} from "@/features/coding-sessions/lib/codingSessionLensPreference";
import {
  readCodingSessionMissionDensity,
  type CodingSessionMissionDensity,
  writeCodingSessionMissionDensity,
} from "@/features/coding-sessions/lib/codingSessionMissionDensity";
import { deriveCodingSessionStreamPresence } from "@/features/coding-sessions/lib/codingSessionStreamPresence";
import { useCodingSessionOperatorProfiles } from "@/features/coding-sessions/hooks/useCodingSessionOperatorProfiles";
import { useCodingSessionTeamWake } from "@/features/coding-sessions/hooks/useCodingSessionTeamWake";
import { useCodingSessionBottomAnchor } from "@/features/coding-sessions/hooks/useCodingSessionBottomAnchor";
import { listCodingSessionUmbrellaParticipants } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import { useCodingSessionActorNameResolver } from "@/features/coding-sessions/lib/useCodingSessionActorNames";
import {
  codingSessionUmbrellaParticipantKey,
  defaultCodingSessionUmbrellaParticipantKey,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import type {
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type {
  CodingSessionGoal,
  CodingSessionGoalReader,
} from "@/features/coding-sessions/lib/codingSessionGoal";
import type { CodingSessionDisplayName } from "@/features/coding-sessions/lib/codingSessionTitle";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import type { CodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { deriveCodingSessionObservedChanges } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { useCodingSessionLane } from "@/features/coding-sessions/useCodingSessionLane";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useElementWidth } from "@/shared/hooks/use-mobile";
import { cn } from "@/shared/lib/cn";
import { CodingSessionUmbrellaHeaderRow } from "./CodingSessionUmbrellaHeaderRow";
import { useCodingSessionFullAccess } from "./useCodingSessionFullAccess";
import { CodingSessionDetailsContinuityProvider } from "./CodingSessionHeaderDetailsContinuity";
import {
  CodingSessionUmbrellaClosedSandboxFooter,
  useCodingSessionUmbrellaContinuity,
} from "./CodingSessionUmbrellaSessionFacts";
import { useCodingSessionColumnGutter } from "../lib/codingSessionWidthPreference";
import {
  CODING_SESSION_COMPOSER_DOCK_FADE,
  CodingSessionColumn,
} from "./CodingSessionColumn";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";
import { CodingSessionNarrativeJumpPill } from "./CodingSessionWorkspaceJumpPill";
import { CodingSessionNameDialog } from "./CodingSessionNameDialog";
import { CodingSessionSubagentPage } from "./CodingSessionSubagentPage";
import { CodingSessionTaskRailSheet } from "./CodingSessionTaskRail";
import { CodingSessionUmbrellaDock } from "./CodingSessionUmbrellaDock";
import { useCodingSessionTaskDock } from "./useCodingSessionTaskDock";
import { deriveCodingSessionActiveTaskModel } from "./useCodingSessionTaskDock";
import {
  CodingSessionAgentFocus,
  type CodingSessionAgentFocusItem,
} from "./CodingSessionAgentFocus";
import { CodingSessionFocusedAgentNotice } from "./CodingSessionFocusedAgentNotice";
import { useCodingSessionStopAll } from "./useCodingSessionStopAll";
import type { CodingSessionActiveWorkAgent } from "./CodingSessionActiveWorkDock";
import { CodingSessionSurfaceHost } from "./CodingSessionSurfaceHost";
import { CodingSessionSurfaceDrawerHost } from "./CodingSessionSurfaceDrawerHost";
import { CodingSessionOpenAgentsSurfaceContext } from "./CodingSessionTranscriptAgentsSurface";
import {
  CodingSessionMinimapSlot,
  CodingSessionSurfaceCtxProvider,
} from "./surfaces/codingSessionSurfaceContext";
import { isTranscriptHiddenByPanel } from "./surfaces/useCodingSessionSurfacePanels";
import { useCodingSessionSurfaceShell } from "./surfaces/useCodingSessionSurfacePanelsShell";
import {
  useCodingSessionReachabilityResolver,
  type CodingSessionReachabilityResolver,
} from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionUmbrellaComposerPrefill } from "./CodingSessionUmbrellaComposer";
import { CodingSessionRouteRail } from "./CodingSessionRouteRail";
import { CodingSessionRouteScrubber } from "./CodingSessionRouteScrubber";
import { useCodingSessionRoute } from "./useCodingSessionRoute";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView";
import {
  useCodingSessionMissionSurface,
  useCodingSessionMissionSurfaceActivation,
  useCodingSessionUmbrellaSubagents,
} from "./useCodingSessionMissionSurface";
import {
  CodingSessionComposerRecipientContext,
  CodingSessionMissionLensContext,
  CodingSessionOpenHoldsContext,
  useScrollNarrativeToLatestOnFocus,
  useCodingSessionElementHeight,
  useCodingSessionUmbrellaSeatLookups,
  useCodingSessionUmbrellaSurfaceTimeline,
} from "./CodingSessionUmbrellaWorkspaceModel";

export { buildUmbrellaTurnBlockHandoff } from "./CodingSessionUmbrellaTurnBlock";
export { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView";

/**
 * The umbrella surface: one time-ordered narrative interleaved at turn-block
 * granularity across N executions, plus the conversation lane. Mounted only
 * when an umbrella actually has more than one execution — an umbrella of one
 * renders today's single-session tree and never sees this component.
 */
export function UmbrellaCodingSessionWorkspace({
  acceptedOperators = null,
  catalogSettled,
  resolveReachability: sharedResolveReachability,
  channelId,
  channelName,
  communityScope,
  generationId,
  channelAccess,
  onAddProvider,
  onCloseSession,
  onReopenSession,
  onClose,
  onOpenPeople,
  peopleCount = 0,
  surface,
  umbrella,
  focusedExecution,
  currentUserPubkey,
  goal,
  goalReader,
  sessionName = null,
  sessionClosed = false,
  turnStartedAtFor,
}: {
  /** Live operator grants from the session roster; null while unknown. */
  acceptedOperators?: ReadonlySet<string> | null;
  /** True only after signed provider history and its live fence have settled. */
  catalogSettled: boolean;
  /** The workspace's own resolver, threaded to avoid a second live REQ. */
  resolveReachability?: CodingSessionReachabilityResolver;
  channelId: string;
  channelName: string | null;
  /** Stable normalized relay/community identity for local lens persistence. */
  communityScope: string;
  generationId: string;
  /** Write access to the session's channel (`codingSessionChannelAccess`). */
  channelAccess: CodingSessionChannelAccess;
  /** Opens the join flow (design §B); absent when this session cannot join. */
  onAddProvider?: () => void;
  onCloseSession?: () => void;
  onReopenSession?: () => void;
  /** Closes the pop-out window. Absent in the main window, where the app's
   * own back/forward in the top chrome is the way out of a session. */
  onClose?: () => void;
  /** Opens the session People surface; absent for no-genesis sessions. */
  onOpenPeople?: () => void;
  peopleCount?: number;
  surface: CodingSessionSurface;
  umbrella: CodingSessionUmbrellaRecord;
  focusedExecution: CodingSessionExecution;
  currentUserPubkey: string | null;
  goal: CodingSessionGoal | null;
  /** The goal reader's own condition — A1. Rendered by the Inspector. */
  goalReader: CodingSessionGoalReader;
  sessionName?: CodingSessionDisplayName | null;
  sessionClosed?: boolean;
  turnStartedAtFor?: (
    channelId: string,
    turnId: string,
    providerAuthorityPubkey: string,
  ) => number | null;
}) {
  const gutter = useCodingSessionColumnGutter();
  const identity = useIdentityQuery();
  const lane = useCodingSessionLane(channelId, umbrella.sessionRef);
  // Team-wake arbitration returns what it observed (one delivery row per wake,
  // one seat-authority row per seated execution), not transient toasts.
  const fullAccess = useCodingSessionFullAccess({
    channelId,
    record: focusedExecution.activeGeneration,
  });
  // SV-16: Details reads the focused execution's continuity, which left the
  // Mission transcript like the single workspace's did.
  const focusedContinuity =
    useCodingSessionUmbrellaContinuity(focusedExecution);
  const teamWake = useCodingSessionTeamWake({
    catalogSettled,
    channelId,
    communityScope,
    currentUserPubkey,
    sessionClosed,
    umbrella,
  });
  // One coordination read for the whole umbrella; every execution composer
  // asks it whether anything is answering for that generation (§2 item 41).
  // Borrowed from the workspace above when it has one, so an open session
  // holds a single coordination subscription rather than one per surface.
  const ownReachability = useCodingSessionReachabilityResolver(
    sharedResolveReachability ? null : channelId,
  );
  const resolveReachability = sharedResolveReachability ?? ownReachability;
  const [workspaceBodyRef, bodyWidthPx] = useElementWidth<HTMLDivElement>();
  // B4: the stream's bottom reserve is the dock's own height (plus its fade)
  // in both lenses, so the unreachable and disconnected notices, the
  // active-work strip and the task dock push the reserve instead of
  // overlapping the last turn block.
  const [dockRef, dockHeightPx] =
    useCodingSessionElementHeight<HTMLDivElement>();
  const isNarrow = bodyWidthPx > 0 && bodyWidthPx < 960;
  const headerCompact = bodyWidthPx > 0 && bodyWidthPx < 1320;
  const isMultiExecution = umbrella.executions.length > 1;
  const lensCoordinates = React.useMemo(
    () => ({
      communityScope,
      channelId,
      sessionKey: umbrella.sessionRef ?? umbrella.umbrellaKey,
    }),
    [channelId, communityScope, umbrella.sessionRef, umbrella.umbrellaKey],
  );
  const [lens, setLens] = React.useState<CodingSessionLens>(() =>
    isMultiExecution
      ? readCodingSessionLensPreference({
          ...lensCoordinates,
          storage: window.localStorage,
        })
      : "conversation",
  );
  const mission = isMultiExecution && lens === "mission";
  const [missionDensity, setMissionDensity] =
    React.useState<CodingSessionMissionDensity>(() =>
      readCodingSessionMissionDensity({
        coordinates: lensCoordinates,
        storage: window.localStorage,
      }),
    );
  React.useEffect(() => {
    setMissionDensity(
      readCodingSessionMissionDensity({
        coordinates: lensCoordinates,
        storage: window.localStorage,
      }),
    );
  }, [lensCoordinates]);
  const [focusedExecutionKey, setFocusedExecutionKey] = React.useState<
    string | null
  >(null);
  const narrativeScrollRef = React.useRef<HTMLDivElement>(null);
  const narrativeMemoryKey = `${channelId}:${umbrella.sessionRef ?? umbrella.umbrellaKey}`;
  const narrativeAnchorRef = useCodingSessionBottomAnchor(
    narrativeScrollRef,
    narrativeMemoryKey,
  );
  const workspaceActorName = useCodingSessionActorNameResolver(umbrella);
  const composerParticipants = React.useMemo(
    () => listCodingSessionUmbrellaParticipants(umbrella, workspaceActorName),
    [umbrella, workspaceActorName],
  );
  const [composerParticipantKey, setComposerParticipantKey] = React.useState<
    string | null
  >(() => defaultCodingSessionUmbrellaParticipantKey(composerParticipants));
  const composerParticipant =
    composerParticipants.find(
      (participant) =>
        codingSessionUmbrellaParticipantKey(participant) ===
        composerParticipantKey,
    ) ??
    composerParticipants.find(
      (participant) => participant.kind === "execution",
    ) ??
    null;
  const composerRecord =
    composerParticipant?.kind === "execution"
      ? composerParticipant.execution.activeGeneration
      : null;
  const composerStatus = composerRecord
    ? deriveCodingSessionWorkspaceStatus(
        composerRecord.transcript,
        composerRecord.status,
        composerRecord.statusAt,
        resolveReachability(composerRecord.commandTarget),
      )
    : null;
  const composerTaskModel = React.useMemo(
    () =>
      composerRecord
        ? deriveCodingSessionTaskModel(composerRecord.transcript)
        : null,
    [composerRecord],
  );
  const composerTaskDock = useCodingSessionTaskDock({
    isNarrow,
    isWorking: composerStatus?.kind === "working",
    model: composerTaskModel,
    transcript: composerRecord?.transcript ?? [],
  });
  const [prefill, setPrefill] =
    React.useState<CodingSessionUmbrellaComposerPrefill | null>(null);
  // Every execution's items, prior generations included, in the order the
  // timeline ingests them: the observed changes and the surfaces' transcript.
  const umbrellaItems = React.useMemo(
    () =>
      umbrella.executions.flatMap((execution) =>
        [...execution.priorGenerations, execution.activeGeneration].flatMap(
          (record) => record.transcript,
        ),
      ),
    [umbrella.executions],
  );
  const observedChanges = React.useMemo(
    () => deriveCodingSessionObservedChanges(umbrellaItems),
    [umbrellaItems],
  );
  // Same item set as the timeline renders, so every operator who drove a turn
  // anywhere in the umbrella is resolvable in one lookup — plus the lane's own
  // authors, who never drove a turn and so appeared in no transcript item,
  // which is why their messages rendered as bare keys (walk finding 4).
  const umbrellaTranscript = React.useMemo(
    () => [
      ...umbrellaItems,
      ...lane.messages.map((message) => ({
        operatorPubkey: message.authorPubkey,
      })),
    ],
    [lane.messages, umbrellaItems],
  );
  const operatorProfiles = useCodingSessionOperatorProfiles(
    umbrellaTranscript,
    currentUserPubkey,
  );
  const {
    contextLoads,
    resolveMissionActor,
    resolvePromptSeat,
    routedSeats,
    seatBeeStamps,
    seatPackRefs,
  } = useCodingSessionUmbrellaSeatLookups(umbrella, workspaceActorName);
  const [renameOpen, setRenameOpen] = React.useState(false);
  const authoritativeTitle = sessionName?.content ?? umbrella.title;
  const canRename =
    umbrella.sessionRef !== null &&
    umbrella.founderPubkey !== null &&
    currentUserPubkey?.toLowerCase() === umbrella.founderPubkey.toLowerCase();
  const agentFocusItems = React.useMemo<CodingSessionAgentFocusItem[]>(
    () =>
      composerParticipants.flatMap((participant) => {
        if (participant.kind !== "execution") return [];
        const record = participant.execution.activeGeneration;
        return [
          {
            executionKey: participant.executionKey,
            label: participant.label,
            status: deriveCodingSessionWorkspaceStatus(
              record.transcript,
              record.status,
              record.statusAt,
              resolveReachability(record.commandTarget),
            ),
          },
        ];
      }),
    [composerParticipants, resolveReachability],
  );
  const activeWorkAgents = React.useMemo<CodingSessionActiveWorkAgent[]>(
    () =>
      composerParticipants.flatMap((participant) => {
        if (participant.kind !== "execution") return [];
        const record = participant.execution.activeGeneration;
        const status = deriveCodingSessionWorkspaceStatus(
          record.transcript,
          record.status,
          record.statusAt,
          resolveReachability(record.commandTarget),
        );
        if (status.kind !== "working") return [];
        const model = deriveCodingSessionTaskModel(record.transcript);
        const activeModel = deriveCodingSessionActiveTaskModel({
          isWorking: true,
          model,
          transcript: record.transcript,
        });
        return [
          {
            executionKey: participant.executionKey,
            label: participant.label,
            model: activeModel,
            turnKey: `${participant.executionKey}:${activeModel?.turnId ?? record.statusAt ?? record.lastEventAt}`,
          },
        ];
      }),
    [composerParticipants, resolveReachability],
  );
  const canSteerTeam =
    currentUserPubkey !== null &&
    (currentUserPubkey.toLowerCase() ===
      umbrella.founderPubkey?.toLowerCase() ||
      acceptedOperators?.has(currentUserPubkey) === true);
  const streamPresence = React.useMemo(
    () =>
      deriveCodingSessionStreamPresence({
        umbrella,
        resolveActorName: workspaceActorName,
        resolveStatus: (execution) => {
          const record = execution.activeGeneration;
          return deriveCodingSessionWorkspaceStatus(
            record.transcript,
            record.status,
            record.statusAt,
            resolveReachability(record.commandTarget),
          );
        },
        resolveTurnStartedAt: (execution, turnId) =>
          turnStartedAtFor?.(channelId, turnId, execution.signerPubkey) ?? null,
        canSteer: canSteerTeam,
      }),
    [
      canSteerTeam,
      channelId,
      resolveReachability,
      turnStartedAtFor,
      umbrella,
      workspaceActorName,
    ],
  );
  // One W1 resolution for the whole surface. The chips, the live strip and
  // now the turn-block byline all read this map, so a seat cannot be `live`
  // in one place and `idle` two inches away (finding 8).
  //
  // `live` is the chip's own test verbatim (`CodingSessionParticipantBar.tsx`
  // `item.status.kind === "working"`) rather than "the word is non-empty": a
  // seat demoted by reachability keeps a word and stops being live, and the
  // block's animation has to make the same call the chip makes (REVIEW-A3 F3).
  const missionLiveness = React.useMemo(
    () =>
      new Map(
        streamPresence.participants.map((participant) => [
          participant.executionKey,
          {
            word: participant.disposition,
            live: participant.status.kind === "working",
          },
        ]),
      ),
    [streamPresence.participants],
  );
  const handleFocusExecution = React.useCallback(
    (executionKey: string | null) => setFocusedExecutionKey(executionKey),
    [],
  );
  const surfaceHostCloseRef = React.useRef<() => void>(() => {});
  const handleMissionDensityChange = React.useCallback(
    (density: CodingSessionMissionDensity) => {
      setMissionDensity(density);
      writeCodingSessionMissionDensity({
        coordinates: lensCoordinates,
        density,
        storage: window.localStorage,
      });
      if (density === "trace") surfaceHostCloseRef.current();
    },
    [lensCoordinates],
  );
  const missionSurfaceResult = useCodingSessionMissionSurface({
    active: mission,
    channelId,
    contextLoads,
    deliveries: teamWake.deliveries,
    focusedExecutionKey,
    goal,
    goalReader,
    // The goal has one editable home now: the Inspector's Current goal
    // section. The stream used to carry a second pill above the narrative.
    goalEditor: (
      <CodingSessionGoalPill
        channelId={channelId}
        currentUserPubkey={currentUserPubkey}
        founderPubkey={umbrella.founderPubkey}
        goal={goal}
        sessionRef={umbrella.sessionRef}
        variant="inspector"
      />
    ),
    isNarrow,
    observedChanges,
    onFocusParticipant: handleFocusExecution,
    onOpenTrace: () => handleMissionDensityChange("trace"),
    participants: streamPresence.participants,
    resolveActorName: workspaceActorName,
    seatAuthorities: teamWake.seatAuthorities,
    umbrella,
  });
  // The Route rail (DESIGN-SPEC §9): a second projection of the rows the
  // stream already renders, laid on a clock. The derivation and its two width
  // gates live in `useCodingSessionRoute`; this file only places the result.
  // F2: what an open-hold line calls the founder's own party. `you` when the
  // person reading the screen is the founder — which is the ordinary case for
  // the hold that matters most, a seat's report awaiting the founder's verdict.
  const founderHoldLabel =
    currentUserPubkey !== null &&
    umbrella.founderPubkey !== null &&
    currentUserPubkey.toLowerCase() === umbrella.founderPubkey.toLowerCase()
      ? "you"
      : (workspaceActorName(umbrella.founderPubkey ?? "") ?? undefined);
  const routeRail = useCodingSessionRoute({
    bodyRef: workspaceBodyRef,
    founderLabel: founderHoldLabel,
    bodyWidthPx,
    deliveries: teamWake.deliveries,
    density: missionDensity,
    founderPubkey: umbrella.founderPubkey,
    gateRows: missionSurfaceResult.observationGates,
    participants: streamPresence.participants,
    resolveMissionActor,
    seatAuthorities: teamWake.seatAuthorities,
    transactions: missionSurfaceResult.transactions,
    umbrella,
  });

  const subagents = useCodingSessionUmbrellaSubagents(umbrella);
  const umbrellaTimeline = useCodingSessionUmbrellaSurfaceTimeline(
    umbrella,
    lane.messages,
  );
  const shell = useCodingSessionSurfaceShell({
    layout: "umbrella",
    channelId,
    communityScope,
    umbrella,
    focusedExecution,
    lens: mission ? "mission" : "conversation",
    transcript: umbrellaItems,
    transcriptModel: null,
    umbrellaTimeline,
    observedChanges,
    subagents,
    taskModel: composerTaskModel,
    currentUserPubkey,
    resolveActorName: workspaceActorName,
    resolveReachability,
    sessionClosed,
    observations: missionSurfaceResult.observationRead,
    // Every lens: the evidence is read whenever there is a genesis (B0).
    openRulings: missionSurfaceResult.teamEvidenceRead
      ? routeRail.openHolds
      : null,
    decisions: missionSurfaceResult.teamEvidenceRead
      ? missionSurfaceResult.decisions
      : null,
    decisionRequests: missionSurfaceResult.teamEvidenceRead
      ? missionSurfaceResult.decisionRequests
      : null,
    teamTransactions: missionSurfaceResult.teamEvidenceRead
      ? missionSurfaceResult.transactions
      : null,
    mission: missionSurfaceResult.missionContent,
    onOpenPeople,
  });
  const { actions: panelActions, state: panelState } = shell.panels;
  React.useEffect(() => {
    surfaceHostCloseRef.current = panelActions.closeRight;
  }, [panelActions.closeRight]);
  useCodingSessionMissionSurfaceActivation({
    bodyWidthPx,
    closeMissionSurfaces: shell.closeMissionSurfaces,
    isMultiExecution,
    mission,
    openProactive: panelActions.openProactive,
    openMissionSurfaces: shell.openMissionSurfacesProactively,
    openMissionSurfaceIds: shell.openMissionSurfaceIds,
    reopenMissionSurfaces: shell.reopenMissionSurfaces,
  });
  const narrativeExpanded = !panelState.rightOpen;
  const surfaceHostId = React.useId();
  /**
   * The Route control acts on what is on screen, not on a stored byte.
   *
   * REVIEW-L4 F3: `toggleCollapsed` flipped the viewer's flag, and at any
   * width where the floor had already folded the rail that flag was the other
   * term of an `&&` that was already false — a control labelled "Expand route
   * rail", pressed twice, changed nothing but a localStorage byte. The rail
   * now behaves like the Inspector's twin: asking for it at a width that
   * cannot hold both panels **closes the Inspector to make the room**, and
   * says so in its own title. Collapsing never re-opens the Inspector — the
   * viewer closed it, and reopening a panel nobody asked for is its own lie.
   */
  const handleToggleRouteRail = React.useCallback(() => {
    if (routeRail.fits) {
      routeRail.setCollapsed(true);
      return;
    }
    if (!routeRail.roomForRail && panelState.rightOpen) {
      panelActions.closeRight();
    }
    routeRail.setCollapsed(false);
  }, [
    panelActions,
    panelState.rightOpen,
    routeRail.fits,
    routeRail.roomForRail,
    routeRail.setCollapsed,
  ]);
  const focusedAgent =
    agentFocusItems.find((item) => item.executionKey === focusedExecutionKey) ??
    null;
  // W1, once. The chips read `streamPresence.participants` directly; the
  // stop-all control reads this projection of the same array, so the header's
  // liveness split and the chips' words cannot disagree (L4.3).
  const streamPresenceLive = React.useMemo(
    () =>
      new Map(
        streamPresence.participants.map((participant) => [
          participant.executionKey,
          participant.status.kind === "working",
        ]),
      ),
    [streamPresence.participants],
  );
  const {
    dialog: stopAllDialog,
    model: stopAll,
    stopAll: handleStopAll,
  } = useCodingSessionStopAll({
    channelId,
    currentUserPubkey,
    liveByExecutionKey: streamPresenceLive,
    mission,
    resolveActorName: workspaceActorName,
    resolveReachability,
    umbrella,
  });
  const handleLensChange = React.useCallback(
    (nextLens: CodingSessionLens) => {
      setLens(nextLens);
      if (nextLens === "mission") shell.openMissionSurfaces();
      else shell.closeMissionSurfaces();
      writeCodingSessionLensPreference({
        ...lensCoordinates,
        lens: nextLens,
        storage: window.localStorage,
      });
    },
    [lensCoordinates, shell.closeMissionSurfaces, shell.openMissionSurfaces],
  );
  useScrollNarrativeToLatestOnFocus(narrativeScrollRef, focusedExecutionKey);
  const { goProject } = useAppNavigation();
  const owningProject = shell.ctx.project;

  const handlePopout = React.useCallback(() => {
    void openCodingSessionPopout(channelId, generationId).catch((error) => {
      toast.error(
        error instanceof Error
          ? error.message
          : "Unable to open the coding-session window.",
      );
    });
  }, [channelId, generationId]);

  return (
    <main
      className="relative flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-background"
      data-testid="coding-session-umbrella-workspace"
    >
      <CodingSessionDetailsContinuityProvider value={focusedContinuity}>
        <CodingSessionUmbrellaHeaderRow
          agentFocusItems={agentFocusItems}
          authoritativeTitle={authoritativeTitle}
          sessionTitleOrigin={sessionName}
          canRename={canRename}
          channelId={channelId}
          channelName={channelName}
          composerTaskDock={composerTaskDock}
          contextLoads={contextLoads}
          focusedExecution={focusedExecution}
          fullAccess={fullAccess}
          focusedExecutionKey={focusedExecutionKey}
          goal={goal}
          handleFocusExecution={handleFocusExecution}
          handleLensChange={handleLensChange}
          handleMissionDensityChange={handleMissionDensityChange}
          handlePopout={handlePopout}
          handleStopAll={handleStopAll}
          handleToggleRouteRail={handleToggleRouteRail}
          headerCompact={headerCompact}
          isMultiExecution={isMultiExecution}
          isNarrow={isNarrow}
          lens={lens}
          mission={mission}
          missionDensity={missionDensity}
          onAddProvider={onAddProvider}
          onClose={onClose}
          onCloseSession={onCloseSession}
          onOpenPeople={onOpenPeople}
          onOpenProject={
            owningProject && surface === "main"
              ? () => void goProject(owningProject.id)
              : undefined
          }
          onReopenSession={onReopenSession}
          peopleCount={peopleCount}
          resolveReachability={resolveReachability}
          routedSeats={routedSeats}
          routeRail={routeRail}
          seatBeeStamps={seatBeeStamps}
          seatPackRefs={seatPackRefs}
          sessionClosed={sessionClosed}
          setRenameOpen={setRenameOpen}
          streamParticipants={streamPresence.participants}
          stopAll={stopAll}
          surface={surface}
          surfaceHostId={surfaceHostId}
          surfaceShell={shell}
          teamWake={teamWake}
          umbrella={umbrella}
          workspaceActorName={workspaceActorName}
        />
      </CodingSessionDetailsContinuityProvider>
      {umbrella.sessionRef ? (
        <CodingSessionNameDialog
          channelId={channelId}
          currentName={authoritativeTitle}
          onOpenChange={setRenameOpen}
          open={renameOpen}
          sessionRef={umbrella.sessionRef}
        />
      ) : null}
      <CodingSessionMissionLensContext.Provider value={mission}>
        <CodingSessionOpenHoldsContext.Provider value={routeRail.openHolds}>
          <CodingSessionComposerRecipientContext.Provider
            value={composerParticipant?.label ?? null}
          >
            <CodingSessionSurfaceCtxProvider value={shell.ctx}>
              <div className="flex min-h-0 flex-1" ref={workspaceBodyRef}>
                {/* §9.2: Mission only — the map when 224 px fit beside the
                  column, else the 40 px scrubber. */}
                {mission ? (
                  routeRail.fits ? (
                    <CodingSessionRouteRail
                      onCollapse={handleToggleRouteRail}
                      onExpandRoad={routeRail.expandRoad}
                      onFocusRoad={handleFocusExecution}
                      onResizeKeyDown={routeRail.onResizeKeyDown}
                      onResizeStart={routeRail.onResizeStart}
                      onRevealRow={routeRail.revealRow}
                      route={routeRail.route}
                      widthPx={routeRail.widthPx}
                    />
                  ) : (
                    <CodingSessionRouteScrubber
                      onExpandRoad={routeRail.expandRoad}
                      onFocusRoad={handleFocusExecution}
                      onRevealRow={routeRail.revealRow}
                      route={routeRail.route}
                    />
                  )
                ) : null}
                <section
                  aria-label="Umbrella session narrative"
                  className={cn(
                    "relative flex min-w-0 flex-1 flex-col overflow-hidden",
                    isTranscriptHiddenByPanel(panelState, isNarrow) && "hidden",
                  )}
                  ref={routeRail.sectionRef}
                >
                  {/* SV-21: the narrative and its composer overlay sit above
                    the drawer, so the composer's bottom-0 anchors over it. */}
                  <div
                    className="relative flex min-h-0 flex-1 flex-col"
                    data-testid="coding-session-narrative-region"
                  >
                    <CodingSessionMinimapSlot slotRef={shell.minimapSlotRef} />
                    <CodingSessionSubagentPage />
                    <div className={cn(gutter, "pb-2")}>
                      {/* Mission's goal lives in the header subtitle (read) and the
                Inspector's Current goal section (edit). Conversation keeps
                this pill exactly as it was. */}
                      {mission ? null : (
                        <CodingSessionGoalPill
                          channelId={channelId}
                          currentUserPubkey={currentUserPubkey}
                          founderPubkey={umbrella.founderPubkey}
                          goal={goal}
                          headerCarriesGoal
                          sessionRef={umbrella.sessionRef}
                          workspaceExpanded={narrativeExpanded}
                        />
                      )}
                      {isMultiExecution && isNarrow && !mission ? (
                        <div className="mt-2 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
                          <CodingSessionAgentFocus
                            agentSurfaceOpen={
                              panelState.rightOpen &&
                              panelState.active === "agents"
                            }
                            focusedExecutionKey={focusedExecutionKey}
                            items={agentFocusItems}
                            onFocus={handleFocusExecution}
                            onOpenAgents={() => {
                              composerTaskDock.close();
                              panelActions.toggle("agents");
                            }}
                            surfaceHostId={surfaceHostId}
                          />
                        </div>
                      ) : null}
                    </div>
                    <div
                      className={cn(
                        "min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain",
                        gutter,
                      )}
                      data-testid="coding-session-narrative-scroll"
                      ref={narrativeAnchorRef}
                    >
                      <CodingSessionColumn
                        className={cn(
                          "min-h-full pt-7",
                          sessionClosed && "pb-7",
                        )}
                        expanded={narrativeExpanded}
                        mission={mission}
                        // One number in both lenses. Conversation's literal
                        // `pb-48` / `pb-[34rem]` put the composer over the last
                        // row and opened ~350 px when a reply started (Andy,
                        // 2026-09-29).
                        style={
                          !sessionClosed && dockHeightPx > 0
                            ? {
                                paddingBottom: `calc(${dockHeightPx}px + ${CODING_SESSION_COMPOSER_DOCK_FADE})`,
                              }
                            : undefined
                        }
                      >
                        {focusedAgent ? (
                          <CodingSessionFocusedAgentNotice
                            agent={focusedAgent}
                            onClear={() => handleFocusExecution(null)}
                          />
                        ) : null}
                        <CodingSessionOpenAgentsSurfaceContext.Provider
                          value={
                            mission ? null : (shell.openAgentsSurface ?? null)
                          }
                        >
                          <CodingSessionUmbrellaTimelineView
                            channelId={channelId}
                            currentUserPubkey={currentUserPubkey}
                            focusedExecutionKey={focusedExecutionKey}
                            laneMessages={lane.messages}
                            onHandoff={setPrefill}
                            onFocusExecution={handleFocusExecution}
                            actorNames={workspaceActorName}
                            operatorProfiles={operatorProfiles}
                            missionDensity={mission ? missionDensity : null}
                            // The signed handoffs between seats are the causality plane.
                            // They used to exist only inside a pinned card's chain list;
                            // now they are rows in the stream, interleaved with the turns
                            // they caused.
                            missionDeliveries={
                              mission ? teamWake.deliveries : undefined
                            }
                            missionFounderPubkey={umbrella.founderPubkey}
                            missionLiveness={
                              mission ? missionLiveness : undefined
                            }
                            missionTransactions={
                              mission
                                ? missionSurfaceResult.transactions
                                : undefined
                            }
                            missionRevealRef={
                              mission ? routeRail.revealRef : undefined
                            }
                            narrativeScrollRef={narrativeScrollRef}
                            scrollMemoryKey={narrativeMemoryKey}
                            onMissionVisibleTimesChange={
                              mission ? routeRail.setVisibleAt : undefined
                            }
                            resolveMissionActor={resolveMissionActor}
                            resolvePromptSeat={resolvePromptSeat}
                            umbrella={umbrella}
                            // Not Mission-gated (L2, finding 17): a wake pointer
                            // reads as the same §1f sentence in both lenses.
                            // Conversation subscribes to no fold, so the index it
                            // passes is empty and its line is the unresolved one.
                            wakeOperations={missionSurfaceResult.wakeOperations}
                          />
                        </CodingSessionOpenAgentsSurfaceContext.Provider>
                      </CodingSessionColumn>
                    </div>
                    <CodingSessionNarrativeJumpPill
                      dockHeightPx={dockHeightPx}
                      hasDock={!sessionClosed}
                      scrollRef={narrativeScrollRef}
                    />
                    {!sessionClosed ? (
                      <CodingSessionUmbrellaDock
                        acceptedOperators={acceptedOperators}
                        activeWorkAgents={activeWorkAgents}
                        actorNames={workspaceActorName}
                        channelId={channelId}
                        currentUserPubkey={identity.data?.pubkey ?? null}
                        dockRef={dockRef}
                        focusedExecutionKey={focusedExecutionKey}
                        gutter={gutter}
                        channelAccess={channelAccess}
                        isMultiExecution={isMultiExecution}
                        isNarrow={isNarrow}
                        mission={mission}
                        narrativeExpanded={narrativeExpanded}
                        onAddProvider={onAddProvider}
                        onFocusExecution={handleFocusExecution}
                        onSelectedParticipantChange={setComposerParticipantKey}
                        prefill={prefill}
                        resolveReachability={resolveReachability}
                        streamPresence={streamPresence}
                        taskDock={composerTaskDock}
                        umbrella={umbrella}
                      />
                    ) : (
                      // A closed umbrella mounts no composer, so every seat's
                      // boundary (full access, unenforced) is recorded in a
                      // footer instead (SV-17).
                      <CodingSessionUmbrellaClosedSandboxFooter
                        focusedExecution={focusedExecution}
                        participants={composerParticipants}
                      />
                    )}
                  </div>
                  <CodingSessionSurfaceDrawerHost
                    ctx={shell.ctx}
                    open={panelState.bottomOpen}
                    surfaces={shell.drawerSurfaces}
                  />
                </section>
                {panelState.rightOpen ? (
                  <CodingSessionSurfaceHost
                    ctx={shell.ctx}
                    hostId={surfaceHostId}
                    layout={
                      bodyWidthPx <= 0 ? null : isNarrow ? "sheet" : "inline"
                    }
                    panels={shell.panels}
                    surfaces={shell.surfaces}
                    widthContainerRef={workspaceBodyRef}
                  />
                ) : null}
              </div>
            </CodingSessionSurfaceCtxProvider>
          </CodingSessionComposerRecipientContext.Provider>
        </CodingSessionOpenHoldsContext.Provider>
      </CodingSessionMissionLensContext.Provider>
      {isNarrow ? <CodingSessionTaskRailSheet dock={composerTaskDock} /> : null}
      {stopAllDialog}
    </main>
  );
}
