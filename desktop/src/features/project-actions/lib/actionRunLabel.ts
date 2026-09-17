import type { WorkflowApproval, WorkflowRun } from "@/shared/api/types";
import type { WorkflowHostStep } from "@/shared/api/workflowTypes";
import { truncatePubkey } from "@/shared/lib/pubkey";

/**
 * One run's row on the Actions tab: a sentence built only from what the
 * relay's records prove (spec § 5.8). No record says "running": a claimed
 * host step with no result reads `claimed by <host>, no result yet`.
 */
export type ActionRunTone = "pending" | "ok" | "bad" | "muted";

export type ActionRunRow = {
  label: string;
  tone: ActionRunTone;
  /** The approval Approve / Deny act on, when the row is waiting on one. */
  pendingApproval: WorkflowApproval | null;
};

export type DescribeActionRunOptions = {
  /** Unix seconds; defaults to the wall clock. */
  nowSeconds?: number;
  /** Renders a unix-seconds instant for `since …`; defaults to ISO 8601. */
  formatTime?: (unixSeconds: number) => string;
};

type Detail = { label: string; tone: ActionRunTone };

function isoTime(unixSeconds: number): string {
  return new Date(unixSeconds * 1_000).toISOString();
}

function rfc3339ToSeconds(value: string | null): number | null {
  if (!value) return null;
  const millis = Date.parse(value);
  return Number.isFinite(millis) ? Math.floor(millis / 1_000) : null;
}

function hostName(step: WorkflowHostStep): string {
  return step.claimedBy ? truncatePubkey(step.claimedBy) : "host";
}

function shortSha(sha: string): string {
  return sha.length > 7 ? sha.slice(0, 7) : sha;
}

/** `action-route-<64 hex>` reads as its prefix plus the first eight hex. */
function shortCommandId(commandId: string): string {
  const match = /^(action-route-)([0-9a-f]{64})$/.exec(commandId);
  return match ? `${match[1]}${match[2].slice(0, 8)}` : commandId;
}

/** The refusal code the run's trace recorded for this step, if any. */
function traceRefusalCode(run: WorkflowRun, stepId: string): string | null {
  for (const entry of run.executionTrace) {
    if (entry.stepId !== stepId) continue;
    // `host_step_output` (crates/buzz-workflow/src/suspend.rs) writes the
    // refusal code flat as `refusal_code`.
    const code = entry.output.refusal_code;
    if (typeof code === "string" && code.length > 0) return code;
  }
  return null;
}

function hostStepDetail(
  step: WorkflowHostStep,
  run: WorkflowRun,
  formatTime: (unixSeconds: number) => string,
): Detail {
  const host = hostName(step);
  switch (step.status) {
    case "requested": {
      const since = rfc3339ToSeconds(step.createdAt);
      return {
        label:
          since === null
            ? "requested on host"
            : `requested on host (since ${formatTime(since)})`,
        tone: "pending",
      };
    }
    case "claimed":
      return { label: `claimed by ${host}, no result yet`, tone: "pending" };
    case "lost":
      return { label: "lost on host restart", tone: "bad" };
    case "expired":
      return { label: "expired: no host claimed it", tone: "muted" };
    case "exited":
      break;
  }
  switch (step.disposition) {
    case "timed_out":
      return { label: `timed out on ${host}`, tone: "bad" };
    case "lost_on_restart":
      return { label: "lost on host restart", tone: "bad" };
    case "refused": {
      const code = traceRefusalCode(run, step.stepId);
      return { label: code ? `refused: ${code}` : "refused", tone: "bad" };
    }
    case "exited":
    case null: {
      if (step.routedAgent) {
        // Spec § 5.8: `routed to <agent> · turn <commandId>` — the host
        // delivered the brief; whether the agent acted is the session's
        // story, not this row's.
        const turn = step.routedCommandId
          ? ` · turn ${shortCommandId(step.routedCommandId)}`
          : "";
        return { label: `routed to ${step.routedAgent}${turn}`, tone: "ok" };
      }
      const code = step.exitCode === null ? "?" : String(step.exitCode);
      let label = `exited ${code} on ${host}`;
      if (step.headSha) label += ` at ${shortSha(step.headSha)}`;
      if (step.dirty) label += ", dirty";
      return { label, tone: step.exitCode === 0 ? "ok" : "bad" };
    }
  }
}

function approvalDetail(approval: WorkflowApproval): Detail | null {
  const who = approval.approverPubkey
    ? ` by ${truncatePubkey(approval.approverPubkey)}`
    : "";
  switch (approval.status) {
    case "granted":
      return { label: `approved${who}`, tone: "ok" };
    case "denied":
      return { label: `denied${who}`, tone: "bad" };
    default:
      return null;
  }
}

function latestHostStep(steps: WorkflowHostStep[]): WorkflowHostStep | null {
  let latest: WorkflowHostStep | null = null;
  for (const step of steps) {
    if (
      !latest ||
      step.stepIndex > latest.stepIndex ||
      (step.stepIndex === latest.stepIndex &&
        step.createdAt.localeCompare(latest.createdAt) > 0)
    ) {
      latest = step;
    }
  }
  return latest;
}

function latestDecidedApproval(
  approvals: WorkflowApproval[],
): WorkflowApproval | null {
  let latest: WorkflowApproval | null = null;
  for (const approval of approvals) {
    if (approval.status !== "granted" && approval.status !== "denied") continue;
    if (!latest || approval.createdAt > latest.createdAt) latest = approval;
  }
  return latest;
}

function pendingApprovalOf(
  approvals: WorkflowApproval[],
  nowSeconds: number,
): WorkflowApproval | null {
  for (const approval of approvals) {
    if (approval.status !== "pending") continue;
    const expires = rfc3339ToSeconds(approval.expiresAt);
    if (expires !== null && expires <= nowSeconds) continue;
    return approval;
  }
  return null;
}

function runDetail(run: WorkflowRun): Detail | null {
  switch (run.status) {
    case "completed":
      return { label: "completed", tone: "ok" };
    case "failed":
      return {
        label: `failed: ${run.errorCode ?? run.errorMessage ?? "unknown"}`,
        tone: "bad",
      };
    case "cancelled":
      return { label: "cancelled", tone: "muted" };
    default:
      return null;
  }
}

/** What a run's own status proves when no approval or host record does. */
function runFallback(run: WorkflowRun): Detail {
  switch (run.status) {
    case "pending":
      return { label: "queued", tone: "pending" };
    case "running":
      return { label: "running relay steps", tone: "pending" };
    case "waiting_approval":
      return { label: "waiting approval", tone: "pending" };
    case "waiting_host":
      return { label: "requested on host", tone: "pending" };
    default:
      return { label: run.status, tone: "muted" };
  }
}

/**
 * Describe one run from its record, its approvals and its host steps.
 *
 * Precedence: a live pending approval wins (the row needs Approve / Deny);
 * otherwise the latest host step, else the latest granted or denied
 * approval, is the detail; a terminal run status (`completed`, `failed`,
 * `cancelled`) leads the sentence with that detail after a separator.
 */
export function describeActionRun(
  run: WorkflowRun,
  approvals: WorkflowApproval[],
  hostSteps: WorkflowHostStep[],
  options: DescribeActionRunOptions = {},
): ActionRunRow {
  const nowSeconds = options.nowSeconds ?? Math.floor(Date.now() / 1_000);
  const formatTime = options.formatTime ?? isoTime;

  const pending = pendingApprovalOf(approvals, nowSeconds);
  if (pending) {
    return {
      label: `waiting approval (since ${formatTime(pending.createdAt)})`,
      tone: "pending",
      pendingApproval: pending,
    };
  }

  const step = latestHostStep(hostSteps);
  const decided = latestDecidedApproval(approvals);
  const detail = step
    ? hostStepDetail(step, run, formatTime)
    : decided
      ? approvalDetail(decided)
      : null;

  const terminal = runDetail(run);
  if (terminal) {
    return {
      label: detail ? `${terminal.label} · ${detail.label}` : terminal.label,
      tone: terminal.tone,
      pendingApproval: null,
    };
  }
  const shown = detail ?? runFallback(run);
  return { label: shown.label, tone: shown.tone, pendingApproval: null };
}
