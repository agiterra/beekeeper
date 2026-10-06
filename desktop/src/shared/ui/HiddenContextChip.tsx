import { EyeOff } from "lucide-react";

import { copyTextToClipboard } from "@/shared/lib/clipboard";
import { cn } from "@/shared/lib/cn";
import {
  hiddenContextDescription,
  hiddenContextLabel,
} from "@/shared/lib/hiddenContext";
import type { RedactionMarker } from "@/shared/lib/redactionMarker";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

/**
 * A value the provider hid before publishing, as one quiet inline chip:
 * `hidden path`, or `hidden` when nothing around it says it was a path.
 *
 * Replaces ninety characters of `[elided private context: N bytes,
 * sha256:…]` — a `pwd` line, a `cd` target — that read as noise. The chip
 * never reveals or guesses the content. The digest is kept, not dropped: the
 * tooltip shows it in full, the accessible name carries its first twelve hex
 * characters, and a click copies `sha256:<digest>` so two readers can tell
 * whether they are looking at the same hidden value.
 *
 * `text-2xs` (rem) so it scales with zoom; `check-px-text.mjs` enforces it.
 */
export function HiddenContextChip({
  className,
  interactive = true,
  marker,
  pathShaped,
}: {
  className?: string;
  /**
   * `false` in previews and notification bodies, where a focusable control
   * would be a trap; the label and `title` stay, the copy affordance and the
   * full digest go (a preview is not where anyone verifies one).
   */
  interactive?: boolean;
  marker: Pick<RedactionMarker, "bytes" | "digest">;
  pathShaped: boolean;
}) {
  const label = hiddenContextLabel(pathShaped);
  const description = hiddenContextDescription(marker);
  const shared = cn(
    "mx-px inline-flex select-none items-baseline gap-1 rounded-sm px-1 py-px align-baseline",
    "bg-muted/70 font-sans font-medium text-2xs text-muted-foreground",
    className,
  );
  const dataProps = {
    "data-elision-cause": "redaction",
    "data-hidden-context-chip": "",
    "data-hidden-path": pathShaped ? "" : undefined,
    "data-redaction-pill": "",
  };

  if (!interactive) {
    return (
      <span
        aria-label={description}
        className={shared}
        role="img"
        title={description}
        {...dataProps}
      >
        <EyeOff aria-hidden className="size-3 self-center" />
        {label}
      </span>
    );
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={
            marker.digest
              ? `${description}. Click to copy the full digest.`
              : description
          }
          className={cn(
            shared,
            "cursor-pointer hover:bg-muted hover:text-foreground/80",
            "focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring",
          )}
          data-redaction-digest={marker.digest || undefined}
          onClick={() => {
            if (!marker.digest) return;
            copyTextToClipboard(`sha256:${marker.digest}`, "Digest copied");
          }}
          type="button"
          {...dataProps}
        >
          <EyeOff aria-hidden className="size-3 self-center" />
          {label}
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <div className="space-y-1">
          <p>{description}</p>
          {marker.digest ? (
            <>
              <p className="wrap-anywhere font-mono text-2xs opacity-80">
                sha256:{marker.digest}
              </p>
              <p className="text-2xs opacity-70">
                Click to copy the full digest.
              </p>
            </>
          ) : null}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
