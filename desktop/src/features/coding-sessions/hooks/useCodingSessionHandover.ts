/**
 * Reads one umbrella's handover records (kind 44247) and folds them.
 *
 * One bounded, batched read — the 44247 records, the 44228 chain and the
 * relay's own 40099 receipts — projected through the same authority twin every
 * other session surface uses, then handed to the canonical fold. Nothing polls:
 * the read is invalidated by the observed-event fan-out the session surfaces
 * already share, so a takeover that lands while somebody is looking updates the
 * panel without a timer.
 *
 * `retry: false` on purpose. A refused read here is a fact a person needs to
 * see ("these records could not be read, and here is why"), and a retry loop
 * would replace it with a spinner that never resolves.
 */
import * as React from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { getRelaySelf } from "@/features/moderation/lib/relaySelf";
import { relayClient } from "@/shared/api/relayClient";
import type { RelaySubscriptionFilter } from "@/shared/api/relayClientShared";
import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  KIND_SYSTEM_MESSAGE,
} from "@/shared/constants/kinds";
import {
  foldCodingSessionHandover,
  type CodingSessionHandoverFold,
} from "../lib/codingSessionHandoverFold";
import { KIND_CODING_SESSION_HANDOVER } from "../lib/codingSessionHandoverWire";
import {
  projectCodingSessionMissionAuthority,
  type CodingSessionMissionAuthorityProjection,
} from "../lib/codingSessionMissionAuthority";
import { subscribeToObservedCodingSessionEvents } from "../lib/codingSessionObservedEvents";

const HEX64 = /^[0-9a-f]{64}$/;

/** How many records one read retains. Bounded, like every session read. */
export const CODING_SESSION_HANDOVER_HISTORY_LIMIT = 256;

/** The exact scope one handover read is about. */
export type CodingSessionHandoverScope = {
  channelRef: string;
  sessionRef: string;
  genesisRef: string;
  founderPubkey: string;
};

export type CodingSessionHandoverRead = {
  fold: CodingSessionHandoverFold | null;
  authority: CodingSessionMissionAuthorityProjection | null;
  isLoading: boolean;
  errorMessage: string | null;
  /**
   * Which of this read's three filters came back at its own ceiling.
   *
   * A capped read is a partial answer: the chain may have links this fold
   * never saw, so a surface must say so rather than present the fold as the
   * whole history. Empty means every filter returned under its limit.
   */
  capped: readonly string[];
  refresh: () => void;
};

type HandoverClient = {
  fetchEventsBatch(filters: RelaySubscriptionFilter[]): Promise<RelayEvent[]>;
};

/** The query key one scope's handover read lives under. */
export function codingSessionHandoverQueryKey(
  scope: CodingSessionHandoverScope | null,
): readonly unknown[] {
  return [
    "coding-session-handover",
    scope?.channelRef ?? null,
    scope?.sessionRef ?? null,
    scope?.genesisRef ?? null,
  ];
}

/** The three filters one handover read needs, all with explicit `kinds`. */
export function buildCodingSessionHandoverFilters(
  scope: CodingSessionHandoverScope,
  relayPubkey: string,
  limit: number = CODING_SESSION_HANDOVER_HISTORY_LIMIT,
): RelaySubscriptionFilter[] {
  return [
    {
      kinds: [KIND_CODING_SESSION_HANDOVER],
      "#h": [scope.channelRef],
      "#d": [scope.sessionRef],
      "#csh-genesis": [scope.genesisRef],
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
 * Fetch, project and fold one umbrella's handover records.
 *
 * Exported without React so a test can drive the whole path with a fake
 * client.
 */
export async function readCodingSessionHandover(input: {
  scope: CodingSessionHandoverScope;
  relayPubkey: string;
  client: HandoverClient;
  /** Whether an accepted whole-session deletion has been witnessed. */
  retired?: boolean;
}): Promise<{
  fold: CodingSessionHandoverFold;
  authority: CodingSessionMissionAuthorityProjection;
  capped: string[];
}> {
  const events = await input.client.fetchEventsBatch(
    buildCodingSessionHandoverFilters(input.scope, input.relayPubkey),
  );
  const records = events.filter(
    (event) => event.kind === KIND_CODING_SESSION_HANDOVER,
  );
  const transitions = events.filter(
    (event) => event.kind === KIND_CODING_SESSION_AUTHORITY_TRANSITION,
  );
  const receipts = events.filter((event) => event.kind === KIND_SYSTEM_MESSAGE);
  // A filter that came back full is a filter that may have been cut short.
  // Named rather than counted, so the surface can say which half of the
  // evidence is partial.
  const capped = [
    records.length >= CODING_SESSION_HANDOVER_HISTORY_LIMIT
      ? "handover records"
      : null,
    transitions.length >= CODING_SESSION_HANDOVER_HISTORY_LIMIT
      ? "the authority chain"
      : null,
    receipts.length >= CODING_SESSION_HANDOVER_HISTORY_LIMIT
      ? "the relay's receipts"
      : null,
  ].filter((entry): entry is string => entry !== null);
  // Retirement, from the relay's own signed statement rather than from a
  // guess. The relay stamps this receipt when it applies a whole-session
  // deletion (`side_effects.rs::CODING_SESSION_DELETION_RECEIPT_TYPE`), which
  // is the only thing this client can verify: a kind 5 it merely *saw* proves
  // a request was made, not that it was applied.
  const retired =
    input.retired === true ||
    receipts.some(
      (event) =>
        event.pubkey === input.relayPubkey &&
        namesDeletionOf(event, input.scope.genesisRef),
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
  const folded = foldCodingSessionHandover({
    channelRef: input.scope.channelRef,
    sessionRef: input.scope.sessionRef,
    genesisRef: input.scope.genesisRef,
    events: records,
    context: {
      founderPubkey: input.scope.founderPubkey,
      // "Live operator" and "active seat" as the chain reports them now, with
      // the acceptance time of the link that granted each. A grant that has
      // since been revoked is not here, so a checkpoint written under it reads
      // `unauthorized` — under-claiming, never over-claiming (the fold's own
      // stated direction).
      grants: authority.value.activeGrants
        .filter((grant) => grant.maySteer)
        .map((grant) => ({
          pubkey: grant.actorPubkey,
          acceptedAt: acceptedAtOf(authority.value, grant.grantEventRef),
        })),
      seats: authority.value.activeSeats.map((seat) => ({
        pubkey: seat.actorPubkey,
        role: seat.role,
        acceptedAt: acceptedAtOf(authority.value, seat.grantEventRef),
      })),
      claim: authority.value.claim,
      claimSince: authority.value.claimSince,
      claimVoidedAt: authority.value.claimVoidedAt,
      retired,
    },
  });
  if (!folded.ok) throw new Error(folded.error);
  return { fold: folded.value, authority: authority.value, capped };
}

/**
 * The live filters this read watches, so a claim that lands while somebody is
 * looking updates the panel without a remount.
 *
 * One REQ per open session, `since` the moment it opened: the history is
 * already in hand from the query above, and a live subscription that replayed
 * it would pay for the same bytes twice. The receipt filter carries no
 * `authors` on purpose — this subscription is a *trigger*, and the read it
 * triggers is the thing that checks who signed what.
 */
export function buildCodingSessionHandoverLiveFilters(
  scope: CodingSessionHandoverScope,
  since: number,
): RelaySubscriptionFilter[] {
  return [
    {
      kinds: [KIND_CODING_SESSION_HANDOVER],
      "#h": [scope.channelRef],
      "#d": [scope.sessionRef],
      since,
      limit: 0,
    },
    {
      kinds: [KIND_CODING_SESSION_AUTHORITY_TRANSITION],
      "#h": [scope.channelRef],
      "#csat-genesis": [scope.genesisRef],
      since,
      limit: 0,
    },
    {
      kinds: [KIND_SYSTEM_MESSAGE],
      "#h": [scope.channelRef],
      since,
      limit: 0,
    },
  ];
}

/** The relay's own `coding_session_deletion_accepted`, for this genesis. */
export const CODING_SESSION_DELETION_RECEIPT_TYPE =
  "coding_session_deletion_accepted";

function namesDeletionOf(event: RelayEvent, genesisRef: string): boolean {
  try {
    const content: unknown = JSON.parse(event.content);
    return (
      typeof content === "object" &&
      content !== null &&
      (content as Record<string, unknown>).type ===
        CODING_SESSION_DELETION_RECEIPT_TYPE &&
      (content as Record<string, unknown>).genesisRef === genesisRef
    );
  } catch {
    return false;
  }
}

/** The receipt time of the link that granted this standing. */
function acceptedAtOf(
  authority: CodingSessionMissionAuthorityProjection,
  transitionEventId: string,
): number {
  return (
    authority.policyGrants.find(
      (grant) => grant.transitionEventId === transitionEventId,
    )?.acceptedAt ?? 0
  );
}

/** Read one umbrella's handover records. A `null` scope reads nothing. */
export function useCodingSessionHandover(
  scope: CodingSessionHandoverScope | null,
  options: { retired?: boolean; client?: HandoverClient } = {},
): CodingSessionHandoverRead {
  const queryClient = useQueryClient();
  const client = options.client ?? relayClient;
  const retired = options.retired ?? false;
  // Four primitives rather than the object: callers build their scope literal
  // inline, so a read keyed on object identity would restart every render.
  const channelRef = scope?.channelRef ?? null;
  const sessionRef = scope?.sessionRef ?? null;
  const genesisRef = scope?.genesisRef ?? null;
  const founderPubkey = scope?.founderPubkey ?? null;
  const stableScope = React.useMemo<CodingSessionHandoverScope | null>(
    () =>
      channelRef === null ||
      sessionRef === null ||
      genesisRef === null ||
      founderPubkey === null
        ? null
        : { channelRef, sessionRef, genesisRef, founderPubkey },
    [channelRef, founderPubkey, genesisRef, sessionRef],
  );
  const key = React.useMemo(
    () => [...codingSessionHandoverQueryKey(stableScope), retired],
    [retired, stableScope],
  );
  const query = useQuery({
    queryKey: key,
    enabled: stableScope !== null,
    retry: false,
    queryFn: async () => {
      if (stableScope === null) throw new Error("no handover scope");
      const relayPubkey = await getRelaySelf();
      if (relayPubkey === null || !HEX64.test(relayPubkey)) {
        throw new Error(
          "The active relay did not advertise a trusted signing key, so no handover receipt can be trusted.",
        );
      }
      return readCodingSessionHandover({
        scope: stableScope,
        relayPubkey,
        client,
        retired,
      });
    },
  });

  // This surface's own live subscription. Nothing else in the app carries
  // 44247 or 44228 on a live REQ — the ingress carries 44223/44224/44222 and
  // the create observer 44221/44224/44226 — so without this a takeover landing
  // while somebody has the session open would never reach the screen, and the
  // fan-out below would only ever fire for events this window itself caused.
  React.useEffect(() => {
    if (stableScope === null) return;
    let disposed = false;
    let leave: (() => Promise<void>) | null = null;
    const since = Math.floor(Date.now() / 1_000);
    void (async () => {
      try {
        const unsubscribe = await relayClient.subscribeLiveMany(
          buildCodingSessionHandoverLiveFilters(stableScope, since),
          () => {
            void queryClient.invalidateQueries({ queryKey: key });
          },
        );
        if (disposed) {
          void unsubscribe();
          return;
        }
        leave = unsubscribe;
      } catch {
        // A relay that refused the subscription leaves the one-shot read as
        // the only source; the panel still renders what it read.
      }
    })();
    return () => {
      disposed = true;
      void leave?.();
    };
  }, [key, queryClient, stableScope]);

  // The in-process fan-out as well: an event another surface in this window
  // already observed invalidates this read without a second round trip.
  React.useEffect(() => {
    if (stableScope === null) return;
    return subscribeToObservedCodingSessionEvents((events) => {
      const touchesThisSession = events.some(
        (event) =>
          (event.kind === KIND_CODING_SESSION_HANDOVER ||
            event.kind === KIND_CODING_SESSION_AUTHORITY_TRANSITION ||
            event.kind === KIND_SYSTEM_MESSAGE) &&
          event.tags.some(
            (tag) => tag[0] === "h" && tag[1] === stableScope.channelRef,
          ),
      );
      if (!touchesThisSession) return;
      void queryClient.invalidateQueries({ queryKey: key });
    });
  }, [key, queryClient, stableScope]);

  const refresh = React.useCallback(() => {
    void queryClient.invalidateQueries({ queryKey: key });
  }, [key, queryClient]);

  return {
    fold: query.data?.fold ?? null,
    authority: query.data?.authority ?? null,
    capped: query.data?.capped ?? [],
    isLoading: stableScope !== null && query.isLoading,
    errorMessage:
      query.error instanceof Error
        ? query.error.message
        : query.error
          ? String(query.error)
          : null,
    refresh,
  };
}
