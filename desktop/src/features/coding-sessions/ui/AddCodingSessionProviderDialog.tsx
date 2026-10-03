import * as React from "react";
import { CircleAlert, LoaderCircle, UserPlus } from "lucide-react";

import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { formatCodingSessionExecutionLabel } from "@/features/coding-sessions/lib/codingSessionLabels";
import { codingSessionSeatStagedFromLine } from "@/features/coding-sessions/lib/codingSessionPackRef";
import {
  resolveCodingSessionSeatProjectScope,
  useCodingSessionSeatDraft,
} from "@/features/coding-sessions/lib/useCodingSessionSeatDraft";
import type { CodingSessionUmbrellaRecord } from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  canStartFreshNewCodingSessionCreate,
  canRetryNewCodingSessionCreate,
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  isNewCodingSessionTargetReady,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  type NewCodingSessionTarget,
} from "@/features/coding-sessions/lib/newCodingSessionModel";
import { useCodingSessionProviderCatalog } from "@/features/coding-sessions/useCodingSessionProviderCatalog";
import { createCodingSessionWorktree } from "@/shared/api/tauriCodingSessionWorktrees";
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
  addCodingSessionProviderGenesisGateMessage,
  addCodingSessionProviderOptionNote,
  addCodingSessionProviderSeatWorkdirNote,
  addCodingSessionProviderSeatWorktreeName,
  buildAddCodingSessionProviderSubmit,
  defaultAddCodingSessionProviderKey,
  listAddCodingSessionProviderOptions,
} from "./addCodingSessionProviderModel";
import {
  NewCodingSessionProviderPicker,
  ProviderLoginNeeded,
} from "./NewCodingSessionProviderPicker";
import { NewCodingSessionAgentSeatField } from "./NewCodingSessionAgentSeatField";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { NewCodingSessionWorktreeField } from "./NewCodingSessionWorktreeField";
import { useNewCodingSessionCreate } from "./useNewCodingSessionCreate";
import { CodingSessionHostAdmissionLine } from "./CodingSessionHostAdmissionLine";

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
 *
 * The join can seat an agent, exactly as founding can — same field, same
 * home-role default, same disclosures, same membership + custody staging
 * before the publish. Without it an agent could only ever be seated at the
 * moment a session was born.
 *
 * A *seated* join also gets its own git worktree, on by default and named
 * `<session-slug>-<role>`: item 80(a) hired three seats into this dialog and
 * every one of them landed in the operator's live checkout, because the
 * working directory defaulted to the last one used. An unseated join is the
 * person's own execution and is unchanged.
 */
export function AddCodingSessionProviderDialog({
  channelId,
  channelName,
  onOpenChange,
  open,
  projectRef = null,
  umbrella,
}: {
  channelId: string;
  channelName: string | null;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  /**
   * The project this session belongs to, when the caller resolved one
   * (LANE-L25) — passed straight through to the seat field's pack preview
   * (LANE-L23). `null` (the default) renders the seat field exactly as it
   * did before that prop existed: no preview.
   */
  projectRef?: string | null;
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
            keeps its own signed transcript — nothing is merged. The new agent
            does not automatically read this transcript — it starts fresh unless
            Beekeeper can prepare verified session history for it, and its
            transcript will say which happened.
          </DialogDescription>
        </DialogHeader>
        {open ? (
          <AddCodingSessionProviderForm
            channelId={channelId}
            channelName={channelName}
            onDone={handleDone}
            projectRef={projectRef}
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
export function AddCodingSessionProviderForm({
  channelId,
  channelName,
  onDone,
  projectRef = null,
  umbrella,
}: {
  channelId: string;
  channelName: string | null;
  onDone: () => void;
  /** See {@link AddCodingSessionProviderDialog}'s `projectRef` (LANE-L25). */
  projectRef?: string | null;
  umbrella: CodingSessionUmbrellaRecord;
}) {
  const sessionRef = umbrella.sessionRef;
  const providerCatalog = useCodingSessionProviderCatalog([channelId]);
  const {
    beginLoginWatch,
    durabilityError,
    hostAdmission,
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
    seat: signedSeat,
    seatPackRef,
    seatPackStaged,
    startFresh,
    stalled,
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
  const [useWorktree, setUseWorktree] = React.useState(true);
  const [worktreeName, setWorktreeName] = React.useState("");
  const [worktreeSource, setWorktreeSource] = React.useState<string | null>(
    null,
  );
  const [worktreeError, setWorktreeError] = React.useState<string | null>(null);
  const [isPreparingWorktree, setIsPreparingWorktree] = React.useState(false);
  const [initialTurn, setInitialTurn] = React.useState("");
  const managedAgentsQuery = useManagedAgentsQuery();
  const managedAgents = React.useMemo(
    () => managedAgentsQuery.data ?? [],
    [managedAgentsQuery.data],
  );
  // Which agents a seat here may hold: the project this join will sign (the
  // umbrella's inherited one), or agents in no project when the session has
  // none. Unknown until an execution reports, and the field says so.
  const seatScope = React.useMemo(
    () =>
      resolveCodingSessionSeatProjectScope({
        executions: umbrella.executions,
      }),
    [umbrella.executions],
  );
  const seatDraft = useCodingSessionSeatDraft(managedAgents, seatScope);
  // What custody actually staged for the seat, once it has. `null` means it
  // has not run here, and is never shown as "no pack".
  const stagedSeatLabel = signedSeat
    ? formatCodingSessionExecutionLabel({
        agentRef: signedSeat.actor,
        role: signedSeat.role,
        agentDisplayName:
          managedAgents.find((agent) => agent.pubkey === signedSeat.actor)
            ?.name ?? null,
        runtime: null,
        model: null,
      }).primary
    : null;

  // A seat runs in a tree of its own; a person's own join runs where they
  // said. Everything worktree-shaped below is gated on this one fact.
  const seated = seatDraft.seat !== null;
  const seatWorktreeName = addCodingSessionProviderSeatWorktreeName({
    title: umbrella.title,
    role: seatDraft.role,
  });
  const seatWorkdirNote = addCodingSessionProviderSeatWorkdirNote({
    seatLabel: seated ? (seatDraft.label ?? "This seat") : null,
    useWorktree,
    workdir,
  });

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
    publishState: transaction?.publishState ?? null,
  });
  const turnOverCap =
    new TextEncoder().encode(initialTurn).byteLength >
    MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES;
  // Never build a join while the umbrella's genesis is unresolved or
  // conflicted — omitting `genesisRef` would attach an ungoverned execution
  // inside a governed session. Fail visibly instead.
  const genesisGateMessage =
    addCodingSessionProviderGenesisGateMessage(umbrella);
  const canSubmit =
    sessionRef !== null &&
    genesisGateMessage === null &&
    !isPublishing &&
    !isPreparingWorktree &&
    transaction === null &&
    selectedTarget !== null &&
    isNewCodingSessionTargetReady(selectedTarget) &&
    seatDraft.error === null &&
    !turnOverCap;

  const handleSubmit = React.useCallback(() => {
    if (!canSubmit) return;
    setWorktreeError(null);
    void (async () => {
      // The worktree exists before the command is signed, because its path
      // *is* the working directory the create names. A seat is never signed
      // against a directory that does not exist yet.
      const checkout = workdir.trim();
      let effectiveWorkdir = checkout;
      if (seated && useWorktree && checkout.length > 0) {
        setIsPreparingWorktree(true);
        try {
          const created = await createCodingSessionWorktree({
            workdir: checkout,
            name: worktreeName.trim(),
            source: worktreeSource,
            projectRef,
          });
          effectiveWorkdir = created.path;
        } catch (error) {
          setWorktreeError(
            `Could not create this seat's worktree: ${
              error instanceof Error ? error.message : String(error)
            }`,
          );
          return;
        } finally {
          setIsPreparingWorktree(false);
        }
      }
      const payload = buildAddCodingSessionProviderSubmit({
        umbrella,
        channelId,
        target: selectedTarget,
        model: effectiveModel,
        initialTurn,
        workdir: effectiveWorkdir,
        // Remember the checkout, never the worktree just made from it.
        rememberWorkdir: effectiveWorkdir === checkout ? null : checkout,
        seat: { actor: seatDraft.actor, role: seatDraft.role },
        seatLabel: seatDraft.label,
      });
      if (payload) await submit(payload);
    })();
  }, [
    canSubmit,
    channelId,
    effectiveModel,
    initialTurn,
    projectRef,
    seatDraft.actor,
    seatDraft.label,
    seatDraft.role,
    seated,
    selectedTarget,
    submit,
    umbrella,
    useWorktree,
    workdir,
    worktreeName,
    worktreeSource,
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
        usesWorktree={useWorktree}
        value={workdir}
      />

      <NewCodingSessionAgentSeatField
        actor={seatDraft.actor}
        agents={seatDraft.agents}
        disabled={transaction !== null}
        error={seatDraft.error}
        scopeSentence={seatDraft.scopeSentence}
        onActorChange={seatDraft.onActorChange}
        onRoleChange={seatDraft.onRoleChange}
        projectRef={projectRef}
        role={seatDraft.role}
        roleLocked={seatDraft.roleLocked}
      />

      {seated ? (
        <NewCodingSessionWorktreeField
          checked={useWorktree}
          disabled={transaction !== null}
          name={worktreeName}
          onCheckedChange={setUseWorktree}
          onNameChange={setWorktreeName}
          onSourceChange={setWorktreeSource}
          sessionName={seatWorktreeName}
          source={worktreeSource}
          workdir={workdir}
        />
      ) : null}

      {seatWorkdirNote ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="add-coding-session-provider-workdir-note"
        >
          {seatWorkdirNote}
        </p>
      ) : null}

      {worktreeError ? (
        <p
          className="flex items-start gap-2 text-sm text-destructive"
          data-testid="add-coding-session-provider-worktree-error"
          role="alert"
        >
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          {worktreeError}
        </p>
      ) : null}

      <CodingSessionHostAdmissionLine admission={hostAdmission} />

      {stagedSeatLabel ? (
        <p
          className="text-xs text-muted-foreground"
          data-testid="add-coding-session-provider-seat"
        >
          {seatPackStaged === false
            ? `Seated: ${stagedSeatLabel} — seated with no role skills: this ` +
              `computer has no role pack behind this persona.`
            : (() => {
                const stagedFrom = codingSessionSeatStagedFromLine(seatPackRef);
                return stagedFrom === null
                  ? `Seated: ${stagedSeatLabel}`
                  : `Seated: ${stagedSeatLabel} — ${stagedFrom}`;
              })()}
        </p>
      ) : null}

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

      {genesisGateMessage ? (
        <p
          className="flex items-start gap-2 text-sm text-destructive"
          data-testid="add-coding-session-provider-genesis-gate"
          role="alert"
        >
          <CircleAlert className="mt-0.5 size-4 shrink-0" />
          {genesisGateMessage}
        </p>
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
              disabled={
                !canStartFreshNewCodingSessionCreate({
                  isPublishing,
                  lifecycleIsLoading,
                  lifecycleErrorMessage,
                  lifecycleState: lifecycle?.state,
                  publishState: transaction.publishState,
                })
              }
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
                !canRetryNewCodingSessionCreate({
                  isPublishing,
                  lifecycleIsLoading,
                  lifecycleErrorMessage,
                  lifecycleState: lifecycle?.state,
                  publishState: transaction.publishState,
                  stalled,
                })
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
