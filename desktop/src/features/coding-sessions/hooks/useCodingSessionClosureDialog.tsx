import * as React from "react";
import { toast } from "sonner";

import { publishCodingSessionClosure } from "@/features/coding-sessions/lib/codingSessionClosure";
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

export type CodingSessionClosureRequest = {
  action: "closed" | "open";
  channelId: string;
  genesisRef: string;
  label: string;
  sessionRef: string;
};

/** Shared confirmation for provider-independent close and reopen facts. */
export function useCodingSessionClosureDialog(): {
  requestClosure: (request: CodingSessionClosureRequest | null) => void;
  dialog: React.ReactNode;
} {
  const [target, setTarget] =
    React.useState<CodingSessionClosureRequest | null>(null);
  const [publishing, setPublishing] = React.useState(false);

  const requestClosure = React.useCallback(
    (request: CodingSessionClosureRequest | null) => {
      if (request) setTarget(request);
    },
    [],
  );

  const confirm = React.useCallback(() => {
    if (!target) return;
    setPublishing(true);
    void publishCodingSessionClosure(target)
      .catch((error: unknown) => {
        toast.error(
          error instanceof Error
            ? error.message
            : target.action === "closed"
              ? "Failed to close the session."
              : "Failed to reopen the session.",
        );
      })
      .finally(() => {
        setPublishing(false);
        setTarget(null);
      });
  }, [target]);

  const isClosing = target?.action === "closed";
  const dialog = (
    <AlertDialog
      open={target !== null}
      onOpenChange={(open) => {
        if (!open) setTarget(null);
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {isClosing ? "Close this session?" : "Reopen this session?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {target
              ? isClosing
                ? `"${target.label}" will move to Settled for every project member. ` +
                  "Its identity and transcript remain available. Closing does not stop " +
                  "provider executions; stop those separately when needed."
                : `"${target.label}" will return to Sessions for every project member. ` +
                  "Reopening starts no provider and consumes no execution slot."
              : ""}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={publishing}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            data-testid="coding-session-closure-confirm"
            disabled={publishing}
            onClick={(event) => {
              event.preventDefault();
              confirm();
            }}
          >
            {isClosing ? "Close session" : "Reopen session"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return { requestClosure, dialog };
}
