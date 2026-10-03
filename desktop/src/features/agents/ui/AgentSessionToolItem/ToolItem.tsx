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
import {
  CompactToolSummaryRow,
  compactSummaryTone,
} from "./CompactToolSummaryRow";
import { getSentMessageLink } from "./messageLinks";
import { isTodoSummary, TodoToolSummary } from "./TodoToolSummary";
import { ToolDetailBlocks } from "./ToolDetailBlocks";

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
};

/**
 * One tool call as a compact row whose detail is one click away.
 *
 * Memoized, and the detail (parameters, output, diff, file content) is built
 * only while the row is open: a long session has hundreds of closed rows, and
 * each used to build its full detail tree on every render. A failed call is
 * the exception — its output is built closed too, so the failure's own words
 * are in the document (find-in-page, screen readers) without a click.
 */
export const ToolItem = React.memo(function ToolItem({
  agentAvatarUrl,
  agentName,
  agentPubkey,
  item,
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
          className={cn(
            "group/row flex min-h-6 max-w-full cursor-pointer list-none items-center gap-1.5",
            compactSummaryTone(failed),
          )}
        >
          <CompactToolSummaryRow
            action={compactSummary.action}
            duration={duration}
            failed={failed}
            fileEditSummary={compactSummary.fileEditSummary}
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

        {isExpanded || failed ? (
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
        ) : null}
      </details>
    </div>
  );
});
