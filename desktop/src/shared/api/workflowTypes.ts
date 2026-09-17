export type WorkflowStatus = "active" | "disabled" | "archived";

export type Workflow = {
  id: string;
  revision: string;
  name: string;
  ownerPubkey: string;
  channelId: string | null;
  definition: Record<string, unknown>;
  status: WorkflowStatus;
  createdAt: number;
  updatedAt: number;
};

export type WorkflowSaveResult = {
  workflow: Workflow;
  webhookSecret: string | null;
};

export type WorkflowRunStatus =
  | "pending"
  | "running"
  | "completed"
  | "failed"
  | "cancelled"
  | "waiting_approval"
  | "waiting_host";

export type TraceEntry = {
  stepId: string;
  status: string;
  output: Record<string, unknown>;
  startedAt: number | null;
  completedAt: number | null;
  error: string | null;
};

export type WorkflowRun = {
  id: string;
  workflowId: string;
  status: WorkflowRunStatus;
  currentStep: number | null;
  executionTrace: TraceEntry[];
  startedAt: number | null;
  completedAt: number | null;
  errorCode: string | null;
  errorMessage: string | null;
  createdAt: number;
};

export type WorkflowApprovalStatus =
  | "pending"
  | "granted"
  | "denied"
  | "expired";

export type WorkflowApproval = {
  /** Opaque, non-actionable identifier for display/correlation only. */
  approvalRef: string;
  workflowId: string;
  runId: string;
  stepId: string;
  stepIndex: number;
  approverSpec: string;
  status: WorkflowApprovalStatus;
  approverPubkey: string | null;
  note: string | null;
  expiresAt: string;
  createdAt: number;
};

export type WorkflowHostStepStatus =
  | "requested"
  | "claimed"
  | "exited"
  | "lost"
  | "expired";

export type WorkflowHostStepDisposition =
  | "exited"
  | "timed_out"
  | "lost_on_restart"
  | "refused";

/**
 * One `run_on_host` step of a run as the relay records it: who claimed it,
 * how it ended, and the event ids that prove each transition. Event ids and
 * the claiming host are lowercase hex; timestamps are RFC 3339 strings.
 */
export type WorkflowHostStep = {
  runId: string;
  stepId: string;
  workflowId: string;
  stepIndex: number;
  status: WorkflowHostStepStatus;
  requestedEventId: string | null;
  expiresAt: string;
  claimedBy: string | null;
  claimedAt: string | null;
  claimEventId: string | null;
  resultEventId: string | null;
  exitedEventId: string | null;
  exitCode: number | null;
  disposition: WorkflowHostStepDisposition | null;
  timedOut: boolean | null;
  durationMs: number | null;
  headSha: string | null;
  dirty: boolean | null;
  artifactRef: string | null;
  exitedAt: string | null;
  createdAt: string;
};

export type TriggerWorkflowResponse = {
  runId: string;
  workflowId: string;
  status: string;
};

export type ApprovalActionResponse = {
  token: string;
  status: string;
  runId: string;
  workflowId: string;
};
