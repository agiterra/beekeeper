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

import { useDeleteShellAnnounceMutation } from "../observe/deleteShellAnnounce";

/** The terminal a delete is being requested for. */
export type DeletableTerminal = {
  sessionId: string;
  ownerPubkey: string;
  title: string;
  /** True when the viewer owns the PTY behind this announce. */
  isOwn: boolean;
};

/**
 * Confirmation for deleting a shared terminal's kind:30623 announce.
 *
 * Follows the `{ request…, dialog }` idiom the channel and shell dialogs
 * already use: the caller wires `requestDelete` into a row and renders
 * `dialog` once. The dialog stays open on failure and shows the error
 * inline, because the interesting failure here is a relay refusal
 * (`must be event author`) that the person needs to read.
 *
 * The copy is split by whose terminal it is, because the two cases have
 * genuinely different consequences and one sentence cannot honestly cover
 * both:
 *
 * - **Your own.** The process keeps running and stays open in your own app;
 *   only the sharing stops. Saying "the terminal will be deleted" would
 *   promise something a Nostr event cannot do to a local PTY.
 * - **Someone else's.** Deleting it as a project Owner unlists it and
 *   revokes everyone's access, and the owner's session keeps running on
 *   their machine — they are the only one who can end it.
 */
export function useDeleteTerminalDialog(projectAddress: string | null): {
  requestDelete: (terminal: DeletableTerminal) => void;
  dialog: React.ReactNode;
} {
  const [target, setTarget] = React.useState<DeletableTerminal | null>(null);
  const deleteMutation = useDeleteShellAnnounceMutation(projectAddress);

  const requestDelete = React.useCallback((terminal: DeletableTerminal) => {
    setTarget(terminal);
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
            {target ? `Delete "${target.title}"?` : "Delete this terminal?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {target?.isOwn
              ? "This removes the shared listing for everyone and revokes every invite to it. Your terminal itself keeps running here — deleting the share does not close the session."
              : "This removes the shared listing for everyone and revokes every invite to it. The owner's session keeps running on their machine; only they can close it."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {deleteMutation.error ? (
          <p
            className="text-sm text-destructive"
            data-testid="delete-terminal-error"
          >
            {deleteMutation.error instanceof Error
              ? deleteMutation.error.message
              : "Failed to delete the terminal."}
          </p>
        ) : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={deleteMutation.isPending}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            data-testid="delete-terminal-confirm"
            disabled={deleteMutation.isPending}
            onClick={(event) => {
              // The dialog must survive a refusal so the message above is
              // readable, so the default close-on-click is suppressed and
              // only success closes it.
              event.preventDefault();
              if (!target) return;
              deleteMutation.mutate(
                {
                  ownerPubkey: target.ownerPubkey,
                  sessionId: target.sessionId,
                },
                {
                  onSuccess: () => {
                    toast.success("Terminal deleted.");
                    close();
                  },
                },
              );
            }}
          >
            {deleteMutation.isPending ? "Deleting…" : "Delete terminal"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return { requestDelete, dialog };
}
