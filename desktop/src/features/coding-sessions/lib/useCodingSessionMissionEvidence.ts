import * as React from "react";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import type { CodingSessionMissionInspectorInput } from "./codingSessionMissionInspectorModel";
import type { CodingSessionSeatAuthorityProjection } from "./codingSessionTeamDeliveryStatus";
import {
  buildCodingSessionMissionEvidenceFilters,
  CodingSessionMissionEvidenceStore,
  type CodingSessionMissionEvidenceScope,
  codingSessionMissionEvidenceScopeKey,
  emptyCodingSessionMissionInspectorInput,
  MISSION_EVIDENCE_HISTORY_LIMIT,
  projectCodingSessionMissionEvidence,
} from "./codingSessionMissionEvidenceModel";

const HEX64 = /^[0-9a-f]{64}$/;
const MAX_ERROR_MESSAGE_LENGTH = 4096;

export type CodingSessionMissionEvidenceClient = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
  subscribeLive(
    filter: RelaySubscriptionFilter,
    onEvent: (event: RelayEvent) => void,
  ): Promise<() => void>;
};

export type CodingSessionMissionEvidenceResult = {
  isLoading: boolean;
  errorMessage: string | null;
  inspectorInput: CodingSessionMissionInspectorInput;
  /**
   * The accepted seat chain this projection was folded against, or `null`
   * while it is loading or errored. `null` is `unknown`, not "no seats".
   */
  authority: CodingSessionSeatAuthorityProjection | null;
  /** Reports the Rust fold disclosed as authored by an unseated actor. */
  unseatedReportEventIds: string[];
  retainedEventCount: number;
  refresh: () => void;
};

function errorText(error: unknown, fallback: string): string {
  const message = error instanceof Error ? error.message : fallback;
  return message.length <= MAX_ERROR_MESSAGE_LENGTH
    ? message
    : `${message.slice(0, MAX_ERROR_MESSAGE_LENGTH)}… [truncated]`;
}

function retainedEventCount(store: CodingSessionMissionEvidenceStore): number {
  const snapshot = store.snapshot();
  return (
    snapshot.transactions.length +
    snapshot.transitions.length +
    snapshot.receipts.length
  );
}

/**
 * Collect and canonically project signed Mission evidence for one exact
 * channel/session/genesis/founder scope. Relay receipt authority comes only
 * from the active community's authenticated NIP-11 `self` query.
 */
export function useCodingSessionMissionEvidence(
  scope: CodingSessionMissionEvidenceScope | null,
  client: CodingSessionMissionEvidenceClient = defaultRelayClient,
  /**
   * L8.3: the umbrella's `gates.verifierRequired` from the surface's own
   * policy read (`codingSessionPolicy.ts:91`), or `null` while unknown / no
   * 44245 reached this view. Handed straight to the native fold — see
   * `withVerifierRequired` in `codingSessionMissionEvidenceModel.ts`.
   */
  verifierRequired: boolean | null = null,
): CodingSessionMissionEvidenceResult {
  const channelRef = scope?.channelRef ?? null;
  const sessionRef = scope?.sessionRef ?? null;
  const genesisRef = scope?.genesisRef ?? null;
  const founderPubkey = scope?.founderPubkey ?? null;
  const scopeIdentity =
    channelRef && sessionRef && genesisRef && founderPubkey
      ? codingSessionMissionEvidenceScopeKey({
          channelRef,
          sessionRef,
          genesisRef,
          founderPubkey,
        })
      : "no-mission-scope";
  const stableScope = React.useMemo(
    () =>
      channelRef && sessionRef && genesisRef && founderPubkey
        ? {
            channelRef,
            sessionRef,
            genesisRef,
            founderPubkey,
          }
        : null,
    [channelRef, founderPubkey, genesisRef, sessionRef],
  );
  const [refreshRevision, setRefreshRevision] = React.useState(0);
  const refresh = React.useCallback(
    () => setRefreshRevision((current) => current + 1),
    [],
  );
  const runIdentity = `${scopeIdentity}\u0000${refreshRevision}`;
  const [state, setState] = React.useState(() => ({
    identity: runIdentity,
    isLoading: Boolean(stableScope),
    errorMessage: null as string | null,
    inspectorInput: emptyCodingSessionMissionInspectorInput(),
    authority: null as CodingSessionSeatAuthorityProjection | null,
    unseatedReportEventIds: [] as string[],
    retainedEventCount: 0,
  }));

  React.useEffect(() => {
    let cancelled = false;
    let projectionRevision = 0;
    const unsubscribes: Array<() => void> = [];
    if (!stableScope) {
      setState({
        identity: runIdentity,
        isLoading: false,
        errorMessage: null,
        inspectorInput: emptyCodingSessionMissionInspectorInput(
          "Mission evidence has no selected session scope.",
        ),
        authority: null,
        unseatedReportEventIds: [],
        retainedEventCount: 0,
      });
      return;
    }

    const store = new CodingSessionMissionEvidenceStore(stableScope);
    let relayPubkey: string | null = null;
    let initialLoad = true;
    let historySettled = false;
    setState({
      identity: runIdentity,
      isLoading: true,
      errorMessage: null,
      inspectorInput: emptyCodingSessionMissionInspectorInput(),
      authority: null,
      unseatedReportEventIds: [],
      retainedEventCount: 0,
    });

    const publishError = (error: unknown, fallback: string) => {
      if (cancelled) return;
      setState({
        identity: runIdentity,
        isLoading: false,
        errorMessage: errorText(error, fallback),
        inspectorInput: emptyCodingSessionMissionInspectorInput(
          "Mission evidence could not be canonically projected.",
        ),
        authority: null,
        unseatedReportEventIds: [],
        retainedEventCount: retainedEventCount(store),
      });
    };

    const project = async () => {
      if (!relayPubkey) return;
      const revision = ++projectionRevision;
      try {
        const projection = await projectCodingSessionMissionEvidence({
          scope: stableScope,
          relayPubkey,
          snapshot: store.snapshot(),
          verifierRequired,
        });
        if (cancelled || revision !== projectionRevision) return;
        initialLoad = false;
        setState({
          identity: runIdentity,
          isLoading: false,
          errorMessage: null,
          inspectorInput: projection.inspectorInput,
          authority: projection.authority,
          unseatedReportEventIds: projection.unseatedReportEventIds,
          retainedEventCount: retainedEventCount(store),
        });
      } catch (error) {
        if (cancelled || revision !== projectionRevision) return;
        initialLoad = false;
        publishError(error, "Failed to project Mission evidence.");
      }
    };

    const ingest = (events: readonly RelayEvent[]) => {
      if (cancelled || !store.ingest(events)) return;
      if (historySettled) void project();
    };

    void (async () => {
      try {
        const observedRelayPubkey = await getRelaySelf();
        if (cancelled) return;
        if (!observedRelayPubkey || !HEX64.test(observedRelayPubkey)) {
          throw new Error(
            "The active relay has no trusted NIP-11 self signing key.",
          );
        }
        relayPubkey = observedRelayPubkey;
        const liveFilters = buildCodingSessionMissionEvidenceFilters(
          stableScope,
          0,
          relayPubkey,
        );
        const liveResults = await Promise.allSettled(
          liveFilters.map((filter) =>
            client.subscribeLive(filter, (event) => ingest([event])),
          ),
        );
        for (const result of liveResults) {
          if (result.status === "fulfilled") {
            if (cancelled) result.value();
            else unsubscribes.push(result.value);
          }
        }
        if (cancelled) return;
        const liveFailure = liveResults.find(
          (result): result is PromiseRejectedResult =>
            result.status === "rejected",
        );
        if (liveFailure) {
          for (const unsubscribe of unsubscribes.splice(0)) unsubscribe();
          throw liveFailure.reason;
        }

        const historyResults = await Promise.allSettled(
          buildCodingSessionMissionEvidenceFilters(
            stableScope,
            MISSION_EVIDENCE_HISTORY_LIMIT,
            relayPubkey,
          ).map((filter) => client.fetchEvents(filter)),
        );
        if (cancelled) return;
        for (const result of historyResults) {
          if (result.status === "fulfilled") store.ingest(result.value);
        }
        const historyFailure = historyResults.find(
          (result): result is PromiseRejectedResult =>
            result.status === "rejected",
        );
        if (historyFailure) throw historyFailure.reason;
        historySettled = true;
        await project();
      } catch (error) {
        if (!cancelled && initialLoad) {
          publishError(error, "Failed to load Mission evidence.");
        } else if (!cancelled) {
          publishError(error, "Failed to refresh Mission evidence.");
        }
      }
    })();

    return () => {
      cancelled = true;
      projectionRevision += 1;
      for (const unsubscribe of unsubscribes) unsubscribe();
    };
    // `verifierRequired` typically resolves after this effect's own history
    // read (it comes from a sibling policy read), so it is a real dependency:
    // when it changes from `null` (unknown) to the record's actual value, the
    // whole read reruns once so the native fold sees it rather than folding
    // with a stale `false` for the surface's whole lifetime (L8.3).
  }, [client, runIdentity, stableScope, verifierRequired]);

  if (state.identity !== runIdentity) {
    return {
      isLoading: Boolean(stableScope),
      errorMessage: null,
      inspectorInput: emptyCodingSessionMissionInspectorInput(),
      authority: null,
      unseatedReportEventIds: [],
      retainedEventCount: 0,
      refresh,
    };
  }
  return {
    isLoading: state.isLoading,
    errorMessage: state.errorMessage,
    inspectorInput: state.inspectorInput,
    authority: state.authority,
    unseatedReportEventIds: state.unseatedReportEventIds,
    retainedEventCount: state.retainedEventCount,
    refresh,
  };
}
