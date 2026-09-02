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

import { publishCodingSessionStop } from "../lib/codingSessionLifecycleCommand";
import { deleteCodingSession } from "../lib/deleteCodingSession";
import {
  publishEndCodingSessionRequest,
  type EndCodingSessionRequest,
} from "../lib/endCodingSessionModel";

/** The session a delete is being requested for. */
export type DeletableCodingSession = {
  channelId: string;
  sessionRef: string;
  /**
   * The genesis event id. NIP-CSG makes this the umbrella's canonical
   * identity — the `csg-session` tag exists for the relay's uniqueness probe
   * and diagnostics, and consumers are told not to select by it — so the
   * selection matches the genesis on this and never on a tag.
   */
  genesisRef: string | null;
  label: string;
  /** Stop coordinates for every execution this row still has running. */
  stops: EndCodingSessionRequest["stops"];
  /** True when nothing is answering for those executions. */
  providerUnanswered?: boolean;
};

/**
 * Confirmation for deleting a coding session.
 *
 * ## Two publishes, in this order
 *
 * A session's transcript can be deleted while its agent is still running on
 * somebody's machine, holding one of that host's session slots. So a delete
 * stops first and tombstones second: one `session.stop` per live execution,
 * then the kind:5 over the session's whole chain. The stop is what returns
 * the slot; deleting the record would only make the still-running execution
 * harder to find.
 *
 * A stop published at a provider that is not answering is a queued request,
 * not an effect — the relay accepts it and it runs whenever that app comes
 * back. The copy says so rather than implying the machine is already clear,
 * which is the same distinction the Stop dialog draws.
 *
 * ## What the copy must not omit
 *
 * The `sessionRef` is never released. A deleted session's reference stays
 * claimed for good — the same rule a deleted repository's name follows — so
 * nothing can be re-founded under it. Somebody who deletes a session in
 * order to start over under the same identity will find that it does not
 * work, and they should learn that here rather than afterwards.
 */
export function useDeleteCodingSessionDialog(onDeleted?: () => void): {
  requestDelete: (session: DeletableCodingSession) => void;
  dialog: React.ReactNode;
} {
  const [target, setTarget] = React.useState<DeletableCodingSession | null>(
    null,
  );
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  const requestDelete = React.useCallback((session: DeletableCodingSession) => {
    setError(null);
    setTarget(session);
  }, []);

  const close = () => {
    setTarget(null);
    setPending(false);
    setError(null);
  };

  const confirm = async () => {
    if (!target) return;
    setPending(true);
    setError(null);
    let stopsQueued = false;
    try {
      if (target.stops.length > 0) {
        const outcome = await publishEndCodingSessionRequest(
          {
            label: target.label,
            channelId: target.channelId,
            stops: target.stops,
          },
          publishCodingSessionStop,
        );
        if (!outcome.ok) {
          // Deliberately fatal. Deleting the record of a session whose agent
          // is still running would remove the only handle on it.
          setError(
            outcome.errorMessage ??
              "Could not stop this session's executions, so nothing was deleted.",
          );
          setPending(false);
          return;
        }
        stopsQueued = target.providerUnanswered === true;
      }
      const { deleted } = await deleteCodingSession({
        channelId: target.channelId,
        sessionRef: target.sessionRef,
        genesisRef: target.genesisRef,
      });
      toast.success(
        deleted === 1
          ? "Session deleted."
          : `Session deleted (${deleted} events).`,
        {
          description: stopsQueued
            ? "No provider was listening, so the stop is queued and will run when that app returns."
            : undefined,
        },
      );
      close();
      onDeleted?.();
    } catch (caught) {
      setError(
        caught instanceof Error
          ? caught.message
          : "Failed to delete the session.",
      );
      setPending(false);
    }
  };

  const dialog = (
    <AlertDialog
      onOpenChange={(open) => {
        if (!open && !pending) close();
      }}
      open={target !== null}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {target ? `Delete "${target.label}"?` : "Delete this session?"}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {target && target.stops.length > 0
              ? "Any execution still running is stopped first, then the session and its transcript are removed for everyone. "
              : "The session and its transcript are removed for everyone. "}
            Its reference stays claimed permanently, so a new session can never
            reuse it. This cannot be undone.
          </AlertDialogDescription>
        </AlertDialogHeader>
        {error ? (
          <p
            className="text-sm text-destructive"
            data-testid="delete-coding-session-error"
          >
            {error}
          </p>
        ) : null}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={pending}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            data-testid="delete-coding-session-confirm"
            disabled={pending}
            onClick={(event) => {
              // Suppressed so a refusal leaves the message above on screen.
              event.preventDefault();
              void confirm();
            }}
          >
            {pending ? "Deleting…" : "Delete session"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return { requestDelete, dialog };
}
