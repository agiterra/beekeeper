import * as React from "react";
import { X } from "lucide-react";
import { toast } from "sonner";

import type { CodingSessionLaneMessage } from "@/features/coding-sessions/lib/codingSessionConversationLane";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  resolveCodingSessionHandoffFactLocation,
  type CodingSessionHandoffLink,
} from "@/features/coding-sessions/lib/codingSessionHandoff";
import { useCodingSessionOperatorProfiles } from "@/features/coding-sessions/hooks/useCodingSessionOperatorProfiles";
import { listCodingSessionUmbrellaParticipants } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import {
  codingSessionUmbrellaParticipantKey,
  defaultCodingSessionUmbrellaParticipantKey,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import {
  buildUmbrellaTimeline,
  codingSessionUmbrellaEntryKey,
  type CodingSessionUmbrellaTimelineEntry,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import type {
  CodingSessionCatalogRecord,
  CodingSessionExecution,
  CodingSessionUmbrellaRecord,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionGoal } from "@/features/coding-sessions/lib/codingSessionGoal";
import type { CodingSessionName } from "@/features/coding-sessions/lib/codingSessionName";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import type { CodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { deriveCodingSessionChangedFiles } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import {
  codingSessionUmbrellaGenerationLabel,
  codingSessionWireWorkspaceStatus,
  deriveCodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { useCodingSessionLane } from "@/features/coding-sessions/useCodingSessionLane";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useElementWidth } from "@/shared/hooks/use-mobile";
import { cn } from "@/shared/lib/cn";
import { Sheet, SheetContent, SheetTitle } from "@/shared/ui/sheet";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { CodingSessionHeader } from "./CodingSessionHeader";
import { CodingSessionFounderLine } from "./CodingSessionFounderLine";
import { useCodingSessionColumnGutter } from "../lib/codingSessionWidthPreference";
import {
  CODING_SESSION_COMPOSER_DOCK_CLASS,
  CodingSessionColumn,
} from "./CodingSessionColumn";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";
import { CodingSessionNameDialog } from "./CodingSessionNameDialog";
import { CodingSessionPendingTurns } from "./CodingSessionPendingTurns";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail";
import { CodingSessionTaskRail } from "./CodingSessionTaskRail";
import { useCodingSessionTaskDock } from "./useCodingSessionTaskDock";
import { deriveCodingSessionActiveTaskModel } from "./useCodingSessionTaskDock";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail";
import {
  CodingSessionAgentFocus,
  type CodingSessionAgentFocusItem,
  codingSessionAgentAccent,
} from "./CodingSessionAgentFocus";
import {
  CodingSessionActiveWorkDock,
  type CodingSessionActiveWorkAgent,
} from "./CodingSessionActiveWorkDock";
import {
  CodingSessionSurfaceHost,
  useCodingSessionSurfaceHostState,
  type CodingSessionSurfaceDescriptor,
} from "./CodingSessionSurfaceHost";
import { useCodingSessionReachabilityResolver } from "@/features/coding-sessions/hooks/useCodingSessionProviderReachability";
import {
  CodingSessionUmbrellaComposer,
  type CodingSessionUmbrellaComposerPrefill,
} from "./CodingSessionUmbrellaComposer";
import { CodingSessionUmbrellaTurnBlock } from "./CodingSessionUmbrellaTurnBlock";

export { buildUmbrellaTurnBlockHandoff } from "./CodingSessionUmbrellaTurnBlock";

/**
 * The umbrella surface: one time-ordered narrative interleaved at turn-block
 * granularity across N executions, plus the conversation lane. Mounted only
 * when an umbrella actually has more than one execution — an umbrella of one
 * renders today's single-session tree and never sees this component.
 */
export function UmbrellaCodingSessionWorkspace({
  acceptedOperators = null,
  channelId,
  channelName,
  generationId,
  isMember,
  onAddProvider,
  onCloseSession,
  onReopenSession,
  onBack,
  onOpenPeople,
  peopleCount = 0,
  surface,
  umbrella,
  focusedExecution,
  currentUserPubkey,
  goal,
  sessionName = null,
  sessionClosed = false,
}: {
  /** Live operator grants from the session roster; null while unknown. */
  acceptedOperators?: ReadonlySet<string> | null;
  channelId: string;
  channelName: string | null;
  generationId: string;
  isMember: boolean;
  /** Opens the join flow (design §B); absent when this session cannot join. */
  onAddProvider?: () => void;
  onCloseSession?: () => void;
  onReopenSession?: () => void;
  onBack: () => void;
  /** Opens the session People surface; absent for no-genesis sessions. */
  onOpenPeople?: () => void;
  peopleCount?: number;
  surface: CodingSessionSurface;
  umbrella: CodingSessionUmbrellaRecord;
  focusedExecution: CodingSessionExecution;
  currentUserPubkey: string | null;
  goal: CodingSessionGoal | null;
  sessionName?: CodingSessionName | null;
  sessionClosed?: boolean;
}) {
  const gutter = useCodingSessionColumnGutter();
  const identity = useIdentityQuery();
  const lane = useCodingSessionLane(channelId, umbrella.sessionRef);
  // One coordination read for the whole umbrella; every execution composer
  // asks it whether anything is answering for that generation (§2 item 41).
  const resolveReachability = useCodingSessionReachabilityResolver(channelId);
  const [workspaceBodyRef, bodyWidthPx] = useElementWidth<HTMLDivElement>();
  const isNarrow = bodyWidthPx > 0 && bodyWidthPx < 960;
  const headerCompact = bodyWidthPx > 0 && bodyWidthPx < 1320;
  const isMultiExecution = umbrella.executions.length > 1;
  const [focusedExecutionKey, setFocusedExecutionKey] = React.useState<
    string | null
  >(null);
  const narrativeScrollRef = React.useRef<HTMLDivElement>(null);
  const composerParticipants = React.useMemo(
    () => listCodingSessionUmbrellaParticipants(umbrella),
    [umbrella],
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
  const changedFiles = React.useMemo(
    () =>
      deriveCodingSessionChangedFiles(
        umbrella.executions.flatMap((execution) =>
          [...execution.priorGenerations, execution.activeGeneration].flatMap(
            (record) => record.transcript,
          ),
        ),
      ),
    [umbrella.executions],
  );
  // Same item set as the timeline renders, so every operator who drove a turn
  // anywhere in the umbrella is resolvable in one lookup.
  const umbrellaTranscript = React.useMemo(
    () =>
      umbrella.executions.flatMap((execution) =>
        [...execution.priorGenerations, execution.activeGeneration].flatMap(
          (record) => record.transcript,
        ),
      ),
    [umbrella.executions],
  );
  const operatorProfiles = useCodingSessionOperatorProfiles(
    umbrellaTranscript,
    currentUserPubkey,
  );
  const surfaces = React.useMemo<CodingSessionSurfaceDescriptor[]>(
    () => [
      {
        id: "agents",
        label: "Agents",
        count: umbrella.executions.length,
        content: <CodingSessionExecutionRail umbrella={umbrella} />,
      },
      {
        id: "changes",
        label: "Observed changes",
        count: changedFiles.length,
        content: <CodingSessionChangesRail files={changedFiles} />,
      },
    ],
    [changedFiles, umbrella],
  );
  const surfaceIds = React.useMemo(
    () => surfaces.map((surfaceEntry) => surfaceEntry.id),
    [surfaces],
  );
  const surfaceHost = useCodingSessionSurfaceHostState(surfaceIds);
  const autoOpenedAgentsRef = React.useRef(false);
  React.useEffect(() => {
    if (
      !autoOpenedAgentsRef.current &&
      shouldAutoOpenAgentsSurface({ bodyWidthPx, isMultiExecution })
    ) {
      autoOpenedAgentsRef.current = true;
      surfaceHost.select("agents");
    }
  }, [bodyWidthPx, isMultiExecution, surfaceHost.select]);
  const narrativeExpanded = surfaceHost.activeTab === null;
  const surfaceHostId = React.useId();
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
  const focusedAgent =
    agentFocusItems.find((item) => item.executionKey === focusedExecutionKey) ??
    null;
  const handleFocusExecution = React.useCallback(
    (executionKey: string | null) => setFocusedExecutionKey(executionKey),
    [],
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

  return (
    <main
      className="relative flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-background"
      data-testid="coding-session-umbrella-workspace"
    >
      <div className="shrink-0" data-testid="coding-session-authority-summary">
        <CodingSessionHeader
          agentControls={
            isMultiExecution && !isNarrow ? (
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
            ) : undefined
          }
          channelName={channelName}
          compact={isNarrow || headerCompact}
          founderDetails={
            umbrella.founderPubkey && umbrella.genesisRef ? (
              <CodingSessionFounderLine
                founderPubkey={umbrella.founderPubkey}
                genesisRef={umbrella.genesisRef}
                variant="label"
              />
            ) : undefined
          }
          generationLabel={codingSessionUmbrellaGenerationLabel(umbrella)}
          goalText={goal?.content ?? null}
          onAddProvider={onAddProvider}
          onBack={onBack}
          onCloseSession={onCloseSession}
          onOpenPeople={onOpenPeople}
          onPopout={surface === "main" ? handlePopout : undefined}
          onRename={canRename ? () => setRenameOpen(true) : undefined}
          onReopenSession={onReopenSession}
          peopleCount={peopleCount}
          onToggleTaskRail={
            !isMultiExecution && composerTaskDock.activeModel
              ? () => {
                  surfaceHost.close();
                  composerTaskDock.toggle();
                }
              : undefined
          }
          onToggleSurface={(id) => {
            composerTaskDock.close();
            surfaceHost.toggle(id);
          }}
          providerAuthorityPubkey={focusedExecution.signerPubkey}
          sessionTitle={authoritativeTitle}
          sessionClosed={sessionClosed}
          status={umbrellaWorkspaceStatus(umbrella)}
          statusLabelOverride={umbrellaAgentStatusSummary(agentFocusItems)}
          surfaceHostId={surfaceHostId}
          surfaceTabs={surfaces
            .filter((surfaceEntry) => surfaceEntry.id !== "agents")
            .map((surfaceEntry) => ({
              id: surfaceEntry.id,
              label: surfaceEntry.label,
              icon: surfaceEntry.id === "agents" ? "agents" : "changes",
              count: surfaceEntry.count ?? 0,
              active: surfaceHost.activeTab === surfaceEntry.id,
            }))}
          taskCount={
            isMultiExecution
              ? 0
              : (composerTaskDock.activeModel?.tasks.length ?? 0)
          }
          taskRailOpen={composerTaskDock.open}
        />
      </div>
      {umbrella.sessionRef ? (
        <CodingSessionNameDialog
          channelId={channelId}
          currentName={authoritativeTitle}
          onOpenChange={setRenameOpen}
          open={renameOpen}
          sessionRef={umbrella.sessionRef}
        />
      ) : null}
      <div className="flex min-h-0 flex-1" ref={workspaceBodyRef}>
        <section
          aria-label="Umbrella session narrative"
          className="relative flex min-w-0 flex-1 flex-col overflow-hidden"
        >
          <div className={cn(gutter, "pb-2")}>
            <CodingSessionGoalPill
              channelId={channelId}
              currentUserPubkey={currentUserPubkey}
              founderPubkey={umbrella.founderPubkey}
              goal={goal}
              headerCarriesGoal
              sessionRef={umbrella.sessionRef}
              workspaceExpanded={narrativeExpanded}
            />
            {isMultiExecution && isNarrow ? (
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
                activeWorkAgents.length > 0 ||
                  (composerTaskDock.open && !isNarrow)
                  ? "pb-[34rem]"
                  : "pb-48",
              )}
              expanded={narrativeExpanded}
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
                operatorProfiles={operatorProfiles}
                umbrella={umbrella}
              />
            </CodingSessionColumn>
          </div>
          {!sessionClosed ? (
            <div className={cn(CODING_SESSION_COMPOSER_DOCK_CLASS, gutter)}>
              <CodingSessionColumn
                className="pointer-events-auto"
                expanded={narrativeExpanded}
              >
                {isMultiExecution ? (
                  <div className="-mb-6">
                    <CodingSessionActiveWorkDock
                      agents={activeWorkAgents}
                      focusedExecutionKey={focusedExecutionKey}
                      onFocusAgent={handleFocusExecution}
                    />
                  </div>
                ) : composerTaskDock.open && !isNarrow ? (
                  <div className="-mb-6">
                    <CodingSessionTaskRail
                      model={composerTaskDock.activeModel}
                      onClose={composerTaskDock.close}
                      variant="dock"
                    />
                  </div>
                ) : null}
                <CodingSessionUmbrellaComposer
                  acceptedOperators={acceptedOperators}
                  channelId={channelId}
                  currentUserPubkey={identity.data?.pubkey ?? null}
                  isMember={isMember}
                  onAddProvider={onAddProvider}
                  onSelectedParticipantChange={setComposerParticipantKey}
                  prefill={prefill}
                  resolveReachability={resolveReachability}
                  umbrella={umbrella}
                />
              </CodingSessionColumn>
            </div>
          ) : null}
        </section>
        {surfaceHost.activeTab !== null ? (
          <CodingSessionSurfaceHost
            activeSurfaceId={surfaceHost.activeTab}
            hostId={surfaceHostId}
            layout={bodyWidthPx <= 0 ? null : isNarrow ? "sheet" : "inline"}
            onClose={surfaceHost.close}
            onSelectSurface={surfaceHost.select}
            surfaces={surfaces}
            widthContainerRef={workspaceBodyRef}
          />
        ) : null}
      </div>
      {isNarrow ? (
        <Sheet
          onOpenChange={(open) =>
            open ? composerTaskDock.show() : composerTaskDock.close()
          }
          open={composerTaskDock.open}
        >
          <SheetContent
            aria-describedby={undefined}
            className="w-[min(90vw,22rem)] max-w-none p-0"
            side="right"
          >
            <SheetTitle className="sr-only">Session plan</SheetTitle>
            <CodingSessionTaskRail
              model={composerTaskDock.activeModel}
              variant="sheet"
            />
          </SheetContent>
        </Sheet>
      ) : null}
    </main>
  );
}

function CodingSessionFocusedAgentNotice({
  agent,
  onClear,
}: {
  agent: CodingSessionAgentFocusItem;
  onClear: () => void;
}) {
  const accent = codingSessionAgentAccent(agent.executionKey);
  const working = agent.status.kind === "working";
  return (
    <div
      className="mb-5 flex min-h-8 items-center gap-2 border-b border-border/45 pb-3 text-xs text-muted-foreground"
      data-testid="coding-session-focused-agent-notice"
    >
      <span
        aria-hidden
        className={cn(
          "grid size-5 shrink-0 place-items-center rounded-full",
          accent.soft,
          working && "coding-session-agent-breathe",
        )}
      >
        <span className={cn("size-2 rounded-full", accent.dot)} />
      </span>
      <span className="min-w-0 truncate">
        Viewing{" "}
        <span className={cn("font-medium", accent.text)}>{agent.label}</span>
      </span>
      <button
        aria-label="Return to the complete session"
        className="ml-auto inline-flex size-6 shrink-0 items-center justify-center rounded-full text-muted-foreground transition-colors hover:bg-muted/55 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        data-testid="coding-session-focused-agent-clear"
        onClick={onClear}
        title="Show the complete session"
        type="button"
      >
        <X aria-hidden className="size-3.5" />
      </button>
    </div>
  );
}

export function scrollCodingSessionNarrativeToLatest(
  viewport: Pick<HTMLElement, "scrollHeight" | "scrollTo"> | null,
): void {
  if (!viewport) return;
  viewport.scrollTo({ behavior: "smooth", top: viewport.scrollHeight });
}

/**
 * Pure view over `buildUmbrellaTimeline`: each turn block renders exactly one
 * (signer, target) stream through the existing single-session transcript
 * renderer, wrapped in that execution's provenance chrome. Items are never
 * cross-ordered between executions — interleaving is between blocks only.
 */
export function CodingSessionUmbrellaTimelineView({
  channelId,
  currentUserPubkey = null,
  focusedExecutionKey = null,
  laneMessages,
  onHandoff,
  onFocusExecution,
  operatorProfiles,
  umbrella,
}: {
  channelId: string;
  /**
   * The viewer's own pubkey, forwarded to each block's transcript so a prompt
   * sent by another operator is attributed to them instead of to the reader.
   */
  currentUserPubkey?: string | null;
  /** Null keeps the full merged narrative; a key folds every other agent. */
  focusedExecutionKey?: string | null;
  laneMessages: readonly CodingSessionLaneMessage[];
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
  onFocusExecution?: (executionKey: string | null) => void;
  /** Profiles for the umbrella's operators, resolved once by the workspace. */
  operatorProfiles?: UserProfileLookup;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const participants = React.useMemo(
    () => listCodingSessionUmbrellaParticipants(umbrella),
    [umbrella],
  );
  const labelsByExecutionKey = React.useMemo(() => {
    const labels = new Map<string, string>();
    for (const participant of participants) {
      if (participant.kind === "execution") {
        labels.set(participant.executionKey, participant.label);
      }
    }
    return labels;
  }, [participants]);
  const recordsByGenerationId = React.useMemo(() => {
    const records = new Map<string, CodingSessionCatalogRecord>();
    for (const execution of umbrella.executions) {
      records.set(
        execution.activeGeneration.generationId,
        execution.activeGeneration,
      );
      for (const prior of execution.priorGenerations) {
        records.set(prior.generationId, prior);
      }
    }
    return records;
  }, [umbrella.executions]);
  const entries = React.useMemo(
    () => buildUmbrellaTimeline(umbrella, laneMessages),
    [laneMessages, umbrella],
  );
  const workingBlockKeys = React.useMemo(
    () => resolveWorkingBlockKeys(umbrella, entries),
    [entries, umbrella],
  );

  // Provenance resolution for handoff chips: the `beekeeper://coding-session` link
  // is a client-side convention nothing registers, so the only honest control
  // is one that jumps to a fact this view already holds.
  const factCandidates = React.useMemo(
    () =>
      entries.flatMap((entry) =>
        entry.kind === "turn-block"
          ? [
              {
                key: codingSessionUmbrellaEntryKey(entry),
                targetKey: blockTargetKey(
                  recordsByGenerationId.get(entry.generationId) ?? null,
                ),
                items: entry.items,
              },
            ]
          : [],
      ),
    [entries, recordsByGenerationId],
  );
  const resolveFactLocation = React.useCallback(
    (link: CodingSessionHandoffLink) =>
      resolveCodingSessionHandoffFactLocation({
        channelId,
        link,
        candidates: factCandidates,
      }),
    [channelId, factCandidates],
  );

  const blockNodes = React.useRef(new Map<string, HTMLElement>());
  const registerBlockNode = React.useCallback(
    (key: string, node: HTMLElement | null) => {
      if (node) blockNodes.current.set(key, node);
      else blockNodes.current.delete(key);
    },
    [],
  );
  const [revealed, setRevealed] = React.useState<{
    key: string;
    nonce: number;
  } | null>(null);
  const revealFact = React.useCallback((key: string) => {
    blockNodes.current
      .get(key)
      ?.scrollIntoView({ behavior: "smooth", block: "center" });
    setRevealed((current) => ({ key, nonce: (current?.nonce ?? 0) + 1 }));
  }, []);
  React.useEffect(() => {
    if (revealed === null) return;
    const handle = window.setTimeout(() => setRevealed(null), 2400);
    return () => window.clearTimeout(handle);
  }, [revealed]);

  // Turns published from this client that their execution has not echoed yet.
  // Newest thing in the session by construction, so they close the narrative;
  // each is labelled with its execution because this surface has several.
  const pendingTurns = (
    <>
      {umbrella.executions.map((execution) => {
        const target = execution.activeGeneration.commandTarget;
        return (
          <CodingSessionPendingTurns
            channelId={channelId}
            echoes={execution.activeGeneration.transcript}
            key={execution.executionKey}
            targetKey={target ? buildCodingSessionTargetKey(target) : null}
            targetLabel={
              umbrella.executions.length > 1
                ? (labelsByExecutionKey.get(execution.executionKey) ?? null)
                : null
            }
          />
        );
      })}
    </>
  );

  if (entries.length === 0) {
    return (
      <div
        className="flex flex-col gap-7"
        data-testid="coding-session-umbrella-timeline"
      >
        <p
          className="py-10 text-center text-sm text-muted-foreground"
          data-testid="coding-session-umbrella-timeline-empty"
        >
          No activity in this session yet.
        </p>
        {pendingTurns}
      </div>
    );
  }

  return (
    <div
      className="flex flex-col gap-7"
      data-testid="coding-session-umbrella-timeline"
    >
      {entries.map((entry, index) => {
        const key = codingSessionUmbrellaEntryKey(entry);
        if (entry.kind === "conversation") {
          return <UmbrellaConversationRow key={key} message={entry.message} />;
        }
        if (entry.kind === "lifecycle") {
          const label =
            labelsByExecutionKey.get(entry.executionKey) ??
            truncatePubkey(entry.signerPubkey);
          return (
            <p
              className="text-center text-2xs text-muted-foreground"
              data-lifecycle-event={entry.event}
              data-testid="coding-session-umbrella-lifecycle"
              key={key}
            >
              {entry.event === "execution-joined"
                ? `${label} joined this session`
                : `${label} started generation ${entry.generation}`}
            </p>
          );
        }
        return (
          <CodingSessionUmbrellaTurnBlock
            block={entry}
            blockKey={key}
            channelId={channelId}
            currentUserPubkey={currentUserPubkey}
            isHighlighted={revealed?.key === key}
            isFolded={
              focusedExecutionKey !== null &&
              focusedExecutionKey !== entry.executionKey
            }
            isWorking={workingBlockKeys.has(key)}
            key={key}
            label={
              labelsByExecutionKey.get(entry.executionKey) ??
              truncatePubkey(entry.signerPubkey)
            }
            labelsByExecutionKey={labelsByExecutionKey}
            onHandoff={onHandoff}
            onFocusExecution={onFocusExecution}
            onRegisterNode={registerBlockNode}
            operatorProfiles={operatorProfiles}
            onRevealFact={revealFact}
            record={recordsByGenerationId.get(entry.generationId) ?? null}
            resolveFactLocation={resolveFactLocation}
            showProvenance={shouldShowTurnBlockProvenance(entries, index)}
            stickyProvenance={
              focusedExecutionKey === null && umbrella.executions.length > 1
            }
            umbrella={umbrella}
          />
        );
      })}
      {pendingTurns}
    </div>
  );
}

/**
 * Provenance labels a contiguous execution run, not every turn. A generation
 * lifecycle row already identifies the execution and generation, so the first
 * block after that row does not repeat the same chrome either.
 */
export function shouldShowTurnBlockProvenance(
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
  index: number,
): boolean {
  const entry = entries[index];
  if (entry?.kind !== "turn-block") return false;
  const previous = entries[index - 1];
  if (!previous) return true;
  if (previous.kind === "conversation") return true;
  if (previous.executionKey !== entry.executionKey) return true;
  if (previous.kind === "lifecycle") {
    return previous.generation !== entry.generation;
  }
  return (
    previous.kind !== "turn-block" || previous.generation !== entry.generation
  );
}

function UmbrellaConversationRow({
  message,
}: {
  message: CodingSessionLaneMessage;
}) {
  return (
    <div
      className="rounded-xl bg-muted/40 px-4 py-2"
      data-testid="coding-session-umbrella-conversation"
    >
      <p className="text-2xs text-muted-foreground">
        <span className="font-mono">
          {truncatePubkey(message.authorPubkey)}
        </span>{" "}
        · {formatLaneTimestamp(message.timestampMs)}
      </p>
      <p className="mt-0.5 text-base whitespace-pre-wrap wrap-break-word">
        {message.content}
      </p>
    </div>
  );
}

function formatLaneTimestamp(timestampMs: number): string {
  const date = new Date(timestampMs);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : "";
}

/** Map the umbrella's derived status onto the header's three honest states. */
export function umbrellaWorkspaceStatus(
  umbrella: Pick<CodingSessionUmbrellaRecord, "status">,
): CodingSessionWorkspaceStatus {
  return codingSessionWireWorkspaceStatus(umbrella.status);
}

export function umbrellaAgentStatusSummary(
  agents: readonly CodingSessionAgentFocusItem[],
): string | null {
  if (agents.length <= 1) return null;
  const working = agents.filter(
    (agent) => agent.status.kind === "working",
  ).length;
  if (working > 0) {
    return `${agents.length} agents · ${working} working`;
  }
  const attention = agents.filter(
    (agent) => agent.status.kind === "unknown" && agent.status.attention,
  ).length;
  if (attention > 0) {
    return `${agents.length} agents · ${attention} need attention`;
  }
  return `${agents.length} agents · idle`;
}

export function shouldAutoOpenAgentsSurface({
  bodyWidthPx,
  isMultiExecution,
}: {
  bodyWidthPx: number;
  isMultiExecution: boolean;
}): boolean {
  return isMultiExecution && bodyWidthPx >= 1920;
}

/** The exact `cs-target` key of a block's stream, when the record has one. */
function blockTargetKey(
  record: CodingSessionCatalogRecord | null,
): string | null {
  return record?.commandTarget
    ? buildCodingSessionTargetKey(record.commandTarget)
    : null;
}

/**
 * The keys of blocks that are visibly streaming: the last block of each
 * execution whose active generation reports a working status.
 */
function resolveWorkingBlockKeys(
  umbrella: CodingSessionUmbrellaRecord,
  entries: readonly CodingSessionUmbrellaTimelineEntry[],
): ReadonlySet<string> {
  const runningExecutions = new Set(
    umbrella.executions
      .filter((execution) => execution.activeGeneration.status === "running")
      .map((execution) => execution.executionKey),
  );
  const lastBlockKeyByExecution = new Map<string, string>();
  for (const entry of entries) {
    if (
      entry.kind === "turn-block" &&
      runningExecutions.has(entry.executionKey)
    ) {
      lastBlockKeyByExecution.set(
        entry.executionKey,
        codingSessionUmbrellaEntryKey(entry),
      );
    }
  }
  return new Set(lastBlockKeyByExecution.values());
}
