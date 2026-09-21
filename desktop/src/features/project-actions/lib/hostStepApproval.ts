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

import { truncatePubkey } from "@/shared/lib/pubkey";

import {
  hostStepCommandJson,
  hostStepCommandLines,
  type HostStepCommand,
} from "./actionDefinition";

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
  /** For a command, the argument vector behind `value`; never re-split. */
  argv?: readonly string[];
  /**
   * The same vector as rows, each with its own stable key.
   *
   * Arguments repeat legitimately (`["cp","a","a"]`), so position *is* the
   * identity here; the key is built once, in the data, rather than from a
   * render-time index.
   */
  argumentRows?: readonly { key: string; index: number; value: string }[];
  /** For a command, the unambiguous JSON form shown beside the lines. */
  json?: string;
};

/**
 * Whether the definition the **run is bound to** could be put in front of the
 * approver.
 *
 * Finding 1: the card used to show the run's bound hash beside a command read
 * off whatever definition was current, which are different objects the moment
 * anyone republishes. A command is displayed only when a definition with the
 * run's exact hash resolved; otherwise the card says which of these is true
 * and Approve is unavailable.
 */
export type DefinitionResolution =
  /** A definition whose hash equals the run's binding was resolved. */
  | { kind: "resolved"; hash: string; command: HostStepCommand | null }
  /** The only fetchable definition has a different hash. */
  | { kind: "not-current"; currentHash: string | null }
  /** The definition read failed; the relay's own words. */
  | { kind: "unread"; reason: string }
  /** The run names no definition, so nothing can be matched against it. */
  | { kind: "hash-unknown" };

/**
 * The commit the run is bound to, as the **run** reports it (lane 206).
 *
 * Three states, kept apart because they are three different claims:
 * `not-reported` is an older relay that does not carry the field at all,
 * `working-directory` is the run naming no commit (a host step runs in the
 * recorded project directory as found), and `commit` is a bound sha. Reading
 * the first as the second is how a card comes to promise an isolated
 * checkout that will not happen.
 */
export type RunCheckout =
  | { state: "not-reported" }
  | { state: "working-directory" }
  | { state: "commit"; sha: string };

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
  /** The exact command of the gated step in **that** definition. */
  command: ApprovalFact;
  /** The commit the run is bound to, off the run itself (lane 206). */
  boundCommit: ApprovalFact;
  /**
   * Whether a grant may be offered at all: what is shown is what would run.
   *
   * `false` unless the run's hash is known, a definition with that exact hash
   * resolved, the step was found in it, and the run's checkout state is
   * known. Deny is not gated by this — refusing something you cannot fully
   * see is always a safe answer, and withholding it would strand a run.
   */
  grantAvailable: boolean;
  /** Why a grant is withheld; `null` exactly when `grantAvailable`. */
  grantBlockedReason: string | null;
};

/**
 * Compose the card's facts.
 *
 * `definitionHash` is the run's binding, never the workflow's current hash:
 * approving against a hash re-read from the workflow row is precisely the
 * race lane 199 closed. `command` comes from the definition that **matched**
 * that hash, or is withheld with its reason.
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
  /** Whether the bound definition could be put in front of the approver. */
  definition: DefinitionResolution;
  /** The run's own checkout state (lane 206). */
  checkout: RunCheckout;
}): HostStepApprovalView {
  const { request } = input;
  const blocked: string[] = [];

  const definitionHash: ApprovalFact = input.runDefinitionHash
    ? { value: input.runDefinitionHash, reason: null }
    : {
        value: null,
        reason: input.runRead
          ? "this run carries no definition binding — it was created before the binding existed, and an approval cannot be tied to a definition it does not name"
          : "the run record has not been read yet",
      };
  if (definitionHash.value === null) {
    blocked.push(definitionHash.reason ?? "the run's definition is unknown");
  }
  const block = (reason: string | null) => {
    if (reason !== null) blocked.push(reason);
  };

  let command: ApprovalFact;
  switch (input.definition.kind) {
    case "resolved": {
      const resolved = input.definition.command;
      if (resolved === null) {
        command = {
          value: null,
          reason: `the definition this run is bound to names no command for step ${request.stepId}`,
        };
        block(command.reason);
      } else {
        command = {
          value: hostStepCommandLines(resolved).join("\n"),
          reason: null,
          argv: hostStepCommandLines(resolved),
          argumentRows: hostStepCommandLines(resolved).map((value, index) => ({
            key: `${index}:${value}`,
            index,
            value,
          })),
          json: hostStepCommandJson(resolved),
        };
      }
      break;
    }
    case "not-current": {
      const current = input.definition.currentHash
        ? ` The published definition now hashes to ${input.definition.currentHash.slice(0, 12)}….`
        : "";
      command = {
        value: null,
        reason: `this run is bound to a definition that is no longer the current one, and the bound text is not fetchable from here, so no command is shown.${current}`,
      };
      block(command.reason);
      break;
    }
    case "unread": {
      command = {
        value: null,
        reason: `the definition this run is bound to could not be read: ${input.definition.reason}`,
      };
      block(command.reason);
      break;
    }
    case "hash-unknown": {
      command = {
        value: null,
        reason:
          "this run names no definition, so no definition can be matched against it",
      };
      block(command.reason);
      break;
    }
  }

  let boundCommit: ApprovalFact;
  switch (input.checkout.state) {
    case "commit":
      boundCommit = { value: input.checkout.sha, reason: null };
      break;
    case "working-directory":
      boundCommit = {
        value: null,
        reason:
          "this run names no commit: the step runs in the project's recorded folder, in the working directory as found",
      };
      break;
    case "not-reported":
      boundCommit = {
        value: null,
        reason:
          "this relay does not report a run's bound commit, so what would be checked out cannot be established from here",
      };
      block(boundCommit.reason);
      break;
  }

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
    definitionHash,
    command,
    boundCommit,
    grantAvailable: blocked.length === 0,
    grantBlockedReason: blocked.length === 0 ? null : blocked[0],
  };
}

/** `project-owner:30621:<creator>:<d>` split into the parts it names. */
export function parseProjectOwnerSpec(
  approverSpec: string | null,
): { coordinate: string; creator: string } | null {
  const spec = approverSpec?.trim() ?? "";
  const coordinate = spec.startsWith("project-owner:")
    ? spec.slice("project-owner:".length).trim()
    : null;
  if (!coordinate) return null;
  // The relay takes the creator from index 1 of the coordinate and nothing
  // else (`approver_admitted`, command_executor.rs). Mirroring that exactly
  // is the point: a reader that computed it differently would offer a
  // control the relay refuses, or withhold one it would admit.
  const creator = coordinate.split(":")[1] ?? "";
  return /^[0-9a-f]{64}$/i.test(creator) ? { coordinate, creator } : null;
}

/** One roster entry, as the kind:39010 projection holds it. */
export type ApprovalRosterEntry = { pubkey: string; role: string };

/**
 * Whether the viewer may answer, and the sentence that says who may.
 *
 * Finding 9: authority is the **project's**, never the publisher's. The relay
 * admits the project creator or a current roster `Owner` of the project the
 * `approverSpec` names (`approver_admitted`); with lane 186's delegation the
 * publisher is often a lead seat, whose key says nothing about who may
 * approve. `publisherPubkey` is accepted and deliberately unused, so a caller
 * that still holds it cannot quietly reintroduce it.
 */
export function resolveApprovalAuthority(input: {
  viewerPubkey: string | null;
  approverSpec: string | null;
  roster: readonly ApprovalRosterEntry[];
  /** False while the roster read is pending or failed: unknown, not empty. */
  rosterRead: boolean;
  /** The workflow owner / `p` tag. Never consulted; see above. */
  publisherPubkey?: string | null;
}): { canApprove: boolean; sentence: string } {
  const viewer = input.viewerPubkey?.trim().toLowerCase() ?? "";
  if (!viewer) {
    return {
      canApprove: false,
      sentence: "This computer's identity is unknown, so no answer is offered.",
    };
  }
  const project = parseProjectOwnerSpec(input.approverSpec);
  if (!project) {
    return {
      canApprove: false,
      sentence: input.approverSpec
        ? `This request's approver rule is ${input.approverSpec}, which this view cannot resolve to a project; answer it with \`bee workflows approve\`.`
        : "This request names no approver rule this view can resolve, so no answer is offered.",
    };
  }
  if (viewer === project.creator.toLowerCase()) {
    return {
      canApprove: true,
      sentence: "You may answer this as the project's creator.",
    };
  }
  if (!input.rosterRead) {
    return {
      canApprove: false,
      sentence:
        "This project's roster has not been read, so whether you are one of its owners is unknown; no answer is offered.",
    };
  }
  const isOwner = input.roster.some(
    (entry) =>
      entry.pubkey.trim().toLowerCase() === viewer &&
      entry.role.trim().toLowerCase() === "owner",
  );
  if (isOwner) {
    return {
      canApprove: true,
      sentence: "You may answer this as an owner on this project's roster.",
    };
  }
  const others = input.roster
    .filter((entry) => entry.role.trim().toLowerCase() === "owner")
    .map((entry) => truncatePubkey(entry.pubkey));
  const who = [`${truncatePubkey(project.creator)} (creator)`, ...others].join(
    ", ",
  );
  return {
    canApprove: false,
    sentence: `Only a project owner may answer this: ${who}. Approving a host step is never delegated (ledger 186).`,
  };
}
