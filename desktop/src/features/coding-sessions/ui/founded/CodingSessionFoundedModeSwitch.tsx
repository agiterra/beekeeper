import { cn } from "@/shared/lib/cn";
import type { CodingSessionSetupMode } from "../../lib/codingSessionSetupMode";

const MODES: readonly { value: CodingSessionSetupMode; label: string }[] = [
  { value: "solo", label: "Solo" },
  { value: "team", label: "Team" },
];

/**
 * **Solo | Team** — the first control on the founded page.
 *
 * The one decision every other field hangs off: Solo is you working directly
 * with one agent, the prompt as your first message; Team is an agent leading
 * and bringing in workers as the task needs them, under an authority chain —
 * not a team assembled up front. Native radios rather
 * than a checkbox because both answers are named, and neither is the default
 * shape of the other; the label carries the testid so a click on the word
 * is the click that selects.
 */
export function CodingSessionFoundedModeSwitch({
  disabled = false,
  mode,
  onModeChange,
}: {
  disabled?: boolean;
  mode: CodingSessionSetupMode;
  onModeChange: (mode: CodingSessionSetupMode) => void;
}) {
  return (
    <div className="flex flex-col gap-1.5">
      <fieldset
        aria-label="Solo or Team"
        className="inline-flex w-fit rounded-md border border-border/60 bg-muted/30 p-0.5"
        data-testid="coding-session-founded-mode"
        disabled={disabled}
      >
        {MODES.map((entry) => {
          const selected = entry.value === mode;
          return (
            <label
              className={cn(
                "cursor-pointer rounded px-3 py-1 text-sm font-medium transition-colors has-[:disabled]:cursor-default has-[:disabled]:opacity-50 has-[:focus-visible]:ring-2 has-[:focus-visible]:ring-ring",
                selected
                  ? "bg-background text-foreground shadow-sm"
                  : "text-muted-foreground hover:text-foreground",
              )}
              data-state={selected ? "checked" : "unchecked"}
              data-testid={`coding-session-founded-mode-${entry.value}`}
              key={entry.value}
            >
              <input
                checked={selected}
                className="sr-only"
                name="coding-session-founded-mode"
                onChange={() => onModeChange(entry.value)}
                type="radio"
                value={entry.value}
              />
              {entry.label}
            </label>
          );
        })}
      </fieldset>
      <p className="text-2xs text-muted-foreground">
        Solo: you work directly with one agent. Team: an agent leads and brings
        in workers as the task needs.
      </p>
    </div>
  );
}
