import * as React from "react";
import { toast } from "sonner";

import { publishCodingSessionCommand } from "../lib/codingSessionCommand";
import {
  acknowledgeCodingSessionTeamWakes,
  baselineCodingSessionTeamWakes,
  buildCodingSessionTeamWakeCommandId,
  codingSessionTeamWakeFallbackNotBefore,
  codingSessionTeamWakeEvidenceIsComplete,
  codingSessionTeamWakeStorageKey,
  codingSessionTeamWakeText,
  deriveCodingSessionTeamWakePlan,
  pendingCodingSessionTeamWake,
  readCodingSessionTeamWakeState,
  recordCodingSessionTeamWakeAttempt,
  observeCodingSessionTeamWake,
  migrateCodingSessionTeamWakeCursor,
  writeCodingSessionTeamWakeState,
} from "../lib/codingSessionTeamWake";
import type { CodingSessionUmbrellaRecord } from "../lib/codingSessionTypes";
import { useCodingSessionMissionEvidence } from "../lib/useCodingSessionMissionEvidence";

const DESKTOP_FALLBACK_GRACE_MS = 15_000;

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

function leadAcknowledgedSourceEventIds(
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
        if (item.type !== "message" || item.role !== "user") continue;
        try {
          const value: unknown = JSON.parse(item.text);
          if (!value || typeof value !== "object" || Array.isArray(value)) {
            continue;
          }
          const record = value as Record<string, unknown>;
          const source =
            typeof record.operationId === "string"
              ? record.operationId
              : typeof record.terminalEventId === "string"
                ? record.terminalEventId
                : null;
          if (source && /^[0-9a-f]{64}$/.test(source)) ids.add(source);
        } catch {
          // Ordinary human prompts are not wake pointers.
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
 * acknowledgement. Either producer's exact operation pointer retires the
 * source durably; relay acceptance alone remains pending and is retried once
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
  const evidenceComplete = codingSessionTeamWakeEvidenceIsComplete({
    isLoading: evidence.isLoading,
    errorMessage: evidence.errorMessage,
  });
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
  const acknowledgedSourceEventIds = React.useMemo(
    () => leadAcknowledgedSourceEventIds(input.umbrella),
    [input.umbrella],
  );
  const acknowledgedCommandIdsRef = React.useRef(acknowledgedCommandIds);
  const acknowledgedSourceEventIdsRef = React.useRef(
    acknowledgedSourceEventIds,
  );
  acknowledgedCommandIdsRef.current = acknowledgedCommandIds;
  acknowledgedSourceEventIdsRef.current = acknowledgedSourceEventIds;
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
      !evidenceComplete ||
      !plan.lead?.activeGeneration.commandTarget ||
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
    const migrated = migrateCodingSessionTeamWakeCursor(state, plan.candidates);
    if (migrated !== state) {
      state = migrated;
      try {
        writeCodingSessionTeamWakeState(storage, storageKey, state);
      } catch {
        toast.error(
          "Automatic team delivery could not migrate its durable wake ledger.",
        );
        return;
      }
    }
    const acknowledged = acknowledgeCodingSessionTeamWakes(
      state,
      acknowledgedCommandIds,
      acknowledgedSourceEventIds,
    );
    if (
      acknowledged.pending.length !== state.pending.length ||
      acknowledged.observed.length !== state.observed.length ||
      acknowledged.resolvedSourceEventIds.length !==
        state.resolvedSourceEventIds.length
    ) {
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
      acknowledgedSourceEventIds,
      attemptedThisMount: attemptedThisMount.current,
    });
    if (!candidate || failedThisMount.current.has(candidate.sourceEventId)) {
      return;
    }
    // Provider-owned delivery is primary. This one-shot grace costs no model
    // turn and gives the daemon time to publish; signed ingress re-renders the
    // hook, where operation-level acknowledgement suppresses this fallback.
    let fallbackNotBefore = codingSessionTeamWakeFallbackNotBefore(
      state,
      candidate.sourceEventId,
    );
    if (fallbackNotBefore === null) {
      fallbackNotBefore = Date.now() + DESKTOP_FALLBACK_GRACE_MS;
      state = observeCodingSessionTeamWake(
        state,
        candidate.sourceEventId,
        fallbackNotBefore,
      );
      try {
        writeCodingSessionTeamWakeState(storage, storageKey, state);
      } catch {
        toast.error(
          "Automatic team delivery could not persist its provider-first grace window.",
        );
        return;
      }
    }
    const waitMs = fallbackNotBefore - Date.now();
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
        if (
          acknowledgedSourceEventIdsRef.current.has(candidate.sourceEventId) ||
          acknowledgedCommandIdsRef.current.has(commandId)
        ) {
          return;
        }
        await publishCodingSessionCommand({
          channelId: input.channelId,
          commandId,
          target,
          text: codingSessionTeamWakeText(candidate),
          deliver: "boundary",
        });
        toast.warning(
          "The provider wake was not observed during its grace window. Beekeeper delivered this signed Desktop fallback; provider-owned delivery still needs attention.",
        );
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
            acknowledgedSourceEventIds,
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
    acknowledgedSourceEventIds,
    evidenceComplete,
    input.catalogSettled,
    input.channelId,
    plan.candidates,
    plan.lead,
    revision,
    storageKey,
  ]);
}
