import * as React from "react";

import { useCodingSessionObservations } from "@/features/coding-sessions/hooks/useCodingSessionObservations";
import type { CodingSessionMissionTransactionInput } from "@/features/coding-sessions/lib/codingSessionMissionContracts";
import type { CodingSessionMissionDecisionInput } from "@/features/coding-sessions/lib/codingSessionMissionDecisions";
import {
  deriveCodingSessionMissionOpenHolds,
  type CodingSessionMissionOpenHolds,
} from "@/features/coding-sessions/lib/codingSessionMissionOpenHolds";
import {
  buildCodingSessionMissionTransactionRows,
  type CodingSessionMissionActorResolver,
} from "@/features/coding-sessions/lib/codingSessionMissionTransactionRows";
import type { CodingSessionMissionEvidenceScope } from "@/features/coding-sessions/lib/codingSessionMissionEvidenceModel";
import { deriveCodingSessionObservationView } from "@/features/coding-sessions/lib/codingSessionObservationView";
import type { CodingSessionObservationFold } from "@/features/coding-sessions/lib/codingSessionObservationWire";
import {
  buildCodingSessionRouteTransactions,
  type CodingSessionRouteParticipant,
} from "@/features/coding-sessions/lib/codingSessionRouteTypes";
import { buildCodingSessionTurnByline } from "@/features/coding-sessions/lib/codingSessionTurnByline";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import type { CodingSessionActorNameResolver } from "@/features/coding-sessions/lib/codingSessionUmbrellaModel";
import { useCodingSessionMissionEvidence } from "@/features/coding-sessions/lib/useCodingSessionMissionEvidence";
import { useStableArrayShallow } from "@/shared/hooks/useStableReference";

import {
  type CodingSessionSurfaceBaseCtx,
  type CodingSessionSurfaceObservations,
  codingSessionSurfaceDecisionRequestRows,
} from "./codingSessionSurfaceContext";

/**
 * The single layout's team reads for `ctx`: the observations and the open
 * rulings, from one Mission-evidence read and one observations read.
 *
 * The umbrella layout gets both from Mission's own hook
 * (`useCodingSessionMissionSurface`), which reads the same evidence in both
 * lenses; this is the single layout's equivalent, so a genesis session never
 * has a ctx without its rulings just because it routed to one execution.
 */

/**
 * The Mission evidence scope for a session: its channel, session, genesis
 * and founder, or `null` when any is missing. Takes no lens — the evidence is
 * read in Conversation and Mission alike, so `ctx.openRulings` exists in
 * both (B0, SV-38).
 */
export function codingSessionSurfaceEvidenceScope(
  channelId: string,
  umbrella: Pick<
    CodingSessionUmbrellaRecord,
    "founderPubkey" | "genesisRef" | "sessionRef"
  >,
): CodingSessionMissionEvidenceScope | null {
  const { founderPubkey, genesisRef, sessionRef } = umbrella;
  return sessionRef !== null && genesisRef !== null && founderPubkey !== null
    ? { channelRef: channelId, sessionRef, genesisRef, founderPubkey }
    : null;
}

/** A fold with its unresolved-pointer disclosure removed: "not checked". */
export function codingSessionObservationFoldUnchecked(
  fold: CodingSessionObservationFold | null,
): CodingSessionObservationFold | null {
  if (fold === null) return null;
  return {
    ...fold,
    unresolved: [],
    truncated: { ...fold.truncated, unresolved: 0 },
  };
}

/**
 * Names a transaction's party from the umbrella's own executions, as the
 * umbrella workspace's `resolveMissionActor` does, so an open-ruling line
 * names a seat the same way in both layouts.
 */
export function codingSessionSurfaceMissionActorResolver(
  umbrella: CodingSessionUmbrellaRecord,
  resolveActorName: CodingSessionActorNameResolver,
): CodingSessionMissionActorResolver {
  return (pubkey) => {
    const execution = umbrella.executions.find(
      (candidate) =>
        candidate.activeGeneration.agentRef?.toLowerCase() ===
        pubkey.toLowerCase(),
    );
    if (!execution) {
      return { label: resolveActorName(pubkey) ?? null, executionKey: null };
    }
    const record = execution.activeGeneration;
    return {
      label: buildCodingSessionTurnByline({
        agentDisplayName: record.agentRef
          ? (resolveActorName(record.agentRef) ?? null)
          : null,
        agentRef: record.agentRef,
        generation: record.commandTarget?.generation ?? 1,
        label: null,
        model: record.model,
        role: record.role,
        runtime: record.runtime,
      }).name,
      executionKey: execution.executionKey,
    };
  };
}

/**
 * Open rulings from signed transactions — the same derivation the umbrella's
 * Route rail feeds (`useCodingSessionRoute`), over the same rows.
 */
export function deriveCodingSessionSurfaceOpenRulings(input: {
  umbrella: CodingSessionUmbrellaRecord;
  transactions: readonly CodingSessionMissionTransactionInput[];
  resolveActorName: CodingSessionActorNameResolver;
  /** `you` when the viewer is the founder, else the founder's name. */
  founderLabel?: string;
  nowMs: number;
}): CodingSessionMissionOpenHolds {
  const resolveActor = codingSessionSurfaceMissionActorResolver(
    input.umbrella,
    input.resolveActorName,
  );
  const participants: CodingSessionRouteParticipant[] =
    input.umbrella.executions.map((execution) => {
      const record = execution.activeGeneration;
      return {
        executionKey: execution.executionKey,
        label:
          (record.agentRef ? resolveActor(record.agentRef).label : null) ??
          record.title,
        actorPubkey: record.agentRef,
        role: record.role,
        targetKey: null,
        word: null,
        live: false,
        firstSignedAt: null,
        releasedAt: null,
      };
    });
  const rows = buildCodingSessionRouteTransactions(
    buildCodingSessionMissionTransactionRows({
      transactions: input.transactions,
      resolveActor,
      founderPubkey: input.umbrella.founderPubkey,
      density: "brief",
    }),
    input.transactions,
  );
  return deriveCodingSessionMissionOpenHolds({
    founderLabel: input.founderLabel,
    founderPubkey: input.umbrella.founderPubkey,
    nowMs: input.nowMs,
    participants,
    transactions: rows,
  });
}

/** What the founder's own party is called in a ruling line. */
export function codingSessionFounderHoldLabel(input: {
  currentUserPubkey: string | null;
  founderPubkey: string | null;
  resolveActorName: CodingSessionActorNameResolver;
}): string | undefined {
  if (input.founderPubkey === null) return undefined;
  if (
    input.currentUserPubkey !== null &&
    input.currentUserPubkey.toLowerCase() === input.founderPubkey.toLowerCase()
  ) {
    return "you";
  }
  return input.resolveActorName(input.founderPubkey) ?? undefined;
}

/**
 * The single layout's observations and open rulings.
 *
 * Reads the Mission evidence only when the session has a genesis and a
 * founder (the evidence's own scope); its assignments resolve the
 * observations' pointers and its transactions yield the open rulings. Until
 * it has been read, the rulings are `null` and the observations say their
 * pointers were not checked.
 */
export function useCodingSessionSurfaceTeamRead(input: {
  channelId: string;
  currentUserPubkey: string | null;
  resolveActorName: CodingSessionActorNameResolver;
  umbrella: CodingSessionUmbrellaRecord;
}): {
  observations: CodingSessionSurfaceObservations;
  openRulings: CodingSessionMissionOpenHolds | null;
  /** The fold's `decisions[]` (DB8), or `null` until it has been read. */
  decisions: readonly CodingSessionMissionDecisionInput[] | null;
  /** The signed `decision.request` rows behind `decisions`, or `null` unread. */
  decisionRequests: CodingSessionSurfaceBaseCtx["decisionRequests"];
  /** The fold's signed 44244 transactions, or `null` until read. */
  teamTransactions: CodingSessionSurfaceBaseCtx["teamTransactions"];
} {
  const { founderPubkey, genesisRef, sessionRef } = input.umbrella;
  const evidenceScope = React.useMemo(
    () =>
      codingSessionSurfaceEvidenceScope(input.channelId, {
        founderPubkey,
        genesisRef,
        sessionRef,
      }),
    [founderPubkey, genesisRef, input.channelId, sessionRef],
  );
  const evidence = useCodingSessionMissionEvidence(evidenceScope);
  const evidenceRead =
    evidenceScope !== null &&
    !evidence.isLoading &&
    evidence.errorMessage === null;
  const knownAssignmentRefs = useStableArrayShallow(
    (evidence.inspectorInput.assignments ?? []).map(
      (assignment) => assignment.sourceEventId,
    ),
  );
  const knownDecisionRefs = useStableArrayShallow(
    (evidence.inspectorInput.decisions ?? []).map(
      (decision) => decision.requestId,
    ),
  );
  const scope = React.useMemo(
    () =>
      sessionRef !== null && genesisRef !== null
        ? { channelRef: input.channelId, sessionRef, genesisRef }
        : null,
    [genesisRef, input.channelId, sessionRef],
  );
  const providerPubkeys = useStableArrayShallow(
    input.umbrella.executions
      .map((execution) => execution.signerPubkey)
      .filter((pubkey) => pubkey.length > 0),
  );
  const read = useCodingSessionObservations(
    scope,
    knownAssignmentRefs,
    providerPubkeys,
  );
  const view = React.useMemo(() => {
    const fold = read.result?.fold ?? null;
    return deriveCodingSessionObservationView({
      fold: evidenceRead ? fold : codingSessionObservationFoldUnchecked(fold),
      resolveLabel: (pubkey) => input.resolveActorName(pubkey),
      knownDecisionRefs,
    });
  }, [evidenceRead, input.resolveActorName, knownDecisionRefs, read.result]);
  const observations = React.useMemo<CodingSessionSurfaceObservations>(
    () =>
      scope === null
        ? {
            state: "not-read",
            reason: "This session has no genesis, so it has no observations.",
          }
        : {
            state: "read",
            isLoading: read.isLoading,
            errorMessage: read.errorMessage,
            result: read.result,
            view,
            assignmentsChecked: evidenceRead,
            readAtMs: read.readAtMs,
            live: read.live,
            refresh: read.refresh,
          },
    [
      evidenceRead,
      read.errorMessage,
      read.isLoading,
      read.live,
      read.readAtMs,
      read.refresh,
      read.result,
      scope,
      view,
    ],
  );
  const founderLabel = codingSessionFounderHoldLabel({
    currentUserPubkey: input.currentUserPubkey,
    founderPubkey,
    resolveActorName: input.resolveActorName,
  });
  const transactions = evidence.inspectorInput.transactions;
  const openRulings = React.useMemo(
    () =>
      evidenceRead
        ? deriveCodingSessionSurfaceOpenRulings({
            umbrella: input.umbrella,
            transactions: transactions ?? [],
            resolveActorName: input.resolveActorName,
            founderLabel,
            nowMs: Date.now(),
          })
        : null,
    [
      evidenceRead,
      founderLabel,
      input.resolveActorName,
      input.umbrella,
      transactions,
    ],
  );
  const decisions = evidenceRead
    ? (evidence.inspectorInput.decisions ?? null)
    : null;
  const requestInputs = evidence.inspectorInput.decisionRequests;
  const decisionRequests = React.useMemo(
    () =>
      evidenceRead
        ? codingSessionSurfaceDecisionRequestRows(requestInputs)
        : null,
    [evidenceRead, requestInputs],
  );
  const teamTransactions = evidenceRead ? (transactions ?? []) : null;
  return {
    observations,
    openRulings,
    decisions,
    decisionRequests,
    teamTransactions,
  };
}
