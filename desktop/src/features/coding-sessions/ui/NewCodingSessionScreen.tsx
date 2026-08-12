import { useCanGoBack, useNavigate, useRouter } from "@tanstack/react-router";
import { ArrowLeft, CircleAlert, LoaderCircle, Send } from "lucide-react";
import * as React from "react";

import { MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES } from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { useChannelsQuery } from "@/features/channels/hooks";
import { Button } from "@/shared/ui/button";
import { Input } from "@/shared/ui/input";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";
import { useCodingSessionProviderCatalog } from "../useCodingSessionProviderCatalog";
import { useNewCodingSessionDraft } from "../lib/newCodingSessionDraft";
import {
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
import { NewCodingSessionWorkdirField } from "./NewCodingSessionWorkdirField";
import { useNewCodingSessionCreate } from "./useNewCodingSessionCreate";

/**
 * Create a coding session that belongs to a channel and nothing else.
 *
 * The donor's screen was project-shaped: it picked a project, then a repo,
 * then a provider narrowed by both. Standalone creation asks the three
 * questions a session actually needs — which channel, which provider, which
 * directory on this machine — and publishes `projectRef: null`. The
 * project-scoped wrapper comes back with the glue patch that owns projects.
 */
export function NewCodingSessionScreen({
  channelId: initialChannelId,
}: {
  channelId?: string;
}) {
  const navigate = useNavigate();
  const router = useRouter();
  const canGoBack = useCanGoBack();
  const channelsQuery = useChannelsQuery({ enabled: true });
  const memberChannels = React.useMemo(
    () =>
      (channelsQuery.data ?? [])
        .filter((channel) => channel.isMember)
        .sort((left, right) => left.name.localeCompare(right.name)),
    [channelsQuery.data],
  );
  const [channelSelection, setChannelSelection] = React.useState<string | null>(
    initialChannelId ?? null,
  );
  const channelId =
    channelSelection ??
    (memberChannels.some((channel) => channel.id === initialChannelId)
      ? (initialChannelId ?? null)
      : (memberChannels[0]?.id ?? null));
  const memberChannelIds = React.useMemo(
    () => memberChannels.map((channel) => channel.id).sort(),
    [memberChannels],
  );
  const providerCatalog = useCodingSessionProviderCatalog(memberChannelIds);

  const scopeId = channelId ?? "unscoped";
  const {
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
  });
  const draftBytes = new TextEncoder().encode(draftText).byteLength;
  const draftOverCap =
    draftBytes > MAX_CODING_SESSION_LIFECYCLE_INITIAL_TURN_BYTES;
  const canSubmit =
    !isPublishing &&
    transaction === null &&
    channelId !== null &&
    selectedTarget !== null &&
    isNewCodingSessionTargetReady(selectedTarget) &&
    !draftOverCap;

  const handleSubmit = React.useCallback(() => {
    if (!canSubmit || !selectedTarget) return;
    void submit({
      target: selectedTarget,
      model:
        effectiveModel && effectiveModel.length > 0 ? effectiveModel : null,
      title: title.trim().length > 0 ? title.trim() : null,
      initialTurn: draftText.trim().length > 0 ? draftText : null,
      workdir: workdir.trim().length > 0 ? workdir.trim() : null,
    }).then(() => {
      clearDraft();
    });
  }, [
    canSubmit,
    clearDraft,
    draftText,
    effectiveModel,
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
        <h1 className="text-sm font-semibold">New coding session</h1>
      </header>

      <div className="mx-auto flex w-full max-w-2xl flex-col gap-5 px-5 py-7 sm:px-8">
        <NewCodingSessionChannelPicker
          channels={memberChannels}
          disabled={transaction !== null}
          onChange={(next) => {
            setChannelSelection(next);
            setTargetSelection({ key: null, explicit: false });
            setModelSelection({ value: null, explicit: false });
          }}
          value={channelId}
        />

        <NewCodingSessionProviderPicker
          disabled={transaction !== null}
          model={effectiveModel}
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
          value={workdir}
        />

        <div className="flex flex-col gap-2">
          <label
            className="text-xs font-medium text-muted-foreground"
            htmlFor="coding-session-title"
          >
            Title <span className="font-normal">(optional)</span>
          </label>
          <Input
            data-testid="new-coding-session-title"
            disabled={transaction !== null}
            id="coding-session-title"
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
          <ProviderLoginNeeded runtime={failedRuntime} />
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

        {targets.length === 0 && channelId !== null && hostPhase === "idle" ? (
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
                onClick={startFresh}
                type="button"
                variant="ghost"
              >
                Start fresh
              </Button>
              <Button
                data-testid="new-coding-session-retry"
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
  onModelChange,
  onTargetChange,
  selectedTarget,
  targets,
}: {
  disabled: boolean;
  model: string | null;
  onModelChange: (model: string) => void;
  onTargetChange: (selectionKey: string) => void;
  selectedTarget: NewCodingSessionTarget | null;
  targets: readonly NewCodingSessionTarget[];
}) {
  const models = selectedTarget?.provider.allowedModels ?? [];
  // One remediation line per unavailable runtime — disabled options say what
  // is wrong, but an <option> cannot carry a full sentence.
  const unavailableHints = [
    ...new Set(
      targets.flatMap((target) =>
        !isNewCodingSessionTargetReady(target) && target.availability?.hint
          ? [target.availability.hint]
          : [],
      ),
    ),
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
            const suffix =
              target.availability?.state === "needs_auth"
                ? " (sign-in needed)"
                : target.availability?.state === "missing"
                  ? " (not installed)"
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
        {unavailableHints.map((hint) => (
          <p className="text-2xs text-muted-foreground" key={hint}>
            {hint}
          </p>
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
  runtime,
}: {
  runtime?: { runtime: string; label?: string } | null;
}) {
  const remediation = codingSessionAuthRemediation(runtime);
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
    </div>
  );
}
