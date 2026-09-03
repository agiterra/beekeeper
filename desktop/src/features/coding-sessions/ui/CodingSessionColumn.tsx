import type * as React from "react";

import { cn } from "@/shared/lib/cn";

import {
  useCodingSessionMeasure,
  useCodingSessionProseMeasure,
} from "../lib/codingSessionWidthPreference";

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
 * Mission's column: the same load-bearing `min-w-0`, and no centring.
 *
 * `mx-auto` is what centres a capped column. Mission has no container cap to
 * centre — the column *is* the space between the rails — so keeping it would
 * be inert at best and, paired with any cap that crept back in, would restore
 * exactly the two dead margins L4.6 removed.
 */
export const CODING_SESSION_MISSION_COLUMN_CLASS = "w-full min-w-0";

/**
 * One axis per scroller, inside the Mission stream.
 *
 * Critique B6 counted five scroll regions on one Mission view, and the worst
 * of them was a fenced block: `max-h-[400px] overflow-x-auto overflow-y-auto`
 * (`shared/ui/markdown/CodeBlock.tsx`) nested inside the stream's own
 * scroller. The wheel did three different things over 40 px of pointer travel
 * and a horizontal trackpad flick that meant "nothing" scrolled a command out
 * of view. (`max-h-[400px]` is also a px height frozen against Cmd +/-.)
 *
 * `CodeBlock` is shared UI and not this lane's file, so the cap is lifted from
 * the call site instead — the stream owns the vertical axis and the block
 * keeps its own horizontal one. Conversation never sets `mission` and its code
 * blocks are untouched.
 */
const CODING_SESSION_MISSION_CODE_BLOCK_CLASS =
  "[&_[data-code-block]>pre]:max-h-none [&_[data-code-block]>pre]:overflow-y-visible";

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
  mission = false,
  ...props
}: React.ComponentPropsWithoutRef<"div"> & {
  expanded?: boolean;
  /**
   * Mission's grid column, not Conversation's centred box.
   *
   * L4.6: in Mission the stream is whatever the two rails leave — no
   * `mx-auto`, no container cap — because a rail the viewer collapsed should
   * hand its pixels to the stream rather than to two symmetric margins. The
   * reading measure does not vanish; it moves inside the row and caps prose
   * in `ch` (see `codingSessionProseMeasure`), so a transaction row, the Work
   * Log, a code block and the Audit table get the whole column while a
   * paragraph still breaks at a readable line.
   *
   * Conversation passes nothing and is byte-identical: same `mx-auto`, same
   * container cap, no second cap inside it.
   */
  mission?: boolean;
}) {
  const measure = useCodingSessionMeasure(expanded);
  const prose = useCodingSessionProseMeasure();

  return (
    <div
      className={cn(
        mission
          ? CODING_SESSION_MISSION_COLUMN_CLASS
          : CODING_SESSION_COLUMN_CLASS,
        mission ? proseMeasureVariant(prose) : measure,
        mission && CODING_SESSION_MISSION_CODE_BLOCK_CLASS,
        className,
      )}
      data-coding-session-column=""
      data-coding-session-column-mission={mission ? "" : undefined}
      {...props}
    >
      {children}
    </div>
  );
}

/**
 * The prose cap, as a descendant variant on the column.
 *
 * Applied from here rather than on each paragraph because the components that
 * render a turn's prose and a message body are Conversation's too, and I8
 * freezes their DOM. `.message-markdown` is the one class both put on the
 * rendered prose block, so capping it from the Mission column reaches exactly
 * the text that needs a measure and nothing else — no shared component moves,
 * and Conversation, which never sets `mission`, is untouched.
 */
function proseMeasureVariant(prose: string): string {
  return cn("max-w-none", prose);
}
