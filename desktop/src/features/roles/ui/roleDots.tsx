/**
 * The page's one vocabulary of colour: a 6px dot, and the rule for which
 * colour it may take.
 *
 * Three rules hold everywhere a dot appears. Colour is only ever a *reported*
 * state — never a summary, a quality or a guess. Every coloured dot is
 * accompanied by the word it stands for, in the row's own text or in its
 * `title`, so the page still reads correctly with no colour at all. And a
 * state nobody reported gets no dot rather than a grey one, because grey
 * would say "stopped" about an agent whose run state was never sent here.
 */
import type * as React from "react";

import type { CodingSessionStatus } from "@/features/coding-sessions/lib/codingSessionTypes";
import { cn } from "@/shared/lib/cn";

import type { RoleAgentChip, SeatRow } from "../lib/rolesViewModel";

/** Muted is a real state ("stopped", "no open sessions"), never an unknown. */
export const DOT_MUTED = "bg-muted-foreground/45";
export const DOT_RUNNING = "bg-emerald-500";
export const DOT_DEPLOYED = "bg-sky-500";
export const DOT_WAITING = "bg-amber-500";

/**
 * A 6px status dot. `title` carries the word the colour stands for; pass
 * `label` as well when no neighbouring text repeats it, so a screen reader
 * gets the state and not a bare decoration.
 */
export function StatusDot({
  className,
  label,
  testId,
  title,
  ...rest
}: {
  className: string;
  label?: string;
  testId?: string;
  title: string;
} & Omit<
  React.ComponentPropsWithoutRef<"span">,
  "className" | "title" | "children"
>) {
  const classes = cn("inline-block size-1.5 shrink-0 rounded-full", className);
  if (label === undefined) {
    return (
      <span
        {...rest}
        aria-hidden
        className={classes}
        data-testid={testId}
        title={title}
      />
    );
  }
  return (
    <span
      {...rest}
      aria-label={label}
      className={classes}
      data-testid={testId}
      role="img"
      title={title}
    />
  );
}

/**
 * A managed agent's own status. `undefined` returns `null` — the caller draws
 * no dot at all and says so in words, because an agent this view was never
 * told the run state of is not a stopped agent.
 */
export function agentStatusDotClass(
  status: RoleAgentChip["status"] | undefined,
): string | null {
  switch (status) {
    case "running":
      return DOT_RUNNING;
    case "deployed":
      return DOT_DEPLOYED;
    case "stopped":
    case "not_deployed":
      return DOT_MUTED;
    default:
      return null;
  }
}

/** A session's own status word: running, waiting for a person, or neither. */
export function seatStatusDotClass(status: CodingSessionStatus): string {
  switch (status) {
    case "running":
      return DOT_RUNNING;
    case "waiting_for_input":
      return DOT_WAITING;
    default:
      return DOT_MUTED;
  }
}

export type RoleActivity = "running" | "idle" | "none";

/**
 * What the card's leading dot may claim: `running` only when a session in
 * this role reports that word right now, `idle` when the role holds open
 * sessions that do not, `none` when it holds none.
 */
export function roleActivity(seats: readonly SeatRow[]): RoleActivity {
  if (seats.length === 0) return "none";
  return seats.some((seat) => seat.status === "running") ? "running" : "idle";
}

export function roleActivityDotClass(activity: RoleActivity): string {
  return activity === "running" ? DOT_RUNNING : DOT_MUTED;
}
