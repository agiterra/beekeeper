import * as React from "react";

import { useCodingSessionAssignmentInputs } from "@/features/coding-sessions/hooks/useCodingSessionAssignmentInputs";
import type { CodingSessionLaneMessage } from "@/features/coding-sessions/lib/codingSessionConversationLane";
import { buildCodingSessionTargetKey } from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  resolveCodingSessionHandoffFactLocation,
  type CodingSessionHandoffLink,
} from "@/features/coding-sessions/lib/codingSessionHandoff";
import type { CodingSessionMissionDensity } from "@/features/coding-sessions/lib/codingSessionMissionDensity";
import type {
  CodingSessionMissionTransactionInput,
  CodingSessionTeamWakeDelivery,
} from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import {
  missionRowClass,
  missionRowMetaClass,
} from "@/features/coding-sessions/lib/codingSessionMissionRowGrammar";
import {
  codingSessionMissionStreamEntryKey,
  projectCodingSessionMissionTimeline,
} from "@/features/coding-sessions/lib/codingSessionMissionStreamModel";
import {
  buildCodingSessionMissionTransactionRows,
  type CodingSessionMissionActorResolver,
} from "@/features/coding-sessions/lib/codingSessionMissionTransactionRows";
import type {
  CodingSessionCatalogRecord,
  CodingSessionUmbrellaRecord,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  buildUmbrellaTimeline,
  codingSessionUmbrellaEntryKey,
  type CodingSessionUmbrellaTimelineEntry,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaTimeline";
import {
  listCodingSessionUmbrellaParticipants,
  type CodingSessionActorNameResolver,
} from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import type { CodingSessionWakeOperationIndex } from "@/features/coding-sessions/lib/codingSessionWakeReading";
import { CODING_SESSION_UNKNOWN_ACTOR } from "@/features/coding-sessions/lib/codingSessionTurnByline";
import type { UserProfileLookup } from "@/features/profile/lib/identity";
import { cn } from "@/shared/lib/cn";
import {
  blockTargetKey,
  resolveWorkingBlockKeys,
  shouldShowTurnBlockProvenance,
} from "./CodingSessionUmbrellaWorkspaceModel";
import { CodingSessionMissionTraceDetails } from "./CodingSessionMissionTraceDetails";
import { CodingSessionMissionTransactionRow } from "./CodingSessionMissionTransactionRow";
import { CodingSessionPendingTurns } from "./CodingSessionPendingTurns";
import type { CodingSessionUmbrellaComposerPrefill } from "./CodingSessionUmbrellaComposer";
import type { CodingSessionPromptSeatResolver } from "@/features/coding-sessions/lib/codingSessionPromptAttribution";
import { UmbrellaConversationRow } from "./CodingSessionUmbrellaConversationRow";
import {
  CodingSessionUmbrellaTurnBlock,
  type CodingSessionTurnBlockLiveness,
} from "./CodingSessionUmbrellaTurnBlock";

/** The turn block's Mission shell: the card grammar, keeping the accent rail. */
const MISSION_TURN_BLOCK_CLASS = missionRowClass("standard", {
  className: "border-l-2",
});
/** The conversation row's Mission shell — Conversation never receives it. */
const MISSION_CONVERSATION_ROW_CLASS = missionRowClass("standard");

/** One chronological umbrella narrative, optionally projected through Mission density. */
export function CodingSessionUmbrellaTimelineView({
  actorNames,
  channelId,
  currentUserPubkey = null,
  focusedExecutionKey = null,
  laneMessages,
  missionDeliveries,
  missionDensity = null,
  missionFounderPubkey = null,
  missionLiveness,
  missionTransactions,
  onHandoff,
  onFocusExecution,
  onMissionVisibleTimesChange,
  missionRevealRef,
  operatorProfiles,
  resolveMissionActor,
  resolvePromptSeat,
  umbrella,
  wakeOperations,
}: {
  channelId: string;
  currentUserPubkey?: string | null;
  focusedExecutionKey?: string | null;
  laneMessages: readonly CodingSessionLaneMessage[];
  /** Delivery evidence per operation source; badged on the row that owns it. */
  missionDeliveries?: readonly CodingSessionTeamWakeDelivery[];
  missionDensity?: CodingSessionMissionDensity | null;
  /** Founder identity, so the founder's own rows read `You` and not a key. */
  missionFounderPubkey?: string | null;
  /**
   * Execution key → that seat's W1 answer, from the roster's own resolvers.
   * Only a working block renders it; the map is the single source so the
   * byline, the chip and the live strip cannot disagree — about the word *or*
   * about whether the seat is actually working (REVIEW-A3 F3).
   */
  missionLiveness?: ReadonlyMap<string, CodingSessionTurnBlockLiveness>;
  /** Signed 44244 transactions. Rendered only while a Mission density is set. */
  missionTransactions?: readonly CodingSessionMissionTransactionInput[];
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
  onFocusExecution?: (executionKey: string | null) => void;
  /**
   * Reports the signed times of the rows currently inside the scroller, so the
   * Route rail's `You are here` band can follow the reader.
   *
   * An IntersectionObserver over the rows this view already registers — never
   * a timer, never a scroll handler. Mission-only: Conversation passes nothing
   * and no observer is created.
   */
  onMissionVisibleTimesChange?: (seconds: readonly number[]) => void;
  /**
   * Publishes this view's own `revealFact` so a sibling — the Route rail, which
   * lives outside the scroller — can scroll a row into view and ring it. A ref
   * rather than a callback prop so publishing it does not re-render the stream.
   */
  missionRevealRef?: React.MutableRefObject<((key: string) => void) | null>;
  operatorProfiles?: UserProfileLookup;
  actorNames?: CodingSessionActorNameResolver;
  /** Pubkey → seat name for transaction rows; the finalizer supplies it. */
  resolveMissionActor?: CodingSessionMissionActorResolver;
  /**
   * Names a seat from its actor pubkey so a turn one seat sent to another is
   * attributed to that seat. Only reaches the rows in Mission — Conversation
   * has a single seat and no second author to confuse it with.
   */
  resolvePromptSeat?: CodingSessionPromptSeatResolver;
  umbrella: CodingSessionUmbrellaRecord;
  /**
   * Fold-resolved operations for the wake reading (finding 17). Not gated on
   * `missionDensity`: §1f's sentence is the same line in both lenses, so the
   * prop is passed straight through in both.
   */
  wakeOperations?: CodingSessionWakeOperationIndex;
}) {
  const participants = React.useMemo(
    () => listCodingSessionUmbrellaParticipants(umbrella, actorNames),
    [actorNames, umbrella],
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
  const transactionRows = React.useMemo(() => {
    if (missionDensity === null) return undefined;
    if (!missionTransactions || missionTransactions.length === 0) {
      return undefined;
    }
    return buildCodingSessionMissionTransactionRows({
      transactions: missionTransactions,
      resolveActor:
        resolveMissionActor ?? (() => ({ label: null, executionKey: null })),
      founderPubkey: missionFounderPubkey,
      deliveries: missionDeliveries,
      density: missionDensity,
    });
  }, [
    missionDeliveries,
    missionDensity,
    missionFounderPubkey,
    missionTransactions,
    resolveMissionActor,
  ]);
  // Every signed assignment, as the host takes them. The *host* decides which
  // need an input established — role and a named revision — so nothing here
  // filters, and a session with no verifier costs no establishment.
  const assignmentInputTargets = React.useMemo(
    () =>
      (missionTransactions ?? [])
        .filter((transaction) => transaction.type === "assignment")
        .map((transaction) => ({
          assignmentId: transaction.sourceEventId,
          assigneeActor: transaction.counterpartyPubkey,
          assigneeRole: transaction.assigneeRole ?? null,
          baseSha: transaction.baseSha ?? null,
        })),
    [missionTransactions],
  );
  // The seat label a worktree was recorded under is the seated agent's own
  // name, which is exactly what this resolver answers (`identity.name` in
  // `codingSessionHireSeat.ts`). No name, no recorded tree to name — and the
  // trigger discloses that rather than calling the host.
  const resolveSeatLabel = React.useCallback(
    (actor: string) => actorNames?.(actor) ?? null,
    [actorNames],
  );
  // Reported whatever the density: handing the host an observation is how it
  // learns the work exists, and gating that on which panel is open is the
  // defect this stopped being (ledger 185). Mission is still the only surface
  // that *draws* a row.
  const verificationInputs = useCodingSessionAssignmentInputs({
    assignments: assignmentInputTargets,
    resolveSeatLabel,
    sessionRef: umbrella.sessionRef,
  });
  const entries = React.useMemo(() => {
    const chronological = buildUmbrellaTimeline(umbrella, laneMessages);
    return missionDensity
      ? projectCodingSessionMissionTimeline(
          chronological,
          missionDensity,
          transactionRows,
        )
      : chronological;
  }, [laneMessages, missionDensity, transactionRows, umbrella]);
  // Provenance, working-block resolution and handoff sources are facts about
  // the narrative alone; a transaction row is never one of their neighbours.
  const narrativeEntries = React.useMemo(
    () =>
      entries.filter(
        (entry): entry is CodingSessionUmbrellaTimelineEntry =>
          entry.kind !== "transaction" &&
          entry.kind !== "transaction-truncation",
      ),
    [entries],
  );
  const narrativeIndexByKey = React.useMemo(() => {
    const indexes = new Map<string, number>();
    narrativeEntries.forEach((entry, index) => {
      indexes.set(codingSessionUmbrellaEntryKey(entry), index);
    });
    return indexes;
  }, [narrativeEntries]);
  const workingBlockKeys = React.useMemo(
    () => resolveWorkingBlockKeys(umbrella, narrativeEntries),
    [narrativeEntries, umbrella],
  );
  const factCandidates = React.useMemo(
    () =>
      narrativeEntries.flatMap((entry) =>
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
    [narrativeEntries, recordsByGenerationId],
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
  // Signed seconds per row key. Derived from the entries themselves rather
  // than registered alongside the node, so `onRegisterNode` keeps the stable
  // identity the turn block memoises on.
  const secondsByRowKey = React.useMemo(() => {
    const seconds = new Map<string, number>();
    for (const entry of entries) {
      if (entry.kind === "transaction-truncation") continue;
      const key = codingSessionMissionStreamEntryKey(entry);
      seconds.set(
        key,
        entry.kind === "transaction"
          ? entry.row.createdAt
          : Math.floor(entry.timestampMs / 1_000),
      );
    }
    return seconds;
  }, [entries]);
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
  React.useEffect(() => {
    if (missionRevealRef === undefined) return;
    missionRevealRef.current = revealFact;
    return () => {
      missionRevealRef.current = null;
    };
  }, [missionRevealRef, revealFact]);
  // The band's source: which registered rows are on screen right now. No
  // timer, no scroll listener, no author time — the observer reports entries
  // and each entry answers with the signed second it was registered under.
  const visibleRowKeys = React.useRef(new Set<string>());
  React.useEffect(() => {
    if (onMissionVisibleTimesChange === undefined) return;
    if (typeof IntersectionObserver === "undefined") return;
    const observer = new IntersectionObserver((observed) => {
      for (const record of observed) {
        const key = (record.target as HTMLElement).dataset.block;
        if (key === undefined) continue;
        if (record.isIntersecting) visibleRowKeys.current.add(key);
        else visibleRowKeys.current.delete(key);
      }
      const seconds: number[] = [];
      for (const key of visibleRowKeys.current) {
        const value = secondsByRowKey.get(key);
        if (value !== undefined) seconds.push(value);
      }
      onMissionVisibleTimesChange(seconds);
    });
    for (const node of blockNodes.current.values()) observer.observe(node);
    return () => observer.disconnect();
  }, [onMissionVisibleTimesChange, secondsByRowKey]);

  const pendingTurns = umbrella.executions.map((execution) => {
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
  });

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
          {missionDensity === "brief"
            ? "No attention or narrative events in Brief."
            : "No activity in this session yet."}
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
      {entries.map((entry) => {
        const key = codingSessionMissionStreamEntryKey(entry);
        if (entry.kind === "transaction") {
          // The rail points at these rows, so they have to be registerable
          // targets the way turn blocks already are. The wrapper exists only in
          // Mission — Conversation never renders a transaction row at all.
          return (
            <div
              data-block={key}
              data-highlighted={revealed?.key === key ? "true" : undefined}
              className={cn(
                revealed?.key === key &&
                  "-mx-3 rounded-2xl bg-primary/5 px-3 ring-1 ring-primary/60",
              )}
              key={key}
              ref={(node) => registerBlockNode(key, node)}
            >
              <CodingSessionMissionTransactionRow
                onRetryVerificationInput={() =>
                  verificationInputs.retry(entry.row.meta.sourceEventId)
                }
                row={entry.row}
                verificationInput={
                  verificationInputs.states.get(entry.row.meta.sourceEventId) ??
                  null
                }
              />
            </div>
          );
        }
        if (entry.kind === "transaction-truncation") {
          return (
            <p
              className={cn(
                missionRowClass("quiet"),
                missionRowMetaClass(),
                "text-center",
              )}
              data-testid="coding-session-mission-transaction-truncation"
              key={key}
              role="status"
            >
              {entry.hiddenCount} earlier transactions not shown
            </p>
          );
        }
        if (entry.kind === "conversation") {
          return (
            <UmbrellaConversationRow
              currentUserPubkey={currentUserPubkey}
              key={key}
              message={entry.message}
              missionRowClassName={
                missionDensity === null
                  ? undefined
                  : MISSION_CONVERSATION_ROW_CLASS
              }
              operatorProfiles={operatorProfiles}
              resolveSeat={
                missionDensity === null ? undefined : resolvePromptSeat
              }
            />
          );
        }
        if (entry.kind === "lifecycle") {
          const label =
            labelsByExecutionKey.get(entry.executionKey) ??
            CODING_SESSION_UNKNOWN_ACTOR;
          return (
            <p
              className={
                missionDensity === null
                  ? "text-center text-2xs text-muted-foreground"
                  : cn(missionRowClass("quiet"), "text-center text-2xs")
              }
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
        const record = recordsByGenerationId.get(entry.generationId) ?? null;
        return (
          <React.Fragment key={key}>
            {missionDensity === "trace" ? (
              <CodingSessionMissionTraceDetails block={entry} record={record} />
            ) : null}
            <CodingSessionUmbrellaTurnBlock
              actorNames={actorNames}
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
              label={labelsByExecutionKey.get(entry.executionKey) ?? null}
              labelsByExecutionKey={labelsByExecutionKey}
              liveness={
                missionDensity === null
                  ? null
                  : (missionLiveness?.get(entry.executionKey) ?? null)
              }
              missionCollapseSettled={missionDensity === "live"}
              missionExecutionBundle={missionDensity === "live"}
              missionRowClassName={
                missionDensity === null ? undefined : MISSION_TURN_BLOCK_CLASS
              }
              onHandoff={onHandoff}
              onFocusExecution={onFocusExecution}
              onRegisterNode={registerBlockNode}
              onRevealFact={revealFact}
              resolvePromptSeat={
                missionDensity === null ? undefined : resolvePromptSeat
              }
              operatorProfiles={operatorProfiles}
              record={record}
              resolveFactLocation={resolveFactLocation}
              showProvenance={shouldShowTurnBlockProvenance(
                narrativeEntries,
                narrativeIndexByKey.get(key) ?? -1,
              )}
              stickyProvenance={
                focusedExecutionKey === null && umbrella.executions.length > 1
              }
              umbrella={umbrella}
              wakeOperations={wakeOperations}
            />
          </React.Fragment>
        );
      })}
      {pendingTurns}
    </div>
  );
}
