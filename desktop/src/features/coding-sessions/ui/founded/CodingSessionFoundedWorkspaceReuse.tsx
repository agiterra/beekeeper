import { useCodingSessionWorkspaceDraftBranch } from "@/features/coding-sessions/hooks/useCodingSessionWorkspaceReuse";
import {
  WORKSPACE_REUSE_SEAM_LANDED,
  WORKSPACE_REUSE_SENTENCES,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceReuse";
import type { NewCodingSessionWorkspaceReuse } from "@/features/coding-sessions/lib/codingSessionWorkspaceReuse";
import { WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE } from "@/features/coding-sessions/lib/codingSessionWorkspaceReuseCopy";
import { CodingSessionWorkspaceReuseSummary } from "../CodingSessionWorkspaceReuseSummary";

/**
 * Whether a founded draft carries a reused workspace worth disclosing.
 *
 * The seam flag is a parameter so the component stays covered while the flag
 * is off; the reuse itself comes from the founded draft's `workspaceSource*`
 * fields, which the founding host wrote at the click.
 */
export function isCodingSessionFoundedWorkspaceReuse(
  workspaceReuse: NewCodingSessionWorkspaceReuse | null,
  seamLanded: boolean = WORKSPACE_REUSE_SEAM_LANDED,
): boolean {
  return seamLanded && workspaceReuse !== null;
}

/** The disclosure, or nothing, for one founded draft. */
export function codingSessionFoundedWorkspaceReuse(
  workspaceReuse: NewCodingSessionWorkspaceReuse | null,
  seamLanded: boolean = WORKSPACE_REUSE_SEAM_LANDED,
) {
  if (!isCodingSessionFoundedWorkspaceReuse(workspaceReuse, seamLanded)) {
    return null;
  }
  return <CodingSessionFoundedWorkspaceReuse workspaceReuse={workspaceReuse} />;
}

/**
 * The reused workspace's summary under the founded page's Where field.
 *
 * The branch recorded at the click is never rendered as a fact about now: the
 * head is read from that exact directory once, when the page opens, and only
 * that read earns "on disk now". A folder that is gone says so and shows no
 * branch at all.
 */
export function CodingSessionFoundedWorkspaceReuse({
  workspaceReuse,
}: {
  workspaceReuse: NewCodingSessionWorkspaceReuse | null;
}) {
  const draft = useCodingSessionWorkspaceDraftBranch(workspaceReuse);
  if (workspaceReuse === null) return null;
  if (draft.missing) return <WorkspaceGoneNote path={workspaceReuse.path} />;
  return (
    <CodingSessionWorkspaceReuseSummary
      branch={draft.branch}
      branchSource={draft.branchSource}
      path={workspaceReuse.path}
    />
  );
}

/**
 * The folder named by this draft is not on this computer any more.
 *
 * Both sentences are the ones already written for this case — the absence
 * this app states everywhere else, and the launcher's own way forward — so
 * there is no second wording of the same fact. The path is still shown: the
 * person is owed the name of what went missing.
 */
function WorkspaceGoneNote({ path }: { path: string }) {
  return (
    <section
      aria-label="Folder this session will use"
      className="flex flex-col gap-1.5 rounded-lg border border-destructive/40 bg-muted/40 p-3"
      data-testid="coding-session-workspace-reuse-gone"
    >
      <span className="break-all font-mono text-sm text-foreground">
        {path}
      </span>
      <span className="text-sm text-foreground">
        {WORKSPACE_REUSE_SENTENCES.missing}
      </span>
      <span className="text-xs text-muted-foreground">
        {WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE}
      </span>
    </section>
  );
}
