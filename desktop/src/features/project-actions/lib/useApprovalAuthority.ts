/**
 * Who may answer a host-step approval, resolved from the project the request
 * names — the one resolver both the inbox card and the Actions tab use.
 *
 * Finding 9 of Astra's Wave 2 review: the inbox inferred authority from the
 * kind:46010's `p` tag, which is the *workflow owner*, i.e. whoever published
 * the action. Under lane 186's delegation that is normally a lead seat, so
 * the card withheld Approve from the project owner and offered the lead a
 * grant the relay refuses. The Actions tab used the project creator, which
 * still excluded legitimate co-owners.
 *
 * The relay's actual rule is `approver_admitted`
 * (`crates/buzz-relay/src/handlers/command_executor.rs`): for
 * `project-owner:<coordinate>`, the coordinate's creator **or** a current
 * roster `Owner` of that project. This mirrors it over the reads the desktop
 * already has, and never looks at the publisher.
 */
import { useQuery } from "@tanstack/react-query";

import { parseRosterEventMembers } from "@/features/projects-container/lib/projectMembers";
import { relayClient } from "@/shared/api/relayClient";
import { useIdentityQuery } from "@/shared/api/hooks";
import { KIND_PROJECT_MEMBERS } from "@/shared/constants/kinds";

import {
  parseProjectOwnerSpec,
  resolveApprovalAuthority,
  type ApprovalRosterEntry,
} from "./hostStepApproval";

/** React Query key for one project's roster, as this resolver reads it. */
export function approvalRosterQueryKey(coordinate: string) {
  return ["approval-roster", coordinate] as const;
}

/**
 * The current roster of the project a `project-owner:` spec names.
 *
 * A read, and only a read. An empty answer is `[]` **with** `rosterRead`
 * true; a failed one propagates, and the caller reports "unknown" rather
 * than offering or withholding a control on a guess.
 */
export async function fetchApprovalRoster(
  coordinate: string,
  fetchEvents: (
    filter: Parameters<typeof relayClient.fetchEvents>[0],
  ) => Promise<Awaited<ReturnType<typeof relayClient.fetchEvents>>> = (
    filter,
  ) => relayClient.fetchEvents(filter),
): Promise<ApprovalRosterEntry[]> {
  const events = await fetchEvents({
    kinds: [KIND_PROJECT_MEMBERS],
    "#d": [coordinate],
    limit: 1,
  });
  const latest = [...events].sort((a, b) => b.created_at - a.created_at)[0];
  return latest ? parseRosterEventMembers(latest) : [];
}

/** Whether this viewer may answer the request, and who may if they cannot. */
export function useApprovalAuthority(approverSpec: string | null): {
  canApprove: boolean;
  sentence: string;
} {
  const identity = useIdentityQuery();
  const project = parseProjectOwnerSpec(approverSpec);
  const roster = useQuery({
    queryKey: approvalRosterQueryKey(project?.coordinate ?? ""),
    enabled: project !== null,
    queryFn: () => fetchApprovalRoster(project?.coordinate ?? ""),
    staleTime: 30_000,
  });
  return resolveApprovalAuthority({
    viewerPubkey: identity.data?.pubkey ?? null,
    approverSpec,
    roster: roster.data ?? [],
    // A creator is admitted without the roster, so `isSuccess` gates only the
    // co-owner case — which is exactly the case that needs the read.
    rosterRead: roster.isSuccess,
  });
}
