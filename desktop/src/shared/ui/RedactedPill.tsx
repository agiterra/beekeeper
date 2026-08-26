import { EyeOff } from "lucide-react";
import * as React from "react";

import { copyTextToClipboard } from "@/shared/lib/clipboard";
import { cn } from "@/shared/lib/cn";
import {
  formatRedactedBytes,
  formatRedactionLabel,
  parseRedactionMarkers,
  type RedactionMarker,
  type RedactionSegment,
} from "@/shared/lib/redactionMarker";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

/**
 * The inline stand-in for a value the provider redacted before signing.
 *
 * On the wire the redaction is 90 characters of hash dropped into the middle
 * of a sentence (see `shared/lib/redactionMarker.ts`). The digest still has to
 * be *reachable* — it is what distinguishes "the provider had this and chose
 * not to publish it" from "nothing was there", and two readers comparing two
 * transcripts need it to tell whether they are looking at the same hidden
 * value. So it moves behind the pill rather than away: hover or keyboard-focus
 * reveals it, and a click copies it.
 *
 * Sizing is on the `text-2xs` meta-text token, not a literal. The transcript
 * is a zoom-sensitive surface and `check-px-text.mjs` fails the build on
 * arbitrary text sizes anyway.
 */
export function RedactedPill({
  className,
  interactive = true,
  marker,
}: {
  className?: string;
  /**
   * `false` in non-interactive renders (previews, notification bodies), where
   * a focusable control would be a trap. The label still reads the same; only
   * the tooltip and the copy affordance are dropped.
   */
  interactive?: boolean;
  marker: RedactionMarker;
}) {
  const label = formatRedactionLabel(marker);
  const shared = cn(
    "mx-px inline-flex select-none items-baseline gap-1 rounded-sm px-1 py-px align-baseline",
    "bg-muted/70 font-medium text-2xs text-muted-foreground",
    className,
  );

  if (!interactive) {
    return (
      <span className={shared} data-redaction-pill="">
        <EyeOff aria-hidden className="size-3 self-center" />
        {label}
      </span>
    );
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={`${label}. Click to copy the SHA-256 digest.`}
          className={cn(
            shared,
            "cursor-pointer hover:bg-muted hover:text-foreground/80",
            "focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring",
          )}
          data-redaction-digest={marker.digest}
          data-redaction-pill=""
          onClick={() => {
            copyTextToClipboard(`sha256:${marker.digest}`, "Digest copied");
          }}
          type="button"
        >
          <EyeOff aria-hidden className="size-3 self-center" />
          {label}
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <RedactedPillTooltipBody marker={marker} />
      </TooltipContent>
    </Tooltip>
  );
}

function RedactedPillTooltipBody({ marker }: { marker: RedactionMarker }) {
  return (
    <div className="space-y-1">
      <p>
        Redacted by this session&rsquo;s provider before the transcript was
        signed.
      </p>
      <p className="wrap-anywhere font-mono text-2xs opacity-80">
        sha256:{marker.digest}
      </p>
      <p className="text-2xs opacity-70">
        {/* Serialized, not visible: the count covers the value's JSON quoting
            and escaping, so it reads a few bytes larger than the text a human
            would have seen. Saying which is cheaper than being wrong. */}
        {formatRedactedBytes(marker.bytes)} serialized &middot; click to copy
      </p>
    </div>
  );
}

/**
 * Render a plain string that may contain redaction markers, with each marker
 * replaced by a pill.
 *
 * For surfaces that are not markdown — serialized tool arguments, tool
 * results, activity labels, status rows — where the text arrives as one
 * already-formatted block. Markdown prose goes through
 * `remarkRedactionMarkers` instead, so the pill survives inside lists, tables,
 * and emphasis.
 *
 * Text with no marker is returned as the string itself, so the overwhelmingly
 * common case adds no elements to the tree.
 */
export function RedactedText({
  interactive = true,
  text,
}: {
  interactive?: boolean;
  text: string;
}) {
  const segments = React.useMemo<RedactionSegment[]>(
    () => parseRedactionMarkers(text),
    [text],
  );
  if (segments.length === 1 && segments[0].kind === "text") {
    return <>{text}</>;
  }
  return (
    <>
      {segments.map((segment, index) =>
        segment.kind === "text" ? (
          // biome-ignore lint/suspicious/noArrayIndexKey: positional segments of one immutable string
          <React.Fragment key={index}>{segment.text}</React.Fragment>
        ) : (
          <RedactedPill
            interactive={interactive}
            // biome-ignore lint/suspicious/noArrayIndexKey: positional segments of one immutable string
            key={index}
            marker={segment}
          />
        ),
      )}
    </>
  );
}
