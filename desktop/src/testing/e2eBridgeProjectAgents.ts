import {
  KIND_MANAGED_AGENT,
  KIND_PROJECT_MEMBERS,
} from "@/shared/constants/kinds";
import { normalizeProjectCoordinate } from "@/shared/lib/projectAgentAssociation";

/**
 * Mock of the durable project association (`docs/PROJECT_AGENT_HIRING_IMPL.md`
 * § Association). Mirrors native `decide_association`
 * (`src-tauri/src/managed_agents/project_agent_association.rs`) refusal for
 * refusal and in the same order, so a spec that drives the Associate control
 * sees native's own sentences rather than a friendlier stand-in.
 */

/**
 * Relay kinds the association surfaces read that the mock serves from the
 * project event store (`__BUZZ_E2E_EXTRA_PROJECT_EVENTS__`): the agent's
 * owner-signed kind:30177 (by `authors`) and the relay-signed kind:39010
 * roster (by `#d`). Without a seeded event both answer empty, exactly as a
 * relay with none would.
 */
export const MOCK_PROJECT_AGENT_KINDS: ReadonlySet<number> = new Set([
  KIND_MANAGED_AGENT,
  KIND_PROJECT_MEMBERS,
]);

/** The fields of a mock managed record the association reads and writes. */
export type MockAssociableAgent = {
  name: string;
  home_role?: string | null;
  project_ref?: string | null;
  updated_at?: string;
};

/** The role the setup bootstrap runs as; native refuses to associate it. */
const SETUP_ACTOR_ROLE = "project-setup";

/**
 * Apply `associate_managed_agent_with_project` to `agent` in place. Throws
 * native's refusal sentence; the same project is a no-op.
 */
export function associateMockManagedAgent(
  agent: MockAssociableAgent,
  projectRef: unknown,
): void {
  const project =
    typeof projectRef === "string"
      ? normalizeProjectCoordinate(projectRef)
      : null;
  if (project === null) {
    throw new Error("Expected a project coordinate 30621:<owner>:<slug>.");
  }
  const role = agent.home_role?.trim() ?? "";
  if (role === SETUP_ACTOR_ROLE) {
    throw new Error(
      `${agent.name} is a project setup agent and cannot be a project agent.`,
    );
  }
  if (role === "") {
    throw new Error(
      "An agent without a primary role cannot be a project agent.",
    );
  }
  if (agent.project_ref) {
    if (normalizeProjectCoordinate(agent.project_ref) === project) return;
    throw new Error(
      `${agent.name} belongs to another project; borrowing is not supported.`,
    );
  }
  agent.project_ref = project;
  agent.updated_at = new Date().toISOString();
}
