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
 * - **Box ticked.** The project's channels (session transports included) and
 *   their workflows are deleted first, then the project. Repositories are
 *   still never deleted — the relay keeps their name reservation.
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
  const [cascade, setCascade] = React.useState(false);
  const deleteMutation = useDeleteProjectContainerMutation();
  const cascadeMutation = useDeleteProjectContainerCascadeMutation();
  const { targets, counts, summary, exclusions, isLoading } =
    useProjectCascadeTargets(project);

  // Every open starts from the safe default; a previous tick must never carry
  // into the next project.
  React.useEffect(() => {
    if (project === null) setCascade(false);
  }, [project]);

  const isPending = deleteMutation.isPending || cascadeMutation.isPending;
  const hasChildren = counts.total > 0;

  const close = () => {
    setCascade(false);
    onOpenChange(false);
  };

  return (
    <AlertDialog onOpenChange={onOpenChange} open={project !== null}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this project?</AlertDialogTitle>
          <AlertDialogDescription>
            {project
              ? cascade
                ? `"${project.name}" will be removed for everyone, along with ${
                    summary || "its channels and workflows"
                  }. Messages in those channels go with them. Its repositories are not deleted — they move to General.`
                : `"${project.name}" will be removed for everyone. Its repositories, channels, and forums are not deleted — they move to General.`
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
              Also delete this project&apos;s channels and workflows
              {isLoading
                ? " (counting…)"
                : hasChildren
                  ? ` (${summary})`
                  : " (nothing to delete)"}
            </span>
          </label>

          {exclusions.map((note) => (
            <p
              className="text-xs text-muted-foreground"
              data-testid="delete-project-cascade-exclusion"
              key={note}
            >
              {note}
            </p>
          ))}

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
                if (!cascade) {
                  toast.success("Project deleted.");
                } else if (counts.foreignWorkflows > 0) {
                  // The receipt names the survivors; a plain "deleted" here
                  // would be the same lie the dialog just avoided telling.
                  toast.success("Project and its channels deleted.", {
                    description:
                      counts.foreignWorkflows === 1
                        ? "1 workflow created by someone else was left in place."
                        : `${counts.foreignWorkflows} workflows created by someone else were left in place.`,
                  });
                } else {
                  toast.success("Project and its channels deleted.");
                }
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
              if (cascade) {
                cascadeMutation.mutate(
                  { project, targets },
                  { onSuccess, onError },
                );
                return;
              }
              deleteMutation.mutate(project, { onSuccess, onError });
            }}
          >
            {cascade ? "Delete everything" : "Delete project"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
