import type * as React from "react";

import { PendingCodingSessionScreen } from "./PendingCodingSessionScreen";

type PendingScreenProps = React.ComponentProps<
  typeof PendingCodingSessionScreen
>;

/**
 * The launch form, once a create is a session-in-waiting.
 *
 * Split out of `NewCodingSessionLaunchForm.tsx` to keep that file under the
 * 1,000-line ceiling with headroom (REVIEW-B3 N4) — it had thirteen lines
 * left, and the next field added to the form would have tripped the gate.
 *
 * The one thing it does beyond forwarding: the pending screen sizes itself
 * with `h-full`, which resolves to nothing inside a dialog's auto-height grid,
 * so it is given a definite box to fill.
 */
export function NewCodingSessionPendingView({
  beginLoginWatch,
  channelName,
  durabilityError,
  hostPhase,
  isPublishing,
  lifecycle,
  lifecycleErrorMessage,
  lifecycleIsLoading,
  onClose,
  onEditRequest,
  projectName,
  publishError,
  retryExact,
  seatLabel,
  seatPackRef = null,
  seatPackStaged,
  signedSeat,
  stalled,
  startFresh,
  transaction,
}: {
  beginLoginWatch: PendingScreenProps["beginLoginWatch"];
  channelName: string | null;
  /** The durable-store failure, which outranks nothing and is shown beside. */
  durabilityError: PendingScreenProps["publishError"];
  hostPhase: PendingScreenProps["hostPhase"];
  isPublishing: boolean;
  lifecycle: PendingScreenProps["lifecycle"];
  lifecycleErrorMessage: PendingScreenProps["lifecycleErrorMessage"];
  lifecycleIsLoading: boolean;
  onClose: () => void;
  onEditRequest: () => void;
  projectName: string | null;
  publishError: PendingScreenProps["publishError"];
  retryExact: PendingScreenProps["retryExact"];
  /** Display name of the seated identity, or null for an unseated create. */
  seatLabel: string | null;
  /** Three-valued: `null` is "nobody asked", which is not `false`. */
  seatPackStaged: boolean | null;
  /** The repository commit that pack came from, when one vouched for it. */
  seatPackRef?: NonNullable<PendingScreenProps["seat"]>["packRef"];
  /** The seat the *signed* create carries — never the form's current state. */
  signedSeat: { actor: string; role: string } | null;
  stalled: boolean;
  startFresh: () => void;
  transaction: PendingScreenProps["transaction"];
}) {
  return (
    <div
      className="flex h-[60vh] min-h-0 flex-col"
      data-testid="new-coding-session-pending"
    >
      <PendingCodingSessionScreen
        beginLoginWatch={beginLoginWatch}
        channelName={channelName}
        closeLabel="Close the new session dialog"
        failedRuntime={null}
        hostPhase={hostPhase}
        isPublishing={isPublishing}
        lifecycle={lifecycle}
        lifecycleErrorMessage={lifecycleErrorMessage}
        lifecycleIsLoading={lifecycleIsLoading}
        onClose={onClose}
        onEditRequest={onEditRequest}
        projectName={projectName}
        publishError={publishError ?? durabilityError}
        retryExact={retryExact}
        // Named from the *signed* create, not the form: a transaction
        // rehydrated after a restart has no form state left to read.
        seat={
          signedSeat
            ? {
                actorLabel: seatLabel,
                role: signedSeat.role,
                packStaged: seatPackStaged,
                packRef: seatPackRef,
              }
            : null
        }
        stalled={stalled}
        startFresh={startFresh}
        transaction={transaction}
      />
    </div>
  );
}
