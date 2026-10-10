import { EyeOff } from "lucide-react";

import { cn } from "@/shared/lib/cn";

import { useSessionPreviewState } from "../hooks/useSessionPreviewState";
import { sessionPreviewHiddenNotice } from "../lib/previewTabModel";

/**
 * The Browser tab's badge: a page is open for this session but drawn
 * nowhere (an agent opened it while the tab was not on screen, or the panel
 * is hidden). Without it a live page — holding a dev server and taking agent
 * input — would be invisible (ledger 371(d)). Nothing hidden, no badge.
 */
export function SessionPreviewHiddenBadge({
  channelId,
  slot,
}: {
  channelId: string;
  slot: "launcher" | "tab" | "header";
}) {
  const { state } = useSessionPreviewState(channelId || null);
  const notice = sessionPreviewHiddenNotice(state);
  if (!notice) return null;
  const label = `${notice}. Open the Browser tab to show it.`;
  return (
    <span
      aria-label={label}
      className={cn(
        "flex items-center justify-center rounded-full bg-muted px-1 text-muted-foreground",
        slot === "header" ? "h-4" : "h-3.5",
      )}
      data-testid="session-preview-hidden-badge"
      data-tone="attention"
      role="status"
      title={label}
    >
      <EyeOff aria-hidden className="size-2.5" />
    </span>
  );
}
