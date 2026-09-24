import { invokeTauri } from "@/shared/api/tauri";
import type { RunCheckout } from "@/features/project-actions/lib/hostStepApproval";
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
  /** Added by lane 190; absent from a relay that predates it. */
  workflow_name?: string | null;
  trigger_event_id?: string | null;
  trigger_author?: string | null;
  /** Hex of the definition the run was created from (lane 193). */
  definition_hash?: string | null;
  /**
   * The commit the run is bound to, added by lane 206.
   *
   * Three states, and the key's **absence** is one of them: an older relay
   * omits the field entirely, `null` means the run names no commit, and a
   * string is the bound sha. `?? null` would collapse the first two.
   */
  checkout?: string | null;
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
  routed_hired_role?: string | null;
  artifacts?: { name: string; url: string; sha256: string; bytes: number }[];
  /** How the tree the step ran in was established (lane 184). */
  checkout?: {
    mode?: string | null;
    sha?: string | null;
    headShaBefore?: string | null;
    dirtyBefore?: boolean | null;
  } | null;
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

// ── Provenance the relay records and the Actions tab renders ──────────────

/**
 * A run with the provenance lane 190 put on the wire.
 *
 * Every field is nullable and every null is a *disclosed non-answer*, never a
 * blank: a relay that predates the field, or a run created before lane 193's
 * definition binding, answers `null`, and the surface says so in those words
 * rather than leaving the row silent.
 */
/**
 * Read the run's bound commit, preserving the three states apart.
 *
 * `"checkout" in raw` is the whole point: a relay that predates lane 206
 * sends no key at all, which is "this relay does not report it" — a
 * different claim from the run naming no commit, and collapsing them is how
 * a card comes to promise an isolated checkout that will not happen.
 */
function readRunCheckout(raw: RawWorkflowRun): RunCheckout {
  if (!("checkout" in raw)) return { state: "not-reported" };
  const value = raw.checkout;
  if (typeof value === "string" && value.trim().length > 0) {
    return { state: "commit", sha: value.trim().toLowerCase() };
  }
  return { state: "working-directory" };
}

export type ProjectWorkflowRun = WorkflowRun & {
  /** The owning action's name, as the relay read it off the definition. */
  workflowName: string | null;
  /** Event id of the kind:46020 that started the run. */
  triggerEventId: string | null;
  /** Lowercase-hex pubkey that signed that trigger. */
  triggerAuthor: string | null;
  /**
   * Hex of the definition this run was created from (lane 193). `null` means
   * the run predates the binding — that is what `definition_unknown` refuses
   * on, and it is not the same fact as "the relay declined to say".
   */
  definitionHash: string | null;
  /** The commit this run is bound to, as the run itself reports it. */
  checkout: RunCheckout;
};

/**
 * How the tree a host step ran in was established (lane 184, ledger 178(g)).
 *
 * `mode` is the host's own sentence: `commit <sha>` for a bound run, or
 * `working directory as found` for an unbound one. `sha`, `headShaBefore` and
 * `dirtyBefore` are present exactly when the host could establish them.
 */
export type WorkflowHostStepCheckout = {
  mode: string | null;
  sha: string | null;
  headShaBefore: string | null;
  dirtyBefore: boolean | null;
};

/** A host step with the checkout sub-object lane 190 exposed. */
export type ProjectWorkflowHostStep = WorkflowHostStep & {
  /** `null` for a result recorded before lane 184 — unknown, not "clean". */
  checkout: WorkflowHostStepCheckout | null;
};

function readCheckout(
  raw: RawWorkflowHostStep["checkout"],
): WorkflowHostStepCheckout | null {
  if (!raw || typeof raw !== "object") return null;
  return {
    mode: typeof raw.mode === "string" ? raw.mode : null,
    sha: typeof raw.sha === "string" ? raw.sha : null,
    headShaBefore:
      typeof raw.headShaBefore === "string" ? raw.headShaBefore : null,
    dirtyBefore: typeof raw.dirtyBefore === "boolean" ? raw.dirtyBefore : null,
  };
}

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

function fromRawWorkflowRun(raw: RawWorkflowRun): ProjectWorkflowRun {
  return {
    id: raw.id,
    workflowId: raw.workflow_id,
    workflowName: raw.workflow_name ?? null,
    triggerEventId: raw.trigger_event_id ?? null,
    triggerAuthor: raw.trigger_author ?? null,
    definitionHash: raw.definition_hash ?? null,
    checkout: readRunCheckout(raw),
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

export function fromRawHostStep(
  raw: RawWorkflowHostStep,
): ProjectWorkflowHostStep {
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
    routedHiredRole: raw.routed_hired_role ?? null,
    artifacts: (raw.artifacts ?? []).map((artifact) => ({
      name: artifact.name,
      url: artifact.url,
      sha256: artifact.sha256,
      bytes: artifact.bytes,
    })),
    exitedAt: raw.exited_at ?? null,
    createdAt: raw.created_at,
    checkout: readCheckout(raw.checkout),
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

/**
 * One kind:30620 read, with the hash of the bytes it returns.
 *
 * Astra's Wave 2 re-check, R1: an approval must never join a body from one
 * read to a hash from another. `definition` is the canonical JSON value the
 * native side hashed, and `definitionHash` is `buzz_workflow::hash`'s answer
 * over exactly that value — the function the relay stores
 * `workflows.definition_hash` with. `definitionHash` is `null` with a reason
 * when this host cannot reproduce the stored hash, and a surface with no hash
 * offers no grant.
 */
export type WorkflowDefinitionRead = {
  id: string;
  /** Event id of the kind:30620 revision these bytes came from. */
  revision: string;
  name: string;
  ownerPubkey: string;
  channelId: string | null;
  /** The canonical JSON value that was hashed. */
  definition: Record<string, unknown>;
  definitionHash: string | null;
  /** Why no hash is offered; `null` exactly when `definitionHash` is set. */
  definitionHashUnavailable: string | null;
  createdAt: number;
};

type RawWorkflowDefinitionRead = {
  id: string;
  revision: string;
  name: string;
  owner_pubkey: string;
  channel_id: string | null;
  definition: Record<string, unknown>;
  definition_hash: string | null;
  definition_hash_unavailable: string | null;
  created_at: number;
};

/** Read one action's published definition and its hash, in a single read. */
export async function getWorkflowDefinition(
  workflowId: string,
): Promise<WorkflowDefinitionRead> {
  const raw = await invokeTauri<RawWorkflowDefinitionRead>(
    "get_workflow_definition",
    { workflowId },
  );
  return {
    id: raw.id,
    revision: raw.revision,
    name: raw.name,
    ownerPubkey: raw.owner_pubkey,
    channelId: raw.channel_id ?? null,
    definition: raw.definition,
    definitionHash: raw.definition_hash ?? null,
    definitionHashUnavailable: raw.definition_hash_unavailable ?? null,
    createdAt: raw.created_at,
  };
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
): Promise<ProjectWorkflowRun[]> {
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
): Promise<ProjectWorkflowHostStep[]> {
  const raw = await invokeTauri<RawWorkflowHostStepsResponse>(
    "get_run_host_steps",
    { workflowId, runId },
  );
  return raw.host_steps.map(fromRawHostStep);
}

/**
 * Start a run of a workflow.
 *
 * `checkout` is the full 40-hex commit the run is bound to (lane 184). An
 * action whose step declares `checkout: required` is refused by the relay
 * when the run names none, so the caller must ask for it rather than start a
 * run that silently tests whatever the recorded folder holds.
 */
export async function triggerWorkflow(
  workflowId: string,
  checkout?: string | null,
): Promise<TriggerWorkflowResponse> {
  const raw = await invokeTauri<RawTriggerWorkflowResponse>(
    "trigger_workflow",
    { workflowId, checkout: checkout ?? null },
  );
  return fromRawTriggerResponse(raw);
}

/** One run resolved by run id alone (`GET /workflow-runs/{run_id}`, lane 190). */
export async function getWorkflowRun(runId: string): Promise<{
  run: ProjectWorkflowRun;
  hostSteps: ProjectWorkflowHostStep[];
  approvals: WorkflowApproval[];
}> {
  // The relay merges `host_steps` and `approvals` into the run object itself
  // (`run_status`, crates/buzz-relay/src/api/workflows.rs) rather than nesting
  // the run under a key, so the raw payload is a run with two extra arrays.
  const raw = await invokeTauri<
    RawWorkflowRun & {
      host_steps?: RawWorkflowHostStep[];
      approvals?: RawWorkflowApproval[];
    }
  >("get_workflow_run", { runId });
  return {
    run: fromRawWorkflowRun(raw),
    hostSteps: (raw.host_steps ?? []).map(fromRawHostStep),
    approvals: (raw.approvals ?? []).map(fromRawApproval),
  };
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

/**
 * Grant a **standing** approval: consent for `workflowId`'s exact published
 * definition (`definitionHash`) to run on this operator's host from now on
 * — no run, no approval token, no kind:46010 in between (spec § 5.4
 * extension; ledger 252's control run 6 finding). The relay records the
 * same autorun grant an in-run `scope: action` answer would.
 */
export async function grantStandingApproval(
  workflowId: string,
  definitionHash: string,
  note?: string,
): Promise<{ eventId: string }> {
  const raw = await invokeTauri<{ event_id: string }>(
    "grant_standing_approval",
    { workflowId, definitionHash, note: note ?? null },
  );
  return { eventId: raw.event_id };
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

/**
 * `fromRawWorkflowRun`, exported for the lane-211 regression test.
 *
 * The mapper is where absent / null / sha are kept apart, and a test that
 * could not reach it would have to reproduce the distinction rather than
 * check it.
 */
export const fromRawWorkflowRunForTest = fromRawWorkflowRun;
