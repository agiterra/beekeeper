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

import { useDeleteRepositoryMutation } from "../deleteRepository";
import type { Repository } from "../hooks";

/**
 * Confirmation for deleting a repository.
 *
 * The copy has one job beyond "are you sure": it has to say what deleting a
 * repository does *not* undo, because two of the three consequences are
 * counter-intuitive and both have bitten people who assumed otherwise.
 *
 * - The clone is gone for everyone — that is the part people expect.
 * - The **name stays reserved** to its owner. It cannot be taken over by
 *   somebody else afterwards, which is a deliberate anti-squat rule, but it
 *   also means "delete and let a teammate re-announce it" does not work.
 * - **Local checkouts are untouched.** This publishes a relay event; the
 *   copy on somebody's laptop keeps working and can be pushed elsewhere.
 *
 * The dialog stays open on failure so a relay refusal — `must be event
 * author` for somebody who is neither the repo's owner nor an Owner of its
 * project — is readable rather than a toast that has already gone.
 */
export function useDeleteRepositoryDialog(): {
  requestDelete: (repo: Repository) => void;
  dialog: React.ReactNode;
} {
  const [target, setTarget] = React.useState<Repository | null>(null);
  const deleteMutation = useDeleteRepositoryMutation();

  const requestDelete = React.useCallback((repo: Repository) => {
    setTarget(repo);
  }, []);

  const close = () => {
    setTarget(null);
    deleteMutation.reset();
  };

  const dialog = (
    <AlertDialog
      onOpenChange={(open) => {
        if (!open) close();
      }}
      open={target !== null}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {target ? `Delete "${target.name}"?` : "Delete this repository?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            The repository stops being listed and can no longer be cloned by
            anyone. Its name stays reserved to its owner, so nobody else can
            take it; local checkouts already on people&apos;s machines are not
            affected. This cannot be undone from here.
          </AlertDialogDescription>
        </AlertDialogHeader>
        {deleteMutation.error ? (
          <p
            className="text-sm text-destructive"
            data-testid="delete-repository-error"
          >
            {deleteMutation.error instanceof Error
              ? deleteMutation.error.message
              : "Failed to delete the repository."}
          </p>
        ) : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={deleteMutation.isPending}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            data-testid="delete-repository-confirm"
            disabled={deleteMutation.isPending}
            onClick={(event) => {
              // Suppressed so a refusal leaves the message above on screen.
              event.preventDefault();
              if (!target) return;
              deleteMutation.mutate(
                {
                  ownerPubkey: target.owner,
                  repoId: target.dtag,
                  name: target.name,
                },
                {
                  onSuccess: () => {
                    toast.success("Repository deleted.");
                    close();
                  },
                },
              );
            }}
          >
            {deleteMutation.isPending ? "Deleting…" : "Delete repository"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return { requestDelete, dialog };
}
