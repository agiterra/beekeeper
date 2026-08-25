import { ArrowUp, Bot, Ellipsis, ShieldCheck, Square } from "lucide-react";

import type { CodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import {
  codingSessionModelDisplayName,
  codingSessionTraitsSummary,
} from "@/features/coding-sessions/lib/codingSessionModelDisplay";
import { formatCodingSessionModelDisplay } from "@/features/coding-sessions/lib/codingSessionLabels";
import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

export type CodingSessionComposerControlContext = {
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

type CodingSessionComposerDeckProps = {
  authorityReason: string | null;
  canControl: boolean;
  canInterrupt: boolean;
  canSessionStop: boolean;
  canSteer: boolean;
  context: CodingSessionComposerControlContext | undefined;
  contextWindow: CodingSessionContextWindow | null;
  isMember: boolean;
  isSending: boolean;
  isUnavailable: boolean;
  isUngovernedSession: boolean;
  isWorking: boolean;
  onInterrupt: () => void;
  onPrimary: () => void;
  onSessionStop: () => void;
  pendingAction: "send" | "interrupt" | "resume" | "stop" | null;
  primaryDisabled: boolean;
};

/** The compact, provider-neutral footer inside the full-screen composer. */
export function CodingSessionComposerDeck({
  authorityReason,
  canControl,
  canInterrupt,
  canSessionStop,
  canSteer,
  context,
  contextWindow,
  isMember,
  isSending,
  isUnavailable,
  isUngovernedSession,
  isWorking,
  onInterrupt,
  onPrimary,
  onSessionStop,
  pendingAction,
  primaryDisabled,
}: CodingSessionComposerDeckProps) {
  const model = context?.model
    ? formatCodingSessionModelDisplay(context.model)
    : null;
  const traits = model
    ? codingSessionTraitsSummary({
        thinking: model.thinking,
        context: model.context,
      })
    : null;
  const modelName = model ? codingSessionModelDisplayName(model.model) : null;
  const providerName =
    context?.providerLabel ?? context?.runtimeLabel ?? "Session provider";
  const identityLabel = [providerName, modelName]
    .filter((value): value is string => Boolean(value))
    .join(" · ");
  const accessLabel = canControl && isMember ? "Can control" : "View only";
  const availableCapabilities = context
    ? capabilityLabels(context.capabilities)
    : [];

  return (
    <div
      className="flex min-h-14 min-w-0 items-center gap-2 px-4 pb-3"
      data-testid="coding-session-control-deck"
    >
      <div className="flex min-w-0 items-center text-xs text-muted-foreground">
        <Popover>
          <PopoverTrigger asChild>
            <button
              aria-label="Show execution identity"
              className="flex min-w-0 items-center gap-2 rounded-lg py-1.5 pr-3 text-foreground/75 transition-colors hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              data-testid="coding-session-control-identity"
              type="button"
            >
              <span className="grid size-5 shrink-0 place-items-center rounded-full border border-border/80 bg-muted/40">
                <Bot aria-hidden className="size-3" />
              </span>
              <span className="max-w-52 truncate">{identityLabel}</span>
            </button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-72" side="top">
            <p className="text-sm font-medium">Execution identity</p>
            <dl className="mt-3 grid gap-2 text-xs">
              {context?.providerLabel ? (
                <ComposerDefinition
                  label="Provider"
                  value={context.providerLabel}
                />
              ) : null}
              {context?.runtimeLabel ? (
                <ComposerDefinition
                  label="Runtime"
                  value={context.runtimeLabel}
                />
              ) : null}
              {modelName ? (
                <ComposerDefinition label="Model" value={modelName} />
              ) : null}
              {traits ? (
                <ComposerDefinition label="Model traits" value={traits} />
              ) : null}
              <ComposerDefinition
                label="Capabilities"
                value={
                  availableCapabilities.length > 0
                    ? availableCapabilities.join(", ")
                    : "Not declared"
                }
              />
            </dl>
            <p className="mt-3 text-xs text-muted-foreground">
              This identifies the signed execution. Its model and traits are
              fixed for this execution.
            </p>
          </PopoverContent>
        </Popover>

        <ComposerDeckSeparator />

        <Popover>
          <PopoverTrigger asChild>
            <button
              className="inline-flex shrink-0 items-center gap-1.5 rounded-lg px-3 py-1.5 transition-colors hover:bg-muted/40 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
              data-testid="coding-session-control-authority"
              type="button"
            >
              <ShieldCheck aria-hidden className="size-3.5" />
              {accessLabel}
            </button>
          </PopoverTrigger>
          <PopoverContent align="start" className="w-72" side="top">
            <p className="text-sm font-medium">{accessLabel}</p>
            <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
              {accessExplanation({
                authorityReason,
                canControl,
                isMember,
                isUngovernedSession,
              })}
            </p>
            <p className="mt-3 text-xs text-muted-foreground">
              Access comes from signed channel membership and the provider’s
              operator grant. It is not a local preference.
            </p>
          </PopoverContent>
        </Popover>

        {traits ? (
          <>
            <ComposerDeckSeparator />
            <span
              className="shrink-0 px-3 py-1.5"
              data-testid="coding-session-control-traits"
              title="Fixed model traits for this execution"
            >
              {traits}
            </span>
          </>
        ) : null}
      </div>

      <div
        className="ml-auto flex shrink-0 items-center gap-2"
        data-testid="coding-session-composer-actions"
      >
        {canSessionStop && !isUnavailable && !isWorking ? (
          <Popover>
            <PopoverTrigger asChild>
              <button
                aria-label="More execution actions"
                className="grid size-8 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                data-testid="coding-session-composer-more"
                type="button"
              >
                <Ellipsis aria-hidden className="size-4" />
              </button>
            </PopoverTrigger>
            <PopoverContent align="end" className="w-64 p-2" side="top">
              <button
                className="flex w-full items-center gap-2 rounded-lg px-3 py-2 text-left text-sm text-destructive transition-colors hover:bg-destructive/10 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
                disabled={isSending}
                onClick={onSessionStop}
                type="button"
              >
                <Square aria-hidden className="size-3.5" />
                <span>
                  <span className="block font-medium">Stop execution</span>
                  <span className="block text-xs text-muted-foreground">
                    Terminal; confirmation required
                  </span>
                </span>
              </button>
            </PopoverContent>
          </Popover>
        ) : null}

        {contextWindow ? (
          <CodingSessionContextMeter usage={contextWindow} />
        ) : null}

        {!isUnavailable ? (
          <button
            aria-label={
              pendingAction === "send"
                ? "Sending"
                : isWorking
                  ? canSteer
                    ? "Steer current turn"
                    : "Queue next turn"
                  : "Send message"
            }
            className="grid size-9 place-items-center rounded-full bg-primary text-primary-foreground shadow-sm transition-transform enabled:hover:scale-105 disabled:cursor-not-allowed disabled:opacity-30"
            data-testid={
              isWorking
                ? canSteer
                  ? "coding-session-composer-steer"
                  : "coding-session-composer-queue"
                : "coding-session-composer-primary"
            }
            disabled={primaryDisabled}
            onClick={onPrimary}
            title={
              isWorking
                ? canSteer
                  ? "Steer current turn"
                  : "Queue next turn"
                : "Send message"
            }
            type="button"
          >
            {pendingAction === "send" ? (
              <span
                aria-hidden
                className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent"
              />
            ) : (
              <ArrowUp aria-hidden className="size-4" />
            )}
          </button>
        ) : null}

        {isWorking ? (
          <button
            aria-label="Interrupt current turn"
            className="grid size-9 place-items-center rounded-full bg-destructive text-destructive-foreground shadow-sm transition-transform enabled:hover:scale-105 disabled:cursor-not-allowed disabled:opacity-35"
            data-testid="coding-session-composer-interrupt"
            disabled={
              !canControl ||
              !isMember ||
              !canInterrupt ||
              pendingAction !== null
            }
            onClick={onInterrupt}
            title={
              canInterrupt
                ? "Interrupt only the current turn"
                : "Current-turn interrupt is unavailable for this provider"
            }
            type="button"
          >
            {pendingAction === "interrupt" ? (
              <span
                aria-hidden
                className="size-4 animate-spin rounded-full border-2 border-current border-r-transparent"
              />
            ) : (
              <Square aria-hidden className="size-3.5 fill-current" />
            )}
          </button>
        ) : null}
      </div>
    </div>
  );
}

function ComposerDeckSeparator() {
  return <span aria-hidden className="h-6 w-px shrink-0 bg-border/70" />;
}

function ComposerDefinition({
  label,
  value,
}: {
  label: string;
  value: string;
}) {
  return (
    <div>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="mt-0.5 wrap-break-word">{value}</dd>
    </div>
  );
}

function CodingSessionContextMeter({
  usage,
}: {
  usage: CodingSessionContextWindow;
}) {
  const percentage = usage.usedPercentage;
  const normalized = percentage ?? 0;
  const radius = 9;
  const circumference = 2 * Math.PI * radius;
  const dashOffset = circumference * (1 - normalized / 100);
  const warning = percentage !== null && percentage > 90;
  return (
    <Popover>
      <PopoverTrigger asChild>
        <button
          aria-label={
            percentage === null
              ? `Context window ${formatTokens(usage.usedTokens)} tokens used`
              : `Context window ${Math.round(percentage)}% used`
          }
          className="grid size-8 place-items-center rounded-full text-muted-foreground transition-colors hover:bg-muted/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
          data-testid="coding-session-context-window"
          type="button"
        >
          <svg aria-hidden className="size-5 -rotate-90" viewBox="0 0 24 24">
            <title>Context window usage</title>
            <circle
              cx="12"
              cy="12"
              fill="none"
              r={radius}
              stroke="currentColor"
              strokeOpacity="0.24"
              strokeWidth="3"
            />
            {percentage !== null ? (
              <circle
                className={cn(warning && "text-destructive")}
                cx="12"
                cy="12"
                fill="none"
                r={radius}
                stroke="currentColor"
                strokeDasharray={circumference}
                strokeDashoffset={dashOffset}
                strokeLinecap="round"
                strokeWidth="3"
              />
            ) : null}
          </svg>
        </button>
      </PopoverTrigger>
      <PopoverContent align="end" className="w-64" side="top">
        <div className="flex items-center justify-between gap-3">
          <p className="text-sm font-medium">Context window</p>
          <span className="text-xs tabular-nums text-muted-foreground">
            {percentage === null
              ? "Capacity unreported"
              : `${Math.round(percentage)}%`}
          </span>
        </div>
        <p className="mt-2 text-xs tabular-nums text-muted-foreground">
          {formatTokens(usage.usedTokens)} used
          {usage.maxTokens === null
            ? ""
            : ` of ${formatTokens(usage.maxTokens)}`}
        </p>
      </PopoverContent>
    </Popover>
  );
}

function accessExplanation(input: {
  authorityReason: string | null;
  canControl: boolean;
  isMember: boolean;
  isUngovernedSession: boolean;
}): string {
  if (input.canControl && input.isMember) {
    return "This identity may publish commands to the selected execution.";
  }
  if (!input.isMember) {
    return "Join this channel, then ask the session owner for an operator grant if the provider requires one.";
  }
  if (input.authorityReason) return input.authorityReason;
  if (input.isUngovernedSession) {
    return "This execution has no current session governor. Adopt it before attempting control.";
  }
  return "Ask the session owner for collaborator access.";
}

function capabilityLabels(
  capabilities: CodingSessionComposerControlContext["capabilities"],
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

function formatTokens(value: number): string {
  return new Intl.NumberFormat("en", { notation: "compact" }).format(value);
}
