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
import { Button } from "@/shared/ui/button";

/**
 * The "Delete message?" confirmation. Single definition shared by every
 * surface that deletes a message — the message action menu (MessageActionBar)
 * and the empty-edit delete path (clearing an edit to empty and hitting accept
 * routes here, so it prompts exactly like the menu's Delete does). `onConfirm`
 * fires when the user presses Delete; the caller owns the actual deletion.
 *
 * `authority` must match the authority the caller will actually publish under.
 * A moderator delete is a different, visible act — the relay posts a
 * `message_deleted` tombstone into the channel — so it says so up front rather
 * than presenting itself as an ordinary delete.
 */
export function DeleteMessageConfirmDialog({
  authority = "self",
  open,
  onOpenChange,
  onConfirm,
}: {
  authority?: "self" | "moderator";
  open: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: () => void;
}) {
  const isModerator = authority === "moderator";
  return (
    <AlertDialog onOpenChange={onOpenChange} open={open}>
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {isModerator ? "Delete as moderator?" : "Delete message?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {isModerator
              ? "This deletes someone else's message using your moderator role. It cannot be undone, and everyone in the channel will see that a message was deleted."
              : "This will permanently delete this message and cannot be undone."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel asChild>
            <Button type="button" variant="outline">
              Cancel
            </Button>
          </AlertDialogCancel>
          <AlertDialogAction asChild>
            <Button onClick={onConfirm} type="button" variant="destructive">
              Delete
            </Button>
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
