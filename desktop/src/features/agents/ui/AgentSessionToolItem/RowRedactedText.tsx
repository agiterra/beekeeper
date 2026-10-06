import { EyeOff } from "lucide-react";
import * as React from "react";

import { cn } from "@/shared/lib/cn";
import {
  annotateHiddenContext,
  type HiddenContextSegment,
} from "@/shared/lib/hiddenContext";
import { parseRedactionMarkers } from "@/shared/lib/redactionMarker";
import { HiddenContextChip } from "@/shared/ui/HiddenContextChip";
import { REVEALED_REDACTION_LABEL } from "@/shared/ui/RedactedPill";
import {
  RedactionDictionaryContext,
  type ResolvedRedaction,
} from "@/shared/ui/redactionDictionary";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

/**
 * Redaction-aware text for a one-line tool or command row (SV-75).
 *
 * `RedactedText` puts the amber "only you see this" eye right after every
 * value this machine resolved, which is right for prose but wedges a glyph
 * into the middle of a command — `mkdir -p /tmp/x 👁 && cd /tmp/x 👁` no
 * longer reads as the command that ran. In a row the command stays intact:
 * a resolved value is its plaintext and nothing else, and the disclosure
 * moves beside the row as one `RevealedRedactionsMarker` naming every value
 * it covers. An unresolved marker is still a pill — it stands *in place of*
 * the value, so it is the text, not a glyph beside it.
 */
export function RowRedactedText({ text }: { text: string }) {
  const dictionary = React.useContext(RedactionDictionaryContext);
  const segments = React.useMemo<HiddenContextSegment[]>(
    () => annotateHiddenContext(parseRedactionMarkers(text)),
    [text],
  );
  if (segments.length === 1 && segments[0].kind === "text") {
    return <>{text}</>;
  }
  return (
    <>
      {segments.map((segment, index) => {
        if (segment.kind === "text") {
          // biome-ignore lint/suspicious/noArrayIndexKey: positional segments of one immutable string
          return <React.Fragment key={index}>{segment.text}</React.Fragment>;
        }
        const resolved = dictionary.get(segment.digest);
        if (resolved) {
          return (
            <span
              data-redaction-digest={segment.digest}
              data-redaction-revealed=""
              // biome-ignore lint/suspicious/noArrayIndexKey: positional segments of one immutable string
              key={index}
            >
              {resolved.plaintext}
            </span>
          );
        }
        return (
          <HiddenContextChip
            // biome-ignore lint/suspicious/noArrayIndexKey: positional segments of one immutable string
            key={index}
            marker={segment}
            pathShaped={segment.pathShaped}
          />
        );
      })}
    </>
  );
}

/** One value this machine resolved for a row, keyed by its digest. */
export type RevealedRowValue = ResolvedRedaction & { digest: string };

/** Every distinct value in `texts` that this machine's dictionary resolves. */
export function useRevealedRowValues(
  texts: ReadonlyArray<string | null | undefined>,
): RevealedRowValue[] {
  const dictionary = React.useContext(RedactionDictionaryContext);
  const key = texts.join("\u0000");
  // biome-ignore lint/correctness/useExhaustiveDependencies: `key` is the content of `texts`
  return React.useMemo(() => {
    const seen = new Set<string>();
    const values: RevealedRowValue[] = [];
    for (const text of texts) {
      if (!text) continue;
      for (const segment of parseRedactionMarkers(text)) {
        if (segment.kind !== "redaction" || seen.has(segment.digest)) continue;
        seen.add(segment.digest);
        const resolved = dictionary.get(segment.digest);
        if (resolved) values.push({ ...resolved, digest: segment.digest });
      }
    }
    return values;
  }, [dictionary, key]);
}

/** The accessible name for a row's marker: the label, then the values. */
export function formatRevealedRowLabel(
  values: ReadonlyArray<RevealedRowValue>,
): string {
  const list = values.map((value) => value.plaintext).join(", ");
  return values.length === 1
    ? `${REVEALED_REDACTION_LABEL}: ${list}`
    : `Redacted for other viewers — only you see these values: ${list}`;
}

/**
 * The single amber eye beside a row whose text shows values redacted for
 * everyone else. Renders nothing when the row reveals nothing. The honesty
 * signal is unchanged — this view is privileged, and the marker says so and
 * says for which values; only its placement moved out of the command.
 */
export function RevealedRedactionsMarker({
  className,
  texts,
}: {
  className?: string;
  texts: ReadonlyArray<string | null | undefined>;
}) {
  const values = useRevealedRowValues(texts);
  if (values.length === 0) return null;
  const label = formatRevealedRowLabel(values);
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          aria-label={label}
          className={cn(
            "inline-flex shrink-0 select-none items-center rounded-sm px-0.5 py-px",
            "bg-amber-500/10 text-amber-700 dark:text-amber-400",
            className,
          )}
          data-redaction-revealed-badge=""
          data-redaction-row-marker=""
          data-testid="redaction-row-marker"
          role="img"
        >
          <EyeOff aria-hidden className="size-3" />
        </span>
      </TooltipTrigger>
      <TooltipContent className="max-w-xs">
        <div className="space-y-1">
          <p>
            Only you see{" "}
            {values.length === 1
              ? "this value"
              : `these ${values.length} values`}
            . Everyone else in the channel sees a digest — redacted before the
            transcript was signed.
          </p>
          <ul className="space-y-0.5 font-mono text-2xs">
            {values.map((value) => (
              <li className="wrap-anywhere" key={value.digest}>
                {value.plaintext}
              </li>
            ))}
          </ul>
          <p className="text-2xs opacity-70">
            Recovered from this machine&rsquo;s own record. It expires with the
            session.
          </p>
        </div>
      </TooltipContent>
    </Tooltip>
  );
}
