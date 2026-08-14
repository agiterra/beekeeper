import * as React from "react";
import { CircleAlert, LoaderCircle, UserPlus } from "lucide-react";

import { MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  isNewCodingSessionTargetReady,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  type NewCodingSessionTarget,
} from "@/features/coding-sessions/lib/newCodingSessionModel";
import { useCodingSessionProviderCatalog } from "@/features/coding-sessions/useCodingSessionProviderCatalog";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import {
  addCodingSessionProviderOptionNote,
  buildAddCodingSessionProviderSubmit,
  defaultAddCodingSessionProviderKey,
  listAddCodingSessionProviderOptions,
} from "./addCodingSessionProviderModel";
import {
  NewCodingSessionProviderPicker,
  ProviderLoginNeeded,
} from "./NewCodingSessionScreen";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { useNewCodingSessionCreate } from "./useNewCodingSessionCreate";

/**
 * Attach a second (third, …) provider execution to a session already running.
 *
 * This is the join path of design §B, and it is deliberately the *same* create
 * machinery as founding: one 44221, signed, staged durably, published after the
 * provider joins the channel, settled by its 44224 receipt. The only two
 * differences are that the create carries the umbrella's existing `sessionRef`
 * — which is what makes the provider mint a new execution *inside* this
 * session rather than a new session — and that the channel is pinned, because
 * every execution of an umbrella lives in one host channel.
 *
 * The workspace re-renders as an umbrella on its own once the joining
 * execution's records land in the catalog, so success here is simply: close.
 */
export function AddCodingSessionProviderDialog({
  channelId,
  channelName,
  onOpenChange,
  open,
  umbrella,
}: {
  channelId: string;
  channelName: string | null;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  // Stable: the create hook's settle effect depends on this callback, and a
  // fresh closure per render would re-run it on every catalog tick.
  const handleDone = React.useCallback(
    () => onOpenChange(false),
    [onOpenChange],
  );
  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-w-lg"
        data-testid="add-coding-session-provider-dialog"
      >
        <DialogHeader>
          <DialogTitle>Add a provider to this session</DialogTitle>
          <DialogDescription>
            The new execution joins this same session and this same channel. It
            keeps its own signed transcript — nothing is merged.
          </DialogDescription>
        </DialogHeader>
        {open ? (
          <AddCodingSessionProviderForm
            channelId={channelId}
            channelName={channelName}
            onDone={handleDone}
            umbrella={umbrella}
          />
        ) : null}
      </DialogContent>
    </Dialog>
  );
}

/**
 * Mounted only while the dialog is open: the create hook provisions this
 * computer's provider on mount, and a person who never adds a provider should
 * never pay for that.
 */
function AddCodingSessionProviderForm({
  channelId,
  channelName,
  onDone,
  umbrella,
}: {
  channelId: string;
  channelName: string | null;
  onDone: () => void;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const sessionRef = umbrella.sessionRef;
  const providerCatalog = useCodingSessionProviderCatalog([channelId]);
  const {
    beginLoginWatch,
    durabilityError,
    hostPhase,
    isPublishing,
    lifecycle,
    providerStatus,
    providerRuntimes,
    providerModelsByInstanceRef,
    publishError,
    lifecycleErrorMessage,
    lifecycleIsLoading,
    retryExact,
    startFresh,
    submit,
    transaction,
  } = useNewCodingSessionCreate({
    scopeId: `add-provider:${umbrella.umbrellaKey}`,
    onCreated: onDone,
  });

  const targets = React.useMemo<NewCodingSessionTarget[]>(
    () =>
      resolveNewCodingSessionTargets({
        catalogs: providerCatalog.entries,
        channelId,
        localProvider: providerStatus?.providerPubkey
          ? {
              providerPubkey: providerStatus.providerPubkey,
              runtimes: providerRuntimes,
              modelsByInstanceRef: providerModelsByInstanceRef,
            }
          : null,
      }),
    [
      channelId,
      providerCatalog.entries,
      providerModelsByInstanceRef,
      providerRuntimes,
      providerStatus?.providerPubkey,
    ],
  );
  const options = React.useMemo(
    () => listAddCodingSessionProviderOptions({ targets, umbrella }),
    [targets, umbrella],
  );
  const notesByKey = React.useMemo(() => {
    const notes = new Map<string, string>();
    for (const option of options) {
      const note = addCodingSessionProviderOptionNote(option);
      if (note) notes.set(option.target.selectionKey, note);
    }
    return notes;
  }, [options]);

  const [selectedKey, setSelectedKey] = React.useState<string | null>(null);
  const effectiveKey =
    selectedKey ?? defaultAddCodingSessionProviderKey(options);
  const selectedTarget =
    options.find((option) => option.target.selectionKey === effectiveKey)
      ?.target ?? null;
  const [modelSelection, setModelSelection] = React.useState<{
    value: string | null;
    explicit: boolean;
  }>({ value: null, explicit: false });
  const effectiveModel = resolveSelectedNewCodingSessionModel({
    provider: selectedTarget?.provider ?? null,
    selectedModel: modelSelection.value,
    selectionExplicit: modelSelection.explicit,
  });
  const [workdir, setWorkdir] = React.useState("");
  const [initialTurn, setInitialTurn] = React.useState("");

  const failureCode =
    lifecycle?.state === "failed" ? lifecycle.error.code : undefined;
  const failedRuntime = selectedTarget
    ? {
        runtime: selectedTarget.provider.runtime,
        label: selectedTarget.availability?.label,
      }
    : null;
  const status = newCodingSessionStatusMessage({
    hostPhase,
    isPublishing,
    publishError: publishError ?? durabilityError,
    lifecycle,
    authRuntime: failedRuntime,
  });
  const turnOverCap =
    new TextEncoder().encode(initialTurn).byteLength >
    MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES;
  const canSubmit =
    sessionRef !== null &&
    !isPublishing &&
    transaction === null &&
    selectedTarget !== null &&
    isNewCodingSessionTargetReady(selectedTarget) &&
    !turnOverCap;

  const handleSubmit = React.useCallback(() => {
    if (!canSubmit) return;
    const payload = buildAddCodingSessionProviderSubmit({
      umbrella,
      channelId,
      target: selectedTarget,
      model: effectiveModel,
      initialTurn,
      workdir,
    });
    if (payload) void submit(payload);
  }, [
    canSubmit,
    channelId,
    effectiveModel,
    initialTurn,
    selectedTarget,
    submit,
    umbrella,
    workdir,
  ]);

  if (sessionRef === null) {
    return (
      <p
        className="text-sm text-muted-foreground"
        data-testid="add-coding-session-provider-unavailable"
      >
        This session was created before sessions could hold more than one
        provider, so it has no session reference to join. Start a new session to
        run two providers together.
      </p>
    );
  }

  return (
    <div className="flex flex-col gap-4">
      <p
        className="text-xs text-muted-foreground"
        data-testid="add-coding-session-provider-channel"
      >
        Channel:{" "}
        <span className="font-medium text-foreground">
          {channelName ? `#${channelName}` : channelId}
        </span>{" "}
        — pinned. Every execution of one session lives in the same channel.
      </p>

      <NewCodingSessionProviderPicker
        disabled={transaction !== null}
        model={effectiveModel}
        noteForTarget={(target) => notesByKey.get(target.selectionKey) ?? null}
        onLoginLaunched={({ runtime }) => beginLoginWatch(runtime)}
        onModelChange={(value) => setModelSelection({ value, explicit: true })}
        onTargetChange={(key) => {
          setSelectedKey(key);
          setModelSelection({ value: null, explicit: false });
        }}
        selectedTarget={selectedTarget}
        targets={targets}
      />

      <NewCodingSessionWorkdirField
        channelId={channelId}
        disabled={
          transaction !== null && !isCodingSessionWorkdirFailure(failureCode)
        }
        onChange={setWorkdir}
        value={workdir}
      />

      <div className="flex flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="add-coding-session-provider-turn"
        >
          First message <span className="font-normal">(optional)</span>
        </label>
        <Textarea
          className="min-h-24"
          data-testid="add-coding-session-provider-turn"
          disabled={transaction !== null}
          id="add-coding-session-provider-turn"
          onChange={(event) => setInitialTurn(event.target.value)}
          placeholder="What should this provider pick up?"
          value={initialTurn}
        />
        {turnOverCap ? (
          <p className="text-xs text-destructive">
            This first message is too long for a signed session request.
          </p>
        ) : null}
      </div>

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
          data-testid="add-coding-session-provider-status"
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

      <DialogFooter>
        {transaction ? (
          <>
            <Button
              data-testid="add-coding-session-provider-start-fresh"
              onClick={startFresh}
              type="button"
              variant="ghost"
            >
              Start fresh
            </Button>
            {/* The signed command already exists; republishing the exact bytes
                is the only recovery that cannot mint a second execution. */}
            <Button
              data-testid="add-coding-session-provider-retry"
              disabled={
                isPublishing ||
                lifecycleIsLoading ||
                lifecycleErrorMessage !== null ||
                lifecycle?.state === "created" ||
                lifecycle?.state === "created-with-failed-initial-turn"
              }
              onClick={retryExact}
              type="button"
              variant="outline"
            >
              Retry this exact request
            </Button>
          </>
        ) : null}
        <Button
          data-testid="add-coding-session-provider-submit"
          disabled={!canSubmit}
          onClick={handleSubmit}
          type="button"
        >
          <UserPlus />
          Add provider
        </Button>
      </DialogFooter>
    </div>
  );
}
