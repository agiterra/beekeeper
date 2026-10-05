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

/**
 * The confirmation a close asks for while a command is running in the
 * terminal (SV-25). T3 confirms every close; Beekeeper skips it only for a
 * shell read as at its prompt, where a close loses nothing but the
 * scrollback. A shell whose state could not be read is asked about, and the
 * dialog says it could not tell rather than claiming a command runs.
 */
export function CodingSessionTerminalCloseDialog({
  label,
  onCancel,
  onConfirm,
  running,
}: {
  /** The terminal's label, or null when no close is pending. */
  label: string | null;
  /** A command was read as running (else: this computer could not tell). */
  running: boolean;
  onCancel: () => void;
  onConfirm: () => void;
}) {
  return (
    <AlertDialog
      onOpenChange={(open) => {
        if (!open) onCancel();
      }}
      open={label !== null}
    >
      <AlertDialogContent data-testid="coding-session-terminal-close-dialog">
        <AlertDialogHeader>
          <AlertDialogTitle>{`Close ${label ?? "this terminal"}?`}</AlertDialogTitle>
          <AlertDialogDescription>
            {running
              ? "A command is still running in it. Closing the terminal stops that command and clears the terminal\u2019s history."
              : "This computer could not tell whether a command is running in it. Closing the terminal stops anything running and clears the terminal\u2019s history."}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel data-testid="coding-session-terminal-close-cancel">
            Keep it open
          </AlertDialogCancel>
          <AlertDialogAction
            data-testid="coding-session-terminal-close-confirm"
            onClick={onConfirm}
          >
            Close terminal
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
