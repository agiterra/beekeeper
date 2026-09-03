/**
 * Reads this umbrella's session policy, folded by `buzz-core`'s own rule.
 *
 * A one-shot read with an explicit refresh rather than a live subscription: a
 * 44245 is published at launch and changed by hand, and the Context tab is
 * opened by a person who can ask again. Nothing here polls (I1).
 *
 * The two halves it fetches are both required. The **records** are the
 * policies; the **accepted authority chain** is what says whose record counts,
 * evaluated at each record's own time — without it a stranger's ceiling and
 * the founder's are indistinguishable, which is REVIEW-B2 F1 exactly.
 */
import * as React from "react";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { invokeTauri } from "@/shared/api/tauri";
import { relayClient as defaultRelayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_CODING_SESSION_POLICY,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";
import { projectCodingSessionMissionAuthority } from "@/features/coding-sessions/lib/codingSessionMissionAuthority";
import {
  CODING_SESSION_POLICY_FOLD_COMMAND,
  CODING_SESSION_POLICY_FOLD_REQUEST_SCHEMA,
  decodeCodingSessionPolicyFoldResult,
  type CodingSessionPolicyFoldResult,
} from "@/features/coding-sessions/lib/codingSessionMissionPolicyView";

const HEX64 = /^[0-9a-f]{64}$/;
const MAX_ERROR_MESSAGE_LENGTH = 4096;

/** How many events of each kind one read may retain. Bounded, like the fold's. */
export const CODING_SESSION_POLICY_HISTORY_LIMIT = 500;

/** The exact scope one policy read is about. */
export type CodingSessionPolicyScope = {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  founderPubkey: string;
};

/** What a caller needs to fetch for a policy read; `relayClient` satisfies it. */
export type CodingSessionPolicyClient = {
  fetchEvents(filter: RelaySubscriptionFilter): Promise<RelayEvent[]>;
};

export type CodingSessionSessionPolicyResult = {
  isLoading: boolean;
  errorMessage: string | null;
  /** The native fold's answer, or null while it is unknown. */
  fold: CodingSessionPolicyFoldResult | null;
  refresh: () => void;
};

/** The three filters one policy read needs, all with explicit `kinds`. */
export function buildCodingSessionPolicyFilters(
  scope: CodingSessionPolicyScope,
  limit: number,
  relayPubkey: string,
): RelaySubscriptionFilter[] {
  return [
    {
      kinds: [KIND_CODING_SESSION_POLICY],
      "#h": [scope.channelRef],
      "#d": [scope.sessionRef],
      limit,
    },
    {
      kinds: [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
      "#h": [scope.channelRef],
      "#csat-genesis": [scope.genesisRef],
      limit,
    },
    {
      kinds: [KIND_SYSTEM_MESSAGE],
      "#h": [scope.channelRef],
      authors: [relayPubkey],
      limit,
    },
  ];
}

/**
 * Fetch, project and fold one umbrella's policies.
 *
 * Exported separately from the hook so a test can drive the whole path with a
 * fake client and a fake invoke, without React.
 */
export async function readCodingSessionSessionPolicy(input: {
  scope: CodingSessionPolicyScope;
  relayPubkey: string;
  client: CodingSessionPolicyClient;
  invoke?: (command: string, args: Record<string, unknown>) => Promise<unknown>;
}): Promise<CodingSessionPolicyFoldResult> {
  const [records, transitions, receipts] = await Promise.all(
    buildCodingSessionPolicyFilters(
      input.scope,
      CODING_SESSION_POLICY_HISTORY_LIMIT,
      input.relayPubkey,
    ).map((filter) => input.client.fetchEvents(filter)),
  );
  const authority = projectCodingSessionMissionAuthority({
    channelRef: input.scope.channelRef,
    genesisRef: input.scope.genesisRef,
    founderPubkey: input.scope.founderPubkey,
    relayPubkey: input.relayPubkey,
    transitions,
    receipts,
  });
  if (!authority.ok) throw new Error(authority.error);
  const invoke = input.invoke ?? invokeTauri;
  return decodeCodingSessionPolicyFoldResult(
    await invoke(CODING_SESSION_POLICY_FOLD_COMMAND, {
      request: {
        schema: CODING_SESSION_POLICY_FOLD_REQUEST_SCHEMA,
        sessionRef: input.scope.sessionRef,
        genesisRef: input.scope.genesisRef,
        founderPubkey: input.scope.founderPubkey,
        grants: authority.value.policyGrants.map((grant) => ({
          transitionEventId: grant.transitionEventId,
          grantee: grant.grantee,
          acceptedAt: grant.acceptedAt,
          transitionType: grant.transitionType,
        })),
        // REVIEW-L2 F15: the signed transitions themselves go with the
        // projection, so Rust can refuse any grant no signature supports
        // rather than taking this file's word for the chain.
        transitions,
        events: records,
      },
    }),
  );
}

/** Read one umbrella's folded session policy. `null` scope reads nothing. */
export function useCodingSessionSessionPolicy(
  scope: CodingSessionPolicyScope | null,
  client: CodingSessionPolicyClient = defaultRelayClient,
): CodingSessionSessionPolicyResult {
  // Deliberately four primitives rather than the object: a caller that builds
  // its scope literal inline hands a new reference on every render, and a relay
  // read keyed on the object identity would restart on each one.
  const channelRef = scope?.channelRef ?? null;
  const sessionRef = scope?.sessionRef ?? null;
  const genesisRef = scope?.genesisRef ?? null;
  const founderPubkey = scope?.founderPubkey ?? null;
  const identity =
    channelRef === null ||
    sessionRef === null ||
    genesisRef === null ||
    founderPubkey === null
      ? "no-policy-scope"
      : [channelRef, sessionRef, genesisRef, founderPubkey].join(" ");
  const stableScope = React.useMemo(
    () =>
      channelRef === null ||
      sessionRef === null ||
      genesisRef === null ||
      founderPubkey === null
        ? null
        : { channelRef, sessionRef, genesisRef, founderPubkey },
    [channelRef, founderPubkey, genesisRef, sessionRef],
  );
  const [revision, setRevision] = React.useState(0);
  const refresh = React.useCallback(
    () => setRevision((current) => current + 1),
    [],
  );
  // The scope plus the refresh count: one string, so the effect re-runs on an
  // explicit refresh without `revision` sitting in the list as a value nothing
  // in the body reads.
  const runIdentity = `${identity}\u0000${revision}`;
  const [state, setState] = React.useState<{
    identity: string;
    isLoading: boolean;
    errorMessage: string | null;
    fold: CodingSessionPolicyFoldResult | null;
  }>(() => ({
    identity: runIdentity,
    isLoading: stableScope !== null,
    errorMessage: null,
    fold: null,
  }));

  React.useEffect(() => {
    let cancelled = false;
    if (stableScope === null) {
      setState({
        identity: runIdentity,
        isLoading: false,
        errorMessage: null,
        fold: null,
      });
      return () => {
        cancelled = true;
      };
    }
    setState({
      identity: runIdentity,
      isLoading: true,
      errorMessage: null,
      fold: null,
    });
    void (async () => {
      try {
        const relayPubkey = await getRelaySelf();
        if (cancelled) return;
        if (!relayPubkey || !HEX64.test(relayPubkey)) {
          throw new Error(
            "The active relay has no trusted NIP-11 self signing key.",
          );
        }
        const fold = await readCodingSessionSessionPolicy({
          client,
          relayPubkey,
          scope: stableScope,
        });
        if (cancelled) return;
        setState({
          identity: runIdentity,
          isLoading: false,
          errorMessage: null,
          fold,
        });
      } catch (error) {
        if (cancelled) return;
        setState({
          identity: runIdentity,
          isLoading: false,
          errorMessage: errorText(error),
          fold: null,
        });
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [client, runIdentity, stableScope]);

  // A state left over from a previous scope or a previous refresh is not this
  // read's answer, so it reads as loading rather than as a stale record.
  const current = state.identity === runIdentity;
  return {
    isLoading: current ? state.isLoading : true,
    errorMessage: current ? state.errorMessage : null,
    fold: current ? state.fold : null,
    refresh,
  };
}

function errorText(error: unknown): string {
  const message =
    error instanceof Error
      ? error.message
      : "Failed to read the session policy.";
  return message.length <= MAX_ERROR_MESSAGE_LENGTH
    ? message
    : `${message.slice(0, MAX_ERROR_MESSAGE_LENGTH)}… [truncated]`;
}
