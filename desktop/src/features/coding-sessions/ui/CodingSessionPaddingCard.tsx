import {
  CODING_SESSION_GUTTER_OPTIONS,
  setCodingSessionGutter,
  useCodingSessionGutter,
} from "@/features/coding-sessions/lib/codingSessionGutterPreference";
import { cn } from "@/shared/lib/cn";

/**
 * How wide a margin a coding session keeps at the window edge.
 *
 * The three choices are listed in the page with their descriptions rather
 * than folded into a dropdown: the difference between them is a measurement,
 * and a person deciding how much of their window to spend on margin should be
 * able to read what each one costs without opening a menu first.
 *
 * The closing note is the part that keeps the control honest. A transcript is
 * capped at a reading measure and centred, so once the session is wider than
 * that measure the margin is slack and every choice here looks identical — a
 * setting that silently does nothing. Saying where it binds costs one line;
 * letting someone conclude the control is broken costs more. The note names
 * the measure rather than a pixel count on purpose: the measure is 48rem with
 * a side surface open and 72rem without, so any single number printed here
 * would be wrong half the time.
 */
export function CodingSessionPaddingCard() {
  const gutter = useCodingSessionGutter();

  return (
    <fieldset
      className="flex flex-col gap-2 px-4 py-4"
      data-testid="settings-coding-session-padding"
    >
      <legend className="sr-only">Coding session padding</legend>
      {CODING_SESSION_GUTTER_OPTIONS.map((option) => {
        const selected = option.value === gutter;
        return (
          <label
            className={cn(
              "flex cursor-pointer items-start gap-3 rounded-lg border px-3 py-2.5 transition-colors",
              selected
                ? "border-primary/50 bg-primary/5"
                : "border-border/60 hover:bg-muted/40",
            )}
            key={option.value}
          >
            <input
              checked={selected}
              className="mt-0.5 size-4 shrink-0 accent-primary"
              data-testid={`coding-session-padding-${option.value}`}
              name="coding-session-padding"
              onChange={() => setCodingSessionGutter(option.value)}
              type="radio"
              value={option.value}
            />
            <span className="flex min-w-0 flex-col">
              <span className="text-sm font-medium">{option.label}</span>
              <span className="text-xs text-muted-foreground">
                {option.description}
              </span>
            </span>
          </label>
        );
      })}
      <p
        className="pt-1 text-xs text-muted-foreground"
        data-testid="coding-session-padding-note"
      >
        This sets the text edges while a session is narrower than the
        transcript's own reading width — a pop-out, a split pane, a smaller
        window, or a side rail taking its share. Give it more room than that and
        the reading width sets them instead, so the choice stops making a
        visible difference.
      </p>
    </fieldset>
  );
}
