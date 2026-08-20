import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  getRelaySelf,
  relaySelfQueryKey,
} from "@/features/moderation/lib/relaySelf";
import {
  banMember,
  type CommunityRestriction,
  listAuditActions,
  listReports,
  listRestrictions,
  type ModerationAction,
  type ModerationReport,
  type ReportType,
  type ResolutionAction,
  type ResolutionStatus,
  resolveReport,
  submitReport,
  timeoutMember,
  unbanMember,
  untimeoutMember,
} from "@/shared/api/moderation";

export const moderationReportsQueryKey = ["moderationReports"] as const;
export const moderationAuditQueryKey = ["moderationAudit"] as const;
export const moderationRestrictionsQueryKey = [
  "moderationRestrictions",
] as const;
export { relaySelfQueryKey };

/**
 * How long an observed relay `self` pubkey is trusted without re-asking.
 *
 * It used to be `Infinity`, which meant the value was fetched once per session
 * and never revalidated — a relay reinstalled at the same URL with a fresh
 * keypair kept being described by the previous instance's key for as long as
 * the app stayed open. Five minutes bounds that window while keeping the NIP-11
 * round trip off the hot path: the value genuinely is near-static (a relay's
 * signing key changes only on reinstall/rotation), and the query is mounted by
 * several frequently-remounted surfaces (channel screen, channel pane, forum,
 * home), which would otherwise refetch on every navigation.
 *
 * `refetchOnWindowFocus` is enabled for this one query — the app's default is
 * `false` — because returning to the app after a break is exactly when a relay
 * is most likely to have been redeployed underneath it.
 */
const RELAY_SELF_STALE_TIME_MS = 5 * 60 * 1_000;

/**
 * The active relay's NIP-11 `self` pubkey (hex), or `null` when it advertises
 * none. Used to recognize relay-signed state and moderation DMs. Community-
 * scoped: the cache entry is dropped on community switch by
 * `resetCommunityState`. A `null` result is a valid answer, while request
 * failures remain query errors.
 */
export function useRelaySelfQuery(enabled = true) {
  return useQuery({
    enabled,
    queryKey: relaySelfQueryKey,
    queryFn: getRelaySelf,
    staleTime: RELAY_SELF_STALE_TIME_MS,
    refetchOnWindowFocus: true,
  });
}

// --- Reads (mod-authz gated; consumed by the U2 queue/audit surfaces) ---

export function useModerationReportsQuery(
  options?: { status?: string; limit?: number },
  enabled = true,
) {
  return useQuery({
    enabled,
    queryKey: [
      ...moderationReportsQueryKey,
      options?.status ?? null,
      options?.limit ?? null,
    ],
    queryFn: () => listReports(options),
    staleTime: 15_000,
  });
}

export function useModerationAuditQuery(limit?: number, enabled = true) {
  return useQuery({
    enabled,
    queryKey: [...moderationAuditQueryKey, limit ?? null],
    queryFn: () => listAuditActions(limit),
    staleTime: 15_000,
  });
}

export function useModerationRestrictionsQuery(enabled = true) {
  return useQuery({
    enabled,
    queryKey: moderationRestrictionsQueryKey,
    queryFn: listRestrictions,
    staleTime: 15_000,
  });
}

// --- Writes ---
//
// Moderation writes are relay-validated command events whose effects surface in
// the queue/audit/restricted reads after processing, so mutations invalidate the
// affected read queries on success rather than fabricating optimistic rows.

function useInvalidateModerationReads() {
  const queryClient = useQueryClient();
  return () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: moderationReportsQueryKey }),
      queryClient.invalidateQueries({ queryKey: moderationAuditQueryKey }),
      queryClient.invalidateQueries({
        queryKey: moderationRestrictionsQueryKey,
      }),
    ]);
}

/** Submit a NIP-56 report. Does not touch the mod-gated read caches. */
export function useSubmitReportMutation() {
  return useMutation({
    mutationFn: (input: {
      authorPubkey: string;
      eventId: string;
      reportType: ReportType;
      note?: string;
    }) => submitReport(input),
  });
}

export function useBanMemberMutation() {
  const invalidate = useInvalidateModerationReads();
  return useMutation({
    mutationFn: (input: {
      pubkey: string;
      expiresAt?: number;
      reason?: string;
    }) => banMember(input),
    onSuccess: invalidate,
  });
}

export function useUnbanMemberMutation() {
  const invalidate = useInvalidateModerationReads();
  return useMutation({
    mutationFn: (pubkey: string) => unbanMember(pubkey),
    onSuccess: invalidate,
  });
}

export function useTimeoutMemberMutation() {
  const invalidate = useInvalidateModerationReads();
  return useMutation({
    mutationFn: (input: {
      pubkey: string;
      expiresAt: number;
      reason?: string;
    }) => timeoutMember(input),
    onSuccess: invalidate,
  });
}

export function useUntimeoutMemberMutation() {
  const invalidate = useInvalidateModerationReads();
  return useMutation({
    mutationFn: (pubkey: string) => untimeoutMember(pubkey),
    onSuccess: invalidate,
  });
}

export function useResolveReportMutation() {
  const invalidate = useInvalidateModerationReads();
  return useMutation({
    mutationFn: (input: {
      reportEventId: string;
      status: ResolutionStatus;
      action: ResolutionAction;
      reason?: string;
    }) => resolveReport(input),
    onSuccess: invalidate,
  });
}

export type {
  CommunityRestriction,
  ModerationAction,
  ModerationReport,
  ReportType,
  ResolutionAction,
  ResolutionStatus,
};
