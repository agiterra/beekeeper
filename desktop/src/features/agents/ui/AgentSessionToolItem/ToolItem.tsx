import * as React from "react";
import { TriangleAlert } from "lucide-react";

import {
  resolveUserLabel,
  type UserProfileLookup,
} from "@/features/profile/lib/identity";
import { cn } from "@/shared/lib/cn";
import { normalizePubkey } from "@/shared/lib/pubkey";
import type { TranscriptItem } from "../agentSessionTypes";
import { getBuzzToolInfo } from "../agentSessionToolCatalog";
import { buildCompactToolSummary } from "../agentSessionToolSummary";
import type { AgentTranscriptIdentityProps } from "../activityRenderClasses/types";
import {
  formatTranscriptTimestampTitle,
  getToolDurationDisplay,
  TOOL_OUTPUT_GAP_HEADLINE,
} from "../agentSessionUtils";
import { CompactMessageSummary } from "./CompactMessageSummary";
import { useCompactToolFailureTone } from "./CompactToolFailureToneContext";
import {
  CompactToolSummaryRow,
  type CompactToolFailureTone,
  compactSummaryTone,
  describeCompactToolFailure,
} from "./CompactToolSummaryRow";
import { getSentMessageLink } from "./messageLinks";
import { isTodoSummary, TodoToolSummary } from "./TodoToolSummary";
import { ToolDetailBlocks } from "./ToolDetailBlocks";
import {
  ACTIVITY_ROW_DETAIL_INSET_CLASS,
  ACTIVITY_ROW_LINE_CLASS,
} from "./ToolItemRowClasses";
import { compactToolRowIcon, ToolItemRowIcon } from "./ToolItemRowIcon";

type ToolItemProps = AgentTranscriptIdentityProps & {
  item: Extract<TranscriptItem, { type: "tool" }>;
  profiles?: UserProfileLookup;
  /**
   * Controlled open state. The coding-session transcript passes it so a row
   * keeps its state across a virtualizer remount; managed-agent views omit
   * it and the row keeps its own.
   */
  open?: boolean;
  /** Called with the new state when the row is toggled; pair with `open`. */
  onOpenChange?: (open: boolean) => void;
  /**
   * How a failed call reads (SV-02, D2): `quiet` for a step inside a settled
   * turn's opened fold, `alarm` everywhere else. Defaults to the nearest
   * `CompactToolFailureToneContext`, which is `alarm` outside any fold.
   */
  failureTone?: CompactToolFailureTone;
  /**
   * Lead the row with its 16px kind glyph (SV-06), as every other activity
   * row in the coding-session transcript does. Off by default, so the
   * managed-agent views keep their own row.
   */
  leadingIcon?: boolean;
};

/**
 * One tool call as a compact row whose detail is one click away.
 *
 * Memoized, and the detail (parameters, output, diff, file content) is built
 * only while the row is open: a long session has hundreds of closed rows, and
 * each used to build its full detail tree on every render. A failed call is
 * the exception — its output is built closed too, so the failure's own words
 * are in the document (find-in-page, screen readers) without a click —
 * unless it is a quiet step inside an opened fold, whose row already says
 * "failed" or the exit code and whose output is one click away.
 */
export const ToolItem = React.memo(function ToolItem({
  agentAvatarUrl,
  agentName,
  agentPubkey,
  failureTone: failureToneProp,
  item,
  leadingIcon = false,
  onOpenChange,
  open,
  profiles,
}: ToolItemProps) {
  const [ownExpanded, setOwnExpanded] = React.useState(false);
  const isExpanded = open ?? ownExpanded;
  const hasArgs = Object.keys(item.args).length > 0;
  const hasResult = item.result.trim().length > 0;
  // `isError` is the tool's own claim; `status === "failed"` is the harness's.
  // Either one means the call failed, and both must reach the detail blocks —
  // passing only `isError` below is what left failed shell calls rendering an
  // empty output panel.
  const failed = item.isError || item.status === "failed";
  const contextFailureTone = useCompactToolFailureTone();
  const failureTone = failureToneProp ?? contextFailureTone;
  const quietFailure = failed && failureTone === "quiet";
  const canonicalToolName = item.buzzToolName ?? item.toolName;
  const buzzTool = getBuzzToolInfo(canonicalToolName);
  const compactSummary = React.useMemo(
    () => buildCompactToolSummary(item),
    [item],
  );
  const duration = getToolDurationDisplay(item);
  const messageLink = getSentMessageLink(item);
  const timestampTitle = formatTranscriptTimestampTitle(item.timestamp);
  const agentProfile = profiles?.[normalizePubkey(agentPubkey)] ?? null;
  const agentLabel = resolveUserLabel({
    pubkey: agentPubkey,
    fallbackName: agentName,
    profiles,
    preferResolvedSelfLabel: true,
  });
  const agentResolvedAvatarUrl = agentProfile?.avatarUrl ?? agentAvatarUrl;
  const handleToggle = React.useCallback(
    (event: React.SyntheticEvent<HTMLDetailsElement>) => {
      const next = event.currentTarget.open;
      if (onOpenChange) onOpenChange(next);
      else setOwnExpanded(next);
    },
    [onOpenChange],
  );

  if (compactSummary.presentation === "message") {
    return (
      <div
        className="not-prose w-full"
        data-testid="transcript-tool-item"
        title={timestampTitle}
      >
        <CompactMessageSummary
          args={item.args}
          avatarUrl={agentResolvedAvatarUrl}
          description={buzzTool?.label}
          displayName={agentLabel}
          duration={duration}
          hasArgs={hasArgs}
          hasResult={hasResult}
          isError={failed}
          label={compactSummary.label}
          messageLink={messageLink}
          preview={compactSummary.preview}
          pubkey={agentPubkey}
          result={item.result}
          timestamp={item.timestamp}
        />
      </div>
    );
  }

  if (isTodoSummary(compactSummary)) {
    return (
      <div
        className="not-prose w-full"
        data-testid="transcript-tool-item"
        title={timestampTitle}
      >
        <TodoToolSummary
          duration={duration}
          fallbackPreview={compactSummary.preview}
          item={item}
        />
      </div>
    );
  }

  return (
    <div
      className="not-prose w-full"
      data-testid="transcript-tool-item"
      title={timestampTitle}
    >
      <details
        className="group w-full"
        onToggle={handleToggle}
        open={isExpanded}
      >
        <summary
          // SV-01: the whole line is the target and fills on hover. The
          // tone stays the summary's own, so a failure keeps its colour.
          className={cn(
            "group/row cursor-pointer list-none",
            ACTIVITY_ROW_LINE_CLASS,
            compactSummaryTone(failed, failureTone),
          )}
          data-leading-icon={leadingIcon ? "" : undefined}
        >
          {leadingIcon ? (
            <ToolItemRowIcon
              failed={failed}
              icon={compactToolRowIcon(compactSummary)}
              quiet={quietFailure}
            />
          ) : null}
          <CompactToolSummaryRow
            action={compactSummary.action}
            duration={duration}
            failed={failed}
            failureDetail={
              quietFailure ? describeCompactToolFailure(item.result) : null
            }
            failureTone={failureTone}
            fileEditSummary={compactSummary.fileEditSummary}
            // With a leading glyph the failure mark moves onto that glyph,
            // so the summary row drops its own `CircleX` rather than draw it
            // twice. Its words ("Tool call failed", "· exit 2") stay.
            hasLeadingIcon={leadingIcon}
            kind={compactSummary.kind}
            preview={compactSummary.preview}
            thumbnailSrc={compactSummary.thumbnailSrc}
            label={compactSummary.label}
          />
          {item.outputGap ? (
            // The full notice is in the details; the collapsed row still has
            // to say the output is partial, or nobody opens it to find out.
            <TriangleAlert
              aria-label={TOOL_OUTPUT_GAP_HEADLINE}
              className="size-3.5 shrink-0 text-amber-700 dark:text-amber-400"
              data-testid="transcript-tool-output-gap-marker"
              role="img"
            >
              <title>{TOOL_OUTPUT_GAP_HEADLINE}</title>
            </TriangleAlert>
          ) : null}
        </summary>

        {isExpanded || (failed && !quietFailure) ? (
          // With a leading glyph, the detail starts under the label (T3's
          // `WorkLogDetails` inset), not under the glyph.
          <div
            className={
              leadingIcon ? ACTIVITY_ROW_DETAIL_INSET_CLASS : undefined
            }
          >
            <ToolDetailBlocks
              args={item.args}
              description={buzzTool?.label}
              fileEditDiff={compactSummary.fileEditDiff}
              fileReadContent={compactSummary.fileReadContent}
              hasArgs={hasArgs}
              hasResult={hasResult}
              imagePreview={
                compactSummary.imageContent != null && isExpanded
                  ? {
                      src: compactSummary.imageContent.src,
                      title: compactSummary.imageContent.title,
                    }
                  : null
              }
              isError={failed}
              outputGap={item.outputGap}
              result={item.result}
              shellCommand={compactSummary.shellContent}
            />
          </div>
        ) : null}
      </details>
    </div>
  );
});
