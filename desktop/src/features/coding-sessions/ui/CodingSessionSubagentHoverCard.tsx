import * as React from "react";
import * as TooltipPrimitive from "@radix-ui/react-tooltip";

import type { CodingSessionSettledSubagentStatus } from "@/features/coding-sessions/lib/codingSessionSubagents";
import {
  codingSessionSubagentModelName,
  formatCodingSessionSubagentTokens,
} from "@/features/coding-sessions/lib/codingSessionSubagents";
import {
  type CodingSessionSubagentCard,
  codingSessionSubagentLiveElapsedMs,
} from "@/features/coding-sessions/lib/codingSessionSubagentsCard";
import { formatCodingSessionDuration } from "@/features/coding-sessions/lib/codingSessionTranscriptModel";
import { cn } from "@/shared/lib/cn";
import {
  POPOVER_RADIX_MOTION_CLASS,
  POPOVER_RADIX_SIDE_MOTION_CLASS,
  POPOVER_SHADOW_STYLE,
  POPOVER_SURFACE_CLASS,
} from "@/shared/ui/popoverSurface";

/*
 * SV-80/SV-81: the small pieces every subagent card shares — the status dot,
 * the elapsed clock, and the hover card (T3 Code's `SubagentTimelineTooltip`)
 * that says model, time, status, tokens, tools and the result's first line.
 * All of it reads the spawn's *settled* status: a spawn whose turn is over
 * never gets a blue dot or a ticking clock here.
 */

/**
 * The dot's colour for a settled status — the one mapping every subagent
 * surface uses (stream card, hover card, Agents panel rows, the orchestration
 * view's Direct spawns), so a subagent that finished reads green everywhere
 * rather than grey in one list and green in the next. Only a *running*
 * status (already read through the turn's settlement) pulses, and never under
 * reduced motion; unknown is a hollow ring, no colour claim.
 */
export function codingSessionSubagentStatusDotClass(
  status: CodingSessionSettledSubagentStatus,
): string {
  switch (status) {
    case "running":
      return "animate-pulse bg-blue-500 motion-reduce:animate-none";
    case "done":
      return "bg-emerald-500";
    case "failed":
      return "bg-destructive";
    case "stopped":
      return "bg-muted-foreground/40";
    default:
      return "border border-muted-foreground/60";
  }
}

/** A coloured dot for a settled status; hollow for unknown (no claim). */
export function CodingSessionSubagentStatusDot({
  className,
  status,
}: {
  className?: string;
  status: CodingSessionSettledSubagentStatus;
}) {
  return (
    <span
      aria-hidden="true"
      className={cn(
        "size-1.5 shrink-0 rounded-full",
        codingSessionSubagentStatusDotClass(status),
        className,
      )}
      data-status={status}
      data-testid="coding-session-subagent-status-dot"
    />
  );
}

/**
 * A running card's clock ticks once a second from its start; a finished one
 * shows its frozen duration; anything else shows nothing. The interval exists
 * only while the card is live, so it stops the moment the status settles.
 */
export function CodingSessionSubagentElapsed({
  card,
  className,
}: {
  card: Pick<CodingSessionSubagentCard, "phase" | "startedAtMs" | "durationMs">;
  className?: string;
}) {
  const live = card.phase === "live" && card.startedAtMs !== null;
  const [now, setNow] = React.useState(() => Date.now());
  React.useEffect(() => {
    if (!live) return;
    setNow(Date.now());
    const interval = window.setInterval(() => setNow(Date.now()), 1_000);
    return () => window.clearInterval(interval);
  }, [live]);
  if (live) {
    const elapsed = codingSessionSubagentLiveElapsedMs(card.startedAtMs, now);
    return elapsed === null ? null : (
      <span
        className={cn("tabular-nums", className)}
        data-live="true"
        data-testid="coding-session-subagent-elapsed"
      >
        {formatCodingSessionDuration(elapsed)}
      </span>
    );
  }
  if (card.phase === "finished" && card.durationMs !== null) {
    return (
      <span
        className={cn("tabular-nums", className)}
        data-live="false"
        data-testid="coding-session-subagent-elapsed"
      >
        {formatCodingSessionDuration(card.durationMs)}
      </span>
    );
  }
  return null;
}

function Fact({
  label,
  children,
}: {
  label: string;
  children: React.ReactNode;
}) {
  return (
    <>
      <dt className="text-muted-foreground">{label}</dt>
      <dd className="min-w-0 truncate font-mono text-foreground/90">
        {children}
      </dd>
    </>
  );
}

const NOT_REPORTED = (
  <span className="text-muted-foreground">not reported</span>
);

/** The hover card's body, separate so it renders without a trigger in tests. */
export function CodingSessionSubagentHoverCardContent({
  card,
}: {
  card: CodingSessionSubagentCard;
}) {
  const timeLabel = card.phase === "live" ? "Elapsed" : "Duration";
  const time =
    card.phase === "live" ? (
      card.startedAtMs !== null ? (
        <CodingSessionSubagentElapsed card={card} />
      ) : (
        NOT_REPORTED
      )
    ) : card.phase === "finished" && card.durationMs !== null ? (
      <CodingSessionSubagentElapsed card={card} />
    ) : card.status === "stopped" ? (
      <span className="text-muted-foreground">never returned</span>
    ) : (
      NOT_REPORTED
    );
  const hasResult = card.status === "done" || card.status === "failed";
  return (
    <div
      className="flex min-w-0 flex-col gap-2 text-xs"
      data-testid="coding-session-subagent-hover-card"
    >
      <p className="flex min-w-0 items-center gap-2 font-medium text-foreground">
        <CodingSessionSubagentStatusDot status={card.status} />
        <span className="truncate">{card.title}</span>
        {card.type ? (
          <span className="shrink-0 rounded-sm border border-border/60 px-1 font-mono text-3xs text-muted-foreground">
            {card.type}
          </span>
        ) : null}
      </p>
      <dl className="grid grid-cols-[auto_minmax(0,1fr)] gap-x-3 gap-y-0.5 text-2xs">
        <Fact label="Status">
          <span
            className={cn(card.status === "failed" && "text-destructive")}
            data-testid="coding-session-subagent-hover-status"
          >
            {card.outcomeLabel}
          </span>
        </Fact>
        <Fact label="Model">
          {card.model !== null ? (
            <span title={card.model}>
              {codingSessionSubagentModelName(card.model)}
            </span>
          ) : (
            NOT_REPORTED
          )}
        </Fact>
        <Fact label={timeLabel}>{time}</Fact>
        <Fact label="Tokens">
          {card.totalTokens !== null
            ? `${formatCodingSessionSubagentTokens(card.totalTokens)} tok`
            : NOT_REPORTED}
        </Fact>
        <Fact label="Tools">
          {card.toolCount !== null ? String(card.toolCount) : NOT_REPORTED}
        </Fact>
      </dl>
      {card.detail ? (
        <p
          className="line-clamp-2 text-2xs text-muted-foreground"
          data-testid="coding-session-subagent-hover-detail"
        >
          <span className="font-medium text-foreground/80">
            {hasResult ? "Result: " : "Latest: "}
          </span>
          {card.detail}
        </p>
      ) : null}
      {card.parentToolId === null ? (
        <p className="text-2xs text-muted-foreground">
          No page to open: the provider sent no call id for this subagent.
        </p>
      ) : null}
    </div>
  );
}

/**
 * Wraps a subagent row's own button: the card opens on hover and on keyboard
 * focus (Radix tooltip semantics), so it is reachable without a pointer.
 */
export function CodingSessionSubagentHoverCard({
  card,
  children,
}: {
  card: CodingSessionSubagentCard;
  children: React.ReactElement;
}) {
  return (
    // Nothing in the card is interactive, so it closes as the pointer leaves
    // the row rather than waiting out Radix's hoverable grace area.
    <TooltipPrimitive.Provider
      delayDuration={250}
      disableHoverableContent
      skipDelayDuration={150}
    >
      <TooltipPrimitive.Root>
        <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
        <TooltipPrimitive.Portal>
          <TooltipPrimitive.Content
            align="start"
            className={cn(
              "z-50 w-72 rounded-xl p-3 outline-hidden",
              POPOVER_SURFACE_CLASS,
              POPOVER_RADIX_MOTION_CLASS,
              POPOVER_RADIX_SIDE_MOTION_CLASS,
            )}
            side="bottom"
            sideOffset={4}
            style={POPOVER_SHADOW_STYLE}
          >
            <CodingSessionSubagentHoverCardContent card={card} />
          </TooltipPrimitive.Content>
        </TooltipPrimitive.Portal>
      </TooltipPrimitive.Root>
    </TooltipPrimitive.Provider>
  );
}
