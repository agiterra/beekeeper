import * as React from "react";
import {
  Check,
  ChevronsDownUp,
  ChevronsUpDown,
  Copy,
  WrapText,
} from "lucide-react";
import { toast } from "sonner";
import {
  getSingletonHighlighter,
  type HighlighterGeneric,
  type BundledLanguage,
  type BundledTheme,
  type ThemedToken,
} from "shiki";

import { useTheme } from "@/shared/theme/ThemeProvider";
import { resolveShikiThemeName } from "@/shared/theme/theme-loader";
import { copyCodeBlockToClipboard } from "@/shared/lib/codeBlockClipboard";
import { Button } from "@/shared/ui/button";
import { useSmoothCorners } from "@/shared/ui/smoothCorners";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/shared/ui/tooltip";

import type { Components } from "react-markdown";

import {
  CodeBlockLanguageLabel,
  extractFenceTitle,
  extractPreCodeMeta,
} from "./CodeBlockLanguage";
import { useExceedsHeightCap } from "./useExceedsHeightCap";
import { useHorizontalOverflow } from "./useHorizontalOverflow";
import { getReactNodeText } from "./utils";

let shikiHighlighter: HighlighterGeneric<BundledLanguage, BundledTheme> | null =
  null;
let shikiInitPromise: Promise<void> | null = null;
const loadedLangs = new Set<string>();
const loadedThemes = new Set<string>();
const tokenCache = new Map<string, ThemedToken[][]>();
const MAX_CACHE_ENTRIES = 100;
const MAX_LOADED_LANGUAGES = 30;
const MAX_HIGHLIGHT_LINES = 150;
export const CODE_BLOCK_CLASS =
  "code-block-lines block min-w-full whitespace-pre font-mono text-sm font-normal text-foreground";
/**
 * Shared chrome for the code-block header actions (wrap, copy). Always
 * visible (SV-12): an action that appears only on hover is invisible to anyone
 * who has not already found it. A pressed wrap toggle keeps a filled state.
 */
const CODE_BLOCK_ACTION_CLASS =
  "text-muted-foreground hover:bg-accent/60 hover:text-foreground aria-pressed:bg-accent aria-pressed:text-foreground disabled:opacity-60";
const DIFF_ADD_RE = /\s*\/\/\s*\[!code\s*\+\+\]\s*$/;
const DIFF_REMOVE_RE = /\s*\/\/\s*\[!code\s*--\]\s*$/;

function ensureHighlighter(): Promise<void> {
  if (shikiHighlighter) return Promise.resolve();
  if (!shikiInitPromise) {
    shikiInitPromise = getSingletonHighlighter({
      themes: [],
      langs: [],
    }).then((h) => {
      shikiHighlighter = h;
    });
  }
  return shikiInitPromise;
}

export function extractLanguage(className?: string): string {
  if (typeof className !== "string") return "";
  const match = className.match(/language-(\S+)/);
  return match ? match[1] : "";
}

function stripDiffMarker(tokens: ThemedToken[], marker: RegExp): ThemedToken[] {
  const last = tokens[tokens.length - 1];
  if (!last) return tokens;
  const stripped = last.content.replace(marker, "");
  if (stripped === last.content) return tokens;
  if (stripped === "") return tokens.slice(0, -1);
  return [...tokens.slice(0, -1), { ...last, content: stripped }];
}

function getCodeBlockText(children: React.ReactNode) {
  return getReactNodeText(children).replace(/\n$/, "");
}

export function StaticCodeBlock({ children }: { children?: React.ReactNode }) {
  return (
    <pre className="buzz-code-scrollbar max-w-full overflow-x-auto rounded-lg border border-border/70 bg-muted/60 px-3 py-1.5">
      {children}
    </pre>
  );
}

/** How long the copy button shows its check after a successful copy. */
const COPIED_FEEDBACK_MS = 1200;

/**
 * A fenced code block, everywhere markdown renders — channels, threads and
 * coding sessions alike (SV-12, decision D4).
 *
 * The header carries the fence's file title when it declares one
 * (```ts title="x.ts"), otherwise its language (icon and name), and the
 * block's two actions, wrap and copy, always visible rather than revealed on
 * hover, the way T3 Code draws them (`ChatMarkdown.tsx` `MarkdownCodeBlock`):
 * a `rounded-lg` (`--radius`, 10px) box, no shadow, lines wrapped by default. The `pre` stays the direct child of
 * `[data-code-block]` because the session column and the width audit both
 * address it as `[data-code-block] > pre`, and the overflow fade masks only
 * the `pre`, never the header.
 *
 * Height: in channels and threads a block is capped at 25rem and scrolls
 * inside itself (`markdown.css`, `data-height-capped`), so a pasted 500-line
 * log does not take over the timeline. When the code is taller than the cap,
 * the header shows an always-visible expand control that lifts it. Inside a
 * coding session (`[data-coding-session-column]`) the cap does not apply —
 * T3 Code's block is uncapped and the page scroll reads it — so the control
 * never appears there: it is drawn only when the cap is actually hiding lines.
 */
export function MarkdownCodeBlock({
  children,
  language,
  title,
}: {
  children?: React.ReactNode;
  language?: string;
  /** A file name the fence declared in its meta (`title="x.ts"`). */
  title?: string | null;
}) {
  const [isCopying, setIsCopying] = React.useState(false);
  const [isCopied, setIsCopied] = React.useState(false);
  // Default on, as T3 Code's is: its `wordWrap` client setting decodes to
  // `true` (`packages/contracts/src/settings.ts`), so a long line wraps and
  // nothing is hidden past the right edge until someone asks to scroll. The
  // toggle is per block and not persisted — Beekeeper has no settings surface
  // for a global word-wrap preference yet.
  const [isWrapped, setIsWrapped] = React.useState(true);
  const [isHeightExpanded, setIsHeightExpanded] = React.useState(false);
  const blockRef = React.useRef<HTMLDivElement | null>(null);
  const codeBlockRef = React.useRef<HTMLPreElement | null>(null);
  const copiedTimerRef = React.useRef<ReturnType<typeof setTimeout> | null>(
    null,
  );
  const code = React.useMemo(() => getCodeBlockText(children), [children]);
  useSmoothCorners(blockRef);
  const [hasHiddenOverflow, measureOverflow] = useHorizontalOverflow(
    codeBlockRef,
    [code, isWrapped],
  );
  const exceedsCap = useExceedsHeightCap(codeBlockRef, isHeightExpanded, [
    code,
    isWrapped,
  ]);

  React.useEffect(
    () => () => {
      if (copiedTimerRef.current != null) clearTimeout(copiedTimerRef.current);
    },
    [],
  );

  const handleCopy = React.useCallback(
    async (event: React.MouseEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      setIsCopying(true);

      try {
        await copyCodeBlockToClipboard(code);
        if (copiedTimerRef.current != null) {
          clearTimeout(copiedTimerRef.current);
        }
        setIsCopied(true);
        copiedTimerRef.current = setTimeout(() => {
          setIsCopied(false);
          copiedTimerRef.current = null;
        }, COPIED_FEEDBACK_MS);
      } catch (error) {
        // A failed copy is said out loud; only success is quiet.
        console.error("Failed to copy code block", error);
        toast.error("Failed to copy code");
      } finally {
        setIsCopying(false);
      }
    },
    [code],
  );

  const handleToggleWrap = React.useCallback(
    (event: React.MouseEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      setIsWrapped((previous) => !previous);
    },
    [],
  );

  const handleToggleHeight = React.useCallback(
    (event: React.MouseEvent<HTMLButtonElement>) => {
      event.preventDefault();
      event.stopPropagation();
      setIsHeightExpanded((previous) => !previous);
    },
    [],
  );

  const heightLabel = isHeightExpanded
    ? "Cap this code block's height"
    : "Show the whole code block";

  const wrapLabel = isWrapped
    ? "Stop wrapping long lines in this code block"
    : "Wrap long lines in this code block";

  return (
    <div
      ref={blockRef}
      className="relative min-w-0 overflow-hidden rounded-lg border border-border/70 bg-muted/60"
      data-code-block=""
      data-language={language || undefined}
      data-overflow={hasHiddenOverflow ? "true" : "false"}
      data-wrap={isWrapped ? "true" : "false"}
      data-height-capped={isHeightExpanded ? "false" : "true"}
      style={{ borderRadius: "var(--radius)" }}
    >
      <div
        className="flex select-none items-center justify-between gap-2 pb-0 pl-3 pr-1.5 pt-1.5"
        data-code-block-header=""
      >
        <CodeBlockLanguageLabel language={language ?? ""} title={title} />
        <div
          aria-label="Code block actions"
          className="flex shrink-0 items-center gap-0.5"
          role="toolbar"
        >
          {exceedsCap ? (
            <Tooltip>
              <TooltipTrigger asChild>
                <Button
                  aria-expanded={isHeightExpanded}
                  aria-label={heightLabel}
                  className={CODE_BLOCK_ACTION_CLASS}
                  data-testid="code-block-height-toggle"
                  onClick={handleToggleHeight}
                  size="icon-xs"
                  type="button"
                  variant="ghost"
                >
                  {isHeightExpanded ? (
                    <ChevronsDownUp aria-hidden="true" />
                  ) : (
                    <ChevronsUpDown aria-hidden="true" />
                  )}
                </Button>
              </TooltipTrigger>
              <TooltipContent>
                {isHeightExpanded ? "Collapse" : "Show all lines"}
              </TooltipContent>
            </Tooltip>
          ) : null}
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                aria-label={wrapLabel}
                aria-pressed={isWrapped}
                className={CODE_BLOCK_ACTION_CLASS}
                data-testid="code-block-wrap-toggle"
                onClick={handleToggleWrap}
                size="icon-xs"
                type="button"
                variant="ghost"
              >
                <WrapText aria-hidden="true" />
              </Button>
            </TooltipTrigger>
            <TooltipContent>
              {isWrapped ? "Scroll long lines" : "Wrap long lines"}
            </TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                aria-label="Copy code block"
                className={CODE_BLOCK_ACTION_CLASS}
                data-copied={isCopied ? "true" : "false"}
                data-testid="code-block-copy"
                disabled={isCopying}
                onClick={handleCopy}
                size="icon-xs"
                type="button"
                variant="ghost"
              >
                {isCopied ? (
                  <Check aria-hidden="true" />
                ) : (
                  <Copy aria-hidden="true" />
                )}
              </Button>
            </TooltipTrigger>
            <TooltipContent>{isCopied ? "Copied" : "Copy code"}</TooltipContent>
          </Tooltip>
        </div>
      </div>
      <pre
        onScroll={measureOverflow}
        ref={codeBlockRef}
        className="buzz-code-scrollbar max-w-full overflow-x-auto px-3 pb-2 pt-0.5"
      >
        {children}
      </pre>
    </div>
  );
}

/**
 * The markdown renderer's `pre`: a fenced block with the shared chrome
 * (language or file title, wrap, copy) when interactive, a plain contained
 * block in previews and search rows.
 */
export function createPreComponent(
  interactive: boolean,
): NonNullable<Components["pre"]> {
  return function MarkdownPre({ children, node }) {
    if (!interactive) return <StaticCodeBlock>{children}</StaticCodeBlock>;
    let language = "";
    React.Children.forEach(children, (child) => {
      if (
        React.isValidElement<Record<string, unknown>>(child) &&
        typeof child.props?.className === "string"
      ) {
        language = extractLanguage(child.props.className);
      }
    });
    return (
      <MarkdownCodeBlock
        language={language}
        title={extractFenceTitle(extractPreCodeMeta(node))}
      >
        {children}
      </MarkdownCodeBlock>
    );
  };
}

export function SyntaxHighlightedCode({
  code,
  language,
  ...props
}: {
  code: string;
  language: string;
} & React.ComponentProps<"code">) {
  const { themeName } = useTheme();
  // Buzz aliases ("buzz" / "buzz-dark") are not bundled Shiki themes — resolve
  // to the real bundle (github-light / github-dark) before touching Shiki, or
  // it throws and code blocks fall back to plain text.
  const shikiTheme = resolveShikiThemeName(themeName);
  const [loadedKey, setLoadedKey] = React.useState(0);

  React.useEffect(() => {
    let cancelled = false;
    async function loadAssets() {
      try {
        await ensureHighlighter();
        if (!shikiHighlighter || cancelled) return;
        let loaded = false;
        if (!loadedLangs.has(language)) {
          if (loadedLangs.size >= MAX_LOADED_LANGUAGES) return;
          try {
            await shikiHighlighter.loadLanguage(language as BundledLanguage);
            loadedLangs.add(language);
            loaded = true;
          } catch {
            return;
          }
        }
        if (!loadedThemes.has(shikiTheme)) {
          try {
            await shikiHighlighter.loadTheme(shikiTheme as BundledTheme);
            loadedThemes.add(shikiTheme);
            loaded = true;
          } catch {
            return;
          }
        }
        if (loaded && !cancelled) setLoadedKey((k) => k + 1);
      } catch {
        /* ignore */
      }
    }
    if (!loadedLangs.has(language) || !loadedThemes.has(shikiTheme)) {
      loadAssets();
    }
    return () => {
      cancelled = true;
    };
  }, [language, shikiTheme]);

  // biome-ignore lint/correctness/useExhaustiveDependencies: loadedKey intentionally triggers re-memoization after async asset loading
  const tokens = React.useMemo(() => {
    if (
      !shikiHighlighter ||
      !loadedLangs.has(language) ||
      !loadedThemes.has(shikiTheme)
    )
      return null;
    if ((code.match(/\n/g) || []).length > MAX_HIGHLIGHT_LINES) return null;
    const cacheKey = `${language}:${shikiTheme}:${code}`;
    const cached = tokenCache.get(cacheKey);
    if (cached) return cached;
    try {
      const result = shikiHighlighter.codeToTokens(code, {
        lang: language as BundledLanguage,
        theme: shikiTheme as BundledTheme,
      });
      if (tokenCache.size >= MAX_CACHE_ENTRIES) {
        const firstKey = tokenCache.keys().next().value;
        if (firstKey !== undefined) tokenCache.delete(firstKey);
      }
      tokenCache.set(cacheKey, result.tokens);
      return result.tokens;
    } catch {
      return null;
    }
  }, [code, language, shikiTheme, loadedKey]);

  const codeClassName = CODE_BLOCK_CLASS;

  if (!tokens) {
    const lines = code.split("\n");
    return (
      <code {...props} className={codeClassName}>
        {lines.map((line, i) => (
          // biome-ignore lint/suspicious/noArrayIndexKey: lines are positional
          <span key={i} data-line="">
            {line}
          </span>
        ))}
      </code>
    );
  }

  return (
    <code {...props} className={codeClassName}>
      {tokens.map((line, lineIdx) => {
        const lineText = line.map((t) => t.content).join("");
        const isAdd = DIFF_ADD_RE.test(lineText);
        const isRemove = DIFF_REMOVE_RE.test(lineText);
        const diffClass = isAdd
          ? "code-line-diff-add"
          : isRemove
            ? "code-line-diff-remove"
            : undefined;

        const renderedTokens =
          isAdd || isRemove
            ? stripDiffMarker(line, isAdd ? DIFF_ADD_RE : DIFF_REMOVE_RE)
            : line;

        return (
          <span
            // biome-ignore lint/suspicious/noArrayIndexKey: tokens are positional and never reordered
            key={lineIdx}
            data-line=""
            className={diffClass}
          >
            {renderedTokens.map((token, tokenIdx) => (
              <span
                // biome-ignore lint/suspicious/noArrayIndexKey: tokens are positional and never reordered
                key={tokenIdx}
                style={token.color ? { color: token.color } : undefined}
              >
                {token.content}
              </span>
            ))}
          </span>
        );
      })}
    </code>
  );
}
