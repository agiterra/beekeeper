import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { TooltipProvider } from "../tooltip.tsx";
import { MarkdownCodeBlock, StaticCodeBlock } from "./CodeBlock.tsx";

function css(name) {
  return readFileSync(
    fileURLToPath(new URL(`../../styles/globals/${name}`, import.meta.url)),
    "utf8",
  );
}

function renderBlock() {
  return renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(
        MarkdownCodeBlock,
        { language: "bash" },
        "buzz --format compact messages thread --channel 00000000-0000-0000-0000-000000000000\n",
      ),
    ),
  );
}

test("a fenced block owns its horizontal overflow instead of escaping the column", () => {
  const html = renderBlock();
  // max-w-full is the mirror of t3code's `.chat-markdown pre { max-width:100% }`:
  // without it a long line pushes the block past the reading column and an
  // ancestor's overflow-hidden clips it mid-word.
  assert.match(html, /<pre[^>]*class="[^"]*\bmax-w-full\b/);
  assert.match(html, /<pre[^>]*class="[^"]*\boverflow-x-auto\b/);
  // An invisible scroll reads as a clip, so the thumb is styled, not overlay.
  assert.match(html, /<pre[^>]*class="[^"]*\bbuzz-code-scrollbar\b/);
});

test("the static (unfenced) block is contained the same way", () => {
  const html = renderToStaticMarkup(
    React.createElement(StaticCodeBlock, null, "echo hi"),
  );
  assert.match(html, /<pre[^>]*class="[^"]*\bmax-w-full\b/);
  assert.match(html, /<pre[^>]*class="[^"]*\bbuzz-code-scrollbar\b/);
});

test("wrapping is off by default, and the block advertises its state", () => {
  const html = renderBlock();
  assert.match(html, /data-code-block=""/);
  assert.match(html, /data-wrap="false"/);
  // Nothing is known to be hidden until the block has been measured.
  assert.match(html, /data-overflow="false"/);
});

test("a scroller with content still to the right fades that edge", () => {
  const markdownCss = css("markdown.css");
  // The platform overlay scrollbar paints nothing until the user is already
  // scrolling, so without this the cut is indistinguishable from truncation.
  assert.match(
    markdownCss,
    /\[data-code-block\]\[data-overflow="true"\] > pre,\s*\[data-table-block\]\[data-overflow="true"\] \{[\s\S]*?mask-image: linear-gradient\(to left/,
  );
  // Both the -webkit- prefixed and standard properties, for WKWebView.
  assert.match(markdownCss, /-webkit-mask-image: linear-gradient\(to left/);
});

test("every fenced block carries a wrap toggle beside the copy button", () => {
  const html = renderBlock();
  assert.match(html, /data-testid="code-block-wrap-toggle"/);
  assert.match(html, /aria-pressed="false"/);
  assert.match(html, /Wrap long lines in this code block/);
  assert.match(html, /Copy code block/);
});

test("the wrap rule beats the whitespace-pre utility on the code element", () => {
  const markdownCss = css("markdown.css");
  // `.code-block-lines` carries `whitespace-pre`; the toggle must override both
  // the pre and that inner element or flipping it does nothing visible.
  assert.match(
    markdownCss,
    /\[data-code-block\]\[data-wrap="true"\] pre,\s*\[data-code-block\]\[data-wrap="true"\] \.code-block-lines \{\s*white-space: pre-wrap;\s*overflow-wrap: anywhere;/,
  );
});

test("the code scrollbar is styled with webkit pseudos, not scrollbar-color", () => {
  const scrollbarsCss = css("scrollbars.css");
  const block = scrollbarsCss.slice(
    scrollbarsCss.indexOf(".buzz-code-scrollbar"),
  );
  assert.match(block, /\.buzz-code-scrollbar::-webkit-scrollbar-thumb \{/);
  // Setting the standard property makes engines drop the pseudo styles and
  // fall back to overlay scrollbars — the bug this class exists to avoid.
  assert.doesNotMatch(block, /scrollbar-color/);
});
