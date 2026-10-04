/**
 * The one activity-row pattern every transcript row shares (SV-01, SV-06): a
 * tool call, a group of them, a thought, a plan snapshot, a subagent batch,
 * and the "Worked for …" fold row.
 *
 * - The row is full width and fills softly on hover, with rounded corners,
 *   so the whole line reads as one target (T3's `hover:bg-accent/20
 *   rounded-md`).
 * - The leading icon is 16px and muted; the label is `text-sm` and dimmed.
 *
 * Tone is the row's own business: a failed call keeps its own colour, which
 * this pattern never overrides. Kept beside `ToolItem.tsx`, which is the
 * shared row both the coding-session transcript and managed-agent views use.
 */

/** Geometry and hover fill for a row's clickable line (a `summary` or `button`). */
export const ACTIVITY_ROW_LINE_CLASS =
  "flex min-h-7 w-full max-w-full items-center gap-2 rounded-md px-1 text-left text-sm transition-colors hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/70";

/** The 16px leading icon. */
export const ACTIVITY_ROW_ICON_CLASS = "size-4 shrink-0 text-muted-foreground";

/** The dimmed label beside it. */
export const ACTIVITY_ROW_LABEL_CLASS =
  "min-w-0 truncate text-muted-foreground";
