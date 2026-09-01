import * as React from "react";

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
import { UmbrellaConversationRow } from "./CodingSessionUmbrellaConversationRow";
import { CodingSessionUmbrellaTurnBlock } from "./CodingSessionUmbrellaTurnBlock";

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
  missionTransactions,
  onHandoff,
  onFocusExecution,
  operatorProfiles,
  resolveMissionActor,
  umbrella,
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
  /** Signed 44244 transactions. Rendered only while a Mission density is set. */
  missionTransactions?: readonly CodingSessionMissionTransactionInput[];
  onHandoff: (prefill: CodingSessionUmbrellaComposerPrefill) => void;
  onFocusExecution?: (executionKey: string | null) => void;
  operatorProfiles?: UserProfileLookup;
  actorNames?: CodingSessionActorNameResolver;
  /** Pubkey → seat name for transaction rows; the finalizer supplies it. */
  resolveMissionActor?: CodingSessionMissionActorResolver;
  umbrella: CodingSessionUmbrellaRecord;
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
          return (
            <CodingSessionMissionTransactionRow key={key} row={entry.row} />
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
              missionExecutionBundle={missionDensity === "live"}
              missionRowClassName={
                missionDensity === null ? undefined : MISSION_TURN_BLOCK_CLASS
              }
              onHandoff={onHandoff}
              onFocusExecution={onFocusExecution}
              onRegisterNode={registerBlockNode}
              onRevealFact={revealFact}
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
            />
          </React.Fragment>
        );
      })}
      {pendingTurns}
    </div>
  );
}
