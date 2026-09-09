import type { WorkspaceReuseResolution } from "./codingSessionWorkspaceReuse";

/**
 * Every user-visible word "New session in this workspace" can say.
 *
 * One module, because the same action is offered from two menus and then
 * explained again inside the draft it opens: three places, and a fourth
 * whenever somebody adds an entry point. Copy that lives at each call site
 * drifts, and the drift here would not be cosmetic — the difference between
 * "uses these files" and "continues that session" is the difference between
 * what this feature does and a thing it deliberately is not.
 *
 * Two rules hold every sentence below:
 *
 * 1. **It is a new conversation over existing files.** Nothing here may say
 *    isolated, forked, inherited, continued, resumed, or taken over. Those
 *    name operations with different semantics (copying a worktree, forking a
 *    conversation, migrating checkpoints, taking a session over), none of
 *    which this action performs. `codingSessionWorkspaceReuseCopy.test.mjs`
 *    walks every export against that vocabulary.
 * 2. **An absence is an absence on *this* computer.** A directory this
 *    machine cannot find is "no such directory on this computer", never "the
 *    directory was deleted"; an execution running on a provider this machine
 *    does not hold is exactly that, never a claim about what that other
 *    machine has or could do.
 *
 * The availability sentences themselves are the resolver's
 * (`WORKSPACE_REUSE_SENTENCES`), reused rather than restated.
 */

/**
 * The one label, shared by the sidebar row's context menu and the open
 * session's header overflow. Both entry points import this; neither writes
 * its own, which is what keeps "consistent label across entry points"
 * (brief) true by construction rather than by review.
 */
export const NEW_SESSION_IN_WORKSPACE_LABEL = "New session in this workspace";

/**
 * The draft's plain summary, verbatim from the brief. A new conversation, and
 * the files it will find are the ones already sitting in that folder.
 */
export const WORKSPACE_REUSE_CONVERSATION_SENTENCE =
  "New conversation; uses these files.";

/**
 * Said in the same breath as the sentence above, because "these files"
 * includes bytes no commit holds — that is a consequence worth naming before
 * somebody starts a second session in a folder they were mid-edit in.
 */
export const WORKSPACE_REUSE_UNCOMMITTED_SENTENCE =
  "Includes uncommitted changes already in this folder.";

/**
 * What the unavailable cases offer instead. The action never substitutes a
 * directory of its own choosing, so what is left is the ordinary launcher —
 * and saying so is the difference between a dead end and a next step.
 */
export const WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE =
  "Pick a folder in the draft to start somewhere else on this computer.";

/**
 * Shown before anything has been read. The menu resolves on open, not on
 * render (contract §2), so between paint and answer there is a moment with
 * no availability — and it says what it is going to do rather than guessing
 * the answer it does not have yet.
 */
export const WORKSPACE_REUSE_UNRESOLVED_DETAIL =
  "Opens a draft. This computer looks up the session's folder first.";

/** The last segment of a path, POSIX or Windows, or the path itself. */
export function workspaceDirectoryName(path: string): string {
  const segments = path.split(/[\\/]/).filter((segment) => segment.length > 0);
  return segments[segments.length - 1] ?? path;
}

/**
 * The menu item's second line.
 *
 * `null` is the not-yet-read state, not an error and not an absence. An
 * available workspace names the folder and the branch — the two facts that
 * decide whether this is the workspace the person meant. Everything else
 * hands over the resolver's sentence unchanged, so the menu cannot soften a
 * refusal the resolver stated plainly.
 *
 * The shared-directory line belongs here rather than in the draft: it is
 * drawn from the read that just ran, and a menu is re-resolved every time it
 * opens. The draft's request survives a reload from `sessionStorage`, so a
 * count carried there would be re-shown long after it stopped being true.
 */
export function newSessionInWorkspaceMenuDetail(
  resolution: WorkspaceReuseResolution | null,
  /**
   * `CodingSessionWorkspaceReuseRead.error`. A read that did not complete
   * outranks the resolution: no rows resolves to "unrecorded", and printing
   * "this computer recorded no directory" over a failed read would state a
   * fact the read never established.
   */
  error: string | null = null,
): string {
  if (error !== null) return error;
  if (resolution === null) return WORKSPACE_REUSE_UNRESOLVED_DETAIL;
  const alsoHere = workspaceReuseAlsoHereLine(resolution.alsoHere);
  const lead =
    resolution.availability !== "available" || resolution.path === null
      ? resolution.sentence
      : describeWorkspaceFolder(resolution.path, resolution.branch);
  return alsoHere === null ? lead : `${lead} — ${alsoHere}`;
}

/** `repo-wt-a · main`, or just the folder when no branch is known. */
function describeWorkspaceFolder(path: string, branch: string | null): string {
  const name = workspaceDirectoryName(path);
  return branch === null || branch.length === 0 ? name : `${name} · ${branch}`;
}

/**
 * The branch, and which fact it is.
 *
 * A branch recorded when the worktree was cut and the branch checked out in
 * that directory right now are different claims, and this app has been wrong
 * about that distinction before. Neither is dressed as the other, and a
 * branch with no provenance is reported as unknown rather than printed bare.
 */
export function workspaceReuseBranchLine(input: {
  branch: string | null;
  branchSource: "recorded" | "live" | null;
}): string {
  if (input.branch === null || input.branch.length === 0) {
    return "No branch recorded for this folder on this computer.";
  }
  if (input.branchSource === "live") return `${input.branch} · on disk now`;
  if (input.branchSource === "recorded") {
    return `${input.branch} · recorded at creation`;
  }
  return `${input.branch} · source not known here`;
}

/**
 * How many other sessions this computer's own worktree records place at the
 * same directory — nothing more.
 *
 * It counts rows already in hand from one host read. It is not a work
 * registry, it does not know about sessions this computer never recorded, and
 * an empty list therefore renders nothing at all rather than "no other
 * sessions", which would be a claim the read cannot support.
 */
export function workspaceReuseAlsoHereLine(
  alsoHere: readonly string[],
): string | null {
  const count = alsoHere.length;
  if (count === 0) return null;
  return count === 1
    ? "1 other session on this computer recorded a tree in this exact folder."
    : `${count} other sessions on this computer recorded a tree in this exact folder.`;
}

/**
 * The two sentences the draft shows under a reused workspace, in order.
 * Fixed: they describe the action, not the directory, so no input can move
 * them.
 */
export function workspaceReuseDraftSentences(): readonly string[] {
  return [
    WORKSPACE_REUSE_CONVERSATION_SENTENCE,
    WORKSPACE_REUSE_UNCOMMITTED_SENTENCE,
  ];
}

/**
 * What the draft says when there was no workspace to reuse: the resolver's
 * own account of why, then the way forward. The launcher opens with nothing
 * seeded, so this text is the whole explanation the person gets.
 */
export function workspaceReuseUnavailableLines(
  resolution: WorkspaceReuseResolution,
  /** A failed read speaks for itself, in place of the resolution's sentence. */
  error: string | null = null,
): readonly string[] {
  return [error ?? resolution.sentence, WORKSPACE_REUSE_CHOOSE_FOLDER_SENTENCE];
}
