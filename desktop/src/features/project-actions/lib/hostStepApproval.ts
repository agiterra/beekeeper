/**
 * A parked host-step approval, as a card can honestly render it.
 *
 * Ledger 171(b): the Dashboard inbox showed a kind:46010 as its raw JSON with
 * a reply composer and no control, so the only way to approve a command about
 * to run on this computer was to hand-sign a kind:46030. This module turns the
 * request and the run's own records into the five facts a person needs before
 * saying yes — which action, which step, which definition, which commit, and
 * the exact command — and is explicit about every one it cannot establish.
 *
 * Nothing here decides authority. The relay does that; these functions only
 * refuse to *offer* a control whose refusal is already knowable, and never
 * present an unknown as a blank.
 */

/** A kind:46010's content, as `approval_request_wire` writes it. */
export const APPROVAL_REQUEST_SCHEMA = "buzz-approval-request/v1";

/** The request itself: everything the kind:46010 carries. */
export type HostStepApprovalRequest = {
  /** The `d` tag: hex of the stored token hash, and the 46030's `d`. */
  approvalRef: string;
  runId: string;
  workflowId: string;
  workflowName: string | null;
  stepId: string;
  stepIndex: number | null;
  approverSpec: string | null;
  message: string | null;
  /** Unix seconds. */
  expiresAt: number | null;
  /** True when the engine inserted the gate before a `run_on_host` step. */
  synthetic: boolean;
  channelId: string | null;
};

function str(value: unknown): string | null {
  return typeof value === "string" && value.trim().length > 0
    ? value.trim()
    : null;
}

/**
 * Read a signed kind:46010 into a request, or `null` when the event does not
 * carry the shape this reader fully recognises.
 *
 * Strict on purpose: a half-read approval request would offer an Approve
 * button for a step the card could not name.
 */
export function readHostStepApprovalRequest(event: {
  kind: number;
  tags: readonly (readonly string[])[];
  content: string;
}): HostStepApprovalRequest | null {
  if (event.kind !== 46010) return null;
  const approvalRef = event.tags.find((tag) => tag[0] === "d")?.[1] ?? null;
  if (approvalRef === null || !/^[0-9a-f]{64}$/i.test(approvalRef)) return null;
  let body: unknown;
  try {
    body = JSON.parse(event.content);
  } catch {
    return null;
  }
  if (!body || typeof body !== "object" || Array.isArray(body)) return null;
  const record = body as Record<string, unknown>;
  if (record.schema !== APPROVAL_REQUEST_SCHEMA) return null;
  const runId = str(record.runId);
  const workflowId = str(record.workflowId);
  const stepId = str(record.stepId);
  if (runId === null || workflowId === null || stepId === null) return null;
  return {
    approvalRef: approvalRef.toLowerCase(),
    runId,
    workflowId,
    workflowName: str(record.workflowName),
    stepId,
    stepIndex:
      typeof record.stepIndex === "number" && Number.isFinite(record.stepIndex)
        ? record.stepIndex
        : null,
    approverSpec: str(record.approverSpec),
    message: str(record.message),
    expiresAt:
      typeof record.expiresAt === "number" && Number.isFinite(record.expiresAt)
        ? record.expiresAt
        : null,
    synthetic: record.synthetic === true,
    channelId: event.tags.find((tag) => tag[0] === "h")?.[1] ?? null,
  };
}

/** One fact of the card: established, or unavailable with its reason. */
export type ApprovalFact = {
  value: string | null;
  /** Why there is no value. `null` exactly when `value` is set. */
  reason: string | null;
};

/** Everything the card renders, each fact established or explicitly absent. */
export type HostStepApprovalView = {
  approvalRef: string;
  runId: string;
  actionName: string;
  stepId: string;
  stepIndex: number | null;
  message: string | null;
  /** Who the relay will admit, in its own `approver_spec` words. */
  approverSpec: string | null;
  expiresAt: number | null;
  /** Hex of the definition this run is bound to (lane 193). */
  definitionHash: ApprovalFact;
  /** The exact argv of the gated step in the published definition. */
  command: ApprovalFact;
  /** The commit the run is bound to (lane 184). */
  boundCommit: ApprovalFact;
};

/**
 * Compose the card's facts.
 *
 * `definitionHash` is the run's binding, never the workflow's current hash:
 * approving against a hash re-read from the workflow row is precisely the race
 * lane 199 closed, and showing that hash would tell the operator they are
 * approving something they are not.
 */
export function buildHostStepApprovalView(input: {
  request: Pick<
    HostStepApprovalRequest,
    | "approvalRef"
    | "runId"
    | "workflowName"
    | "stepId"
    | "stepIndex"
    | "approverSpec"
    | "message"
    | "expiresAt"
  >;
  /** Fallback name when the request carried none. */
  workflowName?: string | null;
  /** The run's own `definition_hash`, or `null` when unread/unbound. */
  runDefinitionHash: string | null;
  /** True when the run was read and answered `null` for its binding. */
  runRead: boolean;
  /** The published definition of the gated step, or `null` when unread. */
  command: string | null;
  /** True when the definition was read and names no command for the step. */
  definitionRead: boolean;
  /** The commit a host step of this run recorded, when one has. */
  boundCommit: string | null;
}): HostStepApprovalView {
  const { request } = input;
  return {
    approvalRef: request.approvalRef,
    runId: request.runId,
    actionName:
      request.workflowName ?? input.workflowName ?? "this project action",
    stepId: request.stepId,
    stepIndex: request.stepIndex,
    message: request.message,
    approverSpec: request.approverSpec,
    expiresAt: request.expiresAt,
    definitionHash: input.runDefinitionHash
      ? { value: input.runDefinitionHash, reason: null }
      : {
          value: null,
          reason: input.runRead
            ? "this run carries no definition binding — it was created before the binding existed, and an approval cannot be tied to a definition it does not name"
            : "the run record has not been read yet",
        },
    command: input.command
      ? { value: input.command, reason: null }
      : {
          value: null,
          reason: input.definitionRead
            ? `the published definition names no command for step ${request.stepId}`
            : "the published definition has not been read yet",
        },
    boundCommit: input.boundCommit
      ? { value: input.boundCommit, reason: null }
      : {
          value: null,
          // The relay's run wire carries `trigger_context.author` and not its
          // `checkout`, so before the host claims the step there is no
          // record of the bound commit to read. Saying so is the honest
          // answer; guessing "the working directory" would not be.
          reason:
            "no record of this run names a commit yet — the host writes it when it establishes the tree",
        },
  };
}

/** Whether the viewer may answer, and the sentence that says who may. */
export function approvalAuthority(input: {
  viewerPubkey: string | null;
  projectOwner: string | null;
  approverSpec: string | null;
}): { canApprove: boolean; sentence: string } {
  const viewer = input.viewerPubkey?.toLowerCase() ?? null;
  const owner = input.projectOwner?.toLowerCase() ?? null;
  if (viewer === null) {
    return {
      canApprove: false,
      sentence: "This computer's identity is unknown, so no answer is offered.",
    };
  }
  if (owner === null) {
    return {
      canApprove: false,
      sentence:
        "The project owner is unknown here, so no answer is offered from this view.",
    };
  }
  if (viewer === owner) {
    return {
      canApprove: true,
      sentence: "You may answer this as the project owner.",
    };
  }
  // Ledger 186: approving a host step is the one project-action capability
  // that is *not* delegable, so a lead seat or a collaborator sees the
  // request and no control, with the reason named.
  return {
    canApprove: false,
    sentence: `Only the project owner ${owner.slice(0, 8)}… may answer this${
      input.approverSpec ? ` (the relay's rule is ${input.approverSpec})` : ""
    }. Approving a host step is never delegated.`,
  };
}
