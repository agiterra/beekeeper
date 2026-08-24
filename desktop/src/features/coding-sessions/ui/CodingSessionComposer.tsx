import * as React from "react";
import { CircleHelp, ShieldCheck, Square } from "lucide-react";

import {
  createCodingSessionCommandId,
  publishCodingSessionCommand,
  publishCodingSessionInterrupt,
  type CodingSessionCommandTarget,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  createCodingSessionLifecycleCommandId,
  publishCodingSessionResume,
} from "@/features/coding-sessions/lib/codingSessionLifecycleCommand";
import { useCodingSessionResumeSettle } from "@/features/coding-sessions/hooks/useCodingSessionResumeSettle";
import { useCodingSessionTurnRefusal } from "@/features/coding-sessions/hooks/useCodingSessionTurnRefusal";
import { useEndCodingSessionDialog } from "@/features/coding-sessions/hooks/useEndCodingSessionDialog";
import { restoreCodingSessionDraft } from "@/features/coding-sessions/lib/codingSessionTurnRefusal";
import { codingSessionWorkspaceStatusDetail } from "@/features/coding-sessions/lib/codingSessionWorkspaceModel";
import type {
  CodingSessionStatus,
  CodingSessionWorkspaceStatus,
} from "@/features/coding-sessions/lib/codingSessionTypes";
import {
  getCodingSessionComposerState,
  shouldSubmitCodingSessionComposerKey,
} from "@/features/coding-sessions/lib/codingSessionComposerModel";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";

type CodingSessionComposerProps = {
  canInterrupt: boolean;
  canControl?: boolean;
  authorityReason?: string | null;
  canSteer?: boolean;
  channelId: string;
  controlContext?: {
    capabilities: {
      threadTurnStart: boolean;
      threadTurnInterrupt: boolean;
      threadSteer: boolean;
      context: boolean;
      diff: boolean;
      plan: boolean;
    } | null;
    model: string | null;
    providerLabel: string | null;
    runtimeLabel: string | null;
    status: CodingSessionWorkspaceStatus;
  };
  immersive?: boolean;
  isMember: boolean;
  isWorking: boolean;
  isUngovernedSession?: boolean;
  lifecycleStatus?: CodingSessionStatus;
  layout?: "inline" | "stacked";
  /**
   * Observe the live draft. The umbrella composer uses this to follow a typed
   * `@handle` with the participant selector; nothing here depends on it.
   */
  /**
   * Opens the add-provider flow. A stopped execution is deliberately not
   * resumable, so the only honest way forward is another provider — the banner
   * that says the execution ended offers it rather than leaving the operator
   * hunting for a Resume button that will never exist (§2 item 42).
   */
  onAddProvider?: () => void;
  onTextChange?: (text: string) => void;
  /**
   * Last transform applied to the draft before it is published — the umbrella
   * composer strips a leading `@handle` that already did its routing. The
   * *prepared* text is what gates Send, so a message that prepares to nothing
   * (a bare handle) is not sendable rather than sent empty.
   */
  prepareText?: (text: string) => string;
  /**
   * Editable text staged into the editor (e.g. a handoff provenance block).
   * Applied once per `id`; the person keeps full control of the text after.
   */
  prefill?: { id: string; text: string } | null;
  providerAuthorityPubkey?: string | null;
  /** Display name for the stop-execution confirm; falls back to "this session". */
  sessionLabel?: string | null;
  target: CodingSessionCommandTarget;
  variant?: "panel" | "floating";
};

/** Composer for steering a selected governed coding-session generation. */
export function CodingSessionComposer({
  authorityReason = null,
  canInterrupt,
  canControl = true,
  canSteer = true,
  channelId,
  controlContext,
  immersive = false,
  isMember,
  isWorking,
  isUngovernedSession = false,
  lifecycleStatus,
  layout = "inline",
  onAddProvider,
  onTextChange,
  prepareText,
  prefill = null,
  providerAuthorityPubkey = null,
  sessionLabel = null,
  target,
  variant = "panel",
}: CodingSessionComposerProps) {
  const [text, setText] = React.useState(prefill?.text ?? "");
  const [appliedPrefillId, setAppliedPrefillId] = React.useState<string | null>(
    prefill?.id ?? null,
  );
  if (prefill && prefill.id !== appliedPrefillId) {
    // Render-time state adjustment: a new prefill replaces the draft exactly
    // once, then the editor is the person's again.
    setAppliedPrefillId(prefill.id);
    setText(prefill.text);
  }
  const [pendingAction, setPendingAction] = React.useState<
    "send" | "interrupt" | "resume" | "stop" | null
  >(null);
  const [error, setError] = React.useState<string | null>(null);
  // A reconnect is only finished when the provider's receipt names the
  // generation it resumed into; until then this composer is as busy as it is
  // during a turn.
  const {
    begin: beginResume,
    error: resumeError,
    fail: failResume,
    isPending: isResuming,
    watcher: resumeWatcher,
  } = useCodingSessionResumeSettle({
    channelId,
    providerAuthorityPubkey,
  });
  const restoreRefusedDraft = React.useCallback((refused: string) => {
    setText((current) => restoreCodingSessionDraft(current, refused));
  }, []);
  // A sent turn the provider refuses (an operator it has not granted) answers
  // with a receipt and nothing else; without this watch the message the person
  // typed disappears with no explanation at all.
  const {
    error: turnRefusalError,
    watch: watchTurn,
    watcher: turnRefusalWatcher,
  } = useCodingSessionTurnRefusal({
    channelId,
    providerAuthorityPubkey,
    restoreDraft: restoreRefusedDraft,
  });
  const isSending = pendingAction !== null || isResuming;
  const visibleError = error ?? resumeError ?? turnRefusalError;
  React.useEffect(() => {
    onTextChange?.(text);
  }, [onTextChange, text]);
  const preparedText = (prepareText ? prepareText(text) : text).trim();
  const state = getCodingSessionComposerState({
    isMember,
    isWorking,
    text: preparedText,
  });
  const isDisconnected = lifecycleStatus === "disconnected";
  const isEnded = lifecycleStatus === "stopped";
  // Coordination, not the newest report: a provider whose lease has lapsed
  // answers nothing, however recently it said `idle` (§2 item 41). Sending
  // into that is how three stops sat unanswered for two hours.
  const unreachableStatus =
    controlContext?.status.kind === "unknown" &&
    controlContext.status.attention === "unreachable"
      ? controlContext.status
      : null;
  const unreachableDetail =
    unreachableStatus === null
      ? null
      : codingSessionWorkspaceStatusDetail(unreachableStatus);
  const isUnavailable = isDisconnected || isEnded || unreachableStatus !== null;
  const canSubmitText =
    canControl &&
    !isUnavailable &&
    state.canSend &&
    (!immersive || !isWorking || canSteer);
  const editorDisabled =
    !canControl ||
    !isMember ||
    isSending ||
    isUnavailable ||
    (immersive && isWorking && !canSteer);

  const submit = React.useCallback(async () => {
    if (!canSubmitText || isSending) return;
    // Keep the person's own words, not the prepared wire text: a refusal has
    // to hand back exactly what they typed, routing handle and all.
    const draft = text;
    setPendingAction("send");
    setError(null);
    try {
      const published = await publishCodingSessionCommand({
        channelId,
        commandId: createCodingSessionCommandId(),
        target,
        text: preparedText,
      });
      setText("");
      // Acceptance by the relay is not consent from the provider. The receipt
      // that refuses this turn is keyed to this command id and nothing else.
      watchTurn({ commandId: published.commandId, draft });
    } catch (submitError) {
      setError(
        submitError instanceof Error
          ? submitError.message
          : "Unable to send coding-session command.",
      );
    } finally {
      setPendingAction(null);
    }
  }, [
    canSubmitText,
    channelId,
    isSending,
    preparedText,
    target,
    text,
    watchTurn,
  ]);

  const handlePrimaryAction = React.useCallback(async () => {
    await submit();
  }, [submit]);

  const handleStop = React.useCallback(async () => {
    if (!canControl || !isMember || !canInterrupt || isSending) return;
    setPendingAction("interrupt");
    setError(null);
    try {
      await publishCodingSessionInterrupt({
        channelId,
        commandId: createCodingSessionCommandId(),
        target,
      });
    } catch (interruptError) {
      setError(
        interruptError instanceof Error
          ? interruptError.message
          : "Unable to interrupt coding-session turn.",
      );
    } finally {
      setPendingAction(null);
    }
  }, [canControl, canInterrupt, channelId, isMember, isSending, target]);

  const handleResume = React.useCallback(async () => {
    if (
      !canControl ||
      !isMember ||
      !isDisconnected ||
      isSending ||
      !providerAuthorityPubkey
    ) {
      return;
    }
    setError(null);
    // The commandId is the only handle on the receipt that answers this
    // resume, so it is kept rather than discarded, and the wait is armed
    // *before* the publish — the settle watcher owns the pending state from
    // here on.
    const commandId = createCodingSessionLifecycleCommandId();
    beginResume(commandId);
    try {
      await publishCodingSessionResume({
        channelId,
        commandId,
        target,
        providerAuthorityPubkey,
      });
    } catch (publishError) {
      failResume(
        publishError instanceof Error
          ? publishError.message
          : "Unable to reconnect the coding session.",
      );
    }
  }, [
    beginResume,
    channelId,
    canControl,
    failResume,
    isDisconnected,
    isMember,
    isSending,
    providerAuthorityPubkey,
    target,
  ]);

  // A durable execution stop is distinct from closing the umbrella session.
  // Route it through a confirm instead of publishing on the raw click.
  const endDialog = useEndCodingSessionDialog();
  const canSessionStop =
    canControl && isMember && providerAuthorityPubkey !== null && !isEnded;
  const requestSessionEnd = React.useCallback(() => {
    if (!canSessionStop || !providerAuthorityPubkey) return;
    endDialog.requestEnd({
      label: sessionLabel?.trim() || "this session",
      channelId,
      stops: [{ target, providerAuthorityPubkey }],
      providerUnanswered: unreachableStatus !== null,
    });
  }, [
    canSessionStop,
    channelId,
    endDialog,
    providerAuthorityPubkey,
    sessionLabel,
    target,
    unreachableStatus,
  ]);

  return (
    <div
      className={cn(
        variant === "floating"
          ? "rounded-3xl border border-border/70 bg-background/95 p-3 shadow-lg backdrop-blur-xl"
          : "border-t border-border/70 bg-background px-3 py-3",
      )}
      data-layout={layout}
      data-mode={immersive ? "immersive" : "compact"}
      data-testid="coding-session-composer"
    >
      {state.showAuthorityFailure ? (
        <p
          className="mb-2 text-sm text-muted-foreground"
          data-testid="coding-session-composer-membership-failure"
        >
          Join this channel for native control. Compatibility control also
          requires an allowlisted operator.
        </p>
      ) : null}
      {isUngovernedSession ? (
        <p
          className="mb-2 text-xs text-muted-foreground"
          data-testid="coding-session-ungoverned-hint"
        >
          ungoverned — adopt to govern.
        </p>
      ) : null}
      {!canControl ? (
        <p
          className="mb-2 rounded-xl border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-sm text-foreground"
          data-testid="coding-session-composer-authority-gated"
        >
          {authorityReason ??
            "Only the session founder can control this session."}
        </p>
      ) : null}
      {/* A refused reconnect (a stale generation, a provider that never
          answered) or a refused turn (an operator this provider has not
          granted) is a signed fact about this composer's own command — it
          shares the composer's error line rather than disappearing. */}
      {visibleError ? (
        <p
          className="mb-2 text-sm text-destructive"
          data-testid="coding-session-composer-error"
        >
          {visibleError}
        </p>
      ) : null}
      {unreachableStatus !== null ? (
        <div
          className="mb-2 flex items-center justify-between gap-3 rounded-xl border border-border/70 px-3 py-2"
          data-testid="coding-session-composer-unreachable"
        >
          <p className="text-sm text-muted-foreground">
            {`No provider is answering for this execution${
              unreachableDetail === null ? "" : ` — ${unreachableDetail}`
            }. Add a provider to the session to continue the work.`}
          </p>
          {onAddProvider ? (
            <Button
              className="shrink-0"
              data-testid="coding-session-composer-add-provider"
              onClick={onAddProvider}
              size="sm"
              type="button"
              variant="outline"
            >
              Add provider
            </Button>
          ) : null}
        </div>
      ) : isDisconnected ? (
        <div className="mb-2 flex items-center justify-between gap-3 rounded-xl border border-border/70 px-3 py-2">
          <p className="text-sm text-muted-foreground">
            This provider execution is disconnected.
          </p>
          <div className="flex shrink-0 items-center gap-2">
            <Button
              data-testid="coding-session-composer-resume"
              disabled={
                !canControl ||
                !isMember ||
                !providerAuthorityPubkey ||
                isSending
              }
              onClick={() => void handleResume()}
              size="sm"
              type="button"
            >
              {isResuming ? "Reconnecting…" : "Reconnect"}
            </Button>
            <Button
              data-testid="coding-session-composer-session-stop"
              disabled={!canSessionStop || isSending}
              onClick={requestSessionEnd}
              size="sm"
              type="button"
              variant="outline"
            >
              Stop execution
            </Button>
          </div>
        </div>
      ) : isEnded ? (
        <div
          className="mb-2 flex items-center justify-between gap-3 rounded-xl border border-border/70 px-3 py-2"
          data-testid="coding-session-composer-ended"
        >
          <p className="text-sm text-muted-foreground">
            This provider execution has ended and cannot be resumed.
          </p>
          {onAddProvider ? (
            <Button
              className="shrink-0"
              data-testid="coding-session-composer-add-provider"
              onClick={onAddProvider}
              size="sm"
              type="button"
              variant="outline"
            >
              Add provider
            </Button>
          ) : null}
        </div>
      ) : null}
      <div
        className={cn(
          "flex gap-2",
          !immersive && (layout === "stacked" ? "flex-col" : "items-end"),
        )}
      >
        <Textarea
          aria-label="Coding-session instruction"
          className={cn(
            "min-h-16 min-w-0 flex-1 resize-y text-foreground caret-primary",
            immersive && "min-h-12",
            variant === "floating" && "border-0 bg-transparent shadow-none",
          )}
          disabled={editorDisabled}
          onChange={(event) => setText(event.target.value)}
          onKeyDown={(event) => {
            if (shouldSubmitCodingSessionComposerKey(event)) {
              event.preventDefault();
              if (canSubmitText) void submit();
            }
          }}
          placeholder={
            isEnded
              ? "This execution has ended."
              : isDisconnected
                ? "Reconnect this execution to continue…"
                : immersive && isWorking && !canSteer
                  ? "Current turn in progress…"
                  : "Steer this coding session…"
          }
          value={text}
        />
        {!immersive ? (
          <div
            className={cn(
              "flex shrink-0 flex-wrap items-center justify-end gap-2",
              layout === "stacked" && "w-full",
            )}
            data-testid="coding-session-composer-actions"
          >
            <Button
              data-testid="coding-session-composer-primary"
              disabled={!canSubmitText || isSending}
              onClick={() => void handlePrimaryAction()}
              type="button"
            >
              {state.sendLabel}
            </Button>
            {state.showStopAction ? (
              <Button
                data-testid="coding-session-composer-stop"
                disabled={
                  !canControl || !isMember || !canInterrupt || isSending
                }
                onClick={() => void handleStop()}
                title={
                  canInterrupt
                    ? "Interrupt only the current turn"
                    : "Current-turn interrupt is unavailable for this provider."
                }
                type="button"
                variant="outline"
              >
                Interrupt
              </Button>
            ) : null}
            {canSessionStop && !isDisconnected ? (
              <Button
                data-testid="coding-session-composer-session-stop"
                disabled={isSending}
                onClick={requestSessionEnd}
                title="Stop this provider execution; the session stays open."
                type="button"
                variant="outline"
              >
                Stop execution
              </Button>
            ) : null}
          </div>
        ) : null}
      </div>
      {immersive ? (
        <ImmersiveCodingSessionControlDeck
          canInterrupt={canInterrupt}
          canControl={canControl}
          canSessionStop={canSessionStop}
          canSteer={canSteer}
          context={controlContext}
          isMember={isMember}
          isWorking={isWorking}
          isUnavailable={isUnavailable}
          onInterrupt={() => void handleStop()}
          onSessionStop={requestSessionEnd}
          onSteer={() => void handlePrimaryAction()}
          pendingAction={pendingAction}
          steerDisabled={!canSubmitText || isSending}
        />
      ) : null}
      {endDialog.dialog}
      {resumeWatcher}
      {turnRefusalWatcher}
    </div>
  );
}

function ImmersiveCodingSessionControlDeck({
  canInterrupt,
  canControl,
  canSessionStop,
  canSteer,
  context,
  isMember,
  isWorking,
  isUnavailable,
  onInterrupt,
  onSessionStop,
  onSteer,
  pendingAction,
  steerDisabled,
}: {
  canInterrupt: boolean;
  canControl: boolean;
  canSessionStop: boolean;
  canSteer: boolean;
  context: CodingSessionComposerProps["controlContext"];
  isMember: boolean;
  isWorking: boolean;
  isUnavailable: boolean;
  onInterrupt: () => void;
  onSessionStop: () => void;
  onSteer: () => void;
  pendingAction: "send" | "interrupt" | "resume" | "stop" | null;
  steerDisabled: boolean;
}) {
  const availableCapabilities = context
    ? capabilityLabels(context.capabilities)
    : [];
  const capabilityDescription =
    availableCapabilities.length > 0
      ? `Provider capabilities: ${availableCapabilities.join(", ")}.`
      : "Live steer and interrupt capabilities have not been declared.";
  return (
    <div
      className="mt-1 flex h-10 min-w-0 items-center gap-2 overflow-hidden border-t border-border/60 px-1 pt-1 whitespace-nowrap"
      data-testid="coding-session-control-deck"
    >
      <ControlDeckStatus context={context} isWorking={isWorking} />
      <ControlDeckIdentity context={context} />
      <span
        className="inline-flex shrink-0 items-center gap-1 rounded-md px-1.5 py-1 text-xs text-muted-foreground"
        data-testid="coding-session-control-authority"
        title="Signed channel membership authorizes command publication; provider capability gates still apply."
      >
        <ShieldCheck aria-hidden className="size-3.5" />
        <span aria-hidden>{isMember ? "Member" : "View only"}</span>
        <span className="sr-only">
          {isMember ? "Signed channel member" : "View only"}
        </span>
      </span>
      <span
        className="inline-flex size-7 shrink-0 items-center justify-center rounded-md text-muted-foreground"
        data-testid="coding-session-control-capabilities"
        title={`${controlProvenance(context)} ${capabilityDescription}`}
      >
        <CircleHelp aria-hidden className="size-3.5" />
        <span className="sr-only">
          Session control provenance. {capabilityDescription}
        </span>
      </span>
      <div
        className="ml-auto flex shrink-0 items-center gap-1.5"
        data-testid="coding-session-composer-actions"
      >
        {isUnavailable ? null : isWorking ? (
          <>
            {canSteer ? (
              <Button
                data-testid="coding-session-composer-steer"
                disabled={steerDisabled}
                onClick={onSteer}
                size="sm"
                type="button"
                variant="outline"
              >
                {pendingAction === "send" ? "Steering…" : "Steer"}
              </Button>
            ) : null}
            <Button
              data-testid="coding-session-composer-interrupt"
              disabled={
                !canControl ||
                !isMember ||
                !canInterrupt ||
                pendingAction !== null
              }
              onClick={onInterrupt}
              size="sm"
              title={
                canInterrupt
                  ? "Interrupt only the current turn"
                  : "Current-turn interrupt is unavailable for this provider."
              }
              type="button"
              variant="destructive"
            >
              <Square className="fill-current" />
              {pendingAction === "interrupt" ? "Interrupting…" : "Interrupt"}
            </Button>
          </>
        ) : (
          <>
            <Button
              data-testid="coding-session-composer-primary"
              disabled={steerDisabled}
              onClick={onSteer}
              size="sm"
              type="button"
            >
              {pendingAction === "send" ? "Sending…" : "Send"}
            </Button>
            {canSessionStop ? (
              <Button
                data-testid="coding-session-composer-session-stop"
                disabled={pendingAction !== null}
                onClick={onSessionStop}
                size="sm"
                title="Stop this provider execution; the session stays open."
                type="button"
                variant="outline"
              >
                Stop execution
              </Button>
            ) : null}
          </>
        )}
      </div>
    </div>
  );
}

function ControlDeckStatus({
  context,
  isWorking,
}: {
  context: CodingSessionComposerProps["controlContext"];
  isWorking: boolean;
}) {
  const status: CodingSessionWorkspaceStatus =
    context?.status ??
    (isWorking
      ? { kind: "working", label: "Working" }
      : { kind: "unknown", label: "Status unknown" });
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-md px-1.5 py-1 text-xs font-medium",
        status.kind === "working" &&
          "bg-blue-500/10 text-blue-700 dark:text-blue-300",
        status.kind === "idle" &&
          "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
        status.kind === "unknown" &&
          (status.attention
            ? // Lifecycle says this execution is not usable; the deck must not
              // read as calm while the banner above offers Reconnect.
              "bg-destructive/10 text-destructive"
            : "text-muted-foreground"),
      )}
      data-attention={
        status.kind === "unknown" ? (status.attention ?? undefined) : undefined
      }
      data-status={status.kind}
      data-testid="coding-session-control-status"
    >
      <span
        aria-hidden
        className={cn(
          "size-1.5 rounded-full bg-current",
          status.kind === "working" && "motion-safe:animate-pulse",
        )}
      />
      {status.label}
    </span>
  );
}

function ControlDeckIdentity({
  context,
}: {
  context: CodingSessionComposerProps["controlContext"];
}) {
  const values = [
    ["provider", context?.providerLabel ?? context?.runtimeLabel],
    ["model", context?.model],
  ].filter((entry): entry is [string, string] => Boolean(entry[1]));
  if (values.length === 0) return null;
  return (
    <span
      className="inline-flex min-w-0 items-center gap-1.5 overflow-hidden text-xs text-foreground/80"
      data-testid="coding-session-control-identity"
      title={controlProvenance(context)}
    >
      {values.map(([kind, value], index) => (
        <React.Fragment key={kind}>
          {index > 0 ? (
            <span aria-hidden className="text-border">
              /
            </span>
          ) : null}
          <span className="max-w-36 truncate">{value}</span>
        </React.Fragment>
      ))}
    </span>
  );
}

function controlProvenance(
  context: CodingSessionComposerProps["controlContext"],
): string {
  if (!context) return "Signed session target.";
  return [
    context.providerLabel ? `Provider: ${context.providerLabel}.` : null,
    context.runtimeLabel ? `Runtime: ${context.runtimeLabel}.` : null,
    context.model ? `Model: ${context.model}.` : null,
  ]
    .filter((value): value is string => value !== null)
    .join(" ");
}

function capabilityLabels(
  capabilities: NonNullable<
    CodingSessionComposerProps["controlContext"]
  >["capabilities"],
): string[] {
  if (!capabilities) return [];
  return [
    capabilities.threadTurnStart ? "Turns" : null,
    capabilities.threadSteer ? "Steer" : null,
    capabilities.threadTurnInterrupt ? "Interrupt" : null,
    capabilities.context ? "Context" : null,
    capabilities.diff ? "Diff" : null,
    capabilities.plan ? "Plan" : null,
  ].filter((value): value is string => value !== null);
}
