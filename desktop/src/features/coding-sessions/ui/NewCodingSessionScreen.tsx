import { useCanGoBack, useNavigate, useRouter } from "@tanstack/react-router";
import {
  ArrowLeft,
  CircleAlert,
  FolderKanban,
  LoaderCircle,
  Send,
} from "lucide-react";
import * as React from "react";

import { MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { MAX_CODING_SESSION_NAME_BYTES } from "@/features/coding-sessions/lib/codingSessionName";
import { useChannelsQuery } from "@/features/channels/hooks";
import { isSessionTransportChannel } from "@/shared/api/channelTypes";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import { useCodingSessionProviderCatalog } from "../useCodingSessionProviderCatalog";
import { useNewCodingSessionDraft } from "../lib/newCodingSessionDraft";
import {
  canRetryNewCodingSessionCreate,
  codingSessionAuthRemediation,
  formatCodingSessionProviderLabel,
  isCodingSessionAuthFailure,
  isCodingSessionWorkdirFailure,
  isNewCodingSessionTargetReady,
  newCodingSessionStatusMessage,
  resolveNewCodingSessionTargets,
  resolveSelectedNewCodingSessionModel,
  resolveSelectedNewCodingSessionTarget,
  type NewCodingSessionTarget,
} from "../lib/newCodingSessionModel";
import { formatCodingSessionRuntimeLabel } from "../lib/codingSessionLabels";
import { CodingSessionRuntimeConnect } from "./CodingSessionRuntimeConnect";
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { PendingCodingSessionScreen } from "./PendingCodingSessionScreen";
import { useNewCodingSessionCreate } from "./useNewCodingSessionCreate";

/**
 * A project the session is being created inside.
 *
 * Supplied by the projects feature's wrapper (glue): the screen stays the one
 * create flow and only swaps the channel *question* for a channel *fact*.
 */
export type NewCodingSessionProjectContext = {
  projectId: string;
  projectName: string;
  /** Coordinate signed into the create as the session's placement authority. */
  projectRef: string | null;
  /** The project's sessions channel, or null until this create publishes one. */
  channelId: string | null;
  /** Name the sessions channel will be published under, when there is none. */
  pendingChannelName: string;
  /**
   * Resolve — creating it if needed — the channel this session belongs in.
   * Called once, on submit: opening the screen and walking away must not leave
   * a channel behind.
   */
  ensureChannelId: () => Promise<string>;
};

/**
 * Create a coding session that belongs to a channel, and optionally to the
 * project that channel serves.
 *
 * The donor's screen was project-shaped: it picked a project, then a repo,
 * then a provider narrowed by both. Standalone creation asks the three
 * questions a session actually needs — which channel, which provider, which
 * directory on this machine — and publishes `projectRef: null`. With a
 * `projectContext` the project is already decided, so the channel picker
 * becomes a statement of where the session will live and the create carries
 * the project's coordinate.
 */
export function NewCodingSessionScreen({
  channelId: initialChannelId,
  projectContext = null,
}: {
  channelId?: string;
  projectContext?: NewCodingSessionProjectContext | null;
}) {
  const navigate = useNavigate();
  const router = useRouter();
  const canGoBack = useCanGoBack();
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
    stalled,
    startFresh,
    submit,
    transaction,
  } = useNewCodingSessionCreate({
    scopeId,
    onCreated: ({ channelId: createdChannelId, generationId }) => {
      void navigate({
        to: "/coding-sessions/$channelId/$generationId",
        params: { channelId: createdChannelId, generationId },
        search: { surface: "main" },
        replace: true,
      });
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
  const [workdir, setWorkdir] = React.useState("");
  const {
    text: draftText,
    setText: setDraftText,
    clear: clearDraft,
    persistence: draftPersistence,
  } = useNewCodingSessionDraft(scopeId);

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
      await submit({
        target: submitTarget,
        model:
          effectiveModel && effectiveModel.length > 0 ? effectiveModel : null,
        title: title.trim().length > 0 ? title.trim() : null,
        initialTurn: draftText.trim().length > 0 ? draftText : null,
        workdir: workdir.trim().length > 0 ? workdir.trim() : null,
        projectRef: projectContext?.projectRef ?? null,
      });
      clearDraft();
    })();
  }, [
    canSubmit,
    clearDraft,
    draftText,
    effectiveModel,
    projectContext,
    selectedTarget,
    submit,
    title,
    workdir,
  ]);

  const handleBack = React.useCallback(() => {
    if (canGoBack) {
      router.history.back();
      return;
    }
    void navigate({ to: "/" });
  }, [canGoBack, navigate, router.history]);

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

  // Pin the route's search param to the in-flight transaction's channel:
  // the durable-create scope is the channel id, and a reload of
  // `/coding-sessions/new` without it would re-derive a default channel and
  // miss the pending transaction entirely.
  React.useEffect(() => {
    if (!transaction) return;
    if (initialChannelId === transaction.input.channelId) return;
    void navigate({
      to: "/coding-sessions/new",
      search: { channelId: transaction.input.channelId },
      replace: true,
    });
  }, [initialChannelId, navigate, transaction]);

  if (transaction !== null && !editRequested) {
    const transactionChannelName =
      memberChannels.find(
        (channel) => channel.id === transaction.input.channelId,
      )?.name ?? null;
    return (
      <PendingCodingSessionScreen
        beginLoginWatch={beginLoginWatch}
        channelName={transactionChannelName}
        failedRuntime={failedRuntime}
        hostPhase={hostPhase}
        isPublishing={isPublishing}
        lifecycle={lifecycle}
        lifecycleErrorMessage={lifecycleErrorMessage}
        lifecycleIsLoading={lifecycleIsLoading}
        onBack={handleBack}
        onEditRequest={() => setEditRequested(true)}
        publishError={publishError ?? durabilityError}
        retryExact={retryExact}
        stalled={stalled}
        startFresh={handleStartFresh}
        transaction={transaction}
      />
    );
  }

  return (
    <main
      className="flex h-full min-h-0 flex-1 flex-col overflow-y-auto bg-background"
      data-testid="new-coding-session-screen"
    >
      <header className="flex h-14 shrink-0 items-center gap-3 border-b border-border/60 px-4">
        <Button
          aria-label="Back"
          data-testid="new-coding-session-back"
          onClick={handleBack}
          size="icon"
          type="button"
          variant="ghost"
        >
          <ArrowLeft />
        </Button>
        <h1 className="text-sm font-semibold">
          {projectContext
            ? `New coding session in ${projectContext.projectName}`
            : "New coding session"}
        </h1>
      </header>

      <div className="mx-auto flex w-full max-w-2xl flex-col gap-5 px-5 py-7 sm:px-8">
        {projectContext ? (
          <NewCodingSessionProjectDestination
            channelName={
              memberChannels.find(
                (channel) => channel.id === projectContext.channelId,
              )?.name ?? null
            }
            pendingChannelName={projectContext.pendingChannelName}
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

        <NewCodingSessionWorkdirField
          channelId={channelId}
          disabled={
            transaction !== null && !isCodingSessionWorkdirFailure(failureCode)
          }
          onChange={setWorkdir}
          projectKey={projectContext?.projectRef ?? null}
          value={workdir}
        />

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
            onChange={(event) => setTitle(event.target.value)}
            placeholder="What is this session for?"
            value={title}
          />
        </div>

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
            No coding-session provider is available. Reopen this screen to retry
            setting up this computer's provider.
          </p>
        ) : null}

        <div className="flex items-center justify-end gap-2">
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
      </div>
    </main>
  );
}

/**
 * Where a project-scoped session will live, stated rather than asked. A project
 * without a sessions channel yet names the one this create is about to publish,
 * so the side effect is visible before the button is pressed.
 */
export function NewCodingSessionProjectDestination({
  channelName,
  pendingChannelName,
  projectName,
}: {
  channelName: string | null;
  pendingChannelName: string;
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
        {channelName ? (
          <>
            The session's signed transcript lives in{" "}
            <span className="font-medium">#{channelName}</span>, visible to that
            channel's members.
          </>
        ) : (
          <>
            This project has no sessions channel yet. Creating this session
            publishes a closed channel called{" "}
            <span className="font-medium">#{pendingChannelName}</span> inside
            the project, and the transcript lives there.
          </>
        )}
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

export function NewCodingSessionProviderPicker({
  disabled,
  model,
  noteForTarget,
  onLoginLaunched,
  onModelChange,
  onTargetChange,
  selectedTarget,
  targets,
}: {
  disabled: boolean;
  model: string | null;
  /**
   * Extra parenthetical for an option, e.g. "already in this session" in the
   * join flow. Availability suffixes still win — a signed-out runtime's
   * remediation is the more urgent thing to say.
   */
  noteForTarget?: (target: NewCodingSessionTarget) => string | null;
  /** Forwarded to each unavailable runtime's Connect button. */
  onLoginLaunched?: (input: { runtime: string; headless: boolean }) => void;
  onModelChange: (model: string) => void;
  onTargetChange: (selectionKey: string) => void;
  selectedTarget: NewCodingSessionTarget | null;
  targets: readonly NewCodingSessionTarget[];
}) {
  const models = selectedTarget?.provider.allowedModels ?? [];
  // One remediation row per unavailable runtime — disabled options say what
  // is wrong, but an <option> cannot carry a full sentence, let alone the
  // Connect button that fixes a signed-out runtime in place.
  const unavailableRuntimes = [
    ...new Map(
      targets.flatMap((target) =>
        !isNewCodingSessionTargetReady(target) && target.availability?.hint
          ? [
              [
                target.provider.runtime,
                {
                  runtime: target.provider.runtime,
                  label:
                    target.availability.label ??
                    formatCodingSessionRuntimeLabel(target.provider.runtime),
                  state: target.availability.state,
                  hint: target.availability.hint,
                },
              ] as const,
            ]
          : [],
      ),
    ).values(),
  ];
  return (
    <div className="flex flex-col gap-3 sm:flex-row">
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-provider"
        >
          Provider
        </label>
        <select
          className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
          data-testid="new-coding-session-provider"
          disabled={disabled || targets.length === 0}
          id="coding-session-provider"
          onChange={(event) => onTargetChange(event.target.value)}
          value={selectedTarget?.selectionKey ?? ""}
        >
          {targets.length === 0 ? (
            <option value="">No provider available</option>
          ) : null}
          {targets.map((target) => {
            const label = formatCodingSessionProviderLabel({
              runtime: target.provider.runtime,
              providerInstanceRef: target.provider.providerInstanceRef,
            });
            const note = noteForTarget?.(target) ?? null;
            const suffix =
              target.availability?.state === "needs_auth"
                ? " (sign-in needed)"
                : target.availability?.state === "missing"
                  ? " (not installed)"
                  : note
                    ? ` (${note})`
                    : // Without this the local provider and a catalog entry
                      // from another (possibly long-dead) provider render as
                      // identical options.
                      target.isLocalProvider
                      ? " (this computer)"
                      : "";
            return (
              <option
                disabled={!isNewCodingSessionTargetReady(target)}
                key={target.selectionKey}
                value={target.selectionKey}
              >
                {label}
                {suffix}
              </option>
            );
          })}
        </select>
        {unavailableRuntimes.map((entry) => (
          <div className="flex flex-col gap-1.5" key={entry.runtime}>
            <p className="text-2xs text-muted-foreground">{entry.hint}</p>
            {entry.state === "needs_auth" ? (
              <CodingSessionRuntimeConnect
                disabled={disabled}
                label={entry.label}
                onLoginLaunched={onLoginLaunched}
                runtime={entry.runtime}
              />
            ) : null}
          </div>
        ))}
      </div>
      <div className="flex min-w-0 flex-1 flex-col gap-2">
        <label
          className="text-xs font-medium text-muted-foreground"
          htmlFor="coding-session-model"
        >
          Model
        </label>
        <select
          className="h-9 rounded-md border border-input bg-transparent px-3 text-sm disabled:opacity-50"
          data-testid="new-coding-session-model"
          disabled={disabled || models.length === 0}
          id="coding-session-model"
          onChange={(event) => onModelChange(event.target.value)}
          value={model ?? ""}
        >
          {models.length === 0 ? (
            <option value="">Provider default</option>
          ) : null}
          {models.map((allowed) => (
            <option key={allowed} value={allowed}>
              {allowed}
            </option>
          ))}
        </select>
      </div>
    </div>
  );
}

export function ProviderLoginNeeded({
  onLoginLaunched,
  runtime,
}: {
  /** Forwarded to the Connect button under the remediation text. */
  onLoginLaunched?: (input: { runtime: string; headless: boolean }) => void;
  runtime?: { runtime: string; label?: string } | null;
}) {
  const remediation = codingSessionAuthRemediation(runtime);
  // The remediation copy falls back to claude with no runtime context; the
  // Connect button targets the same fallback so the two never disagree.
  const runtimeSlug = runtime?.runtime ?? "claude";
  // Split the message around the command so it renders as an inline <code>
  // block; a runtime with no known command shows the sentence as-is.
  const [before, after] = remediation.command
    ? remediation.message.split(`\`${remediation.command}\``)
    : [remediation.message, undefined];
  return (
    <div
      className="rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2.5 text-sm"
      data-testid="new-coding-session-auth-required"
      role="alert"
    >
      <p className="font-medium">{remediation.title}</p>
      <p className="mt-1 text-muted-foreground">
        {before}
        {remediation.command && after !== undefined ? (
          <>
            <code className="rounded bg-muted px-1 py-0.5 font-mono text-xs">
              {remediation.command}
            </code>
            {after}
          </>
        ) : null}
      </p>
      <div className="mt-2">
        <CodingSessionRuntimeConnect
          label={runtime?.label ?? formatCodingSessionRuntimeLabel(runtimeSlug)}
          onLoginLaunched={onLoginLaunched}
          runtime={runtimeSlug}
        />
      </div>
    </div>
  );
}
