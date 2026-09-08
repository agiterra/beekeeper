import * as React from "react";
import { toast } from "sonner";

import {
  buildCodingSessionTargetKey,
  publishCodingSessionCommand,
} from "../lib/codingSessionCommand";
import type { CodingSessionCommandTarget } from "../lib/codingSessionCommand";
import type {
  CodingSessionSeatAuthority,
  CodingSessionTeamWakeDelivery,
} from "../lib/codingSessionMissionContracts";
import {
  deriveCodingSessionSeatAuthorities,
  deriveCodingSessionTeamWakeDeliveryPlan,
  type CodingSessionTeamWakeDeliveryPlan,
} from "../lib/codingSessionTeamDeliveryStatus";
import {
  acknowledgeCodingSessionTeamWakes,
  baselineCodingSessionTeamWakes,
  buildCodingSessionTeamWakeCommandId,
  buildCodingSessionTeamWakeReArmCommandId,
  clearCodingSessionTeamWakePublishFailure,
  CODING_SESSION_TEAM_WAKE_GRACE_MS,
  codingSessionTeamWakeFallbackNotBefore,
  codingSessionTeamWakeEvidenceIsComplete,
  codingSessionTeamWakeStorageKey,
  codingSessionTeamWakeText,
  deriveCodingSessionTeamWakePlan,
  pendingCodingSessionTeamWake,
  emptyCodingSessionTeamWakeState,
  readCodingSessionTeamWakeState,
  recordCodingSessionTeamWakeAttempt,
  recordCodingSessionTeamWakeCustody,
  recordCodingSessionTeamWakePublishFailure,
  recordCodingSessionTeamWakeReArm,
  releaseCodingSessionTeamWakeCustody,
  observeCodingSessionTeamWake,
  migrateCodingSessionTeamWakeCursor,
  writeCodingSessionTeamWakeState,
  type CodingSessionTeamWakeState,
} from "../lib/codingSessionTeamWake";
import type { CodingSessionUmbrellaRecord } from "../lib/codingSessionTypes";
import type { CodingSessionMissionEvidenceClient } from "../lib/useCodingSessionMissionEvidence";
import type { CodingSessionLeadWakeEvidenceClient } from "./useCodingSessionLeadWakeEvidence";
import { useCodingSessionMissionEvidence } from "../lib/useCodingSessionMissionEvidence";
import { useCodingSessionLeadWakeEvidence } from "./useCodingSessionLeadWakeEvidence";

const EMPTY_DELIVERIES: CodingSessionTeamWakeDelivery[] = [];
const EMPTY_PLAN: CodingSessionTeamWakeDeliveryPlan = {
  deliveries: EMPTY_DELIVERIES,
  resolvedSourceEventIds: new Set<string>(),
  custodiedSources: [],
  releasedCustodySourceEventIds: new Set<string>(),
  suppressedSourceEventIds: new Set<string>(),
  publishEligibleSourceEventIds: new Set<string>(),
  reArmEligibleSourceEventIds: new Set<string>(),
  spentCommandIds: new Set<string>(),
};

/** What the workspace renders from this hook. Nothing here is inferred. */
export type CodingSessionTeamWakeResult = {
  deliveries: CodingSessionTeamWakeDelivery[];
  seatAuthorities: CodingSessionSeatAuthority[];
  refusal: string | null;
};

/** Test seams. Production passes neither and uses the real relay and signer. */
export type CodingSessionTeamWakeDependencies = {
  /** Shared by the mission-evidence read (bundled surface) and the lead-wake
   * read (per-filter surface); `relayClient` satisfies both. */
  evidenceClient?: CodingSessionMissionEvidenceClient &
    CodingSessionLeadWakeEvidenceClient;
  publishCommand?: typeof publishCodingSessionCommand;
};

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
 * Push one durable, identifier-only lead wake for signed team results — and,
 * since 2026-09-01, read the lead's own 44224 receipts before doing so.
 *
 * There is deliberately no polling loop. Signed 44244/44225 ingress and the
 * lead's 44220/44224 evidence subscription drive this hook; the one timer is
 * the single-shot local grace, which costs no model turn.
 *
 * The arbitration is the §2a ruling: a command — anyone's — that the lead's
 * provider receipted `turn_queued` owns the operation, so Desktop suppresses
 * and records that durably. Only when *every* known command has failed for a
 * reason other than `DUPLICATE_OPERATION` does Desktop cover, and then exactly
 * once, with a new command id.
 */
export function useCodingSessionTeamWake(
  input: {
    catalogSettled: boolean;
    channelId: string;
    communityScope: string;
    currentUserPubkey: string | null;
    sessionClosed: boolean;
    umbrella: CodingSessionUmbrellaRecord;
  },
  dependencies: CodingSessionTeamWakeDependencies = {},
): CodingSessionTeamWakeResult {
  const publishCommand =
    dependencies.publishCommand ?? publishCodingSessionCommand;
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
  const evidence = useCodingSessionMissionEvidence(
    governedScope,
    dependencies.evidenceClient,
  );
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
  const leadTarget = plan.lead?.activeGeneration.commandTarget ?? null;
  const leadTargetKey = leadTarget
    ? buildCodingSessionTargetKey(leadTarget)
    : null;
  const wakeEvidence = useCodingSessionLeadWakeEvidence(
    {
      channelId: input.channelId,
      leadTargetKey,
      providerAuthorityPubkey:
        plan.lead?.activeGeneration.providerAuthorityPubkey ?? null,
    },
    dependencies.evidenceClient,
  );
  // An index that is loading, errored, or saturated is not an empty index. If
  // it were read as one, the hook would conclude "no provider command exists"
  // and publish — which is the whole defect this batch exists to fix.
  const evidenceComplete =
    codingSessionTeamWakeEvidenceIsComplete({
      isLoading: evidence.isLoading,
      errorMessage: evidence.errorMessage,
    }) &&
    codingSessionTeamWakeEvidenceIsComplete({
      isLoading: wakeEvidence.isLoading,
      errorMessage: wakeEvidence.errorMessage,
    }) &&
    wakeEvidence.index !== null &&
    !wakeEvidence.index.overflowed;
  const acknowledgedCommandIds = React.useMemo(
    () => leadAcknowledgedCommandIds(input.umbrella),
    [input.umbrella],
  );
  const acknowledgedSourceEventIds = React.useMemo(
    () => leadAcknowledgedSourceEventIds(input.umbrella),
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

  const storedState = React.useMemo(() => {
    void revision;
    if (!storageKey) return null;
    try {
      return readCodingSessionTeamWakeState(window.localStorage, storageKey);
    } catch {
      return null;
    }
  }, [revision, storageKey]);

  const deliveryPlan = React.useMemo(() => {
    // The index mutates in place, so its revision — not its identity — is
    // what says the evidence changed.
    void wakeEvidence.revision;
    // `storedState` is null until this Desktop has written a wake ledger for
    // this session — which is most sessions, most of the time. Requiring it
    // here meant a queued provider wake, fully verified on the wire, rendered
    // as "No team wake deliveries observed": absent local state reported as
    // an observed absence. The ledger only adds Desktop's OWN attempts; the
    // evidence being disclosed is the relay's.
    return leadTarget && leadTargetKey
      ? deriveCodingSessionTeamWakeDeliveryPlan({
          candidates: plan.candidates,
          leadTarget,
          leadTargetKey,
          founderPubkey: input.umbrella.founderPubkey,
          index: wakeEvidence.index,
          acknowledgedCommandIds,
          acknowledgedSourceEventIds,
          state: storedState ?? emptyCodingSessionTeamWakeState(),
          nowMs: Date.now(),
          evidenceComplete,
          publishFailedSourceEventIds: failedThisMount.current,
        })
      : EMPTY_PLAN;
  }, [
    acknowledgedCommandIds,
    acknowledgedSourceEventIds,
    evidenceComplete,
    input.umbrella.founderPubkey,
    leadTarget,
    leadTargetKey,
    plan.candidates,
    storedState,
    wakeEvidence.index,
    wakeEvidence.revision,
  ]);
  const seatAuthorities = React.useMemo(
    () =>
      deriveCodingSessionSeatAuthorities({
        channelId: input.channelId,
        umbrella: input.umbrella,
        authority: evidence.authority,
      }),
    [evidence.authority, input.channelId, input.umbrella],
  );

  // Rebuilt every render so the pre-publish recheck reads the newest evidence,
  // including receipts that arrived while the command id was being derived.
  const recheckRef = React.useRef<
    (state: CodingSessionTeamWakeState) => CodingSessionTeamWakeDeliveryPlan
  >(() => EMPTY_PLAN);
  recheckRef.current = (state) =>
    leadTarget && leadTargetKey
      ? deriveCodingSessionTeamWakeDeliveryPlan({
          candidates: plan.candidates,
          leadTarget,
          leadTargetKey,
          founderPubkey: input.umbrella.founderPubkey,
          index: wakeEvidence.index,
          acknowledgedCommandIds,
          acknowledgedSourceEventIds,
          state,
          nowMs: Date.now(),
          evidenceComplete,
          publishFailedSourceEventIds: failedThisMount.current,
        })
      : EMPTY_PLAN;

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
      !leadTarget ||
      !leadTargetKey ||
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
    // Custody and resolution are written before anything is considered for
    // publication, and in that order: a released custody row must be gone
    // before the re-arm rule can see the source as uncovered.
    const previous = state;
    for (const sourceEventId of deliveryPlan.releasedCustodySourceEventIds) {
      state = releaseCodingSessionTeamWakeCustody(state, {
        sourceEventId,
        leadTargetKey,
      });
    }
    for (const custody of deliveryPlan.custodiedSources) {
      state = recordCodingSessionTeamWakeCustody(state, custody);
    }
    state = acknowledgeCodingSessionTeamWakes(
      state,
      acknowledgedCommandIds,
      acknowledgedSourceEventIds,
      deliveryPlan.resolvedSourceEventIds,
    );
    if (state !== previous) {
      try {
        writeCodingSessionTeamWakeState(storage, storageKey, state);
      } catch {
        toast.error(
          "Automatic team delivery was acknowledged but could not persist its receipt.",
        );
        return;
      }
    }
    const target: CodingSessionCommandTarget = leadTarget;
    // A source this mount has already failed on is *skipped*, not a reason to
    // stop: the sibling behind it has its own expired grace and its own row,
    // and leaving it saying "Waiting for the provider wake" forever would be a
    // false status word as well as a lost wake.
    const candidate = pendingCodingSessionTeamWake({
      state,
      candidates: plan.candidates,
      leadTarget: target,
      acknowledgedCommandIds,
      acknowledgedSourceEventIds,
      suppressedSourceEventIds: deliveryPlan.suppressedSourceEventIds,
      skipSourceEventIds: failedThisMount.current,
      attemptedThisMount: attemptedThisMount.current,
    });
    if (!candidate) return;
    const delivery = deliveryPlan.deliveries.find(
      (row) => row.sourceEventId === candidate.sourceEventId,
    );
    // Eligibility, not the rendered kind. A row reading `failed` only because
    // an earlier publish threw is still publishable — that record is
    // disclosure, and the mount-local skip set above is what stops a loop.
    if (
      !delivery ||
      !deliveryPlan.publishEligibleSourceEventIds.has(candidate.sourceEventId)
    ) {
      return;
    }
    const reArm = deliveryPlan.reArmEligibleSourceEventIds.has(
      candidate.sourceEventId,
    );
    // Provider-owned delivery is primary. This one-shot grace costs no model
    // turn and gives the daemon time to publish; signed ingress re-renders the
    // hook, where operation-level acknowledgement suppresses this fallback.
    let fallbackNotBefore = codingSessionTeamWakeFallbackNotBefore(
      state,
      candidate.sourceEventId,
    );
    if (fallbackNotBefore === null) {
      fallbackNotBefore = Date.now() + CODING_SESSION_TEAM_WAKE_GRACE_MS;
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
    const settledState = state;
    void (async () => {
      try {
        const commandId = reArm
          ? await buildCodingSessionTeamWakeReArmCommandId(candidate, target)
          : await buildCodingSessionTeamWakeCommandId(candidate, target);
        if (cancelled) return;
        // The boundary recheck. Between the decision above and this line the
        // provider's `turn_queued` or `turn_started` may have landed; the
        // evidence index is mutated in place, so this reads the newest facts
        // rather than the render's snapshot.
        const latest = recheckRef.current(
          readCodingSessionTeamWakeState(storage, storageKey) ?? settledState,
        );
        const latestDelivery = latest.deliveries.find(
          (row) => row.sourceEventId === candidate.sourceEventId,
        );
        if (
          latest.suppressedSourceEventIds.has(candidate.sourceEventId) ||
          // The runner already refused this exact id as a duplicate; sending
          // it again would only be refused again. A different id (the re-arm)
          // is still allowed once every live command has really failed.
          latest.spentCommandIds.has(commandId) ||
          !latestDelivery ||
          !latest.publishEligibleSourceEventIds.has(candidate.sourceEventId)
        ) {
          return;
        }
        await publishCommand({
          channelId: input.channelId,
          commandId,
          target,
          text: codingSessionTeamWakeText(candidate),
          deliver: "boundary",
        });
        toast.warning(
          `${latestDelivery.detail}. The provider wake was not observed during its grace window, so Beekeeper delivered this signed Desktop fallback; provider-owned delivery still needs attention.`,
        );
        attemptedThisMount.current.add(commandId);
        const current =
          readCodingSessionTeamWakeState(storage, storageKey) ?? settledState;
        const recorded = recordCodingSessionTeamWakeAttempt({
          state: current,
          candidate,
          commandId,
          leadTarget: target,
          acknowledgedCommandIds,
          acknowledgedSourceEventIds,
        });
        writeCodingSessionTeamWakeState(
          storage,
          storageKey,
          // A publish that succeeds clears any earlier recorded failure for
          // this source: the row must stop saying "failed" once it is covered.
          clearCodingSessionTeamWakePublishFailure(
            reArm
              ? recordCodingSessionTeamWakeReArm(recorded, {
                  sourceEventId: candidate.sourceEventId,
                  leadTargetKey,
                  commandId,
                })
              : recorded,
            { sourceEventId: candidate.sourceEventId, leadTargetKey },
          ),
        );
      } catch (error) {
        failedThisMount.current.add(candidate.sourceEventId);
        try {
          const current =
            readCodingSessionTeamWakeState(storage, storageKey) ?? settledState;
          writeCodingSessionTeamWakeState(
            storage,
            storageKey,
            recordCodingSessionTeamWakePublishFailure(current, {
              sourceEventId: candidate.sourceEventId,
              leadTargetKey,
            }),
          );
        } catch {
          // A storage failure here costs the durable row, not the toast below.
        }
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
    deliveryPlan,
    evidenceComplete,
    input.catalogSettled,
    input.channelId,
    leadTarget,
    leadTargetKey,
    plan.candidates,
    publishCommand,
    revision,
    storageKey,
  ]);

  return React.useMemo(
    () => ({
      deliveries: deliveryPlan.deliveries,
      seatAuthorities,
      refusal: plan.refusal,
    }),
    [deliveryPlan.deliveries, plan.refusal, seatAuthorities],
  );
}
