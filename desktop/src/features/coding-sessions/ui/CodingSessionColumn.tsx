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

/** Width used when no secondary rail is sharing the workspace. */
export const CODING_SESSION_COLUMN_EXPANDED_CLASS = "max-w-6xl";

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
 * Apply `useCodingSessionColumnGutter()` (see
 * `../lib/codingSessionGutterPreference`) to the ancestor that owns the
 * viewport edge — the scroll container, the composer overlay, the goal row —
 * and pass only vertical rhythm and layout classes here. The gutter is the
 * person's to set, but where it hangs is not: padding belongs on the outer
 * surface, never on the measure box. Put it inside and the measure silently
 * shrinks by the padding, so a transcript written as `max-w-3xl px-5` no
 * longer lines up with a composer written as `px-5 > max-w-3xl` — which is
 * exactly how the transcript text and the composer edge drifted out of
 * register.
 */
export function CodingSessionColumn({
  children,
  className,
  expanded = false,
  ...props
}: React.ComponentPropsWithoutRef<"div"> & { expanded?: boolean }) {
  return (
    <div
      className={cn(
        CODING_SESSION_COLUMN_CLASS,
        expanded && CODING_SESSION_COLUMN_EXPANDED_CLASS,
        className,
      )}
      data-coding-session-column=""
      {...props}
    >
      {children}
    </div>
  );
}
