/**
 * The one activity-row pattern every transcript row shares (SV-01, SV-06): a
 * tool call, a group of them, a thought, a plan snapshot, a subagent batch,
 * and the "Worked for …" fold row.
 *
 * - The row is full width and fills softly on hover, with rounded corners,
 *   so the whole line reads as one target (T3's `hover:bg-accent/20
 *   rounded-md`, `V2LifecycleRow.tsx`).
 * - The leading icon is 16px and muted; the label is `text-sm` and dimmed.
 *
 * Tone is the row's own business: a failed call keeps its own colour, which
 * this pattern never overrides. Kept beside `ToolItem.tsx`, which is the
 * shared row both the coding-session transcript and managed-agent views use.
 */

/**
 * Geometry and hover fill for a row's clickable line (a `summary` or `button`).
 *
 * Measured against T3 Code's `WorkLogLine` (`components/chat/WorkLog.tsx`):
 * `px-0.5`, a 24px icon box holding a 16px icon, `gap-1.5`, so the label
 * starts 32px in from the row's edge — the same 32px the reference capture
 * `sv01-t3-subagent-row-hover` shows. Wave A had guessed `px-1 gap-2` with a
 * bare icon, which put labels 4px short. The fill stays `accent/30`, not T3's
 * `/20`: the two apps' `--accent` tokens differ, and `/30` is Wave A's
 * by-eye match to the reference fill (not a measured value).
 */
export const ACTIVITY_ROW_LINE_CLASS =
  "flex min-h-7 w-full max-w-full items-center gap-1.5 rounded-md px-0.5 text-left text-sm transition-colors hover:bg-accent/30 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-inset focus-visible:ring-ring/70";

/**
 * The 16px leading icon, centred in a 24px box (`mx-1`), muted and at 70%
 * like T3's `text-icon-muted opacity-70`.
 */
export const ACTIVITY_ROW_ICON_CLASS =
  "mx-1 size-4 shrink-0 text-muted-foreground opacity-70";

/** The dimmed label beside it. */
export const ACTIVITY_ROW_LABEL_CLASS =
  "min-w-0 truncate text-muted-foreground";

/**
 * Where an opened row's detail starts: under the label, 32px in (T3's
 * `WorkLogDetails` is `ms-7 px-0.5`, i.e. 30px; Beekeeper aligns the text to
 * the label itself).
 */
export const ACTIVITY_ROW_DETAIL_INSET_CLASS = "ps-8";

/**
 * A failed step's icon inside a settled turn's fold (SV-02, D2): its own
 * glyph, dimmed red — T3's `text-tool-error-icon/40`. The `opacity-70` of
 * {@link ACTIVITY_ROW_ICON_CLASS} is dropped so the red reads.
 */
export const ACTIVITY_ROW_FAILED_ICON_CLASS =
  "mx-1 size-4 shrink-0 text-destructive/40";
