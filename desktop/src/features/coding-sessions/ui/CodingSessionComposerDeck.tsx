import { ArrowUp, Bot, Ellipsis, ShieldCheck, Square } from "lucide-react";

import { codingSessionTurnBudgetUsage } from "@/features/coding-sessions/lib/codingSessionCapacity";
import type { CodingSessionChannelAccessCopy } from "@/features/coding-sessions/lib/codingSessionChannelAccess";
import type { CodingSessionContextWindow } from "@/features/coding-sessions/lib/codingSessionContextWindow";
import type { CodingSessionTurnBudget } from "@/features/coding-sessions/lib/codingSessionIngressPayloads";
import {
  codingSessionContextLabel,
  codingSessionModelDisplayName,
  codingSessionTraitsSummary,
} from "@/features/coding-sessions/lib/codingSessionModelDisplay";
import { formatCodingSessionModelDisplay } from "@/features/coding-sessions/lib/codingSessionLabels";
import type { CodingSessionWorkspaceStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { cn } from "@/shared/lib/cn";
import { Popover, PopoverContent, PopoverTrigger } from "@/shared/ui/popover";

import { useCodingSessionMissionLens } from "./CodingSessionUmbrellaWorkspaceModel";

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
  /**
   * The crew turn allowance this execution's umbrella runs under, as the
   * provider published it (D9), or null when it published none.
   *
   * Null is "not disclosed", never "unlimited": the popover omits the row
   * rather than claiming a session has no ceiling when nobody said so.
   */
  turnBudget?: CodingSessionTurnBudget | null;
};

type CodingSessionComposerDeckProps = {
  authorityReason: string | null;
  authorityUnresolved: boolean;
  canControl: boolean;
  canInterrupt: boolean;
  canSessionStop: boolean;
  canSteer: boolean;
  context: CodingSessionComposerControlContext | undefined;
  contextWindow: CodingSessionContextWindow | null;
  /**
   * Why this identity may not write to the session's channel, or null when it
   * may (`describeCodingSessionChannelAccess`). A refused or unresolved channel
   * names its own state; it is never folded into "View only" by default.
   */
  accessCopy: CodingSessionChannelAccessCopy | null;
  isSending: boolean;
  isUnavailable: boolean;
  isUngovernedSession: boolean;
  isWorking: boolean;
  onInterrupt: () => void;
  onPrimary: () => void;
  /** Publish explicitly at the next boundary, whatever the primary would do. */
  onQueueNext: () => void;
  onSessionStop: () => void;
  pendingAction: "send" | "interrupt" | "resume" | "stop" | null;
  primaryDisabled: boolean;
  /** The second delivery choice, or `null` when the primary is the only one. */
  secondaryLabel: string | null;
  recipientControl?: React.ReactNode;
};

/**
 * The compact, provider-neutral footer inside the full-screen composer.
 *
 * SESSION_VIEW_UX_PLAN L4: identity, access and traits are small chips, each
 * with its detail one click away in a popover (traits in the identity
 * popover's `Model traits` row). The delivery hint moved to the composer's
 * one-line notice strip (`CodingSessionComposerSurface`), so the deck is a
 * single row.
 */
export function CodingSessionComposerDeck({
  authorityReason,
  authorityUnresolved,
  canControl,
  canInterrupt,
  canSessionStop,
  canSteer,
  context,
  contextWindow,
  accessCopy,
  isSending,
  isUnavailable,
  isUngovernedSession,
  isWorking,
  onInterrupt,
  onPrimary,
  onQueueNext,
  onSessionStop,
  pendingAction,
  primaryDisabled,
  secondaryLabel,
  recipientControl,
}: CodingSessionComposerDeckProps) {
  const mission = useCodingSessionMissionLens();
  const model = context?.model
    ? formatCodingSessionModelDisplay(context.model)
    : null;
  const traits = model
    ? codingSessionTraitsSummary({
        thinking: model.thinking,
        context: model.context,
      })
    : null;
  // A8: `Send to Keystone · Lead · Can control · 1M`. Every token in that run
  // is a sentence except the last, which was a bare number whose only gloss
  // was a `title` attribute. A context size gets its noun; a traits string
  // that carries no context size gets no slot at all, because `High` beside a
  // recipient reads as a claim about the person. The identity popover above
  // still carries the whole string under a `Model traits` label, which is
  // where an unglossed token belongs.
  //
  // Two corrections from REVIEW-L4:
  //
  // F5 — the noun goes on the **context token**, not on the whole traits
  // string. `codingSessionTraitsSummary` returns `High · 1M`, so
  // `${traits} context` shipped `High · 1M context` and left `High` exactly
  // the bare token this ruling removed. Only the window gets the word.
  // F4 — A8 is a **Mission** ruling and this deck renders on both lenses, so
  // Conversation keeps its whole traits summary, byte for byte. A model with
  // a thinking trait and no window would otherwise have lost its slot there.
  const deckTraits = mission
    ? model?.context
      ? `${codingSessionContextLabel(model.context)} context`
      : null
    : traits;
  const modelName = model ? codingSessionModelDisplayName(model.model) : null;
  const providerName =
    context?.providerLabel ?? context?.runtimeLabel ?? "Session provider";
  const identityLabel = [providerName, modelName]
    .filter((value): value is string => Boolean(value))
    .join(" · ");
  const accessLabel = accessCopy
    ? accessCopy.label
    : canControl
      ? "Can control"
      : authorityUnresolved
        ? "Access unresolved"
        : "View only";
  const availableCapabilities = context
    ? capabilityLabels(context.capabilities)
    : [];

  return (
    <div className="flex min-w-0 flex-col">
      <div
        className="flex min-h-11 min-w-0 items-center gap-2 px-3 pb-2"
        data-testid="coding-session-control-deck"
      >
        <div className="flex min-w-0 items-center gap-1 text-xs text-muted-foreground">
          {recipientControl ?? (
            <Popover>
              <PopoverTrigger asChild>
                <button
                  aria-label="Show execution identity"
                  className={cn(
                    COMPOSER_CHIP_CLASS,
                    "min-w-0 text-foreground/75",
                  )}
                  data-testid="coding-session-control-identity"
                  type="button"
                >
                  <Bot aria-hidden className="size-3 shrink-0" />
                  <span className="max-w-48 truncate">{identityLabel}</span>
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
                  {context?.turnBudget ? (
                    <ComposerDefinition
                      label="Team turns"
                      testId="coding-session-control-turn-budget"
                      value={codingSessionTurnBudgetUsage(context.turnBudget)}
                    />
                  ) : null}
                </dl>
                <p className="mt-3 text-xs text-muted-foreground">
                  This identifies the signed execution. Its model and traits are
                  fixed for this execution.
                </p>
                {context?.turnBudget ? (
                  <p className="mt-2 text-xs text-muted-foreground">
                    The turn count is the whole team session's, shared by every
                    execution under it. At the limit the provider refuses
                    further turns from the agents; only the execution's founder
                    is exempt.
                  </p>
                ) : null}
              </PopoverContent>
            </Popover>
          )}

          <Popover>
            <PopoverTrigger asChild>
              <button
                className={cn(COMPOSER_CHIP_CLASS, "shrink-0")}
                data-testid="coding-session-control-authority"
                type="button"
              >
                <ShieldCheck aria-hidden className="size-3 shrink-0" />
                {accessLabel}
              </button>
            </PopoverTrigger>
            <PopoverContent align="start" className="w-72" side="top">
              <p className="text-sm font-medium">{accessLabel}</p>
              <p className="mt-2 text-xs leading-relaxed text-muted-foreground">
                {accessExplanation({
                  accessCopy,
                  authorityReason,
                  canControl,
                  isUngovernedSession,
                })}
              </p>
              <p className="mt-3 text-xs text-muted-foreground">
                Access comes from signed channel membership (for a project
                session, your project role) and the session’s operator grant. It
                is not a local preference.
              </p>
            </PopoverContent>
          </Popover>

          {deckTraits ? (
            <span
              className="hidden shrink-0 rounded-full px-2 py-0.5 sm:inline"
              data-testid="coding-session-control-traits"
              title="Fixed model traits for this execution; the identity chip lists them in full"
            >
              {deckTraits}
            </span>
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
                      : "Send at the next turn boundary; it cannot be recalled"
                    : "Send message"
              }
              className="grid size-9 place-items-center rounded-full bg-primary text-primary-foreground shadow-sm transition-transform enabled:hover:scale-105 disabled:cursor-not-allowed disabled:opacity-30"
              // `…-queue` is a historical selector, kept so specs that already
              // point at the mid-turn action keep working. Nothing is queued in
              // this client any more: the command is published now and the
              // provider's mailbox holds it until the current turn ends.
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
                    : "Sends now; this provider runs it when the current turn ends, and it cannot be recalled"
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

          {secondaryLabel === null ? null : (
            // Labelled, not an icon: "queue this for after the turn" has no
            // established glyph, and the one thing this control must not be is
            // mistaken for the steer beside it.
            <button
              aria-label="Queue for the next turn boundary"
              className="shrink-0 rounded-full border border-border/80 px-3 py-1.5 text-2xs font-medium text-foreground/80 transition-colors hover:bg-muted/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-40"
              data-testid="coding-session-composer-queue-next"
              disabled={primaryDisabled}
              onClick={onQueueNext}
              title="Send now; this provider runs it when the current turn ends, and it cannot be recalled"
              type="button"
            >
              {secondaryLabel}
            </button>
          )}

          {isWorking ? (
            <button
              aria-label="Interrupt current turn"
              className="grid size-9 place-items-center rounded-full bg-destructive text-destructive-foreground shadow-sm transition-transform enabled:hover:scale-105 disabled:cursor-not-allowed disabled:opacity-35"
              data-testid="coding-session-composer-interrupt"
              disabled={
                !canControl ||
                accessCopy !== null ||
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
    </div>
  );
}

/** One compact chip: a quiet pill whose detail lives in its popover. */
const COMPOSER_CHIP_CLASS =
  "inline-flex h-6 items-center gap-1.5 rounded-full border border-border/60 bg-background/40 px-2 transition-colors hover:bg-muted/50 hover:text-foreground focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring";

function ComposerDefinition({
  label,
  testId,
  value,
}: {
  label: string;
  testId?: string;
  value: string;
}) {
  return (
    <div data-testid={testId}>
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
  accessCopy: CodingSessionChannelAccessCopy | null;
  authorityReason: string | null;
  canControl: boolean;
  isUngovernedSession: boolean;
}): string {
  if (input.accessCopy) return input.accessCopy.explanation;
  if (input.canControl) {
    return "This identity may publish commands to the selected execution.";
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
