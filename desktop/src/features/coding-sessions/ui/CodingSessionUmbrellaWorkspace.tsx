import * as React from "react";
import { toast } from "sonner";

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
import { deriveSeatBeeStamps } from "@/features/coding-sessions/lib/codingSessionSeatBee";
import { useCodingSessionOperatorProfiles } from "@/features/coding-sessions/hooks/useCodingSessionOperatorProfiles";
import { useCodingSessionTeamWake } from "@/features/coding-sessions/hooks/useCodingSessionTeamWake";
import {
  readCodingSessionContextLoad,
  type CodingSessionContextLoad,
} from "@/features/coding-sessions/lib/codingSessionContextLoad";
import { buildCodingSessionTurnByline } from "@/features/coding-sessions/lib/codingSessionTurnByline";
import { listCodingSessionRoutedSeats } from "@/features/coding-sessions/lib/codingSessionRoutedSeats";
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
import type { CodingSessionName } from "@/features/coding-sessions/lib/codingSessionName";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import type { CodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { deriveCodingSessionObservedChanges } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { deriveCodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { useCodingSessionLane } from "@/features/coding-sessions/useCodingSessionLane";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useElementWidth } from "@/shared/hooks/use-mobile";
import { cn } from "@/shared/lib/cn";
import { CodingSessionUmbrellaHeaderRow } from "./CodingSessionUmbrellaHeaderRow";
import { useCodingSessionColumnGutter } from "../lib/codingSessionWidthPreference";
import { CodingSessionColumn } from "./CodingSessionColumn";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";
import { CodingSessionNameDialog } from "./CodingSessionNameDialog";
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
import {
  CodingSessionSurfaceHost,
  useCodingSessionSurfaceHostState,
} from "./CodingSessionSurfaceHost";
import { useCodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionUmbrellaComposerPrefill } from "./CodingSessionUmbrellaComposer";
import { CodingSessionRouteRail } from "./CodingSessionRouteRail";
import { CodingSessionRouteScrubber } from "./CodingSessionRouteScrubber";
import { useCodingSessionRoute } from "./useCodingSessionRoute";
import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView";
import {
  useCodingSessionMissionSurface,
  useCodingSessionMissionSurfaceActivation,
  useCodingSessionWorkspaceSurfaces,
} from "./useCodingSessionMissionSurface";
import {
  CodingSessionComposerRecipientContext,
  CodingSessionMissionLensContext,
  CodingSessionOpenHoldsContext,
  scrollCodingSessionNarrativeToLatest,
  useCodingSessionElementHeight,
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
  channelId,
  channelName,
  communityScope,
  generationId,
  isMember,
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
  channelId: string;
  channelName: string | null;
  /** Stable normalized relay/community identity for local lens persistence. */
  communityScope: string;
  generationId: string;
  isMember: boolean;
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
  sessionName?: CodingSessionName | null;
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
  // Team-wake arbitration now *returns* what it observed: one delivery row per
  // wake operation and one seat-authority row per seated execution. Before this
  // batch the result was discarded and delivery state surfaced only as
  // transient toasts, so a queued provider wake or an ungranted seat was
  // invisible the moment the toast faded.
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
  const resolveReachability = useCodingSessionReachabilityResolver(channelId);
  const [workspaceBodyRef, bodyWidthPx] = useElementWidth<HTMLDivElement>();
  // B4: the stream's bottom reserve in Mission is the dock's own height, so
  // the unreachable notice and the task dock push the reserve instead of
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
  // Observed changes across every execution the umbrella narrative renders —
  // prior generations included, in the same order the timeline ingests them.
  const observedChanges = React.useMemo(
    () =>
      deriveCodingSessionObservedChanges(
        umbrella.executions.flatMap((execution) =>
          [...execution.priorGenerations, execution.activeGeneration].flatMap(
            (record) => record.transcript,
          ),
        ),
      ),
    [umbrella.executions],
  );
  // Same item set as the timeline renders, so every operator who drove a turn
  // anywhere in the umbrella is resolvable in one lookup — plus the lane's own
  // authors, who never drove a turn and so appeared in no transcript item,
  // which is why their messages rendered as bare keys (walk finding 4).
  const umbrellaTranscript = React.useMemo(
    () => [
      ...umbrella.executions.flatMap((execution) =>
        [...execution.priorGenerations, execution.activeGeneration].flatMap(
          (record) => record.transcript,
        ),
      ),
      ...lane.messages.map((message) => ({
        operatorPubkey: message.authorPubkey,
      })),
    ],
    [lane.messages, umbrella.executions],
  );
  const operatorProfiles = useCodingSessionOperatorProfiles(
    umbrellaTranscript,
    currentUserPubkey,
  );
  // D7 / W12: how full each seat's context is, folded from the driver's own
  // signed occupancy items. Never estimated, and an execution that has
  // reported nothing carries `null` rather than a zero.
  const contextLoads = React.useMemo(
    () =>
      umbrella.executions.map((execution) => ({
        key: execution.executionKey,
        label: buildCodingSessionTurnByline({
          agentDisplayName: execution.activeGeneration.agentRef
            ? (workspaceActorName(execution.activeGeneration.agentRef) ?? null)
            : null,
          agentRef: execution.activeGeneration.agentRef,
          generation: execution.activeGeneration.commandTarget?.generation ?? 1,
          label: null,
          model: execution.activeGeneration.model,
          role: execution.activeGeneration.role,
          runtime: execution.activeGeneration.runtime,
        }).name,
        load: readCodingSessionContextLoad(
          execution.activeGeneration.transcript,
        ) satisfies CodingSessionContextLoad | null,
      })),
    [umbrella.executions, workspaceActorName],
  );
  const routedSeats = React.useMemo(
    () => listCodingSessionRoutedSeats(umbrella.executions),
    [umbrella.executions],
  );
  // Names a transaction's author or counterparty from the umbrella's own
  // executions. A pubkey with no execution falls back to the actor-name
  // resolver, and an unknown one to `null` — the row then renders a truncated
  // key rather than guessing a name.
  const resolveMissionActor = React.useCallback(
    (pubkey: string) => {
      const execution = umbrella.executions.find(
        (candidate) =>
          candidate.activeGeneration.agentRef?.toLowerCase() ===
          pubkey.toLowerCase(),
      );
      if (!execution) {
        return {
          label: workspaceActorName(pubkey) ?? null,
          executionKey: null,
        };
      }
      const agentRef = execution.activeGeneration.agentRef;
      return {
        label: buildCodingSessionTurnByline({
          agentDisplayName: agentRef
            ? (workspaceActorName(agentRef) ?? null)
            : null,
          agentRef,
          generation: execution.activeGeneration.commandTarget?.generation ?? 1,
          label: null,
          model: execution.activeGeneration.model,
          role: execution.activeGeneration.role,
          runtime: execution.activeGeneration.runtime,
        }).name,
        executionKey: execution.executionKey,
      };
    },
    [umbrella.executions, workspaceActorName],
  );
  // Same lookup, but it answers `null` for a pubkey that is not a seat — the
  // difference that lets prompt attribution tell "a seat sent this" apart from
  // "a person sent this". A seat's turn used to read `You` to the founder or as
  // a bare truncated key to everyone else.
  const resolvePromptSeat = React.useCallback(
    (pubkey: string) => {
      const isSeat = umbrella.executions.some(
        (candidate) =>
          candidate.activeGeneration.agentRef?.toLowerCase() ===
          pubkey.toLowerCase(),
      );
      return isSeat ? resolveMissionActor(pubkey) : null;
    },
    [resolveMissionActor, umbrella.executions],
  );
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

  const surfaces = useCodingSessionWorkspaceSurfaces({
    actorNames: workspaceActorName,
    mission,
    missionSurfaces: missionSurfaceResult.surfaces,
    observedChanges,
    resolveReachability,
    umbrella,
  });
  const surfaceIds = React.useMemo(
    () => surfaces.map((surfaceEntry) => surfaceEntry.id),
    [surfaces],
  );
  const surfaceHost = useCodingSessionSurfaceHostState(surfaceIds);
  React.useEffect(() => {
    surfaceHostCloseRef.current = surfaceHost.close;
  }, [surfaceHost.close]);
  useCodingSessionMissionSurfaceActivation({
    activeTab: surfaceHost.activeTab,
    bodyWidthPx,
    close: surfaceHost.close,
    isMultiExecution,
    mission,
    select: surfaceHost.select,
  });
  const narrativeExpanded = surfaceHost.activeTab === null;
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
    if (!routeRail.roomForRail && surfaceHost.activeTab !== null) {
      surfaceHost.close();
    }
    routeRail.setCollapsed(false);
  }, [
    routeRail.fits,
    routeRail.roomForRail,
    routeRail.setCollapsed,
    surfaceHost.activeTab,
    surfaceHost.close,
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
      if (nextLens === "mission") surfaceHost.select("mission-inspector");
      else surfaceHost.close();
      writeCodingSessionLensPreference({
        ...lensCoordinates,
        lens: nextLens,
        storage: window.localStorage,
      });
    },
    [lensCoordinates, surfaceHost.close, surfaceHost.select],
  );
  React.useLayoutEffect(() => {
    if (focusedExecutionKey === null) return;
    const frame = window.requestAnimationFrame(() => {
      scrollCodingSessionNarrativeToLatest(narrativeScrollRef.current);
    });
    return () => window.cancelAnimationFrame(frame);
  }, [focusedExecutionKey]);

  const handlePopout = React.useCallback(() => {
    void openCodingSessionPopout(channelId, generationId).catch((error) => {
      toast.error(
        error instanceof Error
          ? error.message
          : "Unable to open the coding-session window.",
      );
    });
  }, [channelId, generationId]);

  // Which `bee` each seat is actually running (L12), read straight off the
  // 44223 metadata this umbrella already carries per execution — no new
  // query. `deriveSeatBeeStamps` prefers the active generation's stamp and
  // falls back to the newest prior generation's when a fresh resume has not
  // republished one yet.
  const seatBeeStamps = React.useMemo(
    () => deriveSeatBeeStamps(umbrella.executions),
    [umbrella.executions],
  );

  return (
    <main
      className="relative flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-background"
      data-testid="coding-session-umbrella-workspace"
    >
      <CodingSessionUmbrellaHeaderRow
        agentFocusItems={agentFocusItems}
        authoritativeTitle={authoritativeTitle}
        canRename={canRename}
        channelName={channelName}
        composerTaskDock={composerTaskDock}
        contextLoads={contextLoads}
        focusedExecution={focusedExecution}
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
        onReopenSession={onReopenSession}
        peopleCount={peopleCount}
        resolveReachability={resolveReachability}
        routedSeats={routedSeats}
        routeRail={routeRail}
        seatBeeStamps={seatBeeStamps}
        sessionClosed={sessionClosed}
        setRenameOpen={setRenameOpen}
        streamParticipants={streamPresence.participants}
        stopAll={stopAll}
        surface={surface}
        surfaceHost={surfaceHost}
        surfaceHostId={surfaceHostId}
        surfaces={surfaces}
        teamWake={teamWake}
        umbrella={umbrella}
        workspaceActorName={workspaceActorName}
      />
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
            <div className="flex min-h-0 flex-1" ref={workspaceBodyRef}>
              {/* §9.2: the map takes the left gutter when the body is wide enough to
            give it 224 px without narrowing the reading column; otherwise the
            40 px scrubber keeps the attention signs and Now. Mission only —
            Conversation renders neither, which is what keeps its DOM
            byte-identical. */}
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
                className="relative flex min-w-0 flex-1 flex-col overflow-hidden"
                ref={routeRail.sectionRef}
              >
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
                        agentSurfaceOpen={surfaceHost.activeTab === "agents"}
                        focusedExecutionKey={focusedExecutionKey}
                        items={agentFocusItems}
                        onFocus={handleFocusExecution}
                        onOpenAgents={() => {
                          composerTaskDock.close();
                          surfaceHost.toggle("agents");
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
                  ref={narrativeScrollRef}
                >
                  <CodingSessionColumn
                    className={cn(
                      "min-h-full pt-7",
                      // B4: Conversation keeps its two literal reserves exactly.
                      // Mission measures the dock instead — see `dockHeightPx`.
                      mission
                        ? null
                        : activeWorkAgents.length > 0 ||
                            (composerTaskDock.open && !isNarrow)
                          ? "pb-[34rem]"
                          : "pb-48",
                    )}
                    expanded={narrativeExpanded}
                    mission={mission}
                    style={
                      mission && dockHeightPx > 0
                        ? { paddingBottom: `${dockHeightPx}px` }
                        : undefined
                    }
                  >
                    {focusedAgent ? (
                      <CodingSessionFocusedAgentNotice
                        agent={focusedAgent}
                        onClear={() => handleFocusExecution(null)}
                      />
                    ) : null}
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
                      missionLiveness={mission ? missionLiveness : undefined}
                      missionTransactions={
                        mission ? missionSurfaceResult.transactions : undefined
                      }
                      missionRevealRef={
                        mission ? routeRail.revealRef : undefined
                      }
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
                  </CodingSessionColumn>
                </div>
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
                    isMember={isMember}
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
                ) : null}
              </section>
              {surfaceHost.activeTab !== null ? (
                <CodingSessionSurfaceHost
                  activeSurfaceId={surfaceHost.activeTab}
                  hostId={surfaceHostId}
                  layout={
                    bodyWidthPx <= 0 ? null : isNarrow ? "sheet" : "inline"
                  }
                  onClose={surfaceHost.close}
                  onSelectSurface={surfaceHost.select}
                  surfaces={surfaces}
                  widthContainerRef={workspaceBodyRef}
                />
              ) : null}
            </div>
          </CodingSessionComposerRecipientContext.Provider>
        </CodingSessionOpenHoldsContext.Provider>
      </CodingSessionMissionLensContext.Provider>
      {isNarrow ? <CodingSessionTaskRailSheet dock={composerTaskDock} /> : null}
      {stopAllDialog}
    </main>
  );
}
