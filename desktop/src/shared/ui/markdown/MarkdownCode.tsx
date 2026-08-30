/**
 * The `code` component of the markdown renderer — inline chips and fenced
 * blocks — and the one place a coding-session redaction marker inside code is
 * turned into a pill.
 *
 * `remarkRedactionMarkers` handles prose but cannot reach here: a
 * `code`/`inlineCode` node carries a string `value`, not children, so there is
 * nothing for a remark plugin to split. That is not a corner case — agents
 * write host paths in backticks and paste console output into fences, which is
 * where most redactions in a real session land (2026-08-30: five of twelve in
 * one measured transcript). Leaving them literal showed ninety characters of
 * hash and, on the machine that redacted them, put the value out of reach of
 * its own vault.
 */

import type React from "react";

import { cn } from "@/shared/lib/cn";
import { hasRedactionMarker } from "@/shared/lib/redactionMarker";
import { INLINE_CODE_CHIP_CLASS } from "@/shared/ui/mentionChip";
import { RedactedText } from "@/shared/ui/RedactedPill";

import {
  CODE_BLOCK_CLASS,
  extractLanguage,
  SyntaxHighlightedCode,
} from "./CodeBlock";

/**
 * Build the `code` renderer for one component variant.
 *
 * A factory rather than a component because `interactive` is fixed per variant
 * and the component maps are cached module-side; closing over it here keeps
 * cached element trees free of per-mount closures.
 */
export function createCodeComponent(interactive: boolean) {
  return function MarkdownCode({
    children,
    className,
    ...props
  }: React.ComponentProps<"code">) {
    const rawCode = String(children);
    const code = rawCode.replace(/\n$/, "");
    const isFencedCodeBlock =
      typeof className === "string" && className.includes("language-");
    const hasRedaction = hasRedactionMarker(code);

    if (isFencedCodeBlock || rawCode.endsWith("\n") || code.includes("\n")) {
      const language = extractLanguage(className);

      // A fence holding a redaction gives up highlighting rather than the
      // pill: pills cannot be threaded through Shiki's tokens, and a block
      // whose content was withheld before signing is not faithful source
      // anyway.
      if (language && !hasRedaction) {
        return (
          <SyntaxHighlightedCode code={code} language={language} {...props} />
        );
      }

      const lines = code.split("\n");
      return (
        <code {...props} className={CODE_BLOCK_CLASS}>
          {lines.map((line, i) => (
            // biome-ignore lint/suspicious/noArrayIndexKey: lines are positional
            <span key={i} data-line="">
              {hasRedaction ? (
                <RedactedText interactive={interactive} text={line} />
              ) : (
                line
              )}
            </span>
          ))}
        </code>
      );
    }

    return (
      <code {...props} className={cn(INLINE_CODE_CHIP_CLASS, className)}>
        {hasRedaction ? (
          <RedactedText interactive={interactive} text={rawCode} />
        ) : (
          children
        )}
      </code>
    );
  };
}
