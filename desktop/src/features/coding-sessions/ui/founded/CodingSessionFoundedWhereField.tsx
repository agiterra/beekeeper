import type { NewCodingSessionWorkspaceReuse } from "../../lib/codingSessionWorkspaceReuse";
import { codingSessionLeadWorktreeName } from "../../lib/codingSessionWorktreeName";
import { NewCodingSessionWorkdirField } from "../NewCodingSessionWorkdirField";
import { NewCodingSessionWorktreeField } from "../NewCodingSessionWorktreeField";
import { codingSessionFoundedWorkspaceReuse } from "./CodingSessionFoundedWorkspaceReuse";

/**
 * Where it runs: the worktree and working-directory fields, prefilled from
 * what this computer recorded at the click, and still editable — nothing was
 * cut at the click, so nothing here is final until Start.
 *
 * The prefill's source is said in one line. Founding on one desktop and
 * starting on another finds no draft; the field then shows this computer's
 * own default for the project or channel, and the line says that is what it
 * shows rather than letting a prefilled path pass for a recorded answer. A
 * session founded "in this workspace" shows that workspace's summary — the
 * path, and the branch as read off the disk now.
 */
export function CodingSessionFoundedWhereField({
  channelId,
  disabled,
  draftSource,
  governed,
  projectRef,
  sessionName,
  setUseWorktree,
  setWorkdir,
  setWorktreeName,
  setWorktreeSource,
  useWorktree,
  workdir,
  workspaceReuse,
  worktreeName,
  worktreeSource,
}: {
  channelId: string;
  disabled: boolean;
  /** Whether this computer recorded a directory for this umbrella at the click. */
  /**
   * What this computer recorded at the click: a folder, a record with no
   * folder (founded here, nothing known), or no record at all (founded on
   * another desktop, or the record is gone). Each gets its own sentence —
   * "founded elsewhere" over a session founded here would be a lie.
   */
  draftSource: "workdir" | "empty" | "none";
  /** An agent lead gets the lead-worktree naming; you get the plain name. */
  governed: boolean;
  projectRef: string | null;
  /** The Name field's text, or null — seeds the worktree name. */
  sessionName: string | null;
  setUseWorktree: (checked: boolean) => void;
  setWorkdir: (path: string) => void;
  setWorktreeName: (name: string) => void;
  setWorktreeSource: (source: string | null) => void;
  useWorktree: boolean;
  workdir: string;
  /** The reused workspace recorded at the click, or null. */
  workspaceReuse: NewCodingSessionWorkspaceReuse | null;
  worktreeName: string;
  worktreeSource: string | null;
}) {
  const name = sessionName?.trim() || "";
  return (
    <div
      className="flex flex-col gap-3"
      data-testid="coding-session-founded-where"
    >
      <div className="flex flex-col gap-1">
        <p className="text-xs font-medium text-muted-foreground">
          {governed ? "Where the lead runs" : "Where it runs"}
        </p>
        <p
          className="text-2xs text-muted-foreground"
          data-testid="coding-session-founded-where-source"
        >
          {draftSource === "workdir"
            ? "Prefilled from what this computer recorded when the session was founded. Nothing is cut until Start."
            : draftSource === "empty"
              ? "No folder was known when this session was founded here. The field shows this computer's default for the project or channel, when it has one. Nothing is cut until Start."
              : "Nothing was recorded on this computer for this session — it was founded elsewhere, or the record is gone. The field shows this computer's default for the project or channel, when it has one."}
        </p>
      </div>
      {codingSessionFoundedWorkspaceReuse(workspaceReuse)}
      <NewCodingSessionWorktreeField
        checked={useWorktree}
        disabled={disabled}
        name={worktreeName}
        onCheckedChange={setUseWorktree}
        onNameChange={setWorktreeName}
        onSourceChange={setWorktreeSource}
        sessionName={
          governed ? codingSessionLeadWorktreeName(name || "session") : name
        }
        source={worktreeSource}
        workdir={workdir}
      />
      <NewCodingSessionWorkdirField
        channelId={channelId}
        disabled={disabled}
        onChange={setWorkdir}
        projectKey={projectRef}
        usesWorktree={useWorktree}
        value={workdir}
      />
    </div>
  );
}
