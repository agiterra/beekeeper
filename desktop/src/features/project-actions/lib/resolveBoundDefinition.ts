/**
 * Resolve the definition a run is **bound to**, or say why it cannot be.
 *
 * Finding 1: the cards fetched the run and the current workflow separately
 * and put the run's bound hash beside the current definition's command. Those
 * are different objects the moment anyone republishes, and the interleaving
 * that makes it dangerous is ordinary: A waits for approval, B is published,
 * the card shows A's hash and B's command, and A is republished before the
 * click — so the relay and host correctly execute A while the owner approved
 * what they read as B.
 *
 * A kind:30620 is replaceable, so a superseded revision is not fetchable by
 * `d` tag. The relay's own `GET /workflows/{id}/autorun` reports the
 * **current** definition's hash, computed by the relay with the same
 * function the run was bound with — so comparing it to the run's binding is
 * a check, not a second implementation. When they differ, no command is
 * shown and the card says so.
 */
import { getWorkflow, getWorkflowAutorun } from "@/shared/api/tauriWorkflows";
import type { Workflow } from "@/shared/api/types";

import { hostStepCommand } from "./actionDefinition";
import type { DefinitionResolution } from "./hostStepApproval";

function sentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Compare a run's binding with the current published definition.
 *
 * Pure, so the interleaving is testable without a relay: `currentHash` is
 * what the relay says the workflow hashes to now, `definition` is that same
 * workflow's body, and `runHash` is the run's binding.
 */
export function matchBoundDefinition(input: {
  runHash: string | null;
  currentHash: string | null;
  definition: Record<string, unknown> | null;
  stepId: string;
  /** The read that failed, when one did. */
  readError?: string | null;
}): DefinitionResolution {
  if (input.readError) return { kind: "unread", reason: input.readError };
  if (!input.runHash) return { kind: "hash-unknown" };
  if (input.currentHash === null || input.definition === null) {
    return {
      kind: "unread",
      reason:
        "the published definition and its hash could not both be read, so nothing can be matched against this run's binding",
    };
  }
  if (
    input.currentHash.trim().toLowerCase() !==
    input.runHash.trim().toLowerCase()
  ) {
    return { kind: "not-current", currentHash: input.currentHash };
  }
  return {
    kind: "resolved",
    hash: input.runHash,
    command: hostStepCommand(input.definition, input.stepId),
  };
}

/** One workflow read plus its current hash, for {@link matchBoundDefinition}. */
export type BoundDefinitionRead = {
  workflow: Workflow | null;
  currentHash: string | null;
  error: string | null;
};

/** Read a workflow and the relay's own hash of its current definition. */
export async function readBoundDefinition(
  workflowId: string,
): Promise<BoundDefinitionRead> {
  const [workflow, autorun] = await Promise.all([
    getWorkflow(workflowId).then(
      (value) => ({ value, error: null as string | null }),
      (error: unknown) => ({ value: null, error: sentence(error) }),
    ),
    getWorkflowAutorun(workflowId).then(
      (value) => ({ value, error: null as string | null }),
      (error: unknown) => ({ value: null, error: sentence(error) }),
    ),
  ]);
  return {
    workflow: workflow.value,
    currentHash: autorun.value?.definitionHash ?? null,
    error: workflow.error ?? autorun.error,
  };
}
