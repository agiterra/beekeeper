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

/** What `artifact_preview_open` hands back. */
export type ArtifactPreviewHandle = {
  /** The opaque token the `buzz-doc` scheme serves this snapshot under. */
  token: string;
  /** The window label, for closing it. */
  label: string;
  /** The url the window loads. */
  url: string;
  /** Sibling assets the snapshot carries, by repository path. */
  assets: string[];
  /**
   * Assets the document references that the snapshot could not supply. Shown
   * by name rather than left to render as broken images.
   */
  missing: string[];
};

/**
 * Open an HTML document artifact in its own window, with its scripts running.
 *
 * The window has **no capabilities** (its label matches nothing in
 * `capabilities/default.json`) and the document is served under its own CSP
 * with no network, so the mockup behaves as itself without reaching the app.
 * `draftText` previews the open draft; omit it to preview what is on `main`.
 */
export async function artifactPreviewOpen(
  projectRef: string,
  path: string,
  draftText?: string | null,
): Promise<ArtifactPreviewHandle> {
  return invokeTauri<ArtifactPreviewHandle>("artifact_preview_open", {
    projectRef,
    path,
    draftText: draftText ?? null,
  });
}

/** Drop a preview's snapshot and close its window. */
export async function artifactPreviewClose(token: string): Promise<void> {
  return invokeTauri<void>("artifact_preview_close", { token });
}
