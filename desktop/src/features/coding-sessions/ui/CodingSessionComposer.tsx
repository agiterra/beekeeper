import * as React from "react";
import { CircleHelp, ShieldCheck, Square } from "lucide-react";

import {
  createCodingSessionCommandId,
  publishCodingSessionCommand,
  publishCodingSessionInterrupt,
  type CodingSessionCommandTarget,
} from "@/features/coding-sessions/lib/codingSessionCommand";
import {
  getCodingSessionComposerState,
  shouldSubmitCodingSessionComposerKey,
} from "@/features/coding-sessions/lib/codingSessionComposerModel";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/textarea";
import { cn } from "@/shared/lib/cn";

type CodingSessionComposerProps = {
  canInterrupt: boolean;
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
    status: {
      kind: "working" | "idle" | "unknown";
      label: string;
    };
  };
  immersive?: boolean;
  isMember: boolean;
  isWorking: boolean;
  layout?: "inline" | "stacked";
  target: CodingSessionCommandTarget;
  variant?: "panel" | "floating";
};

/** Composer for steering a selected governed coding-session generation. */
export function CodingSessionComposer({
  canInterrupt,
  canSteer = true,
  channelId,
  controlContext,
  immersive = false,
  isMember,
  isWorking,
  layout = "inline",
  target,
  variant = "panel",
}: CodingSessionComposerProps) {
  const [text, setText] = React.useState("");
  const [pendingAction, setPendingAction] = React.useState<
    "send" | "interrupt" | null
  >(null);
  const [error, setError] = React.useState<string | null>(null);
  const isSending = pendingAction !== null;
  const state = getCodingSessionComposerState({ isMember, isWorking, text });
  const canSubmitText = state.canSend && (!immersive || !isWorking || canSteer);
  const editorDisabled =
    !isMember || isSending || (immersive && isWorking && !canSteer);

  const submit = React.useCallback(async () => {
    if (!canSubmitText || isSending) return;
    setPendingAction("send");
    setError(null);
    try {
      await publishCodingSessionCommand({
        channelId,
        commandId: createCodingSessionCommandId(),
        target,
        text: text.trim(),
      });
      setText("");
    } catch (submitError) {
      setError(
        submitError instanceof Error
          ? submitError.message
          : "Unable to send coding-session command.",
      );
    } finally {
      setPendingAction(null);
    }
  }, [canSubmitText, channelId, isSending, target, text]);

  const handlePrimaryAction = React.useCallback(async () => {
    await submit();
  }, [submit]);

  const handleStop = React.useCallback(async () => {
    if (!isMember || !canInterrupt || isSending) return;
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
  }, [canInterrupt, channelId, isMember, isSending, target]);

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
      {error ? <p className="mb-2 text-sm text-destructive">{error}</p> : null}
      <div
        className={cn(
          "flex gap-2",
          !immersive && (layout === "stacked" ? "flex-col" : "items-end"),
        )}
      >
        <Textarea
          aria-label="Coding-session instruction"
          className={cn(
            "min-h-16 min-w-0 flex-1 resize-y",
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
            immersive && isWorking && !canSteer
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
              disabled={!state.canSend || isSending}
              onClick={() => void handlePrimaryAction()}
              type="button"
            >
              {state.sendLabel}
            </Button>
            {state.showStopAction ? (
              <Button
                data-testid="coding-session-composer-stop"
                disabled={!isMember || !canInterrupt || isSending}
                onClick={() => void handleStop()}
                title={
                  canInterrupt
                    ? "Stop the current turn"
                    : "Stop is unavailable for this session."
                }
                type="button"
                variant="outline"
              >
                Stop
              </Button>
            ) : null}
          </div>
        ) : null}
      </div>
      {immersive ? (
        <ImmersiveCodingSessionControlDeck
          canInterrupt={canInterrupt}
          canSteer={canSteer}
          context={controlContext}
          isMember={isMember}
          isWorking={isWorking}
          onInterrupt={() => void handleStop()}
          onSteer={() => void handlePrimaryAction()}
          pendingAction={pendingAction}
          steerDisabled={!canSubmitText || isSending}
        />
      ) : null}
    </div>
  );
}

function ImmersiveCodingSessionControlDeck({
  canInterrupt,
  canSteer,
  context,
  isMember,
  isWorking,
  onInterrupt,
  onSteer,
  pendingAction,
  steerDisabled,
}: {
  canInterrupt: boolean;
  canSteer: boolean;
  context: CodingSessionComposerProps["controlContext"];
  isMember: boolean;
  isWorking: boolean;
  onInterrupt: () => void;
  onSteer: () => void;
  pendingAction: "send" | "interrupt" | null;
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
        {isWorking ? (
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
              disabled={!isMember || !canInterrupt || pendingAction !== null}
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
          <Button
            data-testid="coding-session-composer-primary"
            disabled={steerDisabled}
            onClick={onSteer}
            size="sm"
            type="button"
          >
            {pendingAction === "send" ? "Sending…" : "Send"}
          </Button>
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
  const status = context?.status ?? {
    kind: isWorking ? ("working" as const) : ("unknown" as const),
    label: isWorking ? "Working" : "Status unknown",
  };
  return (
    <span
      className={cn(
        "inline-flex shrink-0 items-center gap-1.5 rounded-md px-1.5 py-1 text-xs font-medium",
        status.kind === "working" &&
          "bg-blue-500/10 text-blue-700 dark:text-blue-300",
        status.kind === "idle" &&
          "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
        status.kind === "unknown" && "text-muted-foreground",
      )}
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
