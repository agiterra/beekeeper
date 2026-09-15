import type { ManagedAgent } from "@/shared/api/types";
import { invokeTauri } from "@/shared/api/tauri";
import {
  fromRawManagedAgent,
  type RawManagedAgent,
} from "@/shared/api/tauriManagedAgentRecord";

/**
 * Permanently associate a managed agent on this computer with a project.
 *
 * Explicit and durable — the same association project setup records — and
 * distinct from borrowing, which this build does not support. The native
 * command refuses an agent already associated with a different project, a
 * setup actor, and a malformed coordinate. It grants no relay access: project
 * membership and channel access are unchanged. The association is republished
 * on the agent's kind:30177 so other computers and a lead's CLI can read it.
 */
export async function associateManagedAgentWithProject(input: {
  pubkey: string;
  projectRef: string;
}): Promise<ManagedAgent> {
  const raw = await invokeTauri<RawManagedAgent>(
    "associate_managed_agent_with_project",
    { pubkey: input.pubkey, projectRef: input.projectRef },
  );
  return fromRawManagedAgent(raw);
}
