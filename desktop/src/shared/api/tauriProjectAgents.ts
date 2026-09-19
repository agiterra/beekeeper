import type { ManagedAgent } from "@/shared/api/types";
import { invokeTauri } from "@/shared/api/tauri";
import {
  fromRawManagedAgent,
  type RawManagedAgent,
} from "@/shared/api/tauriManagedAgentRecord";

/** What associating an agent with a project produced. */
export type AssociateManagedAgentResult = {
  agent: ManagedAgent;
  /** The host put the agent on the project roster as a collaborator. */
  rosterAdded: boolean;
  /**
   * Why the roster op did not land, in the host's words; the association
   * itself still landed. A host older than this field reports `null` here
   * and `false` above — nothing is invented.
   */
  rosterError: string | null;
};

/**
 * Permanently associate a managed agent on this computer with a project.
 *
 * Explicit and durable — the same association project setup records — and
 * distinct from borrowing, which this build does not support. The native
 * command refuses an agent already associated with a different project, a
 * setup actor, and a malformed coordinate. When the viewer may write the
 * roster it also puts the agent on the project roster as a collaborator, so
 * the agent can read and write the project (Pulse, to-dos) under its own
 * key; a roster failure is disclosed on the result, not swallowed. The
 * association is republished on the agent's kind:30177 so other computers
 * and a lead's CLI can read it.
 */
export async function associateManagedAgentWithProject(input: {
  pubkey: string;
  projectRef: string;
}): Promise<AssociateManagedAgentResult> {
  const raw = await invokeTauri<
    RawManagedAgent & { rosterAdded?: unknown; rosterError?: unknown }
  >("associate_managed_agent_with_project", {
    pubkey: input.pubkey,
    projectRef: input.projectRef,
  });
  return {
    agent: fromRawManagedAgent(raw),
    rosterAdded: raw.rosterAdded === true,
    rosterError: typeof raw.rosterError === "string" ? raw.rosterError : null,
  };
}
