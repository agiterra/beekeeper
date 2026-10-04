import * as React from "react";
import { ChevronDown, CircleX } from "lucide-react";

import { cn } from "@/shared/lib/cn";
import { RedactedText } from "@/shared/ui/RedactedPill";
import { useAgentSessionTranscriptVariant } from "../agentSessionTranscriptContext";
import type { AgentActivityAction } from "../agentSessionTypes";
import type {
  CompactFileEditSummary,
  CompactToolKind,
} from "../agentSessionToolSummary";
import { resolveToolImageSrc } from "../agentSessionUtils";
import {
  ActivityRowLabel,
  splitActivityRowLabel,
  type ActivityRowLabelParts,
} from "../activityRenderClasses/ActivityRow";
import { useCompactToolFailureTone } from "./CompactToolFailureToneContext";
import type { CompactToolFailureTone } from "./CompactToolSummaryRowFailure";

export {
  CompactToolFailureToneContext,
  useCompactToolFailureTone,
} from "./CompactToolFailureToneContext";
export {
  type CompactToolFailureTone,
  describeCompactToolFailure,
} from "./CompactToolSummaryRowFailure";

/**
 * Tone for a collapsed tool row.
 *
 * A failed call keeps its destructive tone in every state — the usual
 * hover/open brightening would make failure read as an ordinary row the moment
 * the pointer crossed it. A `quiet` failure (a step inside a settled turn's
 * fold, SV-02) reads as an ordinary row; its icon and suffix mark it.
 */
export function compactSummaryTone(
  failed = false,
  failureTone: CompactToolFailureTone = "alarm",
) {
  return failed && failureTone === "alarm"
    ? "text-destructive transition-colors"
    : "text-muted-foreground/60 transition-colors group-hover/row:text-foreground group-open:text-foreground";
}

export function CompactToolSummaryRow({
  action,
  duration,
  failed,
  failureDetail = null,
  failureTone: failureToneProp,
  fileEditSummary,
  kind,
  label,
  preview,
  thumbnailSrc,
}: {
  action: AgentActivityAction | null;
  duration: string | null;
  failed: boolean;
  /**
   * The quiet row's suffix, e.g. `describeCompactToolFailure(item.result)`
   * ("exit 2"). Defaults to "failed"; ignored by the alarm row.
   */
  failureDetail?: string | null;
  /**
   * `quiet` inside a settled turn's fold; `alarm` everywhere else. Omitted,
   * it comes from `CompactToolFailureToneContext`, which the fold provides.
   */
  failureTone?: CompactToolFailureTone;
  fileEditSummary: CompactFileEditSummary | null;
  kind: CompactToolKind;
  label: string;
  preview: string | null;
  thumbnailSrc: string | null;
}) {
  const [thumbnailFailed, setThumbnailFailed] = React.useState(false);
  const variant = useAgentSessionTranscriptVariant();
  const contextFailureTone = useCompactToolFailureTone();
  const failureTone = failureToneProp ?? contextFailureTone;
  const isCompactPreview = variant === "compactPreview";
  const tone = compactSummaryTone(failed, failureTone);
  const resolvedThumbnail = React.useMemo(() => {
    if (!thumbnailSrc || thumbnailFailed) return null;
    return resolveToolImageSrc(thumbnailSrc);
  }, [thumbnailFailed, thumbnailSrc]);
  const actionLabel = fileEditSummary
    ? null
    : getCompactToolActionLabel(action, kind, label, preview);

  return (
    <>
      {failed && failureTone === "quiet" ? (
        <QuietFailedToolLabel
          action={action}
          detail={failureDetail}
          label={label}
          preview={preview}
        />
      ) : failed ? (
        <span className="inline-flex min-w-0 items-center gap-1.5 font-semibold text-destructive">
          <CircleX className="size-3.5 shrink-0" />
          <span className="shrink-0">Tool call failed</span>
          {preview ? (
            <span className="min-w-0 truncate font-normal" title={preview}>
              <RedactedText text={preview} />
            </span>
          ) : null}
        </span>
      ) : fileEditSummary ? (
        <CompactFileEditSummaryView summary={fileEditSummary} />
      ) : actionLabel ? (
        <ActivityRowLabel
          object={actionLabel.object}
          openToneScope="tool"
          title={actionLabel.title}
          verb={actionLabel.verb}
        />
      ) : (
        <span
          className={cn(
            "shrink-0 font-semibold",
            isCompactPreview ? "text-xs" : "text-sm",
            tone,
          )}
        >
          <RedactedText text={label} />
        </span>
      )}
      {!fileEditSummary && resolvedThumbnail ? (
        <img
          alt=""
          className="h-5 w-auto max-w-12 shrink-0 rounded-sm object-cover"
          decoding="async"
          loading="lazy"
          onError={() => setThumbnailFailed(true)}
          src={resolvedThumbnail}
          title={preview ?? undefined}
        />
      ) : !fileEditSummary && !actionLabel && preview ? (
        <span
          className={cn(
            "min-w-0 max-w-48 truncate",
            isCompactPreview ? "text-xs" : "text-sm",
            tone,
          )}
          title={preview}
        >
          <RedactedText text={preview} />
        </span>
      ) : null}
      {duration ? (
        <span className={cn("shrink-0 text-xs", tone)}>{duration}</span>
      ) : null}
      <ChevronDown
        className={cn(
          "h-3.5 w-3.5 shrink-0 transition-transform group-open:rotate-180",
          tone,
        )}
      />
    </>
  );
}

/**
 * A failed step inside a fold: dimmed red icon, the call in row text, then
 * "· failed" (or "· exit 2"). A call that ran a command keeps its "Ran …"
 * wording, which stays true of a command that exited non-zero; anything else
 * keeps its "… failed" label, because "Edited foo.rs" beside a failure would
 * claim an edit that did not happen.
 */
function QuietFailedToolLabel({
  action,
  detail,
  label,
  preview,
}: {
  action: AgentActivityAction | null;
  detail: string | null;
  label: string;
  preview: string | null;
}) {
  const ran = action?.verb === "Ran" && action.object ? action : null;
  const object = ran ? ran.object : preview;
  return (
    // The row sets its own colour: a caller that tones the whole <summary>
    // with `compactSummaryTone(failed)` would otherwise repaint it red.
    <span
      className={cn(
        "inline-flex min-w-0 items-center gap-1.5",
        compactSummaryTone(true, "quiet"),
      )}
      data-failure-tone="quiet"
    >
      <CircleX
        aria-label="Failed"
        className="size-3.5 shrink-0 text-destructive/40"
        role="img"
      />
      <span className="shrink-0 font-semibold">
        <RedactedText text={ran ? ran.verb : label} />
      </span>
      {object ? (
        <span className="min-w-0 truncate font-normal" title={object}>
          <RedactedText text={object} />
        </span>
      ) : null}
      {ran || detail ? (
        <span className="shrink-0 font-normal text-muted-foreground/70">
          · {detail ?? "failed"}
        </span>
      ) : null}
    </span>
  );
}

function getCompactToolActionLabel(
  action: AgentActivityAction | null,
  kind: CompactToolKind,
  label: string,
  preview: string | null,
): (ActivityRowLabelParts & { title?: string }) | null {
  if (action) {
    const object = action.object ?? preview ?? undefined;
    return {
      verb: action.verb,
      object,
      title: typeof object === "string" ? object : undefined,
    };
  }

  const parts = splitActivityRowLabel(label);
  if (!parts) return null;

  if (!preview) return parts;

  if (
    kind === "shell" ||
    kind === "file-read" ||
    kind === "skill-read" ||
    kind === "plan" ||
    kind === "image"
  ) {
    return { verb: parts.verb, object: preview, title: preview };
  }

  return parts;
}

function CompactFileEditSummaryView({
  summary,
}: {
  summary: CompactFileEditSummary;
}) {
  return (
    <ActivityRowLabel
      className="max-w-72"
      object={summary.filename}
      openToneScope="tool"
      stats={{
        additions: summary.additions,
        deletions: summary.deletions,
      }}
      title={summary.path}
      verb="Edited"
    />
  );
}
