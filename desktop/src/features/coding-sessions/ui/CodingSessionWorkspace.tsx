import * as React from "react";
import { ArrowDown, CircleAlert } from "lucide-react";
import { toast } from "sonner";

import {
  buildCodingSessionTargetKey,
  codingSessionTargetSupportsInterrupt,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import { deriveTranscriptItemBlockIds } from "@/features/agents/ui/agentSessionTranscriptGrouping";
import type { CodingSessionPopoutBootstrap } from "@/features/coding-sessions/lib/codingSessionBootstrap";
import type { CodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import {
  deriveCodingSessionWorkspaceStatus,
  resolveCodingSessionWorkspace,
  umbrellaHasCollapsedHistory,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { useCodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import type { CodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import { resolveCodingSessionUmbrellaComposerAuthority } from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";
import { useCodingSessionOperatorProfiles } from "@/features/coding-sessions/hooks/useCodingSessionOperatorProfiles";
import { useCodingSessionRoster } from "@/features/coding-sessions/lib/codingSessionRoster";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { deriveCodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import {
  formatCodingSessionExecutionLabel,
  formatCodingSessionRuntimeLabel,
} from "@/features/coding-sessions/lib/codingSessionLabels";
import { useCodingSessionActorNameResolver } from "@/features/coding-sessions/lib/useCodingSessionActorNames";
import { useCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import { useCodingSessionGoals } from "@/features/coding-sessions/useCodingSessionGoals";
import { selectCodingSessionUmbrellaGoal } from "@/features/coding-sessions/lib/codingSessionMissionInspectorModel";
import { deriveCodingSessionGoalReader } from "@/features/coding-sessions/lib/codingSessionGoal";
import { codingSessionNameKey } from "@/features/coding-sessions/lib/codingSessionName";
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
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";
import { Button } from "@/shared/ui/button";
import { FuzzyLogo } from "@/shared/ui/buzz-logo/FuzzyLogo";
import { Sheet, SheetContent, SheetTitle } from "@/shared/ui/sheet";
import { AddCodingSessionProviderDialog } from "./AddCodingSessionProviderDialog";
import { CodingSessionComposer } from "./CodingSessionComposer";
import { CodingSessionPeoplePopover } from "./CodingSessionPeoplePopover";
import { CodingSessionHeader } from "./CodingSessionHeader";
import { CodingSessionFounderLine } from "./CodingSessionFounderLine";
import { CodingSessionHandoverHost } from "./CodingSessionHandoverHost";
import { cn } from "@/shared/lib/cn";

import {
  useCodingSessionDockReserve,
  useCodingSessionReflow,
  useNarrowCodingSessionWorkspace,
} from "../hooks/useCodingSessionWorkspaceLayout";
import { useCodingSessionColumnGutter } from "../lib/codingSessionWidthPreference";
import {
  CODING_SESSION_COMPOSER_DOCK_CLASS,
  CODING_SESSION_REFLOW_CLASS,
  CODING_SESSION_SHELL_CLASS,
  CodingSessionColumn,
} from "./CodingSessionColumn";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";
import { CodingSessionNameDialog } from "./CodingSessionNameDialog";
import { useCodingSessionExport } from "./useCodingSessionExport";
import { CodingSessionTaskRail } from "./CodingSessionTaskRail";
import { useCodingSessionTaskDock } from "./useCodingSessionTaskDock";
import { buildCodingSessionPromptHistory } from "@/features/coding-sessions/lib/codingSessionPromptHistory";
import { CodingSessionTranscript } from "./CodingSessionTranscript";
import {
  CodingSessionPendingTurnList,
  useVisibleCodingSessionPendingTurns,
} from "./CodingSessionPendingTurns";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail";
import {
  CodingSessionSurfaceHost,
  useCodingSessionSurfaceHostState,
  type CodingSessionSurfaceDescriptor,
} from "./CodingSessionSurfaceHost";
import { deriveCodingSessionObservedChanges } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
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
  const isMember = channel?.isMember ?? false;
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
      ? (nameSnapshot.names.get(
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
    isMember &&
    !sessionClosed &&
    umbrella.sessionRef !== null &&
    umbrella.genesisRef !== null &&
    umbrella.founderPubkey !== null &&
    identity.data?.pubkey.toLowerCase() ===
      umbrella.founderPubkey.toLowerCase();
  const canReopenSession =
    isMember &&
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
    isMember &&
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
          isMember={isMember}
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
          resolveReachability={resolveHandoverReachability}
          channelName={channel?.name ?? null}
          generationId={generationId}
          isMember={isMember}
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
  isMember,
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
  resolveReachability?: CodingSessionReachabilityResolver;
  channelName: string | null;
  generationId: string;
  founderPubkey: string | null;
  genesisRef: string | null;
  goal:
    | import("@/features/coding-sessions/lib/codingSessionGoal").CodingSessionGoal
    | null;
  sessionName:
    | import("@/features/coding-sessions/lib/codingSessionName").CodingSessionName
    | null;
  sessionClosed: boolean;
  currentUserPubkey: string | null;
  sessionRef: string | null;
  isMember: boolean;
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
  const operatorProfiles = useCodingSessionOperatorProfiles(
    session.transcript,
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
    echoes: session.transcript,
    targetKey: commandTargetKey,
  });
  // This operator's own earlier prompts, for ⌘↑/⌘↓ recall in the composer.
  const promptHistory = React.useMemo(
    () =>
      buildCodingSessionPromptHistory({
        transcript: session.transcript,
        pending: pendingTurns.turns,
        currentPubkey: currentUserPubkey,
      }),
    [currentUserPubkey, pendingTurns.turns, session.transcript],
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
  const blockIds = React.useMemo(
    () => deriveTranscriptItemBlockIds(session.transcript),
    [session.transcript],
  );
  const stableBlockIds = useStableArrayShallow(blockIds);
  const messages = React.useMemo(
    () => stableBlockIds.map((id) => ({ id })),
    [stableBlockIds],
  );
  const { isAtBottom, newMessageCount, onScroll, scrollToBottom } =
    useAnchoredScroll({
      channelId: `${channelId}:${generationId}`,
      contentRef,
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
    session.transcript,
    session.status,
    session.statusAt,
    reachability,
  );
  const taskModel = React.useMemo(
    () => deriveCodingSessionTaskModel(session.transcript),
    [session.transcript],
  );
  const contextWindow = React.useMemo(
    () => deriveCodingSessionContextWindow(session.transcript),
    [session.transcript],
  );
  const observedChanges = React.useMemo(
    () => deriveCodingSessionObservedChanges(session.transcript),
    [session.transcript],
  );
  const changedFiles = observedChanges.files;
  // Surfaces offered by current data: Observed changes always applies to a
  // transcript; Agents only when the umbrella model actually provides
  // participants (it lists one per signed execution) — never an empty tab.
  const surfaces = React.useMemo<CodingSessionSurfaceDescriptor[]>(
    () => [
      ...(umbrella.executions.length > 0
        ? [
            {
              id: "agents",
              label: "Agents",
              count: umbrella.executions.length,
              content: (
                <CodingSessionExecutionRail
                  resolveReachability={resolveReachability}
                  umbrella={umbrella}
                />
              ),
            },
          ]
        : []),
      {
        id: "changes",
        label: "Observed changes",
        count: changedFiles.length,
        content: (
          <CodingSessionChangesRail
            files={changedFiles}
            unreportedEditCount={observedChanges.unreportedEditCount}
          />
        ),
      },
    ],
    [
      changedFiles,
      observedChanges.unreportedEditCount,
      resolveReachability,
      umbrella,
    ],
  );
  const surfaceIds = React.useMemo(
    () => surfaces.map((surfaceEntry) => surfaceEntry.id),
    [surfaces],
  );
  const surfaceHost = useCodingSessionSurfaceHostState(surfaceIds);
  const surfaceHostId = React.useId();
  const isWorking = status.kind === "working";
  const taskDock = useCodingSessionTaskDock({
    isNarrow,
    isWorking,
    model: taskModel,
    transcript: session.transcript,
  });
  // The dock overlays the transcript; the column reserves its measured
  // height. See the hook for what the old constant cost.
  const dockReserve = useCodingSessionDockReserve(
    taskDock.open && !isNarrow && "pb-[34rem]",
  );
  const reflow = useCodingSessionReflow(workspaceRef, dockReserve.ref);
  const narrativeExpanded = surfaceHost.activeTab === null;

  // Use the sidebar's project resolution for the breadcrumb too.
  const { goProject } = useAppNavigation();
  const owningProject = useCodingSessionProject(channelId, session.projectRef);

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
          seat={seatLabel ? { label: seatLabel } : null}
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
          onToggleTaskRail={
            taskDock.activeModel
              ? () => {
                  surfaceHost.close();
                  taskDock.toggle();
                }
              : undefined
          }
          onToggleSurface={(id) => {
            taskDock.close();
            surfaceHost.toggle(id);
          }}
          projectName={owningProject?.name ?? null}
          providerAuthorityPubkey={session.providerAuthorityPubkey}
          runtimeLabel={runtimeLabel}
          sessionTitle={authoritativeTitle}
          sessionClosed={sessionClosed}
          status={status}
          surfaceHostId={surfaceHostId}
          surfaceTabs={surfaces.map((surfaceEntry) => ({
            id: surfaceEntry.id,
            label: surfaceEntry.label,
            icon: surfaceEntry.id === "agents" ? "agents" : "changes",
            count: surfaceEntry.count ?? 0,
            active: surfaceHost.activeTab === surfaceEntry.id,
          }))}
          taskCount={taskDock.activeModel?.tasks.length ?? 0}
          taskRailOpen={taskDock.open}
          workspaceReuse={{
            channelId,
            sessionRef,
            sourceRepoRef: session.repoRef ?? null,
          }}
        />
        <CodingSessionFounderLine
          founderPubkey={founderPubkey}
          genesisRef={genesisRef}
        />
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
      <div className="flex min-h-0 flex-1" data-testid="coding-session-body">
        <section
          aria-label="Session transcript"
          data-testid="coding-session-transcript-pane"
          className="relative flex min-w-0 flex-1 flex-col overflow-hidden"
        >
          <div
            className={cn(gutter, "pb-2")}
            data-testid="coding-session-goal-slot"
          >
            <CodingSessionGoalPill
              channelId={channelId}
              currentUserPubkey={currentUserPubkey}
              founderPubkey={founderPubkey}
              goal={goal}
              sessionRef={sessionRef}
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
                {session.transcript.length > 0 ||
                pendingTurns.turns.length === 0 ? (
                  <CodingSessionTranscript
                    currentUserPubkey={currentUserPubkey}
                    generationId={generationId}
                    isWorking={isWorking}
                    items={session.transcript}
                    operatorProfiles={operatorProfiles}
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
            <div
              className={
                reflow.active
                  ? "flex justify-center py-2"
                  : "pointer-events-none absolute inset-x-0 bottom-32 z-30 flex justify-center"
              }
            >
              <Button
                className="pointer-events-auto rounded-full bg-background/90 shadow-md backdrop-blur-xl"
                data-testid="coding-session-scroll-to-latest"
                onClick={() => scrollToBottom("smooth")}
                size="sm"
                type="button"
                variant="outline"
              >
                <ArrowDown />
                {newMessageCount > 0
                  ? `${newMessageCount} new`
                  : "Scroll to latest"}
              </Button>
            </div>
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
                <div className="relative z-10">
                  <CodingSessionComposer
                    authorityReason={composerAuthority.reason}
                    authorityUnresolved={composerAuthority.isUnresolved}
                    canInterrupt={
                      codingSessionTargetSupportsInterrupt(
                        session.commandTarget,
                      ) && session.capabilities?.threadTurnInterrupt !== false
                    }
                    canControl={composerAuthority.canPromptExecutions}
                    canStopExecution={canStopExecution}
                    canSteer={session.capabilities?.threadSteer === true}
                    canAttachImages={session.capabilities?.promptImage === true}
                    runtimeLabel={runtimeLabel}
                    channelId={channelId}
                    controlContext={{
                      capabilities: session.capabilities,
                      model: session.model,
                      providerLabel,
                      runtimeLabel,
                      status,
                      turnBudget: session.turnBudget,
                    }}
                    contextWindow={contextWindow}
                    currentUserPubkey={currentUserPubkey}
                    immersive
                    isMember={isMember}
                    onAddProvider={onAddProvider}
                    isWorking={isWorking}
                    isUngovernedSession={composerAuthority.isUngovernedSession}
                    lifecycleStatus={session.status}
                    layout={isNarrow ? "stacked" : "inline"}
                    promptHistory={promptHistory}
                    providerAuthorityPubkey={session.providerAuthorityPubkey}
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
          ) : null}
        </section>
        {surfaceHost.activeTab !== null ? (
          <CodingSessionSurfaceHost
            activeSurfaceId={surfaceHost.activeTab}
            hostId={surfaceHostId}
            layout={
              narrowState === null ? null : narrowState ? "sheet" : "inline"
            }
            onClose={surfaceHost.close}
            onSelectSurface={surfaceHost.select}
            surfaces={surfaces}
            widthContainerRef={workspaceRef}
          />
        ) : null}
      </div>
      {isNarrow ? (
        <Sheet
          onOpenChange={(open) => (open ? taskDock.show() : taskDock.close())}
          open={taskDock.open}
        >
          <SheetContent
            aria-describedby={undefined}
            className="w-[min(90vw,22rem)] max-w-none p-0"
            side="right"
          >
            <SheetTitle className="sr-only">Session plan</SheetTitle>
            <CodingSessionTaskRail
              model={taskDock.activeModel}
              variant="sheet"
            />
          </SheetContent>
        </Sheet>
      ) : null}
    </main>
  );
}

function CodingSessionWorkspaceState({
  channelName,
  generationId,
  onClose,
  resolution,
}: {
  channelName: string | null;
  generationId: string;
  /** Closes the pop-out window. Absent in the main window, where the app's
   * own back/forward in the top chrome is the way out of a session. */
  onClose?: () => void;
  resolution: Exclude<
    ReturnType<typeof resolveCodingSessionWorkspace>,
    { kind: "ready" }
  >;
}) {
  const loading = resolution.kind === "loading";
  return (
    <main
      className="flex h-full min-h-0 flex-1 flex-col bg-background"
      data-testid={`coding-session-workspace-${resolution.kind}`}
    >
      <CodingSessionHeader
        channelName={channelName}
        generationLabel={shortGenerationId(generationId)}
        onClose={onClose}
        status={{ kind: "unknown", label: "Status unknown" }}
      />
      <div className="flex min-h-0 flex-1 items-center justify-center px-6 py-10 text-center">
        <div className="max-w-md">
          {loading ? (
            <FuzzyLogo
              ariaLabel="Loading coding session"
              className="mx-auto text-muted-foreground"
              fuzz={false}
              loop
            />
          ) : (
            <CircleAlert className="mx-auto h-5 w-5 text-muted-foreground" />
          )}
          <h2 className="mt-4 text-base font-semibold">
            {loading
              ? "Loading coding session"
              : resolution.kind === "untrusted"
                ? "Generation not trusted"
                : "Generation not found"}
          </h2>
          <p className="mt-2 text-sm text-muted-foreground">
            {loading
              ? "Resolving the exact signed generation from the relay catalog."
              : resolution.description}
          </p>
        </div>
      </div>
    </main>
  );
}

function shortGenerationId(value: string): string {
  return value.length <= 28 ? value : `${value.slice(0, 28)}…`;
}
