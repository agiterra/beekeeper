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

import type { ProjectContainer } from "../hooks";
import { useDeleteProjectContainerMutation } from "../projectOrganizeMutations";

/**
 * Confirmation for deleting a project container (NIP-09). Open while
 * `project` is non-null. Shared by the all-projects manage panel and the
 * per-project screen; the latter passes `onDeleted` to navigate off the
 * now-dead route.
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
  const deleteMutation = useDeleteProjectContainerMutation();
  return (
    <AlertDialog onOpenChange={onOpenChange} open={project !== null}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this project?</AlertDialogTitle>
          <AlertDialogDescription>
            {project
              ? `"${project.name}" will be removed for everyone. Its repositories, channels, and forums are not deleted — they move to General.`
              : ""}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={deleteMutation.isPending}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            data-testid="manage-project-delete-confirm"
            disabled={deleteMutation.isPending}
            onClick={(event) => {
              event.preventDefault();
              if (!project) return;
              deleteMutation.mutate(project, {
                onSuccess: () => {
                  toast.success("Project deleted.");
                  onOpenChange(false);
                  onDeleted?.();
                },
                onError: (error) => {
                  toast.error(
                    error instanceof Error
                      ? error.message
                      : "Failed to delete the project.",
                  );
                  onOpenChange(false);
                },
              });
            }}
          >
            Delete project
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
