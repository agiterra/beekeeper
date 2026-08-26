import {
  CODING_SESSION_WIDTH_OPTIONS,
  setCodingSessionWidth,
  useCodingSessionWidth,
} from "@/features/coding-sessions/lib/codingSessionWidthPreference";
import { cn } from "@/shared/lib/cn";

/**
 * How much of the window a coding session's text uses.
 *
 * The three choices are listed in the page with their descriptions rather than
 * folded into a dropdown: this is a reading-comfort decision, and someone
 * making it should be able to see what they are trading before they choose,
 * not after.
 *
 * The copy names no measurements on purpose. A person picking how their
 * transcripts should look is choosing an experience, not entering a layout
 * value — and any pixel count printed here would be wrong on half the window
 * sizes it could be read on, since the column also adapts to whether a side
 * panel is open.
 */
export function CodingSessionWidthCard() {
  const width = useCodingSessionWidth();

  return (
    <fieldset
      className="flex flex-col gap-2 px-4 py-4"
      data-testid="settings-coding-session-width"
    >
      <legend className="sr-only">Coding session width</legend>
      {CODING_SESSION_WIDTH_OPTIONS.map((option) => {
        const selected = option.value === width;
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
              data-testid={`coding-session-width-${option.value}`}
              name="coding-session-width"
              onChange={() => setCodingSessionWidth(option.value)}
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
    </fieldset>
  );
}
