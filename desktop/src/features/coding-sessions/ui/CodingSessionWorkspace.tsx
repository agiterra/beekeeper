import * as React from "react";
import { toast } from "sonner";

import {
  buildCodingSessionTargetKey,
  codingSessionTargetSupportsInterrupt,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import type { CodingSessionPopoutBootstrap } from "@/features/coding-sessions/lib/codingSessionBootstrap";
import type { CodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import {
  deriveCodingSessionWorkspaceStatus,
  resolveCodingSessionWorkspace,
  umbrellaHasCollapsedHistory,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { useCodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import { isCodingSessionProducerWriting } from "@/features/coding-sessions/lib/codingSessionProseArriving";
import { useCodingSessionChannelAccess } from "@/features/coding-sessions/hooks/useCodingSessionChannelAccess";
import {
  type CodingSessionChannelAccess,
  codingSessionChannelAccessAllowsSend,
} from "@/features/coding-sessions/lib/codingSessionChannelAccess";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import { resolveCodingSessionUmbrellaComposerAuthority } from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";
import { useCodingSessionOperatorProfiles } from "@/features/coding-sessions/hooks/useCodingSessionOperatorProfiles";
import { useCodingSessionRoster } from "@/features/coding-sessions/lib/codingSessionRoster";
import {
  formatCodingSessionExecutionLabel,
  formatCodingSessionRuntimeLabel,
} from "@/features/coding-sessions/lib/codingSessionLabels";
import { useCodingSessionActorNameResolver } from "@/features/coding-sessions/lib/useCodingSessionActorNames";
import { useCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import { useCodingSessionGoals } from "@/features/coding-sessions/useCodingSessionGoals";
import { selectCodingSessionUmbrellaGoal } from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { deriveCodingSessionGoalReader } from "@/features/coding-sessions/lib/codingSessionGoal";
import type { CodingSessionGoal } from "@/features/coding-sessions/lib/codingSessionGoal";
import { codingSessionNameKey } from "@/features/coding-sessions/lib/codingSessionName";
import type { CodingSessionDisplayName } from "@/features/coding-sessions/lib/codingSessionTitle";
import { useCodingSessionNames } from "@/features/coding-sessions/useCodingSessionNames";
import { codingSessionClosureKey } from "@/features/coding-sessions/lib/codingSessionClosure";
import { useCodingSessionClosures } from "@/features/coding-sessions/useCodingSessionClosures";
import { founderPubkeysByGenesisRef } from "@/features/coding-sessions/lib/codingSessionFoundedModel";
import { useChannelsQuery } from "@/features/channels/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useCodingSessionProject } from "@/features/projects-container/hooks";
import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { useFeatureEnabled } from "@/shared/features/useFeatureEnabled";
import { useAnchoredScroll } from "@/features/messages/ui/useAnchoredScroll";
import { AddCodingSessionProviderDialog } from "./AddCodingSessionProviderDialog";
import { CodingSessionComposer } from "./CodingSessionComposer";
import { CodingSessionPeoplePopover } from "./CodingSessionPeoplePopover";
import { CodingSessionHeader } from "./CodingSessionHeader";
import { CodingSessionHistoryDisclosure } from "./CodingSessionHistoryDisclosure";
import { codingSessionHeaderRepoName } from "./CodingSessionHeaderDetails";
import { useCodingSessionFullAccess } from "./useCodingSessionFullAccess";
import { CodingSessionWorkspaceState } from "./CodingSessionWorkspaceState";
import { CodingSessionFounderLine } from "./CodingSessionFounderLine";
import { CodingSessionHandoverHost } from "./CodingSessionHandoverHost";
import { cn } from "@/shared/lib/cn";

import {
  codingSessionJumpPillPosition,
  useCodingSessionDockReserve,
  useCodingSessionReflow,
  useNarrowCodingSessionWorkspace,
} from "../hooks/useCodingSessionWorkspaceLayout";
import { useCodingSessionColumnGutter } from "../lib/codingSessionWidthPreference";
import { resolveCodingSessionWorkspaceSettlement } from "../lib/codingSessionTranscriptModelSettlement";
import { CodingSessionWorkspaceJumpPill } from "./CodingSessionWorkspaceJumpPill";
import {
  CODING_SESSION_COMPOSER_DOCK_CLASS,
  CODING_SESSION_REFLOW_CLASS,
  CODING_SESSION_SHELL_CLASS,
  CodingSessionColumn,
} from "./CodingSessionColumn";
import { CodingSessionWorkspaceGoalRow } from "./CodingSessionWorkspaceGoalRow";
import { useCodingSessionWorkspaceSessionFacts } from "./CodingSessionWorkspaceSessionFacts";
import { CodingSessionWorkspaceSandboxFooter } from "./CodingSessionWorkspaceSandboxFooter";
import { CodingSessionDetailsContinuityProvider } from "./CodingSessionHeaderDetailsContinuity";
import {
  useCodingSessionSettledSubagents,
  useCodingSessionWorkspaceDerivations,
} from "./CodingSessionWorkspaceDerivations";
import { CodingSessionNameDialog } from "./CodingSessionNameDialog";
import { useCodingSessionExport } from "./useCodingSessionExport";
import { CodingSessionSubagentPage } from "./CodingSessionSubagentPage";
import {
  CodingSessionTaskRail,
  CodingSessionTaskRailSheet,
} from "./CodingSessionTaskRail";
import { CodingSessionSingleWaitingStrip } from "./CodingSessionWaitingStrip";
import { useCodingSessionTaskDock } from "./useCodingSessionTaskDock";
import { buildCodingSessionPromptHistory } from "@/features/coding-sessions/lib/codingSessionPromptHistory";
import {
  CodingSessionTranscript,
  useStableCodingSessionTranscriptModel,
} from "./CodingSessionTranscript";
import {
  CodingSessionPendingTurnList,
  useVisibleCodingSessionPendingTurns,
} from "./CodingSessionPendingTurns";
import { CodingSessionSurfaceHost } from "./CodingSessionSurfaceHost";
import { CodingSessionSurfaceDrawerHost } from "./CodingSessionSurfaceDrawerHost";
import {
  CodingSessionMinimapSlot,
  CodingSessionSurfaceCtxProvider,
} from "./surfaces/codingSessionSurfaceContext";
import { isTranscriptHiddenByPanel } from "./surfaces/useCodingSessionSurfacePanels";
import { useCodingSessionSurfaceShell } from "./surfaces/useCodingSessionSurfacePanelsShell";
import { useCodingSessionSurfaceTeamRead } from "./surfaces/useCodingSessionSurfaceTeamRead";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import { UmbrellaCodingSessionWorkspace } from "./CodingSessionUmbrellaWorkspace";
import { useCodingSessionClosureDialog } from "../hooks/useCodingSessionClosureDialog";
import { useCommunities } from "@/features/communities/useCommunities";
import { normalizeRelayUrl } from "@/shared/lib/normalizeRelayUrl";

type CodingSessionWorkspaceProps = {
  bootstrap?: CodingSessionPopoutBootstrap | null;
  requireBootstrap?: boolean;
  channelId: string;
  generationId: string;
  /** Closes the pop-out window. Absent in the main window, where the app's
   * own back/forward in the top chrome is the way out of a session. */
  onClose?: () => void;
  surface: CodingSessionSurface;
};

export function CodingSessionWorkspace({
  bootstrap = null,
  channelId,
  generationId,
  onClose,
  requireBootstrap = false,
  surface,
}: CodingSessionWorkspaceProps) {
  const [addProviderOpen, setAddProviderOpen] = React.useState(false);
  const identity = useIdentityQuery();
  const { activeCommunity } = useCommunities();
  const communityScope = normalizeRelayUrl(activeCommunity?.relayUrl ?? "");
  const catalog = useCodingSessionCatalog(channelId, bootstrap, {
    requirePopoutBootstrap: requireBootstrap,
    // Viewing is channel-membership authority: any member of the channel sees
    // the same sessions. The local allowlist keeps gating what runs here.
    authorityMode: "open",
  });
  const resolveHandoverReachability =
    useCodingSessionReachabilityResolver(channelId);
  const goalSnapshot = useCodingSessionGoals([channelId]);
  const nameSnapshot = useCodingSessionNames([channelId]);
  const founderByGenesisRef = React.useMemo(
    () =>
      founderPubkeysByGenesisRef({
        entries: catalog.entries,
        creates: catalog.creates,
        geneses: catalog.geneses,
      }),
    [catalog.creates, catalog.entries, catalog.geneses],
  );
  const closureSnapshot = useCodingSessionClosures(
    [channelId],
    founderByGenesisRef,
  );
  const closureDialog = useCodingSessionClosureDialog();
  // Transports included: a project session's workspace lives in a hidden
  // transport channel that the default channel view filters out.
  const channelsQuery = useChannelsQuery({
    enabled: true,
    includeSessionTransports: true,
  });
  const channel =
    channelsQuery.data?.find((candidate) => candidate.id === channelId) ?? null;
  // Channel write access as the relay decides it — membership, or a project
  // owner/collaborator on a transport — kept apart from founder authority.
  const channelAccess = useCodingSessionChannelAccess(channelId);
  const canWriteChannel = codingSessionChannelAccessAllowsSend(channelAccess);
  const resolution = resolveCodingSessionWorkspace({
    catalog,
    generationId,
  });

  // Roster hooks run unconditionally (before the not-ready return) per the
  // Rules of Hooks; the query enables itself only once a genesis is known.
  const readyUmbrella =
    resolution.kind === "ready" ? resolution.umbrella : null;
  const rosterQuery = useCodingSessionRoster(
    channelId,
    readyUmbrella?.genesisRef ?? null,
    readyUmbrella?.founderPubkey ?? null,
  );
  // Same rule: called unconditionally, fed `null` before a session resolves.
  // LANE-L25: the join dialog's seat field needs the same project coordinate
  // the header crumb resolves (`ReadyCodingSessionWorkspace` below), so its
  // pack preview (LANE-L23) can resolve — see `useCodingSessionProject`.
  const joinProject = useCodingSessionProject(
    channelId,
    resolution.kind === "ready" ? resolution.session.projectRef : null,
  );
  const [peopleOpen, setPeopleOpen] = React.useState(false);
  // Live (non-pending) collaborator grants — the composer's operator set.
  // `null` while the roster is unknown, so authority keeps its founder-only
  // fallback rather than treating "not loaded yet" as "nobody is granted".
  const acceptedOperators = React.useMemo(
    () =>
      rosterQuery.data
        ? new Set(
            rosterQuery.data
              .filter(
                (entry) => entry.role === "collaborator" && !entry.pending,
              )
              .map((entry) => entry.pubkey),
          )
        : null,
    [rosterQuery.data],
  );

  if (resolution.kind !== "ready") {
    return (
      <CodingSessionWorkspaceState
        channelName={channel?.name ?? null}
        generationId={generationId}
        onClose={onClose}
        resolution={resolution}
      />
    );
  }

  // A1: the reader's own condition, threaded to the one surface that renders
  // a sentence about it. `errorMessage` has existed since this hook was
  // written and nothing read it.
  const goalReader = deriveCodingSessionGoalReader(goalSnapshot);
  const umbrella = resolution.umbrella;
  // Finding 23: this used to be an exact-key `Map.get`, and a published goal
  // that any one of the three keys spelled differently simply vanished — the
  // Inspector said `No accepted mission goal published` over a goal that was
  // on the wire. One case-folded selection, and a foreign-signed goal is its
  // own answer rather than silence.
  const goalSelection = selectCodingSessionUmbrellaGoal({
    channelId,
    founderPubkey: umbrella.founderPubkey,
    goals: goalSnapshot.goals.values(),
    sessionRef: umbrella.sessionRef,
  });
  const goal = goalSelection.kind === "available" ? goalSelection.goal : null;
  const sessionName =
    umbrella.sessionRef && umbrella.founderPubkey
      ? // The effective name: the founder's 44229, else the provider's
        // standing 44252 (SV-31, S2). The header breadcrumb marks a
        // generated one "Auto-named" (SV-70), and the rename dialog states
        // who generated it and with which model.
        (nameSnapshot.names.get(
          codingSessionNameKey(
            channelId,
            umbrella.sessionRef,
            umbrella.founderPubkey,
          ),
        ) ?? null)
      : null;
  const closure =
    umbrella.sessionRef && umbrella.genesisRef
      ? (closureSnapshot.closures.get(
          codingSessionClosureKey(
            channelId,
            umbrella.sessionRef,
            umbrella.genesisRef,
          ),
        ) ?? null)
      : null;
  // Archived is a close with a filing cabinet: settled either way.
  const sessionClosed =
    closure?.action === "closed" || closure?.action === "archived";
  const canCloseSession =
    canWriteChannel &&
    !sessionClosed &&
    umbrella.sessionRef !== null &&
    umbrella.genesisRef !== null &&
    umbrella.founderPubkey !== null &&
    identity.data?.pubkey.toLowerCase() ===
      umbrella.founderPubkey.toLowerCase();
  const canReopenSession =
    canWriteChannel &&
    sessionClosed &&
    umbrella.sessionRef !== null &&
    umbrella.genesisRef !== null;
  const requestClosure = (action: "closed" | "open") => {
    if (!umbrella.sessionRef || !umbrella.genesisRef) return;
    closureDialog.requestClosure({
      action,
      channelId,
      genesisRef: umbrella.genesisRef,
      label: sessionName?.content ?? umbrella.title,
      sessionRef: umbrella.sessionRef,
    });
  };
  // Joining needs a claimed umbrella ref to join *to* (a pre-Step-4 session has
  // none, so it gets no affordance rather than a button that cannot work), and
  // v1 authority is founder-only — when an observed create binds a founder who
  // is not this user, the attach UI is absent exactly as the design specifies.
  const canAddProvider =
    !sessionClosed &&
    canWriteChannel &&
    umbrella.sessionRef !== null &&
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella,
      currentUserPubkey: identity.data?.pubkey ?? null,
    }).canPromptExecutions;
  const onAddProvider = canAddProvider
    ? () => setAddProviderOpen(true)
    : undefined;
  // The People surface exists exactly when the session has an authority
  // chain to share — a genesis. Legacy no-genesis sessions have no roster.
  const onOpenPeople =
    umbrella.genesisRef !== null ? () => setPeopleOpen(true) : undefined;
  const peopleCount = rosterQuery.data?.length ?? 0;

  return (
    <div
      className={CODING_SESSION_SHELL_CLASS}
      data-testid="coding-session-shell"
    >
      {/* Stable across render branches, including in-flight continuations. */}
      <div className="shrink-0" data-testid="coding-session-handover-slot">
        <CodingSessionHandoverHost
          channelId={channelId}
          focusedExecution={resolution.focusedExecution}
          projectRef={joinProject?.address ?? null}
          resolveReachability={resolveHandoverReachability}
          title={sessionName?.content ?? umbrella.title}
          umbrella={umbrella}
        />
      </div>
      {/* Above both branches: a transcript still paging back, or one that
          stopped short, must not read as the whole session (SV-116). */}
      {catalog.historyCompleteness ? (
        <div className="shrink-0">
          <CodingSessionHistoryDisclosure
            completeness={catalog.historyCompleteness}
          />
        </div>
      ) : null}
      {/* The umbrella surface is a render branch, not a mode: an umbrella with
          no collapsed history falls through to exactly today's single-session
          tree. Routing on collapsed history rather than execution count is
          what lets a resumed session — one execution, several generations —
          reach the only view that renders its earlier turns. */}
      {umbrellaHasCollapsedHistory(umbrella) ? (
        <UmbrellaCodingSessionWorkspace
          catalogSettled={!catalog.isLoading}
          resolveReachability={resolveHandoverReachability}
          channelId={channelId}
          channelName={channel?.name ?? null}
          communityScope={communityScope}
          focusedExecution={resolution.focusedExecution}
          generationId={generationId}
          channelAccess={channelAccess}
          currentUserPubkey={identity.data?.pubkey ?? null}
          key={`${channelId}:${umbrella.umbrellaKey}`}
          acceptedOperators={acceptedOperators}
          onAddProvider={onAddProvider}
          onCloseSession={
            canCloseSession ? () => requestClosure("closed") : undefined
          }
          onReopenSession={
            canReopenSession ? () => requestClosure("open") : undefined
          }
          onClose={onClose}
          onOpenPeople={onOpenPeople}
          peopleCount={peopleCount}
          surface={surface}
          umbrella={umbrella}
          goal={goal}
          goalReader={goalReader}
          sessionName={sessionName}
          sessionClosed={sessionClosed}
          turnStartedAtFor={catalog.turnStartedAtFor}
        />
      ) : (
        <ReadyCodingSessionWorkspace
          channelId={channelId}
          communityScope={communityScope}
          resolveReachability={resolveHandoverReachability}
          channelName={channel?.name ?? null}
          generationId={generationId}
          channelAccess={channelAccess}
          key={`${channelId}:${generationId}`}
          acceptedOperators={acceptedOperators}
          onAddProvider={onAddProvider}
          onCloseSession={
            canCloseSession ? () => requestClosure("closed") : undefined
          }
          onReopenSession={
            canReopenSession ? () => requestClosure("open") : undefined
          }
          onClose={onClose}
          onOpenPeople={onOpenPeople}
          peopleCount={peopleCount}
          founderPubkey={umbrella.founderPubkey}
          genesisRef={umbrella.genesisRef}
          goal={goal}
          sessionName={sessionName}
          sessionClosed={sessionClosed}
          currentUserPubkey={identity.data?.pubkey ?? null}
          sessionRef={umbrella.sessionRef}
          session={resolution.session}
          surface={surface}
          umbrella={umbrella}
        />
      )}
      {/* Deliberately a sibling of both branches: the first join flips the
          workspace from the single-session tree to the umbrella surface, and
          a dialog owned by either branch would unmount mid-create — losing the
          receipt wait and stranding its durable transaction. */}
      {canAddProvider ? (
        <AddCodingSessionProviderDialog
          channelId={channelId}
          channelName={channel?.name ?? null}
          onOpenChange={setAddProviderOpen}
          open={addProviderOpen}
          projectRef={joinProject?.address ?? null}
          umbrella={
            sessionName ? { ...umbrella, title: sessionName.content } : umbrella
          }
        />
      ) : null}
      {closureDialog.dialog}
      {/* Same sibling reasoning as the provider dialog: the People surface
          must survive the single-session → umbrella branch flip. */}
      {umbrella.genesisRef !== null ? (
        <CodingSessionPeoplePopover
          channelId={channelId}
          founderPubkey={umbrella.founderPubkey}
          genesisRef={umbrella.genesisRef}
          onOpenChange={setPeopleOpen}
          open={peopleOpen}
        />
      ) : null}
    </div>
  );
}

function ReadyCodingSessionWorkspace({
  acceptedOperators,
  channelId,
  communityScope,
  resolveReachability: sharedResolveReachability,
  channelName,
  generationId,
  founderPubkey,
  genesisRef,
  goal,
  sessionName,
  sessionClosed,
  currentUserPubkey,
  sessionRef,
  channelAccess,
  onAddProvider,
  onCloseSession,
  onReopenSession,
  onClose,
  onOpenPeople,
  peopleCount,
  session,
  surface,
  umbrella,
}: {
  acceptedOperators: ReadonlySet<string> | null;
  channelId: string;
  communityScope: string;
  resolveReachability?: CodingSessionReachabilityResolver;
  channelName: string | null;
  generationId: string;
  founderPubkey: string | null;
  genesisRef: string | null;
  goal: CodingSessionGoal | null;
  sessionName: CodingSessionDisplayName | null;
  sessionClosed: boolean;
  currentUserPubkey: string | null;
  sessionRef: string | null;
  channelAccess: CodingSessionChannelAccess;
  onAddProvider?: () => void;
  onCloseSession?: () => void;
  onReopenSession?: () => void;
  /** Closes the pop-out window. Absent in the main window, where the app's
   * own back/forward in the top chrome is the way out of a session. */
  onClose?: () => void;
  onOpenPeople?: () => void;
  peopleCount: number;
  session: Extract<
    ReturnType<typeof resolveCodingSessionWorkspace>,
    { kind: "ready" }
  >["session"];
  surface: CodingSessionSurface;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const gutter = useCodingSessionColumnGutter();
  const resolveActorName = useCodingSessionActorNameResolver(umbrella);
  const fullAccess = useCodingSessionFullAccess({ channelId, record: session });
  const workspaceRef = React.useRef<HTMLElement>(null);
  const scrollRef = React.useRef<HTMLDivElement>(null);
  const contentRef = React.useRef<HTMLDivElement>(null);
  const narrowState = useNarrowCodingSessionWorkspace(workspaceRef);
  const isNarrow = narrowState === true;
  const runtimeLabel = React.useMemo(() => {
    const runtime = session.runtime ?? session.provider;
    return runtime ? formatCodingSessionRuntimeLabel(runtime) : null;
  }, [session.provider, session.runtime]);
  const providerLabel = React.useMemo(
    () =>
      session.provider
        ? formatCodingSessionRuntimeLabel(session.provider)
        : null,
    [session.provider],
  );
  // A seated execution wears its seat in the header. Half a seat is not a
  // seat: `formatCodingSessionExecutionLabel` refuses an actor with no role,
  // and an unresolved name falls back to the role rather than a truncated key.
  const seatLabel = React.useMemo(() => {
    if (!session.agentRef || !session.role) return null;
    return formatCodingSessionExecutionLabel({
      agentRef: session.agentRef,
      role: session.role,
      agentDisplayName: resolveActorName(session.agentRef),
      runtime: null,
      model: null,
    }).primary;
  }, [resolveActorName, session.agentRef, session.role]);
  const composerAuthority = resolveCodingSessionUmbrellaComposerAuthority({
    umbrella: { founderPubkey, genesisRef },
    currentUserPubkey,
    acceptedOperators,
  });
  // Every transcript-wide derivation, memoized on a reconciled transcript: a
  // re-projection that changed no item (a status event, another execution's
  // event) keeps the same array, so none of them re-runs; an append keeps
  // every earlier item's identity for the memoized rows downstream.
  const {
    contextWindow,
    messages,
    observedChanges,
    subagents,
    taskModel,
    transcript,
  } = useCodingSessionWorkspaceDerivations(session.transcript);
  const operatorProfiles = useCodingSessionOperatorProfiles(
    transcript,
    currentUserPubkey,
  );
  const commandTargetKey = React.useMemo(
    () =>
      session.commandTarget
        ? buildCodingSessionTargetKey(session.commandTarget)
        : null,
    [session.commandTarget],
  );
  // Turns published from this client that the provider has not echoed yet.
  // Resolved here rather than inside the row list because the transcript's
  // "No conversation yet" empty state has to yield to them.
  const pendingTurns = useVisibleCodingSessionPendingTurns({
    channelId,
    echoes: transcript,
    targetKey: commandTargetKey,
  });
  // This operator's own earlier prompts, for ⌘↑/⌘↓ recall in the composer.
  const promptHistory = React.useMemo(
    () =>
      buildCodingSessionPromptHistory({
        transcript,
        pending: pendingTurns.turns,
        currentPubkey: currentUserPubkey,
      }),
    [currentUserPubkey, pendingTurns.turns, transcript],
  );
  const [renameOpen, setRenameOpen] = React.useState(false);
  const authoritativeTitle = sessionName?.content ?? session.title;
  const isFounder =
    founderPubkey !== null &&
    currentUserPubkey?.toLowerCase() === founderPubkey.toLowerCase();
  const canRename = sessionRef !== null && isFounder;
  const canStopExecution =
    isFounder ||
    (founderPubkey === null &&
      genesisRef === null &&
      composerAuthority.isUngovernedSession &&
      composerAuthority.canPromptExecutions);
  const { isAtBottom, newMessageCount, onScroll, scrollToBottom } =
    useAnchoredScroll({
      channelId: `${channelId}:${generationId}`,
      contentRef,
      // The transcript virtualizes past 40 rows and corrects scrollTop as rows
      // measure; only the reader scrolling up may switch follow-latest off.
      holdBottomUntilReaderScrolls: true,
      isLoading: false,
      messages,
      scrollContainerRef: scrollRef,
    });
  // Borrowed from the workspace: one coordination subscription per session.
  const ownReachability = useCodingSessionReachabilityResolver(
    sharedResolveReachability ? null : channelId,
  );
  const resolveReachability = sharedResolveReachability ?? ownReachability;
  const reachability = resolveReachability(session.commandTarget);
  const status = deriveCodingSessionWorkspaceStatus(
    transcript,
    session.status,
    session.statusAt,
    reachability,
  );
  const surfaceHostId = React.useId();
  const isWorking = status.kind === "working";
  // SV-42/43: the transcript settles through Mission's map, so one signed
  // status reads the same in both views; the header keeps its own labels.
  const settlement = resolveCodingSessionWorkspaceSettlement({
    wireStatus: session.status,
    workspaceStatus: status,
  });
  // One model, shared with the transcript: Details and the chip read its
  // `sessionFacts` without a second pass per streamed item.
  const transcriptModel = useStableCodingSessionTranscriptModel(
    transcript,
    settlement.isWorking,
    undefined,
    // SV-36 rule 7: "Writing…" from this generation's lease, never isWorking.
    // A prior generation is never `provider_reachable`, so none is superseded.
    isCodingSessionProducerWriting({
      reachability,
      status: session.status,
      generationSuperseded: false,
    }),
  );
  // The Agents surface reads each spawn through the turn settlement its
  // stream row reads it through, so the two never disagree side by side.
  const settledSubagents = useCodingSessionSettledSubagents(
    subagents,
    transcriptModel.blocks,
    settlement.restingStatus,
  );
  const sessionFacts = useCodingSessionWorkspaceSessionFacts(
    transcriptModel.sessionFacts,
    fullAccess,
  );
  const taskDock = useCodingSessionTaskDock({
    isNarrow,
    isWorking,
    model: taskModel,
    transcript,
  });
  // The dock overlays the transcript; the column reserves its measured
  // height. See the hook for what the old constant cost.
  const dockReserve = useCodingSessionDockReserve(
    taskDock.open && !isNarrow && "pb-[34rem]",
  );
  const reflow = useCodingSessionReflow(workspaceRef, dockReserve.ref);
  // Use the sidebar's project resolution for the breadcrumb too.
  const { goProject } = useAppNavigation();
  const owningProject = useCodingSessionProject(channelId, session.projectRef);
  const teamRead = useCodingSessionSurfaceTeamRead({
    channelId,
    currentUserPubkey,
    resolveActorName,
    umbrella,
  });
  const shell = useCodingSessionSurfaceShell({
    layout: "single",
    channelId,
    communityScope,
    umbrella,
    focusedExecution:
      umbrella.executions.find(
        (execution) =>
          execution.activeGeneration.generationId === session.generationId,
      ) ?? null,
    lens: "conversation",
    transcript,
    transcriptModel,
    umbrellaTimeline: null,
    observedChanges,
    subagents: settledSubagents,
    taskModel,
    currentUserPubkey,
    resolveActorName,
    resolveReachability,
    sessionClosed,
    observations: teamRead.observations,
    openRulings: teamRead.openRulings,
    decisions: teamRead.decisions,
    decisionRequests: teamRead.decisionRequests,
    teamTransactions: teamRead.teamTransactions,
    mission: null,
    onOpenPeople,
  });
  const { state: panelState } = shell.panels;
  const narrativeExpanded = !panelState.rightOpen;
  // SV-14: the pill rides the measured dock, so it never lands on the composer.
  const jumpPill = codingSessionJumpPillPosition({
    dockHeight: dockReserve.dockHeight,
    hasDock: Boolean(session.commandTarget) && !sessionClosed,
  });

  const exportEnabled = useFeatureEnabled("coding-session-export");
  const exportSession = React.useMemo(
    () => (sessionName ? { ...session, title: authoritativeTitle } : session),
    [authoritativeTitle, session, sessionName],
  );
  const { exportTranscript, isExporting } = useCodingSessionExport(
    generationId,
    exportSession,
  );

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
      className={cn(
        "relative flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-background",
        reflow.active && CODING_SESSION_REFLOW_CLASS,
      )}
      data-reflow={reflow.active ? "true" : undefined}
      data-testid="coding-session-workspace"
      ref={workspaceRef}
    >
      <div className="shrink-0" data-testid="coding-session-authority-summary">
        <CodingSessionDetailsContinuityProvider value={sessionFacts.continuity}>
          <CodingSessionHeader
            channelName={channelName}
            compact={isNarrow}
            // Keep the provenance popover consistent with the visible founder.
            founderDetails={
              founderPubkey ? (
                <CodingSessionFounderLine
                  founderPubkey={founderPubkey}
                  genesisRef={genesisRef}
                  variant="label"
                />
              ) : undefined
            }
            generationLabel={session.label}
            goalText={goal?.content ?? null}
            repoName={codingSessionHeaderRepoName(session.repoRef)}
            seat={seatLabel ? { label: seatLabel } : null}
            fullAccess={fullAccess}
            isExporting={isExporting}
            model={session.model}
            onAddProvider={onAddProvider}
            onClose={onClose}
            onCloseSession={onCloseSession}
            onExport={exportEnabled ? exportTranscript : undefined}
            onOpenPeople={onOpenPeople}
            peopleCount={peopleCount}
            onOpenProject={
              // A pop-out is its own window with no app shell to navigate; the
              // project still shows, it just is not a link there.
              owningProject && surface === "main"
                ? () => void goProject(owningProject.id)
                : undefined
            }
            onPopout={surface === "main" ? handlePopout : undefined}
            onRename={canRename ? () => setRenameOpen(true) : undefined}
            onReopenSession={onReopenSession}
            projectName={owningProject?.name ?? null}
            providerAuthorityPubkey={session.providerAuthorityPubkey}
            runtimeLabel={runtimeLabel}
            sessionTitle={authoritativeTitle}
            sessionTitleOrigin={sessionName}
            sessionClosed={sessionClosed}
            status={status}
            surfaceHostId={surfaceHostId}
            surfaceShell={shell}
            workspaceReuse={{
              channelId,
              sessionRef,
              sourceRepoRef: session.repoRef ?? null,
            }}
          />
        </CodingSessionDetailsContinuityProvider>
      </div>
      {sessionRef ? (
        <CodingSessionNameDialog
          channelId={channelId}
          currentName={authoritativeTitle}
          onOpenChange={setRenameOpen}
          open={renameOpen}
          sessionRef={sessionRef}
        />
      ) : null}
      <CodingSessionSurfaceCtxProvider value={shell.ctx}>
        <div className="flex min-h-0 flex-1" data-testid="coding-session-body">
          <section
            aria-label="Session transcript"
            data-testid="coding-session-transcript-pane"
            className={cn(
              "relative flex min-w-0 flex-1 flex-col overflow-hidden",
              isTranscriptHiddenByPanel(panelState, isNarrow) && "hidden",
            )}
          >
            {/* SV-21: the transcript and its composer overlay sit above the
              drawer, so the composer's bottom-0 anchors over it. */}
            <div
              className="relative flex min-h-0 flex-1 flex-col"
              data-testid="coding-session-transcript-region"
            >
              <CodingSessionMinimapSlot slotRef={shell.minimapSlotRef} />
              <CodingSessionSubagentPage />
              <div
                className={cn(gutter, "pb-2")}
                data-testid="coding-session-goal-slot"
              >
                {/* One quiet row: the goal (withheld when it only restates the
                title) and the founder. The founder is also in the header's
                provenance popover, one click away, when this row is empty. */}
                <CodingSessionWorkspaceGoalRow
                  channelId={channelId}
                  currentUserPubkey={currentUserPubkey}
                  founderPubkey={founderPubkey}
                  genesisRef={genesisRef}
                  goal={goal}
                  sessionRef={sessionRef}
                  title={authoritativeTitle}
                  workspaceExpanded={narrativeExpanded}
                />
              </div>
              <div
                className={cn(
                  "min-h-0 flex-1 overflow-x-hidden overflow-y-auto overscroll-contain",
                  gutter,
                )}
                data-testid="coding-session-transcript-scroll"
                onScroll={onScroll}
                ref={scrollRef}
                style={reflow.active ? { height: reflow.height } : undefined}
              >
                <CodingSessionColumn
                  className={cn(
                    "min-h-full pt-7",
                    reflow.active ? "pb-4" : dockReserve.className,
                  )}
                  expanded={narrativeExpanded}
                  style={reflow.active ? undefined : dockReserve.style}
                >
                  <div className="flex min-w-0 flex-col gap-5" ref={contentRef}>
                    {/* "No conversation yet" is false the moment a turn is in
                    flight, so the empty state stands down for the pending row
                    rather than sitting above it. */}
                    {transcript.length > 0 ||
                    pendingTurns.turns.length === 0 ? (
                      <CodingSessionTranscript
                        currentUserPubkey={currentUserPubkey}
                        generationId={generationId}
                        isWorking={settlement.isWorking}
                        items={transcript}
                        lastTranscriptEventAt={session.lastTranscriptAt}
                        model={transcriptModel}
                        onOpenAgentsSurface={shell.openAgentsSurface}
                        operatorProfiles={operatorProfiles}
                        restingStatus={settlement.restingStatus}
                        scrollRef={scrollRef}
                      />
                    ) : null}
                    <CodingSessionPendingTurnList
                      now={pendingTurns.now}
                      turns={pendingTurns.turns}
                    />
                  </div>
                </CodingSessionColumn>
              </div>
              {!isAtBottom ? (
                <CodingSessionWorkspaceJumpPill
                  inFlow={reflow.active}
                  newCount={newMessageCount}
                  onJump={() => scrollToBottom("smooth")}
                  position={jumpPill}
                />
              ) : null}
              {session.commandTarget && !sessionClosed ? (
                <div
                  className={cn(CODING_SESSION_COMPOSER_DOCK_CLASS, gutter)}
                  data-testid="coding-session-composer-dock"
                  ref={dockReserve.ref}
                >
                  <CodingSessionColumn
                    className="pointer-events-auto"
                    expanded={narrativeExpanded}
                  >
                    {taskDock.open && !isNarrow ? (
                      <div className="-mb-6">
                        <CodingSessionTaskRail
                          model={taskDock.activeModel}
                          onClose={taskDock.close}
                          variant="dock"
                        />
                      </div>
                    ) : null}
                    <CodingSessionSingleWaitingStrip
                      observations={teamRead.observations}
                      session={session}
                      status={status}
                    />
                    <div className="relative z-10">
                      <CodingSessionComposer
                        authorityReason={composerAuthority.reason}
                        authorityUnresolved={composerAuthority.isUnresolved}
                        canInterrupt={
                          codingSessionTargetSupportsInterrupt(
                            session.commandTarget,
                          ) &&
                          session.capabilities?.threadTurnInterrupt !== false
                        }
                        canControl={composerAuthority.canPromptExecutions}
                        canStopExecution={canStopExecution}
                        canSteer={session.capabilities?.threadSteer === true}
                        canAttachImages={
                          session.capabilities?.promptImage === true
                        }
                        runtimeLabel={runtimeLabel}
                        channelId={channelId}
                        controlContext={{
                          capabilities: session.capabilities,
                          model: session.model,
                          providerLabel,
                          runtimeLabel,
                          sandbox: sessionFacts.sandbox,
                          status,
                          turnBudget: session.turnBudget,
                        }}
                        contextWindow={contextWindow}
                        currentUserPubkey={currentUserPubkey}
                        immersive
                        channelAccess={channelAccess}
                        onAddProvider={onAddProvider}
                        isWorking={isWorking}
                        isUngovernedSession={
                          composerAuthority.isUngovernedSession
                        }
                        lifecycleStatus={session.status}
                        layout={isNarrow ? "stacked" : "inline"}
                        promptHistory={promptHistory}
                        providerAuthorityPubkey={
                          session.providerAuthorityPubkey
                        }
                        seatActorPubkey={session.agentRef}
                        seatRole={session.role}
                        projectRef={session.projectRef}
                        sessionLabel={session.title}
                        target={session.commandTarget}
                        variant="floating"
                      />
                    </div>
                  </CodingSessionColumn>
                </div>
              ) : (
                <CodingSessionWorkspaceSandboxFooter
                  sandbox={sessionFacts.sandbox}
                  sessionClosed={sessionClosed}
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
                narrowState === null ? null : narrowState ? "sheet" : "inline"
              }
              panels={shell.panels}
              surfaces={shell.surfaces}
              widthContainerRef={workspaceRef}
            />
          ) : null}
        </div>
      </CodingSessionSurfaceCtxProvider>
      {isNarrow ? <CodingSessionTaskRailSheet dock={taskDock} /> : null}
    </main>
  );
}
