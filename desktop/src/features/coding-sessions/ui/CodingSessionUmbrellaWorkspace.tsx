import * as React from "react";
import { ArrowRightLeft, Flag } from "lucide-react";
import { toast } from "sonner";

import type { CodingSessionLaneMessage } from "@/features/coding-sessions/lib/codingSessionConversationLane";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  buildCodingSessionHandoffPrefill,
  isCompletedCodingSessionTurnBlock,
  parseCodingSessionHandoffPrefill,
  readCodingSessionTurnBlockPrompt,
  resolveCodingSessionHandoffFactLocation,
  resolveCodingSessionHandoffSource,
  type CodingSessionHandoffLink,
} from "@/features/coding-sessions/lib/codingSessionHandoff";
import { useCodingSessionOperatorProfiles } from "@/features/coding-sessions/hooks/useCodingSessionOperatorProfiles";
import { listCodingSessionUmbrellaParticipants } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import {
  buildUmbrellaTimeline,
  codingSessionUmbrellaEntryKey,
  type CodingSessionUmbrellaTimelineEntry,
  type CodingSessionUmbrellaTurnBlock,
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
import { codingSessionWireWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import { useCodingSessionLane } from "@/features/coding-sessions/useCodingSessionLane";
import { useIdentityQuery } from "@/shared/api/hooks";
import { useElementWidth } from "@/shared/hooks/use-mobile";
import { cn } from "@/shared/lib/cn";
import { truncatePubkey } from "@/shared/lib/pubkey";
import { CodingSessionHeader } from "./CodingSessionHeader";
import { CodingSessionFounderLine } from "./CodingSessionFounderLine";
import { CodingSessionGoalPill } from "./CodingSessionGoalPill";
import { CodingSessionNameDialog } from "./CodingSessionNameDialog";
import { CodingSessionTranscript } from "./CodingSessionTranscript";
import { CodingSessionChangesRail } from "./CodingSessionChangesRail";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail";
import {
  CodingSessionSurfaceHost,
  useCodingSessionSurfaceHostState,
  type CodingSessionSurfaceDescriptor,
} from "./CodingSessionSurfaceHost";
import {
  CodingSessionUmbrellaComposer,
  type CodingSessionUmbrellaComposerPrefill,
} from "./CodingSessionUmbrellaComposer";

/**
 * The umbrella surface: one time-ordered narrative interleaved at turn-block
 * granularity across N executions, plus the conversation lane. Mounted only
 * when an umbrella actually has more than one execution — an umbrella of one
 * renders today's single-session tree and never sees this component.
 */
export function UmbrellaCodingSessionWorkspace({
  channelId,
  channelName,
  generationId,
  isMember,
  onAddProvider,
  onCloseSession,
  onReopenSession,
  onBack,
  surface,
  umbrella,
  focusedExecution,
  currentUserPubkey,
  goal,
  sessionName = null,
  sessionClosed = false,
}: {
  channelId: string;
  channelName: string | null;
  generationId: string;
  isMember: boolean;
  /** Opens the join flow (design §B); absent when this session cannot join. */
  onAddProvider?: () => void;
  onCloseSession?: () => void;
  onReopenSession?: () => void;
  onBack: () => void;
  surface: CodingSessionSurface;
  umbrella: CodingSessionUmbrellaRecord;
  focusedExecution: CodingSessionExecution;
  currentUserPubkey: string | null;
  goal: CodingSessionGoal | null;
  sessionName?: CodingSessionName | null;
  sessionClosed?: boolean;
}) {
  const identity = useIdentityQuery();
  const lane = useCodingSessionLane(channelId, umbrella.sessionRef);
  const [prefill, setPrefill] =
    React.useState<CodingSessionUmbrellaComposerPrefill | null>(null);
  const [workspaceBodyRef, bodyWidthPx] = useElementWidth<HTMLDivElement>();
  const isNarrow = bodyWidthPx > 0 && bodyWidthPx < 960;
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
  // The umbrella surface keeps its historical default: agents visible on open.
  const surfaceHost = useCodingSessionSurfaceHostState(surfaceIds, {
    initialTab: "agents",
  });
  const surfaceHostId = React.useId();
  const [renameOpen, setRenameOpen] = React.useState(false);
  const authoritativeTitle = sessionName?.content ?? umbrella.title;
  const canRename =
    umbrella.sessionRef !== null &&
    umbrella.founderPubkey !== null &&
    currentUserPubkey?.toLowerCase() === umbrella.founderPubkey.toLowerCase();

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
          channelName={channelName}
          generationLabel={`${umbrella.executions.length} executions`}
          onAddProvider={onAddProvider}
          onBack={onBack}
          onCloseSession={onCloseSession}
          onPopout={surface === "main" ? handlePopout : undefined}
          onRename={canRename ? () => setRenameOpen(true) : undefined}
          onReopenSession={onReopenSession}
          onToggleSurface={(id) => surfaceHost.toggle(id)}
          providerAuthorityPubkey={focusedExecution.signerPubkey}
          sessionTitle={authoritativeTitle}
          sessionClosed={sessionClosed}
          status={umbrellaWorkspaceStatus(umbrella)}
          surfaceHostId={surfaceHostId}
          surfaceTabs={surfaces.map((surfaceEntry) => ({
            id: surfaceEntry.id,
            label: surfaceEntry.label,
            icon: surfaceEntry.id === "agents" ? "agents" : "changes",
            count: surfaceEntry.count ?? 0,
            active: surfaceHost.activeTab === surfaceEntry.id,
          }))}
        />
        <CodingSessionFounderLine
          founderPubkey={umbrella.founderPubkey}
          genesisRef={umbrella.genesisRef}
        />
        <div className="px-5 pb-2 sm:px-8">
          <CodingSessionGoalPill
            channelId={channelId}
            currentUserPubkey={currentUserPubkey}
            founderPubkey={umbrella.founderPubkey}
            goal={goal}
            sessionRef={umbrella.sessionRef}
          />
        </div>
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
          <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">
            <div className="mx-auto min-h-full w-full max-w-3xl px-5 pt-7 pb-64 sm:px-8">
              <CodingSessionUmbrellaTimelineView
                channelId={channelId}
                currentUserPubkey={currentUserPubkey}
                laneMessages={lane.messages}
                onHandoff={setPrefill}
                operatorProfiles={operatorProfiles}
                umbrella={umbrella}
              />
            </div>
          </div>
          {!sessionClosed ? (
            <div className="pointer-events-none absolute inset-x-0 bottom-0 z-20 bg-linear-to-b from-transparent via-background/85 to-background px-4 pt-8 pb-4">
              <div className="pointer-events-auto mx-auto w-full max-w-3xl">
                <CodingSessionUmbrellaComposer
                  channelId={channelId}
                  currentUserPubkey={identity.data?.pubkey ?? null}
                  isMember={isMember}
                  prefill={prefill}
                  umbrella={umbrella}
                />
              </div>
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
    </main>
  );
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
  laneMessages,
  onHandoff,
  operatorProfiles,
  umbrella,
}: {
  channelId: string;
  /**
   * The viewer's own pubkey, forwarded to each block's transcript so a prompt
   * sent by another operator is attributed to them instead of to the reader.
   */
  currentUserPubkey?: string | null;
  laneMessages: readonly CodingSessionLaneMessage[];
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
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

  // Provenance resolution for handoff chips: the `buzz://coding-session` link
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

  if (entries.length === 0) {
    return (
      <p
        className="py-10 text-center text-sm text-muted-foreground"
        data-testid="coding-session-umbrella-timeline-empty"
      >
        No activity in this session yet.
      </p>
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
          <UmbrellaTurnBlock
            block={entry}
            blockKey={key}
            channelId={channelId}
            currentUserPubkey={currentUserPubkey}
            isHighlighted={revealed?.key === key}
            isWorking={workingBlockKeys.has(key)}
            key={key}
            label={
              labelsByExecutionKey.get(entry.executionKey) ??
              truncatePubkey(entry.signerPubkey)
            }
            labelsByExecutionKey={labelsByExecutionKey}
            onHandoff={onHandoff}
            onRegisterNode={registerBlockNode}
            operatorProfiles={operatorProfiles}
            onRevealFact={revealFact}
            record={recordsByGenerationId.get(entry.generationId) ?? null}
            resolveFactLocation={resolveFactLocation}
            showProvenance={shouldShowTurnBlockProvenance(entries, index)}
            umbrella={umbrella}
          />
        );
      })}
    </div>
  );
}

/**
 * One execution's turn block: provenance strip (provider label + signer,
 * mirroring the header's provenance popover fields), the block's items
 * rendered by the existing transcript renderer, a handoff chip when the
 * prompt carries a recognizable provenance block, and "Send to ⟨execution⟩"
 * on completed blocks.
 */
function UmbrellaTurnBlock({
  block,
  blockKey,
  channelId,
  currentUserPubkey,
  isHighlighted,
  isWorking,
  label,
  labelsByExecutionKey,
  onHandoff,
  onRegisterNode,
  onRevealFact,
  operatorProfiles,
  record,
  resolveFactLocation,
  showProvenance,
  umbrella,
}: {
  block: CodingSessionUmbrellaTurnBlock;
  blockKey: string;
  channelId: string;
  currentUserPubkey: string | null;
  isHighlighted: boolean;
  isWorking: boolean;
  label: string;
  labelsByExecutionKey: ReadonlyMap<string, string>;
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
  onRegisterNode: (key: string, node: HTMLElement | null) => void;
  onRevealFact: (key: string) => void;
  operatorProfiles: UserProfileLookup | undefined;
  record: CodingSessionCatalogRecord | null;
  resolveFactLocation: (link: CodingSessionHandoffLink) => string | null;
  showProvenance: boolean;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const prompt = readCodingSessionTurnBlockPrompt(block);
  const handoff = prompt ? parseCodingSessionHandoffPrefill(prompt.text) : null;
  const isForeign =
    umbrella.founderPubkey !== null &&
    (umbrella.executions.find(
      (execution) => execution.executionKey === block.executionKey,
    )?.operatorPubkey ?? null) !== null &&
    umbrella.executions.find(
      (execution) => execution.executionKey === block.executionKey,
    )?.operatorPubkey !== umbrella.founderPubkey;
  const handoffTargets = umbrella.executions.filter(
    (execution) => execution.executionKey !== block.executionKey,
  );
  const completed = isCompletedCodingSessionTurnBlock(block);
  const source = completed ? resolveCodingSessionHandoffSource(block) : null;
  const sourceLocation =
    handoff?.link != null ? resolveFactLocation(handoff.link) : null;
  const registerNode = React.useCallback(
    (node: HTMLElement | null) => onRegisterNode(blockKey, node),
    [blockKey, onRegisterNode],
  );

  return (
    <article
      className={cn(
        "relative py-1 transition-colors",
        isHighlighted &&
          "-mx-3 rounded-2xl bg-primary/5 px-3 ring-1 ring-primary/60",
      )}
      data-block={blockKey}
      data-execution={block.executionKey}
      data-highlighted={isHighlighted ? "true" : undefined}
      data-signer={block.signerPubkey}
      data-testid="coding-session-umbrella-turn-block"
      ref={registerNode}
    >
      <span className="sr-only">
        Response from {label}, signer {truncatePubkey(block.signerPubkey)},
        generation {block.generation}.
      </span>
      {showProvenance ? (
        <header
          className="mb-3 flex flex-wrap items-center gap-2"
          data-testid="coding-session-umbrella-provenance"
        >
          <span className="rounded-full bg-muted/70 px-2.5 py-1 text-xs font-medium">
            {label}
          </span>
          <span
            className="font-mono text-2xs text-muted-foreground"
            title="Fact-stream signer for every item in this execution run"
          >
            {truncatePubkey(block.signerPubkey)}
          </span>
          <span className="text-2xs text-muted-foreground">
            generation {block.generation}
          </span>
          {isForeign ? (
            <span
              className="inline-flex items-center gap-1 rounded-md bg-amber-500/10 px-1.5 py-0.5 text-2xs text-amber-700 dark:text-amber-300"
              data-testid="coding-session-umbrella-foreign-flag"
              title="This execution was attached by an operator other than the session founder."
            >
              <Flag aria-hidden className="size-3" />
              foreign
            </span>
          ) : null}
        </header>
      ) : null}
      {handoff ? (
        <p
          className="mb-2 inline-flex flex-wrap items-center gap-1.5 rounded-lg bg-primary/10 px-2 py-1 text-xs"
          data-testid="coding-session-umbrella-handoff-chip"
        >
          <ArrowRightLeft aria-hidden className="size-3.5" />
          <span>
            Handoff from {handoff.sourceLabel} → {label}
          </span>
          {handoff.link === null ? null : sourceLocation !== null ? (
            <button
              className="underline underline-offset-2"
              data-testid="coding-session-umbrella-view-source"
              onClick={() => onRevealFact(sourceLocation)}
              type="button"
            >
              View source
            </button>
          ) : (
            // The quoted fact is not in this view (another channel, a
            // generation this surface has not ingested). A dead anchor would
            // leak an unhandled scheme to the OS, so the provenance stays
            // visible but inert — the durable link is still in the prompt text.
            <span
              className="text-muted-foreground"
              data-testid="coding-session-umbrella-source-unavailable"
              title={handoff.linkUrl}
            >
              source not in this view
            </span>
          )}
        </p>
      ) : null}
      <CodingSessionTranscript
        currentUserPubkey={currentUserPubkey}
        generationId={block.generationId}
        isWorking={isWorking}
        items={block.items}
        operatorProfiles={operatorProfiles}
      />
      {completed && source && handoffTargets.length > 0 ? (
        <footer
          className="mt-2 flex flex-wrap items-center gap-1.5"
          data-testid="coding-session-umbrella-handoff-actions"
        >
          {handoffTargets.map((execution) => (
            <button
              className="inline-flex items-center gap-1 rounded-full border border-border/70 px-2.5 py-1 text-2xs text-muted-foreground transition-colors hover:text-foreground"
              data-testid="coding-session-umbrella-send-to"
              key={execution.executionKey}
              onClick={() =>
                onHandoff(
                  buildUmbrellaTurnBlockHandoff({
                    block,
                    channelId,
                    quote: source.quote,
                    eventSeq: source.eventSeq,
                    record,
                    sourceLabel: label,
                    targetExecutionKey: execution.executionKey,
                  }),
                )
              }
              type="button"
            >
              <ArrowRightLeft aria-hidden className="size-3" />
              Send to{" "}
              {labelsByExecutionKey.get(execution.executionKey) ??
                truncatePubkey(execution.signerPubkey)}
            </button>
          ))}
        </footer>
      ) : null}
    </article>
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
      <p className="mt-0.5 text-base whitespace-pre-wrap">{message.content}</p>
    </div>
  );
}

function formatLaneTimestamp(timestampMs: number): string {
  const date = new Date(timestampMs);
  return Number.isFinite(date.getTime())
    ? date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })
    : "";
}

/**
 * Build the composer prefill for "Send to ⟨execution⟩". With a resolvable
 * signed-fact coordinate the provenance line carries the deep link; without
 * one it degrades to a plain quoted block, which old and new clients alike
 * render as an ordinary prompt with a visible quote.
 */
export function buildUmbrellaTurnBlockHandoff(input: {
  block: CodingSessionUmbrellaTurnBlock;
  channelId: string;
  quote: string;
  eventSeq: number | null;
  record: CodingSessionCatalogRecord | null;
  sourceLabel: string;
  targetExecutionKey: string;
}): CodingSessionUmbrellaComposerPrefill {
  const target = input.record?.commandTarget ?? null;
  const text =
    target !== null && input.eventSeq !== null
      ? buildCodingSessionHandoffPrefill({
          sourceLabel: input.sourceLabel,
          link: {
            channelId: input.channelId,
            targetKey: buildCodingSessionTargetKey(target),
            eventSeq: input.eventSeq,
          },
          quote: input.quote,
        })
      : `> From ${input.sourceLabel} (this session)\n${input.quote
          .trim()
          .split("\n")
          .map((line) => `> ${line}`)
          .join("\n")}\n\n`;
  return {
    id: `handoff:${input.block.generationId}:${input.block.turnId ?? "no-turn"}:${input.targetExecutionKey}:${Date.now()}`,
    participantKey: `execution:${input.targetExecutionKey}`,
    text,
  };
}

/** Map the umbrella's derived status onto the header's three honest states. */
export function umbrellaWorkspaceStatus(
  umbrella: Pick<CodingSessionUmbrellaRecord, "status">,
): CodingSessionWorkspaceStatus {
  return codingSessionWireWorkspaceStatus(umbrella.status);
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
