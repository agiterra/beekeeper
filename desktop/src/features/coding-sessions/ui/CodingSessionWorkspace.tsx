import * as React from "react";
import { ArrowDown, CircleAlert } from "lucide-react";
import { toast } from "sonner";

import { codingSessionTargetSupportsInterrupt } from "@/features/coding-sessions/lib/codingSessionCommand";
import { deriveTranscriptItemBlockIds } from "@/features/agents/ui/agentSessionTranscriptGrouping";
import type { CodingSessionPopoutBootstrap } from "@/features/coding-sessions/lib/codingSessionBootstrap";
import type { CodingSessionSurface } from "@/features/coding-sessions/lib/codingSessionRoute";
import { openCodingSessionPopout } from "@/features/coding-sessions/lib/codingSessionWindow";
import {
  deriveCodingSessionWorkspaceStatus,
  resolveCodingSessionWorkspace,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { resolveCodingSessionUmbrellaComposerAuthority } from "@/features/coding-sessions/lib/codingSessionUmbrellaComposerModel";
import { deriveCodingSessionTaskModel } from "@/features/coding-sessions/lib/codingSessionTaskModel";
import { formatCodingSessionRuntimeLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { useCodingSessionCatalog } from "@/features/coding-sessions/useCodingSessionCatalog";
import {
  codingSessionGoalKey,
  useCodingSessionGoals,
} from "@/features/coding-sessions/useCodingSessionGoals";
import { useChannelsQuery } from "@/features/channels/hooks";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useFeatureEnabled } from "@/shared/features/useFeatureEnabled";
import { useAnchoredScroll } from "@/features/messages/ui/useAnchoredScroll";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";
import { Button } from "@/shared/ui/button";
import { FuzzyLogo } from "@/shared/ui/buzz-logo/FuzzyLogo";
import { Sheet, SheetContent, SheetTitle } from "@/shared/ui/sheet";
import { AddCodingSessionProviderDialog } from "./AddCodingSessionProviderDialog";
import { CodingSessionComposer } from "./CodingSessionComposer";
import { CodingSessionHeader } from "./CodingSessionHeader";
import { CodingSessionFounderLine } from "./CodingSessionFounderLine";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";
import { useCodingSessionExport } from "./useCodingSessionExport";
import {
  codingSessionTaskRailPreferenceKey,
  type CodingSessionTaskRailPreference,
  CodingSessionTaskRail,
  deriveCodingSessionTaskRailOpen,
} from "./CodingSessionTaskRail";
import { CodingSessionTranscript } from "./CodingSessionTranscript";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail";
import {
  CodingSessionSurfaceHost,
  useCodingSessionSurfaceHostState,
  type CodingSessionSurfaceDescriptor,
} from "./CodingSessionSurfaceHost";
import { deriveCodingSessionChangedFiles } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import { UmbrellaCodingSessionWorkspace } from "./CodingSessionUmbrellaWorkspace";

type CodingSessionWorkspaceProps = {
  bootstrap?: CodingSessionPopoutBootstrap | null;
  requireBootstrap?: boolean;
  channelId: string;
  generationId: string;
  onBack: () => void;
  surface: CodingSessionSurface;
};

export function CodingSessionWorkspace({
  bootstrap = null,
  channelId,
  generationId,
  onBack,
  requireBootstrap = false,
  surface,
}: CodingSessionWorkspaceProps) {
  const [addProviderOpen, setAddProviderOpen] = React.useState(false);
  const identity = useIdentityQuery();
  const catalog = useCodingSessionCatalog(channelId, bootstrap, {
    requirePopoutBootstrap: requireBootstrap,
    // Viewing is channel-membership authority: any member of the channel sees
    // the same sessions. The local allowlist keeps gating what runs here.
    authorityMode: "open",
  });
  const goalSnapshot = useCodingSessionGoals([channelId]);
  const channelsQuery = useChannelsQuery({ enabled: true });
  const channel =
    channelsQuery.data?.find((candidate) => candidate.id === channelId) ?? null;
  const resolution = resolveCodingSessionWorkspace({
    catalog,
    generationId,
  });

  if (resolution.kind !== "ready") {
    return (
      <CodingSessionWorkspaceState
        channelName={channel?.name ?? null}
        generationId={generationId}
        onBack={onBack}
        resolution={resolution}
      />
    );
  }

  const isMember = channel?.isMember ?? false;
  const umbrella = resolution.umbrella;
  const goal =
    umbrella.sessionRef && umbrella.founderPubkey
      ? (goalSnapshot.goals.get(
          codingSessionGoalKey(
            channelId,
            umbrella.sessionRef,
            umbrella.founderPubkey,
          ),
        ) ?? null)
      : null;
  // Joining needs a claimed umbrella ref to join *to* (a pre-Step-4 session has
  // none, so it gets no affordance rather than a button that cannot work), and
  // v1 authority is founder-only — when an observed create binds a founder who
  // is not this user, the attach UI is absent exactly as the design specifies.
  const canAddProvider =
    isMember &&
    umbrella.sessionRef !== null &&
    resolveCodingSessionUmbrellaComposerAuthority({
      umbrella,
      currentUserPubkey: identity.data?.pubkey ?? null,
    }).canPromptExecutions;
  const onAddProvider = canAddProvider
    ? () => setAddProviderOpen(true)
    : undefined;

  return (
    <>
      {/* The umbrella surface is a render branch, not a mode: an umbrella of
          one falls through to exactly today's single-session tree. */}
      {umbrella.executions.length > 1 ? (
        <UmbrellaCodingSessionWorkspace
          channelId={channelId}
          channelName={channel?.name ?? null}
          focusedExecution={resolution.focusedExecution}
          generationId={generationId}
          isMember={isMember}
          currentUserPubkey={identity.data?.pubkey ?? null}
          key={`${channelId}:${umbrella.umbrellaKey}`}
          onAddProvider={onAddProvider}
          onBack={onBack}
          surface={surface}
          umbrella={umbrella}
          goal={goal}
        />
      ) : (
        <ReadyCodingSessionWorkspace
          channelId={channelId}
          channelName={channel?.name ?? null}
          generationId={generationId}
          isMember={isMember}
          key={`${channelId}:${generationId}`}
          onAddProvider={onAddProvider}
          onBack={onBack}
          founderPubkey={umbrella.founderPubkey}
          genesisRef={umbrella.genesisRef}
          goal={goal}
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
          umbrella={umbrella}
        />
      ) : null}
    </>
  );
}

function ReadyCodingSessionWorkspace({
  channelId,
  channelName,
  generationId,
  founderPubkey,
  genesisRef,
  goal,
  currentUserPubkey,
  sessionRef,
  isMember,
  onAddProvider,
  onBack,
  session,
  surface,
  umbrella,
}: {
  channelId: string;
  channelName: string | null;
  generationId: string;
  founderPubkey: string | null;
  genesisRef: string | null;
  goal:
    | import("@/features/coding-sessions/lib/codingSessionGoal").CodingSessionGoal
    | null;
  currentUserPubkey: string | null;
  sessionRef: string | null;
  isMember: boolean;
  onAddProvider?: () => void;
  onBack: () => void;
  session: Extract<
    ReturnType<typeof resolveCodingSessionWorkspace>,
    { kind: "ready" }
  >["session"];
  surface: CodingSessionSurface;
  umbrella: CodingSessionUmbrellaRecord;
}) {
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
  const composerAuthority = resolveCodingSessionUmbrellaComposerAuthority({
    umbrella: { founderPubkey, genesisRef },
    currentUserPubkey,
  });
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
  const status = deriveCodingSessionWorkspaceStatus(
    session.transcript,
    session.status,
  );
  const taskModel = React.useMemo(
    () => deriveCodingSessionTaskModel(session.transcript),
    [session.transcript],
  );
  const changedFiles = React.useMemo(
    () => deriveCodingSessionChangedFiles(session.transcript),
    [session.transcript],
  );
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
              content: <CodingSessionExecutionRail umbrella={umbrella} />,
            },
          ]
        : []),
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
  const surfaceHostId = React.useId();
  const taskRailPreferenceKey = React.useMemo(
    () => codingSessionTaskRailPreferenceKey(channelId, generationId),
    [channelId, generationId],
  );
  const [taskRailPreference, setTaskRailPreference] =
    React.useState<CodingSessionTaskRailPreference>(() =>
      readTaskRailPreference(taskRailPreferenceKey),
    );
  const taskRailOpen = deriveCodingSessionTaskRailOpen({
    hasPlan: taskModel !== null,
    isNarrow,
    preference: taskRailPreference,
  });
  const isWorking = status.kind === "working";

  const setTaskRailOpen = React.useCallback(
    (open: boolean | ((current: boolean) => boolean)) => {
      const nextOpen = typeof open === "function" ? open(taskRailOpen) : open;
      const preference = nextOpen ? "open" : "closed";
      setTaskRailPreference(preference);
      writeTaskRailPreference(taskRailPreferenceKey, preference);
    },
    [taskRailOpen, taskRailPreferenceKey],
  );

  const exportEnabled = useFeatureEnabled("coding-session-export");
  const { exportTranscript, isExporting } = useCodingSessionExport(
    generationId,
    session,
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
      className="relative flex h-full min-h-0 flex-1 flex-col overflow-hidden bg-background"
      data-testid="coding-session-workspace"
      ref={workspaceRef}
    >
      <div className="shrink-0" data-testid="coding-session-authority-summary">
        <CodingSessionHeader
          channelName={channelName}
          compact={isNarrow}
          generationLabel={session.label}
          isExporting={isExporting}
          model={session.model}
          onAddProvider={onAddProvider}
          onBack={onBack}
          onExport={exportEnabled ? exportTranscript : undefined}
          onPopout={surface === "main" ? handlePopout : undefined}
          onToggleTaskRail={() => {
            surfaceHost.close();
            setTaskRailOpen((open) => !open);
          }}
          onToggleSurface={(id) => {
            setTaskRailOpen(false);
            surfaceHost.toggle(id);
          }}
          providerAuthorityPubkey={session.providerAuthorityPubkey}
          runtimeLabel={runtimeLabel}
          sessionTitle={session.title}
          status={status}
          surfaceHostId={surfaceHostId}
          surfaceTabs={surfaces.map((surfaceEntry) => ({
            id: surfaceEntry.id,
            label: surfaceEntry.label,
            icon: surfaceEntry.id === "agents" ? "agents" : "changes",
            count: surfaceEntry.count ?? 0,
            active: surfaceHost.activeTab === surfaceEntry.id,
          }))}
          taskCount={taskModel?.tasks.length ?? 0}
          taskRailOpen={taskRailOpen}
        />
        <CodingSessionFounderLine
          founderPubkey={founderPubkey}
          genesisRef={genesisRef}
        />
        <div className="px-5 pb-2 sm:px-8">
          <CodingSessionGoalPill
            channelId={channelId}
            currentUserPubkey={currentUserPubkey}
            founderPubkey={founderPubkey}
            goal={goal}
            sessionRef={sessionRef}
          />
        </div>
      </div>
      <div className="flex min-h-0 flex-1">
        <section
          aria-label="Session transcript"
          className="relative flex min-w-0 flex-1 flex-col overflow-hidden"
        >
          <div
            className="min-h-0 flex-1 overflow-y-auto overscroll-contain"
            onScroll={onScroll}
            ref={scrollRef}
          >
            <div className="mx-auto min-h-full w-full max-w-3xl px-5 pt-7 pb-44 sm:px-8">
              <div ref={contentRef}>
                <CodingSessionTranscript
                  generationId={generationId}
                  isWorking={isWorking}
                  items={session.transcript}
                  scrollRef={scrollRef}
                />
              </div>
            </div>
          </div>
          {!isAtBottom ? (
            <div className="pointer-events-none absolute inset-x-0 bottom-32 z-30 flex justify-center">
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
          {session.commandTarget ? (
            <div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 bg-linear-to-b from-transparent via-background/85 to-background px-4 pt-8 pb-4">
              <div className="pointer-events-auto mx-auto w-full max-w-3xl">
                <CodingSessionComposer
                  authorityReason={composerAuthority.reason}
                  canInterrupt={
                    codingSessionTargetSupportsInterrupt(
                      session.commandTarget,
                    ) && session.capabilities?.threadTurnInterrupt !== false
                  }
                  canControl={composerAuthority.canPromptExecutions}
                  canSteer={session.capabilities?.threadSteer === true}
                  channelId={channelId}
                  controlContext={{
                    capabilities: session.capabilities,
                    model: session.model,
                    providerLabel,
                    runtimeLabel,
                    status,
                  }}
                  immersive
                  isMember={isMember}
                  isWorking={isWorking}
                  isUngovernedSession={composerAuthority.isUngovernedSession}
                  lifecycleStatus={session.status}
                  layout={isNarrow ? "stacked" : "inline"}
                  providerAuthorityPubkey={session.providerAuthorityPubkey}
                  target={session.commandTarget}
                  variant="floating"
                />
              </div>
            </div>
          ) : null}
        </section>
        {!isNarrow && taskRailOpen ? (
          <CodingSessionTaskRail model={taskModel} />
        ) : null}
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
        <Sheet onOpenChange={setTaskRailOpen} open={taskRailOpen}>
          <SheetContent
            aria-describedby={undefined}
            className="w-[min(90vw,22rem)] max-w-none p-0"
            side="right"
          >
            <SheetTitle className="sr-only">Session plan</SheetTitle>
            <CodingSessionTaskRail model={taskModel} variant="sheet" />
          </SheetContent>
        </Sheet>
      ) : null}
    </main>
  );
}

function CodingSessionWorkspaceState({
  channelName,
  generationId,
  onBack,
  resolution,
}: {
  channelName: string | null;
  generationId: string;
  onBack: () => void;
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
        onBack={onBack}
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

function readTaskRailPreference(key: string): CodingSessionTaskRailPreference {
  if (typeof window === "undefined") return null;
  try {
    const value = window.sessionStorage.getItem(key);
    return value === "open" || value === "closed" ? value : null;
  } catch {
    return null;
  }
}

function writeTaskRailPreference(
  key: string,
  preference: Exclude<CodingSessionTaskRailPreference, null>,
) {
  try {
    window.sessionStorage.setItem(key, preference);
  } catch {
    // Storage can be unavailable in hardened webviews; local state still works.
  }
}

const NARROW_CODING_SESSION_WORKSPACE_WIDTH = 960;

/**
 * `null` until the workspace width is first measured, so responsive chrome
 * (the surface host in particular) never mounts its desktop inline layout
 * only to be torn down and replaced by an animated sheet a frame later.
 */
function useNarrowCodingSessionWorkspace(
  workspaceRef: React.RefObject<HTMLElement | null>,
): boolean | null {
  const [isNarrow, setIsNarrow] = React.useState<boolean | null>(null);

  React.useEffect(() => {
    const workspace = workspaceRef.current;
    if (!workspace) return;

    const update = (width: number) => {
      setIsNarrow(width < NARROW_CODING_SESSION_WORKSPACE_WIDTH);
    };
    update(workspace.getBoundingClientRect().width);

    if (typeof ResizeObserver === "undefined") {
      const media = window.matchMedia(
        `(max-width: ${NARROW_CODING_SESSION_WORKSPACE_WIDTH - 1}px)`,
      );
      const updateFromMedia = () => setIsNarrow(media.matches);
      media.addEventListener("change", updateFromMedia);
      return () => media.removeEventListener("change", updateFromMedia);
    }

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      if (entry) update(entry.contentRect.width);
    });
    observer.observe(workspace);
    return () => observer.disconnect();
  }, [workspaceRef]);

  return isNarrow;
}
