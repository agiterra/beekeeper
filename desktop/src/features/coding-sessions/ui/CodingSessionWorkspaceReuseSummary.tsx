import {
  workspaceReuseBranchLine,
  workspaceReuseDraftSentences,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceReuseCopy";

/**
 * What the launcher draft discloses when it was opened on an existing
 * session's workspace.
 *
 * The draft is where the person decides whether this is the folder they
 * meant, and the only three facts that answer that are the absolute path,
 * the branch, and which kind of fact that branch is. So all three are shown
 * as text — no colour, no icon and no badge carries meaning on its own here,
 * because a person reading this at 250% zoom in a high-contrast theme has to
 * get the same answer as everyone else.
 *
 * Two sentences follow, fixed (`codingSessionWorkspaceReuseCopy`): this is a
 * new conversation over the files already in that folder, uncommitted bytes
 * included. Nothing in this block says the earlier session is continued or
 * taken over, because it is not: the first conversation is untouched and
 * still running wherever it was.
 *
 * How many other sessions this computer recorded at the same directory is
 * deliberately *not* here. That count belongs to the read that produced it,
 * and this block is rendered from a request restored out of `sessionStorage`
 * — a count carried this far could be re-shown long after it stopped being
 * true. It is said once, on the menu item, where it is resolved fresh.
 */
export function CodingSessionWorkspaceReuseSummary({
  branch = null,
  branchSource = null,
  path,
}: {
  branch?: string | null;
  /** `recorded` at creation, or `live` off disk. Never guessed. */
  branchSource?: "recorded" | "live" | null;
  /** Absolute, on this computer, already verified by the caller. */
  path: string;
}) {
  const sentences = workspaceReuseDraftSentences();
  return (
    <section
      aria-label="Folder this session will use"
      className="flex flex-col gap-1.5 rounded-lg border border-border bg-muted/40 p-3"
      data-testid="coding-session-workspace-reuse"
    >
      <span className="text-xs font-medium text-muted-foreground">
        Folder on this computer
      </span>
      <span
        className="break-all font-mono text-sm text-foreground"
        data-testid="coding-session-workspace-reuse-path"
      >
        {path}
      </span>
      <span
        className="text-xs text-muted-foreground"
        data-testid="coding-session-workspace-reuse-branch"
      >
        {workspaceReuseBranchLine({ branch, branchSource })}
      </span>
      <span
        className="flex flex-col text-sm text-foreground"
        data-testid="coding-session-workspace-reuse-note"
      >
        {sentences.map((sentence) => (
          <span key={sentence}>{sentence}</span>
        ))}
      </span>
    </section>
  );
}
