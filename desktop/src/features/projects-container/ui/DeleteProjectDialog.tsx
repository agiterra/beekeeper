import * as React from "react";
import { toast } from "sonner";

import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/shared/ui/alert-dialog";
import { Checkbox } from "@/shared/ui/checkbox";

import type { ProjectContainer } from "../hooks";
import {
  useDeleteProjectContainerCascadeMutation,
  useDeleteProjectContainerMutation,
} from "../projectOrganizeMutations";
import { useProjectCascadeTargets } from "../useProjectCascadeTargets";

/**
 * Confirmation for deleting a project container (NIP-09). Open while
 * `project` is non-null. Shared by the all-projects manage panel and the
 * per-project screen; the latter passes `onDeleted` to navigate off the
 * now-dead route.
 *
 * Two outcomes, and the copy has to be exact about which one is armed:
 *
 * - **Default (box clear).** Only the kind:30621 event is deleted. Repos,
 *   channels, and forums survive and fall back to General. This is NIP-MP's
 *   stated contract and stays the default.
 * - **Box ticked.** The project's channels (session transports included),
 *   their workflows, and the shared terminals announced into the project are
 *   deleted first, then the project. Repositories are still never deleted
 *   here — the relay keeps their name reservation, and deleting somebody's
 *   repository is a larger act than deleting the grouping, so it stays a
 *   per-repository decision.
 *
 * The one thing the copy must not hide is that a session transport channel
 * becomes unreachable *either way*: transports admit project members through
 * the project ACL alone, so deleting the project strands them whether or not
 * the box is ticked. Ticking the box deletes them outright instead.
 *
 * Two more rules keep the receipt honest:
 *
 * - **Everything skipped is named.** Workflows a teammate authored cannot be
 *   deleted by this identity, so they are listed as survivors rather than
 *   folded into the count. Same for workflows this build cannot enumerate.
 * - **The action is gated on `isLoading`, not just `isPending`.** The
 *   inventory is re-derived whenever the channel list changes; acting during
 *   that window would delete against a collapsed set while the copy still
 *   showed the old one.
 */
export function DeleteProjectDialog({
  project,
  onOpenChange,
  onDeleted,
}: {
  project: ProjectContainer | null;
  onOpenChange: (open: boolean) => void;
  onDeleted?: () => void;
}) {
  // Ticked by default: the button this dialog answers for is "Delete
  // project", and Andy's call (2026-09-22) is that it should mean what it
  // says. Unticking still gives NIP-MP's contract — the kind:30621 alone —
  // and `bee projects delete` keeps the safe default on its side.
  const [cascade, setCascade] = React.useState(true);
  // Unticked, always. Andy's call (2026-09-22): deleting code is a bigger
  // act than deleting the grouping, so it is armed on its own and never by
  // the checkbox above.
  const [withRepos, setWithRepos] = React.useState(false);
  const deleteMutation = useDeleteProjectContainerMutation();
  const cascadeMutation = useDeleteProjectContainerCascadeMutation();
  const {
    targets,
    counts,
    summary,
    repoSummary,
    exclusions,
    isLoading,
    localPlan,
  } = useProjectCascadeTargets(project);

  // Every open starts from the same default; a previous *untick* must never
  // carry into the next project any more than a previous tick could.
  React.useEffect(() => {
    if (project === null) {
      setCascade(true);
      setWithRepos(false);
    }
  }, [project]);

  const isPending = deleteMutation.isPending || cascadeMutation.isPending;
  const hasChildren = counts.total > 0;
  // What the confirm will *actually* do. With the box ticked by default,
  // a project with no children would otherwise arm the cascade path and
  // report "Project and its channels deleted" having deleted no channel —
  // the same class of lie this dialog already refuses to tell about foreign
  // workflows. Ticked-but-nothing-to-cascade is a plain delete, and says so.
  const cascading = cascade && hasChildren;
  const hasRepos = counts.repos > 0;
  // Same rule as `cascading`: a ticked box over nothing must not make the
  // receipt claim repositories were deleted.
  const deletingRepos = withRepos && hasRepos;
  // The local identities go with the project whenever anything else does.
  // They are not their own checkbox: an agent minted for a project that no
  // longer exists is not a thing anybody keeps on purpose, and leaving its
  // signing key in the keyring is worse than removing it.
  const localAgents = localPlan?.agents.length ?? 0;

  const close = () => {
    setCascade(true);
    setWithRepos(false);
    onOpenChange(false);
  };

  return (
    <AlertDialog onOpenChange={onOpenChange} open={project !== null}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this project?</AlertDialogTitle>
          <AlertDialogDescription>
            {project
              ? [
                  cascading
                    ? `"${project.name}" will be removed for everyone, along with ${summary}. Messages in those channels go with them.`
                    : `"${project.name}" will be removed for everyone. Its channels and forums are not deleted — they move to General.`,
                  // Said here rather than only under the tick, because this
                  // sentence is the one a person reads before deciding, and
                  // "repositories are not deleted" was true of every delete
                  // until the repository checkbox existed.
                  deletingRepos
                    ? `Its repositories (${repoSummary}) will be deleted too.`
                    : "Its repositories are not deleted — they move to General.",
                ].join(" ")
              : ""}
          </AlertDialogDescription>
        </AlertDialogHeader>

        <div className="space-y-2">
          <label
            className="flex cursor-pointer items-start gap-2.5 text-sm has-[button:disabled]:cursor-not-allowed has-[button:disabled]:opacity-60"
            htmlFor="delete-project-cascade"
          >
            <Checkbox
              checked={cascade}
              className="mt-0.5"
              data-testid="delete-project-cascade"
              disabled={isPending || isLoading || !hasChildren}
              id="delete-project-cascade"
              onCheckedChange={(checked) => setCascade(checked === true)}
            />
            <span>
              Also delete this project&apos;s channels, workflows and shared
              terminals
              {isLoading
                ? " (counting…)"
                : hasChildren
                  ? ` (${summary})`
                  : " (nothing to delete)"}
            </span>
          </label>

          <label
            className="flex cursor-pointer items-start gap-2.5 text-sm has-[button:disabled]:cursor-not-allowed has-[button:disabled]:opacity-60"
            htmlFor="delete-project-repos"
          >
            <Checkbox
              checked={withRepos}
              className="mt-0.5"
              data-testid="delete-project-repos"
              disabled={isPending || isLoading || !hasRepos}
              id="delete-project-repos"
              onCheckedChange={(checked) => setWithRepos(checked === true)}
            />
            <span>
              Also delete this project&apos;s repositories
              {isLoading
                ? " (counting…)"
                : hasRepos
                  ? ` (${repoSummary})`
                  : " (none you can delete)"}
            </span>
          </label>

          {deletingRepos ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="delete-project-repos-note"
            >
              The repository stops being cloneable and its branches stop being
              readable. Its <em>name</em> stays reserved to you — deleting a
              repository never frees its name for somebody else to take — and
              the packed objects are left alone, because they are shared with
              any fork and reclaiming them could destroy a neighbour&apos;s
              history. No clone on anybody&apos;s disk is touched.
            </p>
          ) : null}

          {localAgents > 0 ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="delete-project-local-agents"
            >
              {localAgents === 1
                ? "1 agent this project created on this computer"
                : `${localAgents} agents this project created on this computer`}{" "}
              will be deleted, along with their signing keys, this
              project&apos;s team and its role definitions. That cannot be
              undone — the keys are destroyed, not archived. Only this computer
              is cleaned: anyone else who has this project keeps their own
              copies.
            </p>
          ) : null}

          {localPlan !== null && localPlan.remoteDeployed.length > 0 ? (
            <p
              className="text-xs text-destructive"
              data-testid="delete-project-remote-deployed"
            >
              {localPlan.remoteDeployed.map((agent) => agent.name).join(", ")}{" "}
              {localPlan.remoteDeployed.length === 1 ? "is" : "are"} deployed to
              a remote provider. Deleting the local record would orphan the
              deployment, so this will refuse — delete{" "}
              {localPlan.remoteDeployed.length === 1 ? "it" : "them"} from the
              Agents list first.
            </p>
          ) : null}

          {localPlan !== null && localPlan.liveSessions.length > 0 ? (
            <p
              className="text-xs text-muted-foreground"
              data-testid="delete-project-live-sessions"
            >
              {localPlan.liveSessions.length === 1
                ? "1 coding session is still open"
                : `${localPlan.liveSessions.length} coding sessions are still open`}{" "}
              for this project ({localPlan.liveSessions.join(", ")}). Deleting
              the project does not close{" "}
              {localPlan.liveSessions.length === 1 ? "it" : "them"} — close{" "}
              {localPlan.liveSessions.length === 1 ? "it" : "them"} from{" "}
              {localPlan.liveSessions.length === 1 ? "its" : "their"} own row
              first if you want that.
            </p>
          ) : null}

          {exclusions.map((note) => (
            <p
              className="text-xs text-muted-foreground"
              data-testid="delete-project-cascade-exclusion"
              key={note}
            >
              {note}
            </p>
          ))}

          {counts.total > 0 ? (
            <p className="text-xs text-muted-foreground">
              Coding sessions in these channels are not deleted individually —
              they become unreachable with their channel. To remove one
              outright, delete it from its row first.
            </p>
          ) : null}

          {counts.transports > 0 ? (
            <p className="text-xs text-muted-foreground">
              {counts.transports === 1
                ? "1 session transport channel is"
                : `${counts.transports} session transport channels are`}{" "}
              reachable only through this project&apos;s member list, so
              deleting the project makes{" "}
              {counts.transports === 1 ? "it" : "them"} unreachable either way.
              Ticking the box deletes {counts.transports === 1 ? "it" : "them"}{" "}
              outright instead of leaving{" "}
              {counts.transports === 1 ? "it" : "them"} stranded.
            </p>
          ) : null}
        </div>

        <AlertDialogFooter>
          <AlertDialogCancel disabled={isPending}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            data-testid="manage-project-delete-confirm"
            // `isLoading` too, not just `isPending`: the cascade inventory is
            // re-derived whenever the channel list changes, and during that
            // window `targets` collapses toward empty. Firing then would
            // delete against a stale set while the dialog still displayed the
            // full one, and report success.
            disabled={isPending || isLoading}
            onClick={(event) => {
              event.preventDefault();
              if (!project) return;
              const onSuccess = () => {
                // The receipt says what actually went and names every
                // survivor. A plain "deleted" over a partial result is the
                // same lie this dialog spent its copy avoiding.
                const went = ["Project"];
                if (cascading) went.push("its channels");
                if (deletingRepos) went.push("its repositories");
                if (localAgents > 0) went.push("its agents");
                const left: string[] = [];
                if (cascading && counts.foreignWorkflows > 0) {
                  left.push(
                    counts.foreignWorkflows === 1
                      ? "1 workflow created by someone else"
                      : `${counts.foreignWorkflows} workflows created by someone else`,
                  );
                }
                if (deletingRepos && counts.foreignRepos > 0) {
                  left.push(
                    counts.foreignRepos === 1
                      ? "1 repository you cannot delete"
                      : `${counts.foreignRepos} repositories you cannot delete`,
                  );
                }
                toast.success(
                  `${went.length === 1 ? "Project" : went.join(" and ")} deleted.`,
                  left.length > 0
                    ? {
                        description: `Left in place: ${left.join(", ")}.`,
                      }
                    : undefined,
                );
                close();
                onDeleted?.();
              };
              const onError = (error: unknown) => {
                toast.error(
                  error instanceof Error
                    ? error.message
                    : "Failed to delete the project.",
                );
                close();
              };
              if (cascading || deletingRepos || localAgents > 0) {
                cascadeMutation.mutate(
                  {
                    project,
                    targets,
                    deleteRepos: deletingRepos,
                    localPlan,
                  },
                  { onSuccess, onError },
                );
                return;
              }
              deleteMutation.mutate(project, { onSuccess, onError });
            }}
          >
            {isLoading
              ? "Counting…"
              : cascading
                ? "Delete everything"
                : "Delete project"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
