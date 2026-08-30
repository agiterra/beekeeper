import type { CodingSessionLens } from "@/features/coding-sessions/lib/codingSessionLensPreference";
import { cn } from "@/shared/lib/cn";

/** Explicit view choice; changing it never publishes or steers the session. */
export function CodingSessionLensControl({
  lens,
  onChange,
}: {
  lens: CodingSessionLens;
  onChange: (lens: CodingSessionLens) => void;
}) {
  return (
    <fieldset
      aria-label="Session lens"
      className="inline-flex h-9 items-center rounded-xl border border-border/65 bg-muted/25 p-1"
      data-testid="coding-session-lens-control"
    >
      {(["conversation", "mission"] as const).map((candidate) => {
        const selected = candidate === lens;
        const label = candidate === "conversation" ? "Conversation" : "Mission";
        return (
          <button
            aria-label={`${label} lens`}
            aria-pressed={selected}
            className={cn(
              "h-7 rounded-lg px-3 text-xs font-medium transition-colors focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring",
              selected
                ? "bg-background text-foreground shadow-sm"
                : "text-muted-foreground hover:bg-muted/45 hover:text-foreground",
            )}
            data-testid={`coding-session-lens-${candidate}`}
            key={candidate}
            onClick={() => onChange(candidate)}
            type="button"
          >
            {label}
          </button>
        );
      })}
    </fieldset>
  );
}
