import {
  type CodingSessionLaunchGoalReader,
  type CodingSessionLaunchLead,
  type CodingSessionLaunchPlanLine,
  type CodingSessionLaunchReadiness,
  codingSessionLaunchPlan,
  codingSessionLaunchReadiness,
} from "../../lib/codingSessionLaunchForm";
import type { CodingSessionSetupMode } from "../../lib/codingSessionSetupMode";

/**
 * Every busy state of the founded page, as the sentence a person reads.
 *
 * It reaches Start through readiness rather than beside it, so a disabled
 * control can never be silent (REVIEW-B3 F7). Null when nothing is in
 * flight.
 */
export function codingSessionFoundedBusySentence(input: {
  /**
   * A receipt-joined create already claims this umbrella and its first
   * metadata has not arrived — the projection's `starting` state. Starting
   * again here would seat a second lead under the same genesis.
   */
  starting?: boolean;
  /** Team: the worktree being cut is the lead's. Solo: it is yours. */
  governed: boolean;
  /** The channel's project is being read, or could not be; null when settled. */
  channelSentence: string | null;
  isPublishing: boolean;
  isLaunching: boolean;
  isPreparing: boolean;
  transactionPending: boolean;
  isLaunchPreflighting: boolean;
  isPreparingRoles: boolean;
  isScanning: boolean;
}): string | null {
  if (input.starting) {
    return "The provider accepted a create for this session; its first report is still to come. Starting again would seat a second lead.";
  }
  if (input.channelSentence) return input.channelSentence;
  if (input.isPublishing) return "Publishing this session…";
  if (input.isLaunching) return "Starting…";
  if (input.isPreparing) {
    return input.governed
      ? "Cutting the lead's worktree…"
      : "Cutting the worktree…";
  }
  if (input.transactionPending) {
    return "A create for this session is already in flight from this screen.";
  }
  if (input.isLaunchPreflighting) {
    return "Re-checking readiness before starting…";
  }
  if (input.isPreparingRoles) return "Preparing the project's roles…";
  if (input.isScanning) return "Scanning the project's role packs…";
  return null;
}

/**
 * Readiness and the plan for the founded page's Start, Solo or Team.
 *
 * The shared readiness carries everything, including the goal reader gate:
 * a Start must not publish or shadow an initial prompt it has not read off
 * the wire. The plan is the shared one too — its first lines are the two
 * text fields, only when they differ from the wire, so the list never names
 * a 44229 or 44227 that Start would not publish.
 */
export function codingSessionFoundedReadiness(input: {
  mode: CodingSessionSetupMode;
  /** The initial prompt as the field holds it — draft or wire. */
  goal: string;
  goalOverflow: { bytes: number; cap: number } | null;
  goalReader: CodingSessionLaunchGoalReader;
  /** Whether `useCodingSessionNames` has settled once for this channel. */
  nameResolved: boolean;
  lead: CodingSessionLaunchLead;
  providerInstanceRef: string | null;
  providerAuthorityPubkey: string | null;
  leadModel: string | null;
  modelOverrideReason: string;
  modelOverridden: boolean;
  providerRefusal: string | null;
  projectRef: string | null;
  useRoles: boolean;
  readinessGate: { allowed: boolean; reason: string | null };
  teamReadinessLoading: boolean;
  teamReadinessError: string | null;
  policySet: boolean;
  benchCount: number;
  unresolvedBenchIdentities: readonly string[];
  busySentence: string | null;
  nameDirty: boolean;
  promptDirty: boolean;
  useWorktree: boolean;
  worktreeName: string;
}): {
  readiness: CodingSessionLaunchReadiness;
  plan: CodingSessionLaunchPlanLine[];
} {
  // Team drafts survive mode switches, but hidden team choices cannot gate Solo.
  const team = input.mode === "team";
  const readiness = codingSessionLaunchReadiness({
    mode: input.mode,
    goal: input.goal,
    goalOverflow: input.goalOverflow,
    goalReader: input.goalReader,
    nameReader: input.nameResolved ? "resolved" : "unresolved",
    nameDirty: input.nameDirty,
    useWorktree: input.useWorktree,
    worktreeName: input.worktreeName,
    lead: input.lead,
    providerInstanceRef: input.providerInstanceRef,
    providerAuthorityPubkey: input.providerAuthorityPubkey,
    leadModel: input.leadModel,
    modelOverrideReason: input.modelOverrideReason,
    modelOverridden: input.modelOverridden,
    providerRefusal: input.providerRefusal,
    projectReadiness:
      team && input.projectRef && input.useRoles
        ? {
            allowed: input.readinessGate.allowed,
            reason: input.readinessGate.reason,
          }
        : null,
    projectReadinessUnknown:
      team &&
      input.useRoles &&
      input.projectRef !== null &&
      (input.teamReadinessLoading || input.teamReadinessError !== null),
    ...(input.lead.kind === "agent" && input.lead.hasRolePack !== undefined
      ? { leadHasRolePack: input.lead.hasRolePack }
      : {}),
    policySet: input.policySet,
    unresolvedBenchIdentities: team ? input.unresolvedBenchIdentities : [],
    busySentence: input.busySentence,
  });
  const plan = codingSessionLaunchPlan({
    mode: input.mode,
    lead: input.lead,
    nameDirty: input.nameDirty,
    promptDirty: input.promptDirty,
    policySet: input.policySet,
    benchCount: input.benchCount,
  });
  return { readiness, plan };
}
