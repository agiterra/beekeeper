import type * as React from "react";

import { cn } from "@/shared/lib/cn";

/**
 * The reading measure for every coding-session surface.
 *
 * `max-w-3xl` is 48rem — deliberately a **rem** quantity, not px and not vw.
 * The desktop app implements Cmd +/- by scaling the root font size
 * (`app/useWebviewZoomShortcuts.ts`), so a rem measure widens with the glyphs
 * and holds a roughly constant character count. A px measure would freeze
 * against zoom (the PR #891 class of regression) and a vw measure would track
 * the window while ignoring zoom entirely.
 *
 * `min-w-0` is load-bearing, not decoration: without it a `pre` or table wider
 * than the column blows the flex/grid item out past its parent and the nearest
 * `overflow-hidden` ancestor clips it mid-word. With it, the wide child's own
 * `overflow-x-auto` engages and the content scrolls inside its block.
 */
export const CODING_SESSION_COLUMN_CLASS = "mx-auto w-full min-w-0 max-w-3xl";

/**
 * Gutter padding for the surface that *contains* a {@link CodingSessionColumn}.
 *
 * Padding belongs on the outer surface, never on the measure box. Put it
 * inside and the measure silently shrinks by the padding, so a transcript
 * written as `max-w-3xl px-5` no longer lines up with a composer written as
 * `px-5 > max-w-3xl` — which is exactly how the transcript text and the
 * composer edge drifted 20–32px out of register.
 */
export const CODING_SESSION_COLUMN_GUTTER = "px-5 sm:px-8";

/**
 * Centers `children` in the coding-session reading measure.
 *
 * Apply {@link CODING_SESSION_COLUMN_GUTTER} to the ancestor that owns the
 * viewport edge (the scroll container, the composer overlay, the header row);
 * pass only vertical rhythm and layout classes here.
 */
export function CodingSessionColumn({
  children,
  className,
  ...props
}: React.ComponentPropsWithoutRef<"div">) {
  return (
    <div className={cn(CODING_SESSION_COLUMN_CLASS, className)} {...props}>
      {children}
    </div>
  );
}
