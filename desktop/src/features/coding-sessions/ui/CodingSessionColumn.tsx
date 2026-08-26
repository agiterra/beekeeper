import type * as React from "react";

import { cn } from "@/shared/lib/cn";

import { useCodingSessionMeasure } from "../lib/codingSessionWidthPreference";

/**
 * Layout the reading measure needs whatever cap is in force.
 *
 * The cap itself is the person's choice and lives in
 * `../lib/codingSessionWidthPreference`; what stays fixed is everything
 * around it. `min-w-0` is load-bearing, not decoration: without it a `pre` or
 * table wider than the column blows the flex/grid item out past its parent and
 * the nearest `overflow-hidden` ancestor clips it mid-word. With it, the wide
 * child's own `overflow-x-auto` engages and the content scrolls inside its
 * block. `mx-auto` is what centres a capped column; at the Full width there is
 * no cap left to centre and it does nothing.
 */
export const CODING_SESSION_COLUMN_CLASS = "mx-auto w-full min-w-0";

/**
 * Fixed composer dock with a fade that ends before interactive content begins.
 *
 * The old gradient covered the dock itself, leaving participant chips and
 * controls on an 85%-opaque background with transcript text visibly running
 * behind them. The dock is now solid; a separate pseudo-element fades only the
 * strip immediately above it.
 */
export const CODING_SESSION_COMPOSER_DOCK_CLASS =
  "pointer-events-none absolute inset-x-0 bottom-0 z-20 bg-background pb-4 before:pointer-events-none before:absolute before:inset-x-0 before:bottom-full before:h-8 before:bg-linear-to-b before:from-transparent before:to-background before:content-['']";

/**
 * Centers `children` in the coding-session reading measure.
 *
 * `expanded` says no side surface is sharing the workspace, which earns a
 * wider cap at every setting — a rail takes real width, and a column sized for
 * an empty workspace would be cramped beside one.
 *
 * Apply `useCodingSessionColumnGutter()` to the ancestor that owns the
 * viewport edge — the scroll container, the composer dock, the goal row — and
 * pass only vertical rhythm and layout classes here. Padding belongs on the
 * outer surface, never on the measure box. Put it inside and the measure
 * silently shrinks by the padding, so a transcript written as `max-w-3xl px-5`
 * no longer lines up with a composer written as `px-5 > max-w-3xl` — which is
 * exactly how the transcript text and the composer edge drifted out of
 * register.
 */
export function CodingSessionColumn({
  children,
  className,
  expanded = false,
  ...props
}: React.ComponentPropsWithoutRef<"div"> & { expanded?: boolean }) {
  const measure = useCodingSessionMeasure(expanded);

  return (
    <div
      className={cn(CODING_SESSION_COLUMN_CLASS, measure, className)}
      data-coding-session-column=""
      {...props}
    >
      {children}
    </div>
  );
}
