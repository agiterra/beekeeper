import * as React from "react";
import { toast } from "sonner";

import {
  type CodingSessionClosureAction,
  publishCodingSessionClosure,
} from "@/features/coding-sessions/lib/codingSessionClosure";
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
  action: CodingSessionClosureAction;
  channelId: string;
  genesisRef: string;
  label: string;
  sessionRef: string;
};

const COPY: Record<
  CodingSessionClosureAction,
  {
    title: string;
    confirm: string;
    failed: string;
    describe: (label: string) => string;
  }
> = {
  closed: {
    title: "Close this session?",
    confirm: "Close session",
    failed: "Failed to close the session.",
    describe: (label) =>
      `"${label}" will move to Closed for every project member. ` +
      "Its identity and transcript remain available. Any execution still " +
      "running on its host is stopped and its slot freed.",
  },
  archived: {
    title: "Archive this session?",
    confirm: "Archive session",
    failed: "Failed to archive the session.",
    describe: (label) =>
      `"${label}" will be closed and archived for every project member: it ` +
      "leaves the sidebar unless a filter asks for archived sessions, but its " +
      "identity and transcript remain available and it can be reopened. " +
      "Any execution still running on its host is stopped and its slot " +
      "freed.",
  },
  open: {
    title: "Reopen this session?",
    confirm: "Reopen session",
    failed: "Failed to reopen the session.",
    describe: (label) =>
      `"${label}" will return to open sessions for every project member. ` +
      "Reopening starts no provider and consumes no execution slot.",
  },
};

/**
 * Shared confirmation for the close, archive and reopen facts.
 *
 * The copy used to end "Closing does not stop provider executions; stop
 * those separately when needed." That was true and is not any more: the
 * host now subscribes to closure revisions and stops the executions a
 * settled umbrella still holds, because before it did, a closed session went
 * on occupying one of the host's (default four) session slots until a
 * four-hour idle timeout — and the only symptom was a later create refused
 * with a limit the operator could not reconcile with what they saw.
 *
 * The reopen copy still says a reopen starts no provider and consumes no
 * slot, which remains exactly true.
 */
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
          error instanceof Error ? error.message : COPY[target.action].failed,
        );
      })
      .finally(() => {
        setPublishing(false);
        setTarget(null);
      });
  }, [target]);

  const copy = target ? COPY[target.action] : null;
  const dialog = (
    <AlertDialog
      open={target !== null}
      onOpenChange={(open) => {
        if (!open) setTarget(null);
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{copy?.title ?? ""}</AlertDialogTitle>
          <AlertDialogDescription>
            {target && copy ? copy.describe(target.label) : ""}
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
            {copy?.confirm ?? ""}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return { requestClosure, dialog };
}
