/**
 * Hiring readiness for one project on this computer — the rollout notice.
 *
 * A lead's hire for a role is answered only by an agent on the founder's
 * computer whose record is associated with the project in that primary role
 * (`docs/PROJECT_AGENT_HIRING_IMPL.md` § Hiring rules). Projects that did
 * work before association existed have the work and none of the agents, so
 * their hires are refused until someone associates one.
 *
 * A line is said for a role only when the project has **evidence** of work in
 * that role (an execution or assignment, open or closed) and **no** agent on
 * this computer associated with the project in that home role. The names are
 * the identities that did that work without being associated here. Nothing
 * is inferred from them: a past seat is not membership, and nobody is
 * preselected or associated on the reader's behalf.
 */
import type { ProjectAgentRow } from "./projectAgentsModel";

/** The setup bootstrap is never hired and never associated. */
const SETUP_ROLE = "project-setup";

export type ProjectAgentsReadinessLine = {
  /** The role slug as the evidence names it. */
  role: string;
  /** Who did that role's work here without being associated here, by name. */
  workers: { pubkey: string; name: string }[];
};

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

/** An agent this computer holds, associated with the project, in `role`. */
function isHireable(row: ProjectAgentRow, role: string): boolean {
  return (
    row.section === "project" &&
    row.managedHere &&
    !row.associationMissing &&
    row.primaryRole === role
  );
}

/** An identity whose work may be named as "not associated" on this computer. */
function isUnassociatedHere(row: ProjectAgentRow): boolean {
  return !row.isProjectAgent || row.associationMissing;
}

/**
 * One line per role with evidence here and no hireable agent, sorted by role.
 * `rows` is every row of the built model, whatever its section.
 */
export function buildProjectAgentsReadiness(
  rows: readonly ProjectAgentRow[],
): ProjectAgentsReadinessLine[] {
  const workersByRole = new Map<string, Map<string, string>>();
  for (const row of rows) {
    const roles = new Set<string>();
    for (const session of row.sessions) {
      if (session.role) roles.add(session.role);
    }
    for (const assignment of row.assignments) roles.add(assignment.role);
    for (const role of roles) {
      if (role === SETUP_ROLE) continue;
      const workers = workersByRole.get(role) ?? new Map<string, string>();
      if (isUnassociatedHere(row)) workers.set(row.pubkey, row.name);
      workersByRole.set(role, workers);
    }
  }

  const lines: ProjectAgentsReadinessLine[] = [];
  for (const [role, workers] of workersByRole) {
    if (rows.some((row) => isHireable(row, role))) continue;
    lines.push({
      role,
      workers: [...workers]
        .map(([pubkey, name]) => ({ pubkey, name }))
        .sort(
          (a, b) =>
            a.name.localeCompare(b.name) || compareStrings(a.pubkey, b.pubkey),
        ),
    });
  }
  return lines.sort((a, b) => compareStrings(a.role, b.role));
}
