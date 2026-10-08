import type * as React from "react";
import { Bot, HardDrive } from "lucide-react";

import { SESSION_PREVIEW_LOCAL_ONLY_LABEL } from "../lib/previewModel";

/**
 * The honesty strip under every preview: who is driving (when an agent is),
 * and whether the page is shared (`share`, C5's share strip) or stays on
 * this computer. `bindingNote` names what the preview is bound to, when that
 * needs saying.
 */
export function SessionPreviewStrip({
  bindingNote,
  drivingText,
  share,
}: {
  bindingNote: string | null;
  drivingText: string | null;
  /** The share strip; absent, the C4 "local only" label stands. */
  share?: React.ReactNode;
}) {
  return (
    <div
      className="flex min-h-7 shrink-0 flex-wrap items-center gap-x-3 gap-y-1 border-t border-border/60 px-2.5 py-1 text-2xs text-muted-foreground"
      data-testid="session-preview-strip"
    >
      {drivingText ? (
        <span
          className="flex items-center gap-1 font-medium text-foreground"
          data-testid="session-preview-driving"
        >
          <Bot aria-hidden className="size-3" />
          {drivingText}
        </span>
      ) : null}
      {share ?? (
        <span
          className="flex items-center gap-1"
          data-testid="session-preview-local-only"
        >
          <HardDrive aria-hidden className="size-3" />
          {SESSION_PREVIEW_LOCAL_ONLY_LABEL}
        </span>
      )}
      {bindingNote ? (
        <span data-testid="session-preview-binding-note">{bindingNote}</span>
      ) : null}
    </div>
  );
}
