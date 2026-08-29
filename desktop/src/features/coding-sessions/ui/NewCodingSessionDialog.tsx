import { CircleAlert, FolderKanban, LoaderCircle, Send } from "lucide-react";
import * as React from "react";

import { MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { useCodingSessionSeatDraft } from "@/features/coding-sessions/lib/useCodingSessionSeatDraft";
import { useManagedAgentsQuery } from "@/features/agents/hooks";
import { MAX_CODING_SESSION_NAME_BYTES } from "@/features/coding-sessions/lib/codingSessionName";
import { useChannelsQuery } from "@/features/channels/hooks";
import { useAppNavigation } from "@/app/navigation/useAppNavigation";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import { Button } from "@/shared/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/shared/ui/dialog";
import { Input } from "@/shared/ui/input";
import { Tabs, TabsList, TabsTrigger } from "@/shared/ui/tabs";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import { createCodingSessionWorktree } from "@/shared/api/tauriCodingSessionWorktrees";
import { useCodingSessionProviderCatalog } from "../useCodingSessionProviderCatalog";
import { useCodingSessionNameSuggestion } from "../useCodingSessionNameSuggestion";
import { codingSessionNameSuggestStatus } from "../lib/codingSessionNameSuggestion";
import { useNewCodingSessionDraft } from "../lib/newCodingSessionDraft";
import {
  canRetryNewCodingSessionCreate,
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  isNewCodingSessionTargetReady,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  resolveSelectedNewCodingSessionTarget,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import {
  NewCodingSessionProviderPicker,
  ProviderLoginNeeded,
} from "./NewCodingSessionProviderPicker";
import { NewCodingSessionAgentSeatField } from "./NewCodingSessionAgentSeatField";
import { NewCodingSessionCrewTab } from "./NewCodingSessionCrewTab";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { NewCodingSessionWorktreeField } from "./NewCodingSessionWorktreeField";
import { PendingCodingSessionScreen } from "./PendingCodingSessionScreen";
import { useNewCodingSessionCreate } from "./useNewCodingSessionCreate";

/**
 * A project the session is being created inside.
 *
 * Supplied by the projects feature's wrapper (glue): the dialog stays the one
 * create flow and only swaps the channel *question* for a channel *fact*.
 */
export type NewCodingSessionProjectContext = {
  projectId: string;
  projectName: string;
  /** Coordinate signed into the create as the session's placement authority. */
  projectRef: string | null;
  /** The project's sessions channel, or null until this create publishes one. */
  channelId: string | null;
  /**
   * Local checkout of one of the project's repositories, resolved async —
   * the workdir prefill when the provider has nothing remembered yet.
   */
  defaultWorkdir: string | null;
  /**
   * The project's per-device default agent seat, prefilled once the managed
   * agents resolve — only while the seat is untouched, and only when the
   * agent is still one this computer manages (a stale default is silently
   * ignored rather than producing a create the provider refuses).
   */
  defaultSeat?: { actor: string; role: string } | null;
  /**
   * Resolve — creating it if needed — the channel this session belongs in.
   * Called once, on submit: opening the dialog and walking away must not
   * leave a channel behind.
   */
  ensureChannelId: () => Promise<string>;
};

/**
 * Create a coding session that belongs to a channel, and optionally to the
 * project that channel serves.
 *
 * This was a full-page route until it became a dialog. The change is not
 * cosmetic: creating a session is something a person does *from* somewhere —
 * a channel, a project — and taking the whole window away made the answer to
 * "which channel is this for?" harder to see, not easier. As a dialog the
 * context stays behind it.
 *
 * The order of the fields is the order of the thinking. The first message
 * comes before the name because the name is *derived from it* — by a model
 * when one is configured, and by the person's own reading of what they just
 * wrote when one is not. Asking for a title first asks someone to summarize
 * a task they have not yet described.
 */
export function NewCodingSessionDialog({
  channelId,
  onOpenChange,
  open,
  projectContext = null,
}: {
  channelId?: string;
  onOpenChange: (open: boolean) => void;
  open: boolean;
  projectContext?: NewCodingSessionProjectContext | null;
}) {
  return (
    <Dialog onOpenChange={onOpenChange} open={open}>
      <DialogContent
        className="max-w-2xl"
        data-testid="new-coding-session-dialog"
      >
        <DialogHeader>
          <DialogTitle>
            {projectContext
              ? `New coding session in ${projectContext.projectName}`
              : "New coding session"}
          </DialogTitle>
          <DialogDescription className="sr-only">
            Describe the first thing to do, then choose where and how the
            session runs.
          </DialogDescription>
        </DialogHeader>
        <NewCodingSessionForm
          channelId={channelId}
          onDone={() => onOpenChange(false)}
          projectContext={projectContext}
        />
      </DialogContent>
    </Dialog>
  );
}

/**
 * The create form itself, without the dialog around it.
 *
 * Exported for the tests and for any surface that already owns a modal.
 */
export function NewCodingSessionForm({
  channelId: initialChannelId,
  onDone,
  projectContext = null,
}: {
  channelId?: string;
  /** Close the surface holding this form — a landed create, or a dismissal. */
  onDone: () => void;
  projectContext?: NewCodingSessionProjectContext | null;
}) {
  const { goCodingSession } = useAppNavigation();
  // Session transports are included here on purpose: the provider-catalog
  // subscription and the project flow's channel summary need them. They are
  // filtered back out of the standalone picker below — a hidden transport is
  // never a channel a person chooses.
  const channelsQuery = useChannelsQuery({
    enabled: true,
    includeSessionTransports: true,
  });
  const memberChannels = React.useMemo(
    () =>
      (channelsQuery.data ?? [])
        .filter((channel) => channel.isMember)
        .sort((left, right) => left.name.localeCompare(right.name)),
    [channelsQuery.data],
  );
  const pickerChannels = React.useMemo(
    () =>
      memberChannels.filter((channel) => !isSessionTransportChannel(channel)),
    [memberChannels],
  );
  const [channelSelection, setChannelSelection] = React.useState<string | null>(
    initialChannelId ?? null,
  );
  const channelId = projectContext
    ? projectContext.channelId
    : (channelSelection ??
      (pickerChannels.some((channel) => channel.id === initialChannelId)
        ? (initialChannelId ?? null)
        : (pickerChannels[0]?.id ?? null)));
  const memberChannelIds = React.useMemo(
    () => memberChannels.map((channel) => channel.id).sort(),
    [memberChannels],
  );
  const providerCatalog = useCodingSessionProviderCatalog(memberChannelIds);

  // A project whose sessions channel does not exist yet still has a definite
  // destination — it just has no id until submit publishes it. The pending key
  // stands in so the provider picker and the durable-create scope stay stable
  // across that transition.
  const pendingChannelKey = projectContext
    ? `pending-project-channel:${projectContext.projectId}`
    : null;
  const targetChannelId = channelId ?? pendingChannelKey;

  // One in-flight create per project, not per channel: the channel can change
  // identity mid-flow (it gets published), the project cannot.
  const scopeId = projectContext
    ? `project:${projectContext.projectId}`
    : (channelId ?? "unscoped");
  const {
    beginLoginWatch,
    durabilityError,
    hostPhase,
    isPublishing,
    lifecycle,
    lifecycleErrorMessage,
    lifecycleIsLoading,
    providerStatus,
    providerRuntimes,
    providerModelsByInstanceRef,
    publishError,
    retryExact,
    seat: signedSeat,
    seatPackStaged,
    stalled,
    startFresh,
    submit,
    transaction,
  } = useNewCodingSessionCreate({
    scopeId,
    onCreated: ({ channelId: createdChannelId, generationId }) => {
      onDone();
      void goCodingSession(createdChannelId, generationId, { replace: true });
    },
  });

  // Catalog-discovered providers merged with this computer's own runtimes.
  // Before any catalog exists for the channel — the ordinary state before the
  // provider has ever been added to it — the local runtimes alone make the
  // first session possible at all.
  const targets = React.useMemo<NewCodingSessionTarget[]>(
    () =>
      resolveNewCodingSessionTargets({
        catalogs: providerCatalog.entries,
        channelId: targetChannelId,
        localProvider: providerStatus?.providerPubkey
          ? {
              providerPubkey: providerStatus.providerPubkey,
              runtimes: providerRuntimes,
              modelsByInstanceRef: providerModelsByInstanceRef,
            }
          : null,
      }),
    [
      providerCatalog.entries,
      providerModelsByInstanceRef,
      providerRuntimes,
      providerStatus?.providerPubkey,
      targetChannelId,
    ],
  );

  const [targetSelection, setTargetSelection] = React.useState<{
    key: string | null;
    explicit: boolean;
  }>({ key: null, explicit: false });
  const selectedTarget = resolveSelectedNewCodingSessionTarget({
    targets,
    selectedTargetKey: targetSelection.key,
    selectionExplicit: targetSelection.explicit,
  });
  const [modelSelection, setModelSelection] = React.useState<{
    value: string | null;
    explicit: boolean;
  }>({ value: null, explicit: false });
  const effectiveModel = resolveSelectedNewCodingSessionModel({
    provider: selectedTarget?.provider ?? null,
    selectedModel: modelSelection.value,
    selectionExplicit: modelSelection.explicit,
  });

  const [title, setTitle] = React.useState("");
  // One session or a whole crew. Two answers to "what am I starting?", so two
  // tabs rather than a checkbox that silently changes what Create means.
  const [mode, setMode] = React.useState<"session" | "crew">("session");
  const managedAgentsQuery = useManagedAgentsQuery();
  const managedAgents = React.useMemo(
    () => managedAgentsQuery.data ?? [],
    [managedAgentsQuery.data],
  );
  // Both halves or neither, resolved the same way the signed create resolves
  // them — so the field can refuse a half-filled seat before anything is
  // published rather than throwing from the builder.
  const seatDraft = useCodingSessionSeatDraft(managedAgents);
  // The project's per-device default seat, applied once managed agents
  // resolve. Once a person touches the seat it stays out — including after
  // they explicitly choose "No agent" — and a default naming an agent this
  // computer no longer manages is ignored rather than refused at create.
  const seatTouchedRef = React.useRef(false);
  const defaultSeat = projectContext?.defaultSeat ?? null;
  const { onActorChange: draftActorChange, onRoleChange: draftRoleChange } =
    seatDraft;
  React.useEffect(() => {
    if (seatTouchedRef.current || seatDraft.actor !== null) return;
    if (!defaultSeat) return;
    if (!managedAgents.some((agent) => agent.pubkey === defaultSeat.actor)) {
      return;
    }
    draftActorChange(defaultSeat.actor);
    // The stored role was chosen deliberately in project settings, so it
    // outranks the agent's home-role default.
    draftRoleChange(defaultSeat.role.trim() || "builder");
  }, [
    defaultSeat,
    draftActorChange,
    draftRoleChange,
    managedAgents,
    seatDraft.actor,
  ]);
  const [workdir, setWorkdir] = React.useState("");
  const [useWorktree, setUseWorktree] = React.useState(true);
  const [worktreeName, setWorktreeName] = React.useState("");
  const [worktreeSource, setWorktreeSource] = React.useState<string | null>(
    null,
  );
  const {
    text: draftText,
    setText: setDraftText,
    clear: clearDraft,
    persistence: draftPersistence,
  } = useNewCodingSessionDraft(scopeId);

  const naming = useNewCodingSessionTitleSuggestion({
    firstMessage: draftText,
    title,
    setTitle,
  });

  const failureCode =
    lifecycle?.state === "failed" ? lifecycle.error.code : undefined;
  // The runtime whose sign-in a failed receipt is asking for: the one the
  // published command targeted, falling back to the current selection.
  const failedRuntime = React.useMemo(() => {
    const provider =
      (transaction
        ? targets.find(
            (target) =>
              target.provider.providerInstanceRef ===
                transaction.input.providerInstanceRef &&
              target.signerPubkey === transaction.input.providerAuthorityPubkey,
          )?.provider
        : null) ?? selectedTarget?.provider;
    if (!provider) return null;
    const availability = targets.find(
      (target) => target.provider === provider,
    )?.availability;
    return { runtime: provider.runtime, label: availability?.label };
  }, [selectedTarget?.provider, targets, transaction]);
  const status = newCodingSessionStatusMessage({
    hostPhase,
    isPublishing,
    publishError: publishError ?? durabilityError,
    lifecycle,
    authRuntime: failedRuntime,
    stalled,
    publishState: transaction?.publishState ?? null,
  });
  const draftBytes = new TextEncoder().encode(draftText).byteLength;
  const draftOverCap =
    draftBytes > MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES;
  const [setupError, setSetupError] = React.useState<string | null>(null);
  const [isPreparingChannel, setIsPreparingChannel] = React.useState(false);
  const canSubmit =
    !isPublishing &&
    !isPreparingChannel &&
    transaction === null &&
    targetChannelId !== null &&
    selectedTarget !== null &&
    isNewCodingSessionTargetReady(selectedTarget) &&
    seatDraft.error === null &&
    !draftOverCap;

  const handleSubmit = React.useCallback(() => {
    if (!canSubmit || !selectedTarget) return;
    setSetupError(null);
    void (async () => {
      let submitTarget = selectedTarget;
      if (projectContext) {
        // The sessions channel is published here, on the first create that
        // needs it, and never on mount.
        setIsPreparingChannel(true);
        try {
          const ensured = await projectContext.ensureChannelId();
          if (ensured !== selectedTarget.channelId) {
            // Only reachable on the bootstrap path: with no channel there was
            // no catalog, so the selection is one of this computer's current
            // runtimes. Preserve that runtime/model choice and bind only its
            // destination to the channel that was just published.
            submitTarget = { ...selectedTarget, channelId: ensured };
          }
        } catch (error) {
          setSetupError(
            error instanceof Error
              ? error.message
              : "Could not prepare a channel for this project's sessions.",
          );
          return;
        } finally {
          setIsPreparingChannel(false);
        }
      }
      // The worktree is created before the command is signed, because its
      // path *is* the working directory the session will run in. Creating it
      // afterwards would mean publishing a create that points at a directory
      // which does not exist yet.
      const checkout = workdir.trim();
      let effectiveWorkdir = checkout;
      if (useWorktree && effectiveWorkdir.length > 0) {
        setIsPreparingChannel(true);
        try {
          const created = await createCodingSessionWorktree({
            workdir: effectiveWorkdir,
            name: worktreeName.trim(),
            source: worktreeSource,
          });
          effectiveWorkdir = created.path;
        } catch (error) {
          setSetupError(
            `Could not create the worktree: ${
              error instanceof Error ? error.message : String(error)
            }`,
          );
          return;
        } finally {
          setIsPreparingChannel(false);
        }
      }
      await submit({
        target: submitTarget,
        model:
          effectiveModel && effectiveModel.length > 0 ? effectiveModel : null,
        title: title.trim().length > 0 ? title.trim() : null,
        initialTurn: draftText.trim().length > 0 ? draftText : null,
        workdir: effectiveWorkdir.length > 0 ? effectiveWorkdir : null,
        // Remember the checkout, not the worktree that was just made from it.
        rememberWorkdir: checkout.length > 0 ? checkout : null,
        projectRef: projectContext?.projectRef ?? null,
        seat: seatDraft.seat,
        seatLabel: seatDraft.label,
      });
      clearDraft();
    })();
  }, [
    canSubmit,
    clearDraft,
    draftText,
    effectiveModel,
    projectContext,
    seatDraft.label,
    seatDraft.seat,
    selectedTarget,
    submit,
    title,
    useWorktree,
    workdir,
    worktreeName,
    worktreeSource,
  ]);

  // Once a transaction exists, the create is a session-in-waiting and renders
  // as one (PendingCodingSessionScreen). The form returns only for the
  // workdir-failure remediation, which needs its fields editable.
  const [editRequested, setEditRequested] = React.useState(false);
  React.useEffect(() => {
    if (transaction === null) setEditRequested(false);
  }, [transaction]);
  const handleStartFresh = React.useCallback(() => {
    setEditRequested(false);
    startFresh();
  }, [startFresh]);

  if (transaction !== null && !editRequested) {
    const transactionChannelName =
      memberChannels.find(
        (channel) => channel.id === transaction.input.channelId,
      )?.name ?? null;
    return (
      // The pending screen is built to fill a window: it sizes itself with
      // `h-full`, which resolves to nothing inside a dialog's auto-height
      // grid. Give it a definite box to fill.
      <div
        className="flex h-[60vh] min-h-0 flex-col"
        data-testid="new-coding-session-pending"
      >
        <PendingCodingSessionScreen
          beginLoginWatch={beginLoginWatch}
          channelName={projectContext ? null : transactionChannelName}
          failedRuntime={failedRuntime}
          hostPhase={hostPhase}
          isPublishing={isPublishing}
          lifecycle={lifecycle}
          lifecycleErrorMessage={lifecycleErrorMessage}
          lifecycleIsLoading={lifecycleIsLoading}
          onBack={onDone}
          onEditRequest={() => setEditRequested(true)}
          projectName={projectContext?.projectName ?? null}
          publishError={publishError ?? durabilityError}
          // Named from the *signed* create, not the form: a transaction
          // rehydrated after a restart has no form state left to read.
          seat={
            signedSeat
              ? {
                  actorLabel:
                    managedAgents.find(
                      (agent) => agent.pubkey === signedSeat.actor,
                    )?.name ?? null,
                  role: signedSeat.role,
                  packStaged: seatPackStaged,
                }
              : null
          }
          retryExact={retryExact}
          stalled={stalled}
          startFresh={handleStartFresh}
          transaction={transaction}
        />
      </div>
    );
  }

  const modeTabs = (
    <Tabs
      className="shrink-0"
      onValueChange={(value) => setMode(value === "crew" ? "crew" : "session")}
      value={mode}
    >
      <TabsList>
        <TabsTrigger
          data-testid="new-coding-session-tab-session"
          value="session"
        >
          One session
        </TabsTrigger>
        <TabsTrigger data-testid="new-coding-session-tab-crew" value="crew">
          Team
        </TabsTrigger>
      </TabsList>
    </Tabs>
  );

  if (mode === "crew") {
    return (
      <>
        {modeTabs}
        <div className="-mx-px flex max-h-[65vh] min-h-0 flex-col overflow-y-auto px-px">
          <NewCodingSessionCrewTab
            channelId={channelId}
            defaultWorkdir={projectContext?.defaultWorkdir ?? null}
            disabled={transaction !== null}
            // A project's sessions channel is published by the first create
            // that needs one — a team launch is one of those creates, and it
            // mints it through the same helper the one-session path uses.
            ensureChannelId={projectContext?.ensureChannelId ?? null}
            model={effectiveModel}
            // D14 seats the lead alone, so there *is* one generation to open —
            // the one its receipt minted. The launch used to close the dialog
            // and navigate nowhere, which is how a session that had really
            // been founded appeared nowhere the person was looking (item 87b).
            onLaunched={({ channelId: launchedChannelId, generationId }) => {
              onDone();
              if (generationId) {
                void goCodingSession(launchedChannelId, generationId, {
                  replace: true,
                });
              }
            }}
            projectName={projectContext?.projectName ?? null}
            projectRef={projectContext?.projectRef ?? null}
            providerAuthorityPubkey={selectedTarget?.signerPubkey ?? null}
            providerAllowedModels={selectedTarget?.provider.allowedModels ?? []}
            providerInstanceRef={
              selectedTarget?.provider.providerInstanceRef ?? null
            }
            providerLabel={
              targets.find(
                (entry) => entry.provider === selectedTarget?.provider,
              )?.availability?.label ??
              selectedTarget?.provider.runtime ??
              null
            }
          />
        </div>
      </>
    );
  }

  return (
    <>
      {modeTabs}
      <div
        // `overflow-y-auto` clips the x-axis as well — CSS has no way to
        // scroll one axis and leave the other visible — so a `w-full` child
        // sits exactly on the clip edge and loses its side borders, and the
        // textarea's 1px focus ring (which draws *outside* its border box)
        // disappears entirely. One pixel of horizontal padding gives them
        // somewhere to land; the matching negative margin keeps the fields
        // aligned with the footer outside this box.
        className="-mx-px flex max-h-[65vh] min-h-0 flex-col gap-5 overflow-y-auto px-px"
        data-testid="new-coding-session-form"
      >
        {projectContext ? (
          <NewCodingSessionProjectDestination
            projectName={projectContext.projectName}
          />
        ) : (
          <NewCodingSessionChannelPicker
            channels={pickerChannels}
            disabled={transaction !== null}
            onChange={(next) => {
              setChannelSelection(next);
              setTargetSelection({ key: null, explicit: false });
              setModelSelection({ value: null, explicit: false });
            }}
            value={channelId}
          />
        )}

        <div className="flex flex-col gap-2">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="coding-session-initial-turn"
          >
            First message <span className="font-normal">(optional)</span>
          </label>
          <Textarea
            className="min-h-32"
            data-testid="new-coding-session-initial-turn"
            disabled={transaction !== null}
            id="coding-session-initial-turn"
            onBlur={naming.requestNow}
            onChange={(event) => setDraftText(event.target.value)}
            placeholder="Describe the first thing to do…"
            value={draftText}
          />
          {draftOverCap ? (
            <p className="text-xs text-destructive">
              This first message is too long for a signed session request.
            </p>
          ) : null}
          {draftPersistence.message ? (
            <p className="text-xs text-muted-foreground">
              {draftPersistence.message}
            </p>
          ) : null}
        </div>

        <div className="flex flex-col gap-2">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="coding-session-title"
          >
            Name <span className="font-normal">(optional)</span>
          </label>
          <Input
            data-testid="new-coding-session-title"
            disabled={transaction !== null}
            id="coding-session-title"
            maxLength={MAX_CODING_SESSION_NAME_BYTES}
            onChange={(event) => naming.setTitleByHand(event.target.value)}
            placeholder="What is this session for?"
            value={title}
          />
          {naming.status.message ? (
            <p
              className={cn(
                "text-2xs",
                naming.status.state === "failed"
                  ? "text-destructive"
                  : "text-muted-foreground",
              )}
              data-testid="new-coding-session-title-suggestion"
            >
              {naming.status.message}
            </p>
          ) : null}
        </div>

        <NewCodingSessionProviderPicker
          disabled={transaction !== null}
          model={effectiveModel}
          onLoginLaunched={({ runtime }) => beginLoginWatch(runtime)}
          onModelChange={(value) =>
            setModelSelection({ value, explicit: true })
          }
          onTargetChange={(key) => {
            setTargetSelection({ key, explicit: true });
            setModelSelection({ value: null, explicit: false });
          }}
          selectedTarget={selectedTarget}
          targets={targets}
        />

        <NewCodingSessionAgentSeatField
          actor={seatDraft.actor}
          agents={managedAgents}
          disabled={transaction !== null}
          error={seatDraft.error}
          onActorChange={(next) => {
            seatTouchedRef.current = true;
            seatDraft.onActorChange(next);
          }}
          onRoleChange={(next) => {
            seatTouchedRef.current = true;
            seatDraft.onRoleChange(next);
          }}
          role={seatDraft.role}
        />

        <NewCodingSessionWorkdirField
          channelId={channelId}
          disabled={
            transaction !== null && !isCodingSessionWorkdirFailure(failureCode)
          }
          fallbackPath={projectContext?.defaultWorkdir ?? null}
          onChange={setWorkdir}
          projectKey={projectContext?.projectRef ?? null}
          value={workdir}
        />

        <NewCodingSessionWorktreeField
          checked={useWorktree}
          disabled={
            transaction !== null && !isCodingSessionWorkdirFailure(failureCode)
          }
          name={worktreeName}
          onCheckedChange={setUseWorktree}
          onNameChange={setWorktreeName}
          onSourceChange={setWorktreeSource}
          sessionName={title}
          source={worktreeSource}
          workdir={workdir}
        />

        {isCodingSessionAuthFailure(failureCode) ? (
          <ProviderLoginNeeded
            onLoginLaunched={({ runtime }) => beginLoginWatch(runtime)}
            runtime={failedRuntime}
          />
        ) : null}

        {setupError ? (
          <p
            className="flex items-start gap-2 text-sm text-destructive"
            data-testid="new-coding-session-setup-error"
            role="alert"
          >
            <CircleAlert className="mt-0.5 size-4 shrink-0" />
            {setupError}
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
            data-testid="new-coding-session-status"
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

        {targets.length === 0 &&
        targetChannelId !== null &&
        hostPhase === "idle" ? (
          <p className="text-sm text-muted-foreground">
            No coding-session provider is available. Reopen this dialog to retry
            setting up this computer's provider.
          </p>
        ) : null}
      </div>

      {/* Outside the scroll box: a create button that scrolls away with the
          fields is a create button a short window hides entirely. */}
      <div className="flex shrink-0 items-center justify-end gap-2">
        {transaction ? (
          <>
            <Button
              data-testid="new-coding-session-start-fresh"
              onClick={handleStartFresh}
              type="button"
              // Once a wait has stalled, "Start fresh" is the only real
              // escape — promote it from ghost so it reads as the action.
              variant={stalled ? "outline" : "ghost"}
            >
              Start fresh
            </Button>
            <Button
              data-testid="new-coding-session-retry"
              disabled={
                !canRetryNewCodingSessionCreate({
                  isPublishing,
                  lifecycleIsLoading,
                  lifecycleErrorMessage,
                  lifecycleState: lifecycle?.state ?? null,
                  stalled,
                  publishState: transaction?.publishState ?? null,
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
          data-testid="new-coding-session-submit"
          disabled={!canSubmit}
          onClick={handleSubmit}
          type="button"
        >
          <Send />
          Create session
        </Button>
      </div>
    </>
  );
}

/**
 * The name field's relationship with the namer.
 *
 * A generated name may replace an earlier generated name — the first message
 * grows, and so should its title — but never one a person typed. That is the
 * whole reason this is a hook rather than two `useState` calls: the
 * distinction lives in a ref that both the setter and the suggestion handler
 * have to agree on.
 */
function useNewCodingSessionTitleSuggestion({
  firstMessage,
  setTitle,
  title,
}: {
  firstMessage: string;
  setTitle: (title: string) => void;
  title: string;
}) {
  const autoFilledRef = React.useRef<string | null>(null);
  const titleRef = React.useRef(title);
  titleRef.current = title;

  const setTitleByHand = React.useCallback(
    (next: string) => {
      // Anything typed here is the person's, and stops the namer from
      // touching the field for the rest of this create.
      autoFilledRef.current = null;
      setTitle(next);
    },
    [setTitle],
  );

  const handleSuggestion = React.useCallback(
    (suggestion: string) => {
      const current = titleRef.current;
      if (current.trim().length > 0 && current !== autoFilledRef.current) {
        return;
      }
      autoFilledRef.current = suggestion;
      setTitle(suggestion);
    },
    [setTitle],
  );

  const { enabled, error, isGenerating, requestNow } =
    useCodingSessionNameSuggestion({
      firstMessage,
      onSuggestion: handleSuggestion,
    });

  return {
    requestNow,
    setTitleByHand,
    status: codingSessionNameSuggestStatus({ enabled, error, isGenerating }),
  };
}

/**
 * Where a project-scoped session will live, stated rather than asked. The
 * transport channel carrying the transcript is deliberately never named:
 * members reach sessions through the project, so surfacing the channel would
 * only advertise plumbing they cannot (and should not) interact with.
 */
export function NewCodingSessionProjectDestination({
  projectName,
}: {
  projectName: string;
}) {
  return (
    <div
      className="flex flex-col gap-1 rounded-lg border border-border/60 bg-muted/30 px-3 py-2.5"
      data-testid="new-coding-session-project-destination"
    >
      <p className="flex items-center gap-2 text-sm font-medium">
        <FolderKanban className="size-4 shrink-0 text-muted-foreground" />
        <span className="truncate">{projectName}</span>
      </p>
      <p className="text-2xs text-muted-foreground">
        The session and its signed transcript live in this project, visible to
        project members.
      </p>
    </div>
  );
}

export function NewCodingSessionChannelPicker({
  channels,
  disabled,
  onChange,
  value,
}: {
  channels: ReadonlyArray<{ id: string; name: string }>;
  disabled: boolean;
  onChange: (channelId: string) => void;
  value: string | null;
}) {
  return (
    <div className="flex flex-col gap-2">
      <label
        className="text-xs font-medium text-muted-foreground"
        htmlFor="coding-session-channel"
      >
        Channel
      </label>
      <select
        className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
        data-testid="new-coding-session-channel"
        disabled={disabled}
        id="coding-session-channel"
        onChange={(event) => onChange(event.target.value)}
        value={value ?? ""}
      >
        {channels.length === 0 ? (
          <option value="">No channels available</option>
        ) : null}
        {channels.map((channel) => (
          <option key={channel.id} value={channel.id}>
            #{channel.name}
          </option>
        ))}
      </select>
      <p className="text-2xs text-muted-foreground">
        The session's signed transcript lives here, visible to this channel's
        members.
      </p>
    </div>
  );
}
