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

import { inlineCodeFilePathCandidate } from "@/features/coding-sessions/lib/filePathCandidate";
import { cn } from "@/shared/lib/cn";
import { hasRedactionMarker } from "@/shared/lib/redactionMarker";
import { INLINE_CODE_CHIP_CLASS } from "@/shared/ui/mentionChip";
import { RedactedText } from "@/shared/ui/RedactedPill";

import {
  CODE_BLOCK_CLASS,
  extractLanguage,
  SyntaxHighlightedCode,
} from "./CodeBlock";
import { FileChip, FileRefPlainCode } from "./FileChip";
import { presentFileRef, useFileRefScope } from "./fileRefContext";

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

    // SV-32: a path the agent wrote may become a chip. The scope is read in
    // a child component, so this one stays hook-free (and callable directly).
    const candidate =
      interactive && !hasRedaction
        ? inlineCodeFilePathCandidate(rawCode)
        : null;
    if (candidate) {
      return (
        <FileRefInlineCode
          {...props}
          candidate={candidate}
          className={className}
        >
          {children}
        </FileRefInlineCode>
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

/**
 * Inline code naming a file: a chip on the computer that ran the session,
 * plain code with the reason elsewhere in a session, and untouched outside a
 * coding-session answer (no `FileRefContext`).
 */
function FileRefInlineCode({
  candidate,
  children,
  className,
  ...props
}: React.ComponentProps<"code"> & { candidate: string }) {
  const fileRefScope = useFileRefScope();
  const fileRef = presentFileRef(fileRefScope, candidate);
  if (fileRef.kind === "chip" && fileRefScope) {
    return (
      <FileChip
        candidate={candidate}
        fileRef={fileRef.ref}
        scope={fileRefScope}
      />
    );
  }
  if (fileRef.kind === "plain") {
    return (
      <FileRefPlainCode
        {...props}
        className={className}
        reason={fileRef.reason}
      >
        {children}
      </FileRefPlainCode>
    );
  }
  return (
    <code {...props} className={cn(INLINE_CODE_CHIP_CLASS, className)}>
      {children}
    </code>
  );
}
