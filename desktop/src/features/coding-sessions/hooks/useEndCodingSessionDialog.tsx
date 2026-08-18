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

import { buildCodingSessionTargetKey } from "../lib/codingSessionCommand";
import { recordPendingCodingSessionLifecycle } from "../lib/codingSessionPendingLifecycle";
import { publishCodingSessionStop } from "../lib/codingSessionLifecycleCommand";
import {
  publishEndCodingSessionRequest,
  type EndCodingSessionRequest,
} from "../lib/endCodingSessionModel";

/**
 * The one confirm that stands between a click and a durable stop.
 *
 * Ending is destructive by nature — the provider retires the execution for
 * everyone and the row files under Recent Sessions — so both entry
 * points (sidebar
 * context menu, workspace composer) route through this dialog rather than
 * publishing on the raw click.
 */
export function useEndCodingSessionDialog(): {
  requestEnd: (request: EndCodingSessionRequest | null) => void;
  dialog: React.ReactNode;
} {
  const [target, setTarget] = React.useState<EndCodingSessionRequest | null>(
    null,
  );
  const [ending, setEnding] = React.useState(false);

  const requestEnd = React.useCallback(
    (request: EndCodingSessionRequest | null) => {
      if (request) setTarget(request);
    },
    [],
  );

  const confirmEnd = React.useCallback(() => {
    if (!target) return;
    setEnding(true);
    void publishEndCodingSessionRequest(target, publishCodingSessionStop)
      .then((result) => {
        if (!result.ok) {
          toast.error(result.errorMessage ?? "Failed to end the session.");
          return;
        }
        // The signed stops are on the relay; file the row under Recent
        // Sessions now instead of waiting for the provider's `stopped`
        // metadata.
        for (const stop of target.stops) {
          recordPendingCodingSessionLifecycle({
            kind: "stop",
            channelId: target.channelId,
            targetKey: buildCodingSessionTargetKey(stop.target),
            providerAuthorityPubkey: stop.providerAuthorityPubkey,
            recordedAt: Date.now(),
          });
        }
      })
      .finally(() => {
        setEnding(false);
        setTarget(null);
      });
  }, [target]);

  const dialog = (
    <AlertDialog
      open={target !== null}
      onOpenChange={(open) => {
        if (!open) setTarget(null);
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>End this session?</AlertDialogTitle>
          <AlertDialogDescription>
            {target
              ? `"${target.label}" will be durably stopped for everyone and ` +
                "moves to Recent Sessions. Its transcript stays readable, " +
                "but this session can't be restarted from here."
              : ""}
          </AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel disabled={ending}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            disabled={ending}
            onClick={(event) => {
              // Keep the dialog mounted through the async publish; dismiss
              // ourselves once it settles.
              event.preventDefault();
              confirmEnd();
            }}
            data-testid="coding-session-end-confirm"
          >
            End session
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return { requestEnd, dialog };
}
