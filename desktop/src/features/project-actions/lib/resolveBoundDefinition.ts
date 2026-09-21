/**
 * Resolve the definition a run is **bound to**, from one read.
 *
 * Astra's Wave 2 re-check, R1: the previous version fetched the workflow body
 * and the relay's `/autorun` hash independently and joined them with a
 * comparison. Run A waits, B is current, the body read returns B, A is
 * republished, the hash read returns A — the comparison passes, command B is
 * displayed, and the owner approves what the relay will execute as A. **Two
 * reads joined by a comparison authenticate nothing**, whatever the
 * comparison says, so this module now takes both from
 * `get_workflow_definition`: the hash is computed natively, by the relay's own
 * canonical function, over the very value the `definition` field carries.
 *
 * Nothing here hashes anything. A TypeScript reimplementation of the relay's
 * hash would be a second answer to a question that must have one.
 */
import {
  getWorkflowDefinition,
  type WorkflowDefinitionRead,
} from "@/shared/api/tauriWorkflows";

import { hostStepCommand } from "./actionDefinition";
import type { DefinitionResolution } from "./hostStepApproval";

function sentence(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/** One `get_workflow_definition` answer, or the reason there is none. */
export type BoundDefinitionRead = {
  /** The single read's answer, or `null` when it failed. */
  read: WorkflowDefinitionRead | null;
  /** The failure, in the failing call's own words. */
  error: string | null;
};

/**
 * Compare a run's binding with the definition that arrived **with** its hash.
 *
 * Pure. The fixture for this function is a read — bytes and the hash of those
 * bytes — and cannot express a body from one source beside a hash from
 * another, which is the interleaving R1 named.
 */
export function matchBoundDefinition(input: {
  runHash: string | null;
  read: BoundDefinitionRead;
  stepId: string;
}): DefinitionResolution {
  if (input.read.error !== null) {
    return { kind: "unread", reason: input.read.error };
  }
  if (!input.runHash) return { kind: "hash-unknown" };
  const read = input.read.read;
  if (read === null) {
    return {
      kind: "unread",
      reason:
        "the published definition was not read, so nothing can be matched against this run's binding",
    };
  }
  if (read.definitionHash === null) {
    return {
      kind: "unread",
      reason:
        read.definitionHashUnavailable ??
        "this host could not reproduce the published definition's hash, so it cannot be matched against this run's binding",
    };
  }
  if (
    read.definitionHash.trim().toLowerCase() !==
    input.runHash.trim().toLowerCase()
  ) {
    return { kind: "not-current", currentHash: read.definitionHash };
  }
  return {
    kind: "resolved",
    hash: read.definitionHash,
    // From the same value the hash was taken over.
    command: hostStepCommand(read.definition, input.stepId),
  };
}

/**
 * Read the bound definition and its hash together.
 *
 * `readDefinition` is injectable so a test drives the one read; there is
 * deliberately no second parameter, because a second source is the defect.
 */
export async function readBoundDefinition(
  workflowId: string,
  deps: {
    readDefinition?: (id: string) => Promise<WorkflowDefinitionRead>;
  } = {},
): Promise<BoundDefinitionRead> {
  const read = deps.readDefinition ?? getWorkflowDefinition;
  try {
    return { read: await read(workflowId), error: null };
  } catch (error) {
    return { read: null, error: sentence(error) };
  }
}
