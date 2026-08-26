import { EyeOff, Scissors } from "lucide-react";
import * as React from "react";

import { copyTextToClipboard } from "@/shared/lib/clipboard";
import { cn } from "@/shared/lib/cn";
import {
  type ElisionCause,
  formatElisionLabel,
  formatRedactedBytes,
  parseRedactionMarkers,
  type RedactionMarker,
  type RedactionSegment,
} from "@/shared/lib/redactionMarker";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

/**
 * The stand-in for content the reader cannot see.
 *
 * Two causes share this vocabulary and neither is allowed to impersonate the
 * other: a **redaction** (the provider removed a host-private or
 * credential-bearing value before signing) and a **cap** (the item exceeded
 * the 32 KiB event cap). Same shape, different verb and different tooltip.
 *
 * The digest stays *reachable* either way — it is what distinguishes "the
 * producer had this and chose not to publish it" from "nothing was there", and
 * two readers comparing two transcripts need it to tell whether they are
 * looking at the same hidden value. So it moves behind the pill rather than
 * away: hover or keyboard-focus reveals it, a click copies it.
 *
 * Sizing is on the `text-2xs` meta-text token, not a literal. The transcript
 * is a zoom-sensitive surface and `check-px-text.mjs` fails the build on
 * arbitrary text sizes anyway.
 */
export function ElisionPill({
  bytes,
  cause,
  className,
  digest,
  interactive = true,
}: {
  bytes: number | null;
  cause: ElisionCause;
  className?: string;
  digest: string | null;
  /**
   * `false` in non-interactive renders (previews, notification bodies), where
   * a focusable control would be a trap. The label still reads the same; only
   * the tooltip and the copy affordance are dropped.
   */
  interactive?: boolean;
}) {
  const label = formatElisionLabel(cause, bytes);
  const Icon = cause === "redaction" ? EyeOff : Scissors;
  const shared = cn(
    "mx-px inline-flex select-none items-baseline gap-1 rounded-sm px-1 py-px align-baseline",
    "bg-muted/70 font-medium text-2xs text-muted-foreground",
    className,
  );

  if (!interactive) {
    return (
      <span
        className={shared}
        data-elision-cause={cause}
        data-redaction-pill=""
      >
        <Icon aria-hidden className="size-3 self-center" />
        {label}
      </span>
    );
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          aria-label={
            digest
              ? `${label}. Click to copy the SHA-256 digest.`
              : `${label}. No digest was recorded.`
          }
          className={cn(
            shared,
            "cursor-pointer hover:bg-muted hover:text-foreground/80",
            "focus-visible:outline-hidden focus-visible:ring-1 focus-visible:ring-ring",
          )}
          data-elision-cause={cause}
          data-redaction-digest={digest ?? undefined}
          data-redaction-pill=""
          onClick={() => {
            if (!digest) return;
            copyTextToClipboard(`sha256:${digest}`, "Digest copied");
          }}
          type="button"
        >
          <Icon aria-hidden className="size-3 self-center" />
          {label}
        </button>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <ElisionPillTooltipBody bytes={bytes} cause={cause} digest={digest} />
      </TooltipContent>
    </Tooltip>
  );
}

/** `ElisionPill` for a redaction marker parsed out of published text. */
export function RedactedPill({
  className,
  interactive = true,
  marker,
}: {
  className?: string;
  interactive?: boolean;
  marker: RedactionMarker;
}) {
  return (
    <ElisionPill
      bytes={marker.bytes}
      cause="redaction"
      className={className}
      digest={marker.digest}
      interactive={interactive}
    />
  );
}

function ElisionPillTooltipBody({
  bytes,
  cause,
  digest,
}: {
  bytes: number | null;
  cause: ElisionCause;
  digest: string | null;
}) {
  return (
    <div className="space-y-1">
      <p>
        {cause === "redaction"
          ? "Redacted by this session’s provider before the transcript was signed."
          : "Dropped by this session’s provider: the item did not fit the event cap."}
      </p>
      {digest ? (
        <p className="wrap-anywhere font-mono text-2xs opacity-80">
          sha256:{digest}
        </p>
      ) : (
        <p className="text-2xs opacity-70">No digest was recorded.</p>
      )}
      <p className="text-2xs opacity-70">
        {/* Serialized, not visible: the count covers the value's JSON quoting
            and escaping, so it reads a few bytes larger than the text a human
            would have seen. Saying which is cheaper than being wrong. */}
        {bytes === null
          ? "Size unknown"
          : `${formatRedactedBytes(bytes)} serialized`}
        {digest ? " · click to copy" : null}
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
