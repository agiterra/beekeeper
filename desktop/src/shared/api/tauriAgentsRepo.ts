import type {
  AgentsRepoCommitRequest,
  AgentsRepoCommitResult,
  AgentsRepoFile,
  AgentsRepoListing,
  PlanSourceCheck,
} from "@/shared/api/agentsRepoTypes";
import { invokeTauri } from "@/shared/api/tauri";

/**
 * Every file on the agents repository's `main`, read from the packs
 * cache's fetched tip. `refresh` fetches first (the default); otherwise the
 * last fetched tip is listed and `syncedAt` says how old it is.
 */
export async function agentsRepoLs(
  projectRef: string,
  refresh = true,
): Promise<AgentsRepoListing> {
  return invokeTauri<AgentsRepoListing>("agents_repo_ls", {
    projectRef,
    refresh,
  });
}

/** One file at the tip; `state` says when `text` is null and why. */
export async function agentsRepoRead(
  projectRef: string,
  path: string,
  refresh = false,
): Promise<AgentsRepoFile> {
  return invokeTauri<AgentsRepoFile>("agents_repo_read", {
    projectRef,
    path,
    refresh,
  });
}

/**
 * Land open draft heads on `main`. Always fetches first; refuses a moved
 * tip, a stale base, or a tree the composer refuses, naming each; a verify
 * that could not run comes back `pushed: "unknown"`. The caller publishes
 * the `commit.record`.
 */
export async function agentsRepoCommitDrafts(
  request: AgentsRepoCommitRequest,
): Promise<AgentsRepoCommitResult> {
  return invokeTauri<AgentsRepoCommitResult>("agents_repo_commit_drafts", {
    request,
  });
}

/** Check a plan's source with the same reader `bee plans` and the relay use. */
export async function validatePlanSource(
  text: string,
): Promise<PlanSourceCheck> {
  return invokeTauri<PlanSourceCheck>("validate_plan_source", { text });
}
