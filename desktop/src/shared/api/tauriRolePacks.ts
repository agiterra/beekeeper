import { invokeTauri } from "@/shared/api/tauri";
import type { RolePackSummary } from "@/shared/api/types";

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
