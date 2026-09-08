import { invokeTauri } from "@/shared/api/tauri";
import type {
  ProjectPackRevisionComparison,
  RolePackSummary,
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
