import type { CodingSessionMissionDensity } from "@/features/coding-sessions/lib/codingSessionMissionDensity";
import { cn } from "@/shared/lib/cn";

const OPTIONS: ReadonlyArray<{
  value: CodingSessionMissionDensity;
  label: string;
  description: string;
}> = [
  { value: "brief", label: "Brief", description: "What mattered" },
  { value: "live", label: "Live", description: "What is happening" },
  { value: "trace", label: "Trace", description: "Exactly what happened" },
];

/**
 * Brief / Live / Trace, as part of the header's second row.
 *
 * The default variant is **flush**: it sits inside the participant strip's own
 * container, so it must not draw a competing box around itself — that extra
 * border was one of the five chrome bands stacked above the first row of the
 * stream. `standalone` keeps the bordered fieldset for any surface that mounts
 * the control on its own.
 */
export function CodingSessionMissionDensityControl({
  density,
  onChange,
  variant = "flush",
}: {
  density: CodingSessionMissionDensity;
  onChange: (density: CodingSessionMissionDensity) => void;
  variant?: "flush" | "standalone";
}) {
  return (
    <fieldset
      className={cn(
        "inline-flex rounded-lg p-0.5",
        variant === "standalone"
          ? "border border-border/60 bg-muted/25"
          : "bg-muted/25",
      )}
      data-testid="coding-session-mission-density"
      data-variant={variant}
    >
      <legend className="sr-only">Mission reading density</legend>
      {OPTIONS.map((option) => (
        <button
          aria-pressed={density === option.value}
          className={cn(
            "rounded-md px-3 py-1.5 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring motion-reduce:transition-none",
            density === option.value
              ? "bg-background text-foreground shadow-sm"
              : "text-muted-foreground hover:text-foreground",
          )}
          data-testid={`coding-session-mission-density-${option.value}`}
          key={option.value}
          onClick={() => onChange(option.value)}
          title={option.description}
          type="button"
        >
          {option.label}
        </button>
      ))}
    </fieldset>
  );
}
