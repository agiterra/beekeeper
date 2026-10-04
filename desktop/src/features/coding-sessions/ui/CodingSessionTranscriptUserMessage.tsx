import type * as React from "react";

import { cn } from "@/shared/lib/cn";
import { useCodingSessionDisclosure } from "./CodingSessionTranscriptDisclosure";

/**
 * A long prompt clamps (SV-15), as T3 Code's does: past eight lines or 600
 * characters the bubble shows its top under a fade, with "Show full message"
 * beneath it.
 *
 * Clamped is not cut: every word stays in the document, so find-in-page,
 * selection and assistive technology still reach the whole prompt. The
 * open state lives in the transcript's disclosure store under
 * `prompt:<item id>`, so a prompt opened, scrolled out of the virtualizer's
 * window and back is still open.
 */

const MAX_CLAMPED_LINES = 8;
const MAX_CLAMPED_CHARACTERS = 600;
const FADE_MASK =
  "linear-gradient(to bottom, black calc(100% - 1.75rem), transparent)";

export function shouldClampCodingSessionUserMessage(text: string): boolean {
  if (text.trim().length === 0) return false;
  return (
    text.length > MAX_CLAMPED_CHARACTERS ||
    text.split("\n").length > MAX_CLAMPED_LINES
  );
}

export function codingSessionPromptDisclosureId(itemId: string): string {
  return `prompt:${itemId}`;
}

export function CodingSessionClampedUserMessage({
  children,
  itemId,
  text,
}: {
  children: React.ReactNode;
  itemId: string;
  /** The prompt's text, which decides whether it clamps at all. */
  text: string;
}) {
  const [expanded, setExpanded] = useCodingSessionDisclosure(
    codingSessionPromptDisclosureId(itemId),
  );
  const canClamp = shouldClampCodingSessionUserMessage(text);
  const clamped = canClamp && !expanded;
  return (
    <>
      <div
        className={cn("relative", clamped && "max-h-44 overflow-hidden")}
        data-testid="coding-session-user-message-body"
        data-user-message-clamped={clamped ? "true" : "false"}
        style={
          clamped
            ? { WebkitMaskImage: FADE_MASK, maskImage: FADE_MASK }
            : undefined
        }
      >
        {children}
      </div>
      {canClamp ? (
        <div className="mt-1.5 flex justify-end">
          <button
            aria-expanded={expanded}
            className="-me-1 rounded-md px-1.5 py-0.5 text-xs text-muted-foreground transition-colors hover:bg-accent/30 hover:text-foreground"
            data-testid="coding-session-user-message-toggle"
            onClick={() => setExpanded(!expanded)}
            type="button"
          >
            {expanded ? "Show less" : "Show full message"}
          </button>
        </div>
      ) : null}
    </>
  );
}
