import {
  NewCodingSessionChannelPicker,
  NewCodingSessionProjectDestination,
} from "./NewCodingSessionDestination";

import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import {
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionTarget,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import { useCodingSessionWorkspaceDraftBranch } from "@/features/coding-sessions/hooks/useCodingSessionWorkspaceReuse";
import {
  WORKSPACE_REUSE_SEAM_LANDED,
  WORKSPACE_REUSE_SENTENCES,
} from "@/features/coding-sessions/lib/codingSessionWorkspaceReuse";
import { WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE } from "@/features/coding-sessions/lib/codingSessionWorkspaceReuseCopy";
import { CodingSessionWorkspaceReuseSummary } from "./CodingSessionWorkspaceReuseSummary";
import { NewCodingSessionForm } from "./NewCodingSessionLaunchForm";

/**
 * One session's existing checkout, offered to a *new* session.
 *
 * Resolved and verified before the dialog opens (`codingSessionWorkspaceReuse
 * .ts`): only an "available" resolution produces one of these, so the dialog
 * never has to ask whether the directory is really there. It is a local launch
 * hint and nothing more — the path names one machine's disk and never enters a
 * published event field.
 */
export type NewCodingSessionWorkspaceReuse = {
  /** Absolute path on this machine, already verified by the caller. */
  path: string;
  /** Recorded branch, display only. Never published. */
  branch: string | null;
  /**
   * Only ever `"recorded"`: the branch the worktree was cut on, which is a
   * creation-time fact and cannot go stale. A live head is never carried
   * here — the draft reads that itself, on open.
   */
  branchSource?: "recorded" | null;
};

/**
 * A project the session is being created inside.
 *
 * Supplied by the projects feature's wrapper (glue): the dialog stays the one
 * create flow and only swaps the channel *question* for a channel *fact*.
 */
export type NewCodingSessionProjectContext = {
  projectId: string;
  projectName: string;
  /** Coordinate signed into the create as the session's placement authority. */
  projectRef: string | null;
  /**
   * The repository coordinate (`30617:<owner>:<d>`) this launch's create
   * should name, or null when none is known.
   *
   * LANE-L20 (finding 38): resolved from the checkout the launch found — the
   * project repo whose registered/scanned checkout matched, or the project's
   * only repository — never guessed among two or more with no checkout match.
   */
  repoRef: string | null;
  /** The project's sessions channel, or null until this create publishes one. */
  channelId: string | null;
  /**
   * Local checkout of one of the project's repositories, resolved async —
   * the workdir prefill when the provider has nothing remembered yet.
   */
  defaultWorkdir: string | null;
  /**
   * The project's per-device default agent seat, prefilled once the managed
   * agents resolve — only while the seat is untouched, and only when the
   * agent is still one this computer manages (a stale default is silently
   * ignored rather than producing a create the provider refuses).
   */
  defaultSeat?: { actor: string; role: string } | null;
  /**
   * Resolve — creating it if needed — the channel this session belongs in.
   * Called once, on submit: opening the dialog and walking away must not
   * leave a channel behind.
   */
  ensureChannelId: () => Promise<string>;
};

/**
 * Everything the launcher is handed, assembled in one place.
 *
 * Exported because two facts have to survive together and nothing downstream
 * can re-derive either: the project's placement (`projectRef`) and repository
 * binding (`repoRef`), which the create signs, and the one-off directory a
 * "New session in this workspace" draft reuses. A reuse draft opened from a
 * project session carries all three — reusing a directory changes where the
 * session *runs*, never which project it belongs to.
 *
 * The `workspaceReuse` prop is root's seam (contract §1) and has not landed
 * in `NewCodingSessionLaunchForm.tsx` yet — a reserved file no lane may edit.
 * Widening the props here rather than at the JSX site keeps the value flowing
 * the moment root lands it, without a `@ts-expect-error` and without this file
 * pretending to know the form's prop bag. Delete the intersection below (and
 * this paragraph) once the prop exists.
 */
export function newCodingSessionFormProps(input: {
  channelId?: string;
  onDone: () => void;
  projectContext: NewCodingSessionProjectContext | null;
  workspaceReuse: NewCodingSessionWorkspaceReuse | null;
}): Parameters<typeof NewCodingSessionForm>[0] & {
  workspaceReuse?: NewCodingSessionWorkspaceReuse | null;
} {
  return {
    channelId: input.channelId,
    onDone: input.onDone,
    projectContext: input.projectContext,
    workspaceReuse: input.workspaceReuse,
  };
}

/**
 * What a reuse draft discloses, and what an ordinary draft does not.
 *
 * It sits above the form because it answers the question the person is about
 * to act on — is this the folder I meant? — and a varied title alone says
 * "this workspace" without ever naming which one. An ordinary draft renders
 * nothing at all here: there is no workspace to describe, and an empty
 * bordered block would imply a fact that does not exist.
 *
 * The branch is read from that directory once when the draft opens
 * (`useCodingSessionWorkspaceDraftBranch`), because the request carries only
 * a recorded branch and a record is not a claim about now. If the read says
 * the folder is gone, the draft says *that* and shows no branch — the
 * launcher's own folder picker is still right there, which is what the second
 * sentence points at.
 */
/**
 * Is this draft actually a workspace reuse?
 *
 * The one predicate behind both the heading and the disclosure. They were two
 * conditions once, and the heading kept the one the disclosure had already
 * dropped: with the seam unlanded, an ordinary unseeded draft was headed "New
 * session in this workspace" over a repository folder it had nothing to do
 * with. A title is a claim like any other block on the screen.
 *
 * The flag is a parameter so both states stay testable while it is off.
 */
export function isNewCodingSessionWorkspaceReuse(
  workspaceReuse: NewCodingSessionWorkspaceReuse | null,
  seamLanded: boolean = WORKSPACE_REUSE_SEAM_LANDED,
): boolean {
  return seamLanded && workspaceReuse !== null;
}

/**
 * The dialog's heading.
 *
 * Same predicate as the disclosure, on purpose: a heading that says "this
 * workspace" over a draft that has not been seeded with one is the same
 * untruth in fewer words.
 */
export function newCodingSessionDialogTitle(input: {
  projectContext: NewCodingSessionProjectContext | null;
  workspaceReuse: NewCodingSessionWorkspaceReuse | null;
  seamLanded?: boolean;
}): string {
  if (
    isNewCodingSessionWorkspaceReuse(
      input.workspaceReuse,
      input.seamLanded ?? WORKSPACE_REUSE_SEAM_LANDED,
    )
  ) {
    return "New session in this workspace";
  }
  if (input.projectContext !== null) {
    return `New coding session in ${input.projectContext.projectName}`;
  }
  return "New coding session";
}

/** The disclosure, or nothing, for one draft. */
export function newCodingSessionWorkspaceDisclosure(
  workspaceReuse: NewCodingSessionWorkspaceReuse | null,
  seamLanded: boolean = WORKSPACE_REUSE_SEAM_LANDED,
) {
  if (!isNewCodingSessionWorkspaceReuse(workspaceReuse, seamLanded)) {
    return null;
  }
  return (
    <NewCodingSessionWorkspaceDisclosure workspaceReuse={workspaceReuse} />
  );
}

export function NewCodingSessionWorkspaceDisclosure({
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

type NewCodingSessionTargetsInput = Parameters<
  typeof resolveNewCodingSessionTargets
>[0];

/** Resolve click-time selection only from the freshly probed runtime snapshot. */
export function resolveRefreshedNewCodingSessionTarget(input: {
  catalogs: NewCodingSessionTargetsInput["catalogs"];
  channelId: string | null;
  localProvider: NewCodingSessionTargetsInput["localProvider"];
  selectedTargetKey: string | null;
  selectionExplicit: boolean;
}): NewCodingSessionTarget | null {
  return resolveSelectedNewCodingSessionTarget({
    targets: resolveNewCodingSessionTargets({
      catalogs: input.catalogs,
      channelId: input.channelId,
      localProvider: input.localProvider,
    }),
    selectedTargetKey: input.selectedTargetKey,
    selectionExplicit: input.selectionExplicit,
  });
}

/**
 * Launch a coding session into a channel, and optionally into the project that
 * channel serves.
 *
 * This was a full-page route until it became a dialog, and it was two tabs
 * until 2026-09-01. Neither change is cosmetic. Starting a session is
 * something a person does *from* somewhere — a channel, a project — and taking
 * the whole window away made "which channel is this for?" harder to see, not
 * easier. Splitting it into *One session* and *Team* then made "who is
 * running this?" two questions with two half-answers, and the halves drifted:
 * the Team tab had no provider control at all and the One-session tab's model
 * leaked into seats it never showed (item 103, finding 12).
 *
 * There is one form now, and the order of its fields is the order of the
 * thinking: the goal, then who leads it, then what that lead runs on, then
 * who it may hire, then the limits, the directory, and the destination. The
 * name comes late because it is derived from the goal — asking for a title
 * first asks someone to summarize a task they have not described yet.
 */
export function NewCodingSessionDialog({
  channelId,
  onOpenChange,
  open,
  projectContext = null,
  workspaceReuse = null,
}: {
  channelId?: string;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  projectContext?: NewCodingSessionProjectContext | null;
  /**
   * Reuse an existing session's directory instead of cutting a new worktree.
   * The form seeds its directory from this and turns worktree creation off;
   * the person may still change either before submitting.
   */
  workspaceReuse?: NewCodingSessionWorkspaceReuse | null;
}) {
  const formProps = newCodingSessionFormProps({
    channelId,
    onDone: () => onOpenChange(false),
    projectContext,
    workspaceReuse,
  });
  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-w-2xl"
        data-testid="new-coding-session-dialog"
      >
        <DialogHeader>
          <DialogTitle>
            {newCodingSessionDialogTitle({ projectContext, workspaceReuse })}
          </DialogTitle>
          <DialogDescription className="sr-only">
            Describe the goal and destination. Saved setup starts the session;
            optional advanced settings are available when needed.
          </DialogDescription>
        </DialogHeader>
        {newCodingSessionWorkspaceDisclosure(workspaceReuse)}
        <NewCodingSessionForm {...formProps} />
      </DialogContent>
    </Dialog>
  );
}

// Re-exported so this file stays the one import site for the dialog's parts;
// `WORKSPACE_REUSE_SEAM_LANDED` lives in the lib (no CSS in its import graph,
// so a Playwright spec can import it) and is re-exported here for the same
// reason.
export { WORKSPACE_REUSE_SEAM_LANDED };

// the implementations moved to keep every file under the 1000-line ceiling.
export { NewCodingSessionChannelPicker, NewCodingSessionProjectDestination };
export { NewCodingSessionForm };
