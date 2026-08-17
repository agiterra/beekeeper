import { CircleAlert, FolderCog, LoaderCircle } from "lucide-react";

import { Button } from "@/shared/ui/button";
import { cn } from "@/shared/lib/cn";

import type { DurableCodingSessionCreateTransaction } from "../lib/durableCodingSessionCreate";
import {
  canRetryNewCodingSessionCreate,
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  newCodingSessionStatusMessage,
  pendingCodingSessionWorkspaceStatus,
  type NewCodingSessionHostPhase,
} from "../lib/newCodingSessionModel";
import { CodingSessionHeader } from "./CodingSessionHeader";
import { ProviderLoginNeeded } from "./NewCodingSessionScreen";

type PendingLifecycle = Parameters<
  typeof newCodingSessionStatusMessage
>[0]["lifecycle"];

/**
 * The session screen a create lands on the moment its signed request is
 * durable — before the provider has answered.
 *
 * Instead of parking the person on a disabled form ("Waiting for the session
 * provider to accept this request…"), the screen assumes the provider will
 * accept: it is shaped like the workspace the real session will replace it
 * with (header, echoed first message, one status line), so the eventual
 * navigation reads as the session filling in rather than a context switch.
 * Failures surface here in place, with the same remediation the form offered.
 */
export function PendingCodingSessionScreen({
  transaction,
  lifecycle,
  lifecycleIsLoading,
  lifecycleErrorMessage,
  isPublishing,
  hostPhase,
  publishError,
  stalled,
  failedRuntime,
  channelName,
  projectName = null,
  onBack,
  retryExact,
  startFresh,
  beginLoginWatch,
  onEditRequest,
}: {
  transaction: DurableCodingSessionCreateTransaction;
  lifecycle: PendingLifecycle;
  lifecycleIsLoading: boolean;
  lifecycleErrorMessage: string | null;
  isPublishing: boolean;
  hostPhase: NewCodingSessionHostPhase;
  /** `publishError ?? durabilityError`, exactly as the form combines them. */
  publishError: string | null;
  stalled: boolean;
  failedRuntime: { runtime: string; label?: string } | null;
  channelName: string | null;
  projectName?: string | null;
  onBack: () => void;
  retryExact: () => void;
  startFresh: () => void;
  beginLoginWatch: (runtime: string) => void;
  /** Return to the form with the transaction retained (workdir remediation). */
  onEditRequest: () => void;
}) {
  const failureCode =
    lifecycle?.state === "failed" ? lifecycle.error.code : undefined;
  const initialTurn = transaction.input.initialTurn;
  const status = newCodingSessionStatusMessage({
    hostPhase,
    isPublishing,
    publishError,
    lifecycle,
    authRuntime: failedRuntime,
    stalled,
  });
  const headerStatus = pendingCodingSessionWorkspaceStatus({
    lifecycleState: lifecycle?.state ?? null,
    publishError,
    hasInitialTurn: initialTurn !== null,
  });

  return (
    <main
      className="flex h-full min-h-0 flex-1 flex-col bg-background"
      data-testid="pending-coding-session-screen"
    >
      <CodingSessionHeader
        channelName={channelName}
        generationLabel="pending"
        onBack={onBack}
        projectName={projectName}
        sessionTitle={transaction.input.title}
        status={headerStatus}
      />

      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">
        <div className="mx-auto flex w-full max-w-3xl flex-1 flex-col gap-6 px-5 py-6 sm:px-8">
          {initialTurn ? (
            // The optimistic transcript of one: the first message, echoed as
            // the conversation the workspace will pick up.
            <div className="flex justify-end">
              <div
                className="max-w-[85%] whitespace-pre-wrap rounded-2xl rounded-br-md bg-primary/10 px-4 py-2.5 text-base"
                data-testid="pending-coding-session-first-message"
              >
                {initialTurn}
              </div>
            </div>
          ) : null}

          {isCodingSessionAuthFailure(failureCode) ? (
            <ProviderLoginNeeded
              onLoginLaunched={({ runtime }) => beginLoginWatch(runtime)}
              runtime={failedRuntime}
            />
          ) : null}

          {status ? (
            <p
              className={cn(
                "flex items-start gap-2 text-sm",
                status.tone === "destructive"
                  ? "text-destructive"
                  : "text-muted-foreground",
              )}
              data-testid="pending-coding-session-status"
              role="status"
            >
              {status.tone === "destructive" ? (
                <CircleAlert className="mt-0.5 size-4 shrink-0" />
              ) : (
                <LoaderCircle className="mt-0.5 size-4 shrink-0 animate-spin motion-reduce:animate-none" />
              )}
              {status.message}
            </p>
          ) : null}

          <div className="mt-auto flex items-center justify-end gap-2 pt-4">
            {isCodingSessionWorkdirFailure(failureCode) ? (
              <Button
                data-testid="pending-coding-session-fix-workdir"
                onClick={onEditRequest}
                type="button"
                variant="outline"
              >
                <FolderCog />
                Fix working directory
              </Button>
            ) : null}
            <Button
              data-testid="pending-coding-session-start-fresh"
              onClick={startFresh}
              type="button"
              // Once a wait has stalled, "Start fresh" is the only real
              // escape — promote it from ghost so it reads as the action.
              variant={stalled ? "outline" : "ghost"}
            >
              Start fresh
            </Button>
            <Button
              data-testid="pending-coding-session-retry"
              disabled={
                !canRetryNewCodingSessionCreate({
                  isPublishing,
                  lifecycleIsLoading,
                  lifecycleErrorMessage,
                  lifecycleState: lifecycle?.state ?? null,
                  stalled,
                })
              }
              onClick={retryExact}
              type="button"
              variant="outline"
            >
              Retry this exact request
            </Button>
          </div>
        </div>
      </div>
    </main>
  );
}
