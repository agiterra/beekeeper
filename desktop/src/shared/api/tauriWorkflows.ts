import { invokeTauri } from "@/shared/api/tauri";
import type {
  ApprovalActionResponse,
  TriggerWorkflowResponse,
  Workflow,
  WorkflowApproval,
  WorkflowRun,
  WorkflowSaveResult,
  TraceEntry,
} from "@/shared/api/types";
import type { WorkflowHostStep } from "@/shared/api/workflowTypes";

// ── Raw types (snake_case from backend) ───────────────────────────────────

type RawWorkflow = {
  id: string;
  revision: string;
  name: string;
  owner_pubkey: string;
  channel_id: string | null;
  definition: Record<string, unknown>;
  status: Workflow["status"];
  created_at: number;
  updated_at: number;
};

type RawWorkflowSaveResponse = RawWorkflow & {
  webhook_secret?: string | null;
};

type RawTraceEntry = {
  step_id: string;
  status: string;
  output?: Record<string, unknown>;
  started_at?: number | null;
  completed_at?: number | null;
  error?: string | null;
};

type RawWorkflowRun = {
  id: string;
  workflow_id: string;
  status: WorkflowRun["status"];
  current_step: number | null;
  execution_trace: RawTraceEntry[];
  started_at: number | null;
  completed_at: number | null;
  error_code?: string | null;
  error_message: string | null;
  created_at: number;
};

type RawWorkflowRunCursor = {
  before: string;
  before_id: string;
};

type RawWorkflowRunsResponse = {
  runs: RawWorkflowRun[];
  next: RawWorkflowRunCursor | null;
};

type RawWorkflowApproval = {
  approval_ref: string;
  workflow_id: string;
  run_id: string;
  step_id: string;
  step_index: number;
  approver_spec: string;
  status: WorkflowApproval["status"];
  approver_pubkey: string | null;
  note: string | null;
  expires_at: string;
  created_at: number;
};

type RawWorkflowApprovalsResponse = {
  approvals: RawWorkflowApproval[];
};

/** One row of `GET /workflows/{id}/runs/{run}/host-steps`, verbatim. */
type RawWorkflowHostStep = {
  run_id: string;
  step_id: string;
  workflow_id: string;
  step_index: number;
  status: WorkflowHostStep["status"];
  requested_event_id: string | null;
  expires_at: string;
  claimed_by: string | null;
  claimed_at: string | null;
  claim_event_id: string | null;
  result_event_id: string | null;
  exited_event_id: string | null;
  exit_code: number | null;
  disposition: WorkflowHostStep["disposition"];
  timed_out: boolean | null;
  duration_ms: number | null;
  head_sha: string | null;
  dirty: boolean | null;
  artifact_ref: string | null;
  routed_agent?: string | null;
  routed_command_id?: string | null;
  exited_at: string | null;
  created_at: string;
};

type RawWorkflowHostStepsResponse = {
  host_steps: RawWorkflowHostStep[];
};

type RawTriggerWorkflowResponse = {
  run_id: string;
  workflow_id: string;
  status: string;
};

type RawApprovalActionResponse = {
  token: string;
  status: string;
  run_id: string;
  workflow_id: string;
};

// ── Conversion functions ──────────────────────────────────────────────────

function fromRawWorkflow(raw: RawWorkflow): Workflow {
  return {
    id: raw.id,
    revision: raw.revision,
    name: raw.name,
    ownerPubkey: raw.owner_pubkey,
    channelId: raw.channel_id,
    definition: raw.definition,
    status: raw.status,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function fromRawWorkflowSave(raw: RawWorkflowSaveResponse): WorkflowSaveResult {
  return {
    workflow: fromRawWorkflow(raw),
    webhookSecret: raw.webhook_secret ?? null,
  };
}

function fromRawTraceEntry(raw: RawTraceEntry): TraceEntry {
  return {
    stepId: raw.step_id,
    status: raw.status,
    output: raw.output ?? {},
    startedAt: raw.started_at ?? null,
    completedAt: raw.completed_at ?? null,
    error: raw.error ?? null,
  };
}

function fromRawWorkflowRun(raw: RawWorkflowRun): WorkflowRun {
  return {
    id: raw.id,
    workflowId: raw.workflow_id,
    status: raw.status,
    currentStep: raw.current_step,
    executionTrace: raw.execution_trace.map(fromRawTraceEntry),
    startedAt: raw.started_at,
    completedAt: raw.completed_at,
    errorCode: raw.error_code ?? null,
    errorMessage: raw.error_message,
    createdAt: raw.created_at,
  };
}

export function fromRawApproval(raw: RawWorkflowApproval): WorkflowApproval {
  return {
    approvalRef: raw.approval_ref,
    workflowId: raw.workflow_id,
    runId: raw.run_id,
    stepId: raw.step_id,
    stepIndex: raw.step_index,
    approverSpec: raw.approver_spec,
    status: raw.status,
    approverPubkey: raw.approver_pubkey,
    note: raw.note,
    expiresAt: raw.expires_at,
    createdAt: raw.created_at,
  };
}

export function fromRawHostStep(raw: RawWorkflowHostStep): WorkflowHostStep {
  return {
    runId: raw.run_id,
    stepId: raw.step_id,
    workflowId: raw.workflow_id,
    stepIndex: raw.step_index,
    status: raw.status,
    requestedEventId: raw.requested_event_id ?? null,
    expiresAt: raw.expires_at,
    claimedBy: raw.claimed_by ?? null,
    claimedAt: raw.claimed_at ?? null,
    claimEventId: raw.claim_event_id ?? null,
    resultEventId: raw.result_event_id ?? null,
    exitedEventId: raw.exited_event_id ?? null,
    exitCode: raw.exit_code ?? null,
    disposition: raw.disposition ?? null,
    timedOut: raw.timed_out ?? null,
    durationMs: raw.duration_ms ?? null,
    headSha: raw.head_sha ?? null,
    dirty: raw.dirty ?? null,
    artifactRef: raw.artifact_ref ?? null,
    routedAgent: raw.routed_agent ?? null,
    routedCommandId: raw.routed_command_id ?? null,
    exitedAt: raw.exited_at ?? null,
    createdAt: raw.created_at,
  };
}

function fromRawTriggerResponse(
  raw: RawTriggerWorkflowResponse,
): TriggerWorkflowResponse {
  return {
    runId: raw.run_id,
    workflowId: raw.workflow_id,
    status: raw.status,
  };
}

function fromRawApprovalResponse(
  raw: RawApprovalActionResponse,
): ApprovalActionResponse {
  return {
    token: raw.token,
    status: raw.status,
    runId: raw.run_id,
    workflowId: raw.workflow_id,
  };
}

// ── Tauri invoke wrappers ─────────────────────────────────────────────────

export async function getChannelWorkflows(
  channelId: string,
): Promise<Workflow[]> {
  const raw = await invokeTauri<RawWorkflow[]>("get_channel_workflows", {
    channelId,
  });
  return raw.map(fromRawWorkflow);
}

/**
 * Fetch workflows across many channels in a single relay round-trip.
 *
 * Replaces the per-channel `Promise.all(getChannelWorkflows)` fanout on the
 * Workflows overview: the backend `#h` filter matches any listed channel, and
 * each returned workflow carries its own `channelId` so callers can group.
 */
export async function getChannelsWorkflows(
  channelIds: string[],
): Promise<Workflow[]> {
  const raw = await invokeTauri<RawWorkflow[]>("get_channels_workflows", {
    channelIds,
  });
  return raw.map(fromRawWorkflow);
}

export async function getWorkflow(workflowId: string): Promise<Workflow> {
  const raw = await invokeTauri<RawWorkflow>("get_workflow", { workflowId });
  return fromRawWorkflow(raw);
}

export async function createWorkflow(
  channelId: string,
  yamlDefinition: string,
): Promise<WorkflowSaveResult> {
  const raw = await invokeTauri<RawWorkflowSaveResponse>("create_workflow", {
    channelId,
    yamlDefinition,
  });
  return fromRawWorkflowSave(raw);
}

export async function updateWorkflow(
  workflowId: string,
  yamlDefinition: string,
  expectedRevision: string,
): Promise<WorkflowSaveResult> {
  const raw = await invokeTauri<RawWorkflowSaveResponse>("update_workflow", {
    workflowId,
    yamlDefinition,
    expectedRevision,
  });
  return fromRawWorkflowSave(raw);
}

export async function deleteWorkflow(workflowId: string): Promise<void> {
  await invokeTauri("delete_workflow", { workflowId });
}

export async function getWorkflowRuns(
  workflowId: string,
  limit?: number,
): Promise<WorkflowRun[]> {
  const raw = await invokeTauri<RawWorkflowRunsResponse>("get_workflow_runs", {
    workflowId,
    limit: limit ?? null,
  });
  return raw.runs.map(fromRawWorkflowRun);
}

export async function getRunApprovals(
  workflowId: string,
  runId: string,
): Promise<WorkflowApproval[]> {
  const raw = await invokeTauri<RawWorkflowApprovalsResponse>(
    "get_run_approvals",
    {
      workflowId,
      runId,
    },
  );
  return raw.approvals.map(fromRawApproval);
}

/**
 * The `run_on_host` steps of one run, as the relay records them. Empty for a
 * run whose definition has no host step.
 */
export async function getRunHostSteps(
  workflowId: string,
  runId: string,
): Promise<WorkflowHostStep[]> {
  const raw = await invokeTauri<RawWorkflowHostStepsResponse>(
    "get_run_host_steps",
    { workflowId, runId },
  );
  return raw.host_steps.map(fromRawHostStep);
}

export async function triggerWorkflow(
  workflowId: string,
): Promise<TriggerWorkflowResponse> {
  const raw = await invokeTauri<RawTriggerWorkflowResponse>(
    "trigger_workflow",
    { workflowId },
  );
  return fromRawTriggerResponse(raw);
}

/**
 * Grant a pending approval. `approvalRef` is the `approvalRef` the approvals
 * listing returns (hex of the stored token hash); the backend puts it in the
 * kind:46030's `d` tag, which is how the relay resolves the approval.
 */
export async function grantApproval(
  approvalRef: string,
  note?: string,
  scope: WorkflowApprovalScope = "run",
): Promise<ApprovalActionResponse> {
  const raw = await invokeTauri<RawApprovalActionResponse>("grant_approval", {
    approvalRef,
    note: note ?? null,
    scope,
  });
  return fromRawApprovalResponse(raw);
}

/** What a grant releases: this run, or every later run of the same definition. */
export type WorkflowApprovalScope = "run" | "action";

type RawWorkflowAutorunGrant = {
  id: string;
  definition_hash: string;
  matches_current: boolean;
  granted_by: string;
  grant_event_id: string;
  granted_at: string;
  revoked_at: string | null;
  revoke_event_id: string | null;
};

type RawWorkflowAutorun = {
  definition_hash: string;
  active: boolean;
  grants: RawWorkflowAutorunGrant[];
};

/** One autorun grant, as the relay records it (spec § 5.4). */
export type WorkflowAutorunGrant = {
  id: string;
  definitionHash: string;
  /** Whether it binds the definition as stored now; an edit changes the hash. */
  matchesCurrent: boolean;
  grantedBy: string;
  grantEventId: string;
  grantedAt: string;
  revokedAt: string | null;
  revokeEventId: string | null;
};

/** A workflow's autorun state: whether an unrevoked grant binds it now. */
export type WorkflowAutorun = {
  definitionHash: string;
  active: boolean;
  grants: WorkflowAutorunGrant[];
};

/** Read a workflow's autorun grants. */
export async function getWorkflowAutorun(
  workflowId: string,
): Promise<WorkflowAutorun> {
  const raw = await invokeTauri<RawWorkflowAutorun>("get_workflow_autorun", {
    workflowId,
  });
  return {
    definitionHash: raw.definition_hash,
    active: raw.active,
    grants: raw.grants.map((grant) => ({
      id: grant.id,
      definitionHash: grant.definition_hash,
      matchesCurrent: grant.matches_current,
      grantedBy: grant.granted_by,
      grantEventId: grant.grant_event_id,
      grantedAt: grant.granted_at,
      revokedAt: grant.revoked_at ?? null,
      revokeEventId: grant.revoke_event_id ?? null,
    })),
  };
}

/** Revoke every autorun grant of a workflow (kind 46032). */
export async function revokeAutorun(
  workflowId: string,
  channelId: string,
): Promise<{ eventId: string }> {
  const raw = await invokeTauri<{ event_id: string }>("revoke_autorun", {
    workflowId,
    channelId,
  });
  return { eventId: raw.event_id };
}

/** Deny a pending approval; same reference as {@link grantApproval}. */
export async function denyApproval(
  approvalRef: string,
  note?: string,
): Promise<ApprovalActionResponse> {
  const raw = await invokeTauri<RawApprovalActionResponse>("deny_approval", {
    approvalRef,
    note: note ?? null,
  });
  return fromRawApprovalResponse(raw);
}
