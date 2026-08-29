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
  endCodingSessionDialogDescription,
  publishEndCodingSessionRequest,
  type EndCodingSessionRequest,
} from "../lib/endCodingSessionModel";

/**
 * The one confirm that stands between a click and a durable provider stop.
 *
 * This controls executions only. Session closure is a separate human-signed
 * fact and must never be inferred from a provider accepting this command.
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
        // A stop nobody can hear is a queued request, and saying nothing is
        // how three of them looked like dead buttons for two hours (§2 item
        // 42). Only the unanswered case speaks: a delivered stop shows itself
        // in the transcript.
        if (target.providerUnanswered) {
          toast.info("Stop requested — no provider is listening", {
            description:
              "The signed stop is on the relay and will run if a provider for this execution comes back.",
          });
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
          <AlertDialogTitle data-testid="coding-session-end-title">
            {target?.confirm?.title ?? "Stop this execution?"}
          </AlertDialogTitle>
          <AlertDialogDescription data-testid="coding-session-end-description">
            {target?.confirm?.description ??
              endCodingSessionDialogDescription(target)}
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
            {target?.confirm?.action ?? "Stop execution"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );

  return { requestEnd, dialog };
}
