import { invokeTauri } from "@/shared/api/tauri";
import type {
  ProjectPackRevisionComparison,
  RolePackSummary,
  SeatDefinitionDrift,
} from "@/shared/api/types";

/**
 * Every role pack this computer would stage for `projectRef`, one row per
 * role, sorted by slug — read from the same ladder a seat's staging walks
 * (project source, checkout, installed, shipped).
 *
 * Read-only: it stages nothing and writes nothing. `projectRef` is the
 * project coordinate (`30621:<owner>:<dtag>`), or `null` for no project, in
 * which case only the rungs that need no project answer.
 */
export async function listProjectRolePacks(
  projectRef: string | null,
): Promise<RolePackSummary[]> {
  return invokeTauri<RolePackSummary[]>("list_project_role_packs", {
    projectRef,
  });
}

/**
 * How each of `shas` relates to this machine's packs checkout `HEAD`, for
 * `projectRef` — read-only, no fetch, no checkout, no write. Each `sha` must
 * be 40 lowercase hex; the caller filters shipped (`app:shipped`) refs out
 * before calling.
 */
export async function compareProjectPackRevisions(
  projectRef: string,
  shas: readonly string[],
): Promise<ProjectPackRevisionComparison> {
  return invokeTauri<ProjectPackRevisionComparison>(
    "compare_project_pack_revisions",
    { projectRef, shas },
  );
}

/**
 * Has a running seat's role definition drifted from what this computer
 * would stage for it now (spec § 4.9)? `seatSha` is the seat's
 * `packRef.sha` off its 44223; `worktree` is the seat's own tree when
 * known, so its branch override counts as "now".
 *
 * Read-only for the seat: it syncs the project's packs cache and stages the
 * two compositions under it, publishes nothing and touches no execution.
 * The answer is `unknown` — never `current` — when one side cannot be
 * composed here.
 */
export async function seatDefinitionDrift(input: {
  projectRef: string;
  role: string;
  seatSha: string;
  worktree?: string | null;
}): Promise<SeatDefinitionDrift> {
  return invokeTauri<SeatDefinitionDrift>("seat_definition_drift", {
    projectRef: input.projectRef,
    role: input.role,
    seatSha: input.seatSha,
    worktree: input.worktree ?? null,
  });
}
