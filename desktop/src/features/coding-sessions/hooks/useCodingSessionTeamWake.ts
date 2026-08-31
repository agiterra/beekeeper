import * as React from "react";
import { toast } from "sonner";

import { publishCodingSessionCommand } from "../lib/codingSessionCommand";
import {
  acknowledgeCodingSessionTeamWakes,
  baselineCodingSessionTeamWakes,
  buildCodingSessionTeamWakeCommandId,
  codingSessionTeamWakeStorageKey,
  codingSessionTeamWakeText,
  deriveCodingSessionTeamWakePlan,
  pendingCodingSessionTeamWake,
  readCodingSessionTeamWakeState,
  recordCodingSessionTeamWakeAttempt,
  writeCodingSessionTeamWakeState,
} from "../lib/codingSessionTeamWake";
import type { CodingSessionUmbrellaRecord } from "../lib/codingSessionTypes";
import { useCodingSessionMissionEvidence } from "../lib/useCodingSessionMissionEvidence";

const TERMINAL_OPERATION_GRACE_MS = 2_000;

function leadAcknowledgedCommandIds(
  umbrella: CodingSessionUmbrellaRecord,
): ReadonlySet<string> {
  const ids = new Set<string>();
  for (const execution of umbrella.executions) {
    if (execution.activeGeneration.role?.trim().toLowerCase() !== "lead") {
      continue;
    }
    for (const generation of [
      ...execution.priorGenerations,
      execution.activeGeneration,
    ]) {
      for (const item of generation.transcript) {
        if (
          item.type === "message" &&
          item.role === "user" &&
          typeof item.commandId === "string"
        ) {
          ids.add(item.commandId);
        }
      }
    }
  }
  return ids;
}

/**
 * Push one durable, identifier-only lead wake for signed team results.
 *
 * There is deliberately no polling loop. Signed 44244/44225 ingress changes
 * drive this hook, and a provider-signed user-prompt echo is the only delivery
 * acknowledgement. Relay acceptance alone remains pending and is retried once
 * on a later mount with the same deterministic command id.
 */
export function useCodingSessionTeamWake(input: {
  catalogSettled: boolean;
  channelId: string;
  communityScope: string;
  currentUserPubkey: string | null;
  sessionClosed: boolean;
  umbrella: CodingSessionUmbrellaRecord;
}): void {
  const governedScope = React.useMemo(
    () =>
      input.umbrella.executions.length > 1 &&
      input.umbrella.sessionRef &&
      input.umbrella.genesisRef &&
      input.umbrella.founderPubkey
        ? {
            channelRef: input.channelId,
            sessionRef: input.umbrella.sessionRef,
            genesisRef: input.umbrella.genesisRef,
            founderPubkey: input.umbrella.founderPubkey,
          }
        : null,
    [
      input.channelId,
      input.umbrella.executions.length,
      input.umbrella.founderPubkey,
      input.umbrella.genesisRef,
      input.umbrella.sessionRef,
    ],
  );
  const evidence = useCodingSessionMissionEvidence(governedScope);
  const plan = React.useMemo(
    () =>
      deriveCodingSessionTeamWakePlan({
        umbrella: input.umbrella,
        evidence: evidence.inspectorInput,
        currentUserPubkey: input.currentUserPubkey,
        sessionClosed: input.sessionClosed,
      }),
    [
      evidence.inspectorInput,
      input.currentUserPubkey,
      input.sessionClosed,
      input.umbrella,
    ],
  );
  const acknowledgedCommandIds = React.useMemo(
    () => leadAcknowledgedCommandIds(input.umbrella),
    [input.umbrella],
  );
  const storageKey =
    input.umbrella.sessionRef === null
      ? null
      : codingSessionTeamWakeStorageKey({
          communityScope: input.communityScope,
          channelId: input.channelId,
          sessionRef: input.umbrella.sessionRef,
        });
  const attemptedThisMount = React.useRef(new Set<string>());
  const failedThisMount = React.useRef(new Set<string>());
  const inFlight = React.useRef<string | null>(null);
  const activeStorageKey = React.useRef<string | null>(null);
  const [revision, setRevision] = React.useState(0);

  React.useEffect(() => {
    if (activeStorageKey.current === storageKey) return;
    activeStorageKey.current = storageKey;
    attemptedThisMount.current.clear();
    failedThisMount.current.clear();
    inFlight.current = null;
  }, [storageKey]);

  React.useEffect(() => {
    void revision;
    if (
      !storageKey ||
      !input.catalogSettled ||
      evidence.isLoading ||
      !plan.lead ||
      !plan.lead.activeGeneration.commandTarget ||
      inFlight.current !== null
    ) {
      return;
    }
    const storage = window.localStorage;
    let state = readCodingSessionTeamWakeState(storage, storageKey);
    if (state === null) {
      try {
        writeCodingSessionTeamWakeState(
          storage,
          storageKey,
          baselineCodingSessionTeamWakes(plan.candidates),
        );
      } catch {
        toast.error(
          "Automatic team delivery could not establish durable local state.",
        );
      }
      return;
    }
    const acknowledged = acknowledgeCodingSessionTeamWakes(
      state,
      acknowledgedCommandIds,
    );
    if (acknowledged.pending.length !== state.pending.length) {
      state = acknowledged;
      try {
        writeCodingSessionTeamWakeState(storage, storageKey, state);
      } catch {
        toast.error(
          "Automatic team delivery was acknowledged but could not persist its receipt.",
        );
        return;
      }
    }
    const target = plan.lead.activeGeneration.commandTarget;
    const candidate = pendingCodingSessionTeamWake({
      state,
      candidates: plan.candidates,
      leadTarget: target,
      acknowledgedCommandIds,
      attemptedThisMount: attemptedThisMount.current,
    });
    if (!candidate || failedThisMount.current.has(candidate.sourceEventId)) {
      return;
    }
    const waitMs =
      candidate.kind === "turn_ended_without_required_operation"
        ? candidate.sourceCreatedAtMs + TERMINAL_OPERATION_GRACE_MS - Date.now()
        : 0;
    if (waitMs > 0) {
      const timeout = window.setTimeout(
        () => setRevision((current) => current + 1),
        waitMs,
      );
      return () => window.clearTimeout(timeout);
    }
    inFlight.current = candidate.sourceEventId;
    let cancelled = false;
    void (async () => {
      try {
        const commandId = await buildCodingSessionTeamWakeCommandId(
          candidate,
          target,
        );
        if (cancelled) return;
        await publishCodingSessionCommand({
          channelId: input.channelId,
          commandId,
          target,
          text: codingSessionTeamWakeText(candidate),
          deliver: "boundary",
        });
        attemptedThisMount.current.add(commandId);
        const current =
          readCodingSessionTeamWakeState(storage, storageKey) ?? state;
        writeCodingSessionTeamWakeState(
          storage,
          storageKey,
          recordCodingSessionTeamWakeAttempt({
            state: current,
            candidate,
            commandId,
            leadTarget: target,
            acknowledgedCommandIds,
          }),
        );
      } catch (error) {
        failedThisMount.current.add(candidate.sourceEventId);
        const detail = error instanceof Error ? error.message : String(error);
        toast.error(`Automatic team delivery is unconfirmed: ${detail}`);
      } finally {
        if (!cancelled) {
          inFlight.current = null;
          setRevision((current) => current + 1);
        }
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [
    acknowledgedCommandIds,
    evidence.isLoading,
    input.catalogSettled,
    input.channelId,
    plan.candidates,
    plan.lead,
    revision,
    storageKey,
  ]);
}
