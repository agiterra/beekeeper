import { useQuery } from "@tanstack/react-query";

import { useMyRelayMembershipQuery } from "@/features/community-members/hooks";
import {
  classifyRelaySystemHealthError,
  parseRelaySystemHealth,
  type RelaySystemHealth,
} from "@/features/dashboard/lib/relaySystemHealth";
import { getRelaySystemHealth } from "@/shared/api/tauriRelaySystemHealth";

export const relaySystemHealthQueryKey = ["relaySystemHealth"] as const;

/** The card polls on the sampler's own cadence (`SAMPLE_INTERVAL`, 10 s). */
export const RELAY_SYSTEM_HEALTH_POLL_MS = 10_000;

/**
 * Whether this identity should even ask: stewards always; an identity with
 * no roster row might be on an open relay with no steward, which the relay
 * admits, so it asks once and the 403 settles it. A plain member is refused
 * by rule, so the request is not made at all.
 */
export function shouldReadRelaySystemHealth(
  membership: { role: string } | null | undefined,
  membershipSettled: boolean,
): boolean {
  if (!membershipSettled) return false;
  if (membership === null || membership === undefined) return true;
  return membership.role === "owner" || membership.role === "admin";
}

/**
 * The relay's machine health, refreshed every ten seconds while mounted.
 * A document the validator rejects is an error, not a partial answer.
 * Polling stops once the relay has said the caller may not read it or that
 * it has no such endpoint — neither changes by asking again.
 */
export function useRelaySystemHealthQuery() {
  const membershipQuery = useMyRelayMembershipQuery();
  const membershipSettled = !membershipQuery.isPending;
  const enabled = shouldReadRelaySystemHealth(
    membershipQuery.data,
    membershipSettled,
  );
  return useQuery<RelaySystemHealth>({
    enabled,
    queryKey: relaySystemHealthQueryKey,
    queryFn: async () => {
      const health = parseRelaySystemHealth(await getRelaySystemHealth());
      if (health === null) {
        throw new Error(
          "relay answered a machine health document this app cannot read",
        );
      }
      return health;
    },
    retry: false,
    staleTime: RELAY_SYSTEM_HEALTH_POLL_MS / 2,
    refetchInterval: (query) => {
      const failure =
        query.state.error === null
          ? null
          : classifyRelaySystemHealthError(query.state.error);
      return failure === "forbidden" || failure === "unsupported"
        ? false
        : RELAY_SYSTEM_HEALTH_POLL_MS;
    },
  });
}
