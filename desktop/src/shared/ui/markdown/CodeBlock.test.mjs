import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { TooltipProvider } from "../tooltip.tsx";
import {
  CODE_BLOCK_CLASS,
  createPreComponent,
  MarkdownCodeBlock,
  StaticCodeBlock,
} from "./CodeBlock.tsx";
import {
  codeBlockLanguageIcon,
  extractFenceTitle,
  extractPreCodeMeta,
} from "./CodeBlockLanguage.tsx";

function css(name) {
  return readFileSync(
    fileURLToPath(new URL(`../../styles/globals/${name}`, import.meta.url)),
    "utf8",
  );
}

function renderBlock(language = "bash", title = undefined) {
  return renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(
        MarkdownCodeBlock,
        { language, title },
        "bee --format compact messages thread --channel 00000000-0000-0000-0000-000000000000\n",
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
  assert.match(html, /<pre[^>]*class="[^"]*\bbeekeeper-code-scrollbar\b/);
});

test("SV-12: a session block is uncapped; a channel block is capped at 25rem with an expand control", () => {
  // T3's code block (ChatMarkdown.tsx, index.css `.chat-markdown pre`) sets
  // only `overflow-x: auto`, and the session view matches it. Channels and
  // threads keep a cap so a pasted 500-line log cannot take over the timeline.
  // The cap lives in markdown.css keyed on `data-height-capped`, not on the
  // pre's classes, so the session column can lift it by selector.
  const html = renderBlock();
  const pre = html.match(/<pre[^>]*class="([^"]*)"/);
  assert.ok(pre, "the fenced block renders a pre");
  assert.doesNotMatch(pre[1], /\bmax-h-/);
  assert.match(pre[1], /\boverflow-x-auto\b/);
  assert.match(html, /data-height-capped="true"/);
  const markdownCss = css("markdown.css");
  assert.match(
    markdownCss,
    /\[data-code-block\]\[data-height-capped="true"\] > pre \{\s*max-height: 25rem;\s*overflow-y: auto;/,
  );
  assert.match(
    markdownCss,
    /\[data-coding-session-column\] \[data-code-block\]\[data-height-capped\] > pre \{\s*max-height: none;\s*overflow-y: visible;/,
  );
  // No px heights: a px cap is frozen against Cmd +/-.
  assert.doesNotMatch(markdownCss, /max-height:\s*\d+px/);
  // The expand control is drawn only once the cap is measured hiding lines;
  // a static render (no layout) has measured nothing and draws none.
  assert.doesNotMatch(html, /code-block-height-toggle/);
  const source = readFileSync(
    fileURLToPath(new URL("./CodeBlock.tsx", import.meta.url)),
    "utf8",
  );
  assert.match(source, /data-testid="code-block-height-toggle"/);
  assert.match(source, /useExceedsHeightCap\(codeBlockRef, isHeightExpanded/);
});

test("the static (unfenced) block is contained the same way", () => {
  const html = renderToStaticMarkup(
    React.createElement(StaticCodeBlock, null, "echo hi"),
  );
  assert.match(html, /<pre[^>]*class="[^"]*\bmax-w-full\b/);
  assert.match(html, /<pre[^>]*class="[^"]*\bbeekeeper-code-scrollbar\b/);
});

test("SV-12: lines wrap by default (T3's wordWrap default), and the block advertises its state", () => {
  const html = renderBlock();
  assert.match(html, /data-code-block=""/);
  assert.match(html, /data-wrap="true"/);
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
  // Pressed: wrapping is on, and the toggle offers to stop it.
  assert.match(html, /aria-pressed="true"/);
  assert.match(html, /Stop wrapping long lines in this code block/);
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
    scrollbarsCss.indexOf(".beekeeper-code-scrollbar"),
  );
  assert.match(block, /\.beekeeper-code-scrollbar::-webkit-scrollbar-thumb \{/);
  // Setting the standard property makes engines drop the pseudo styles and
  // fall back to overlay scrollbars — the bug this class exists to avoid.
  assert.doesNotMatch(block, /scrollbar-color/);
});

// ── SV-12: code-block chrome, app-wide (D4) ─────────────────────────────

test("SV-12: the header names the fence's language with an icon and the name", () => {
  const html = renderBlock("bash");
  assert.match(html, /data-code-block-header=""/);
  assert.match(html, /data-code-block-language="bash"/);
  // The name is printed, not hidden behind a hover: the icon is only a family.
  assert.match(html, /<span class="truncate">bash<\/span>/);
  assert.match(html, /<svg[^>]*aria-hidden="true"/);
});

test("SV-12: a fence with no language draws no language label", () => {
  const html = renderBlock("");
  assert.match(html, /data-code-block-header=""/);
  assert.doesNotMatch(html, /data-code-block-language=/);
  assert.doesNotMatch(html, /data-language=/);
});

test("SV-12: wrap and copy are always visible, never hover-revealed", () => {
  const html = renderBlock();
  assert.match(
    html,
    /role="toolbar"[^>]*aria-label="Code block actions"|aria-label="Code block actions"[^>]*role="toolbar"/,
  );
  assert.match(html, /data-testid="code-block-copy"/);
  // The old chrome hid both buttons until the block was hovered.
  assert.doesNotMatch(html, /opacity-0/);
  assert.doesNotMatch(html, /group-hover:opacity-100/);
});

test("SV-12: the pre stays the direct child of [data-code-block], after the header", () => {
  const html = renderBlock();
  // `[data-code-block] > pre` is addressed by the session column and the width
  // audit; the overflow fade masks only the pre, never the header.
  assert.match(
    html,
    /^(?:<[^>]+>)*?<div[^>]*data-code-block=""[^>]*><div[^>]*data-code-block-header=""[\s\S]*<\/div><\/div><pre[^>]*>[\s\S]*<\/pre><\/div>$/,
  );
  assert.match(
    css("markdown.css"),
    /\[data-code-block\]\[data-overflow="true"\] > pre/,
  );
});

test("SV-12: no line-number counter; diff markers keep a glyph gutter", () => {
  const markdownCss = css("markdown.css");
  assert.doesNotMatch(markdownCss, /counter\(code-line\)/);
  assert.doesNotMatch(markdownCss, /counter-increment/);
  assert.match(
    markdownCss,
    /\.code-block-lines:has\(> \.code-line-diff-add, > \.code-line-diff-remove\)\s*\[data-line\]::before/,
  );
  assert.match(
    markdownCss,
    /\.code-block-lines \[data-line\]\.code-line-diff-add::before \{\s*content: "\+";/,
  );
  assert.match(
    markdownCss,
    /\.code-block-lines \[data-line\]\.code-line-diff-remove::before \{\s*content: "-";/,
  );
});

test("SV-12: language families map onto the existing icon set", () => {
  const shell = codeBlockLanguageIcon("bash");
  assert.ok(shell);
  assert.equal(codeBlockLanguageIcon("ZSH"), shell);
  assert.equal(codeBlockLanguageIcon("powershell"), shell);
  assert.equal(codeBlockLanguageIcon("json"), codeBlockLanguageIcon("jsonc"));
  assert.notEqual(codeBlockLanguageIcon("json"), shell);
  // Anything else still gets a code-file mark, with its name beside it.
  const generic = codeBlockLanguageIcon("rust");
  assert.ok(generic);
  assert.equal(codeBlockLanguageIcon("haskell"), generic);
  assert.equal(codeBlockLanguageIcon(""), null);
  assert.equal(codeBlockLanguageIcon("   "), null);
});

// ── SV-10: inline code as bordered pills ────────────────────────────────

test("SV-10: inline code is a bordered pill sized to its sentence", () => {
  const markdownCss = css("markdown.css");
  const rule = [
    ...markdownCss.matchAll(
      /\.message-markdown \.inline-code-chip,\s*\.message-markdown :not\(pre\) > code \{[^}]*\}/g,
    ),
  ].find((match) => match[0].includes("font-family: ui-monospace"));
  assert.ok(rule, "inline code rule present");
  assert.match(rule[0], /border: 1px solid hsl\(var\(--border\)\)/);
  assert.match(rule[0], /border-radius: 0\.375rem/);
  assert.match(rule[0], /padding: 0\.1rem 0\.35rem/);
  // Relative to the sentence, not a fixed step: zoom-safe and never towering.
  assert.match(markdownCss, /--inline-code-font-size: 0\.857em;/);
});

// ── SV-09: answers read as documents ────────────────────────────────────

test("SV-09: document rhythm is scoped to session answers, not channel chat", () => {
  const markdownCss = css("markdown.css");
  const docStart = markdownCss.indexOf("Answers read as documents (SV-09)");
  assert.ok(docStart > 0);
  const doc = markdownCss.slice(
    docStart,
    markdownCss.indexOf("Tables (SV-11)"),
  );
  // Every rule in the block is scoped; nothing restyles bare .message-markdown.
  for (const selector of doc.matchAll(/^([^\s/*][^{]*)\{/gm)) {
    assert.match(
      selector[1],
      /\[data-role="assistant-message"\] > \.message-markdown|\.message-markdown\.markdown-document|^\s*\)/,
    );
  }
  assert.match(doc, /line-height: 1\.625;/);
  assert.match(doc, /> h1 \{\s*font-size: 1\.5rem;/);
  assert.match(doc, /> h2 \{\s*font-size: 1\.25rem;/);
  assert.match(doc, /> h3 \{\s*font-size: 1\.125rem;/);
  // Bold list leads stand out against slightly softer prose.
  assert.match(
    doc,
    /:is\(strong, h1, h2, h3, h4, h5, th, :not\(pre\) > code\) \{\s*color: hsl\(var\(--foreground\)\);/,
  );
  // rem, never px, so Cmd +/- zoom keeps scaling the document.
  assert.doesNotMatch(doc, /font-size: [0-9.]+px/);
});

// ── Wave B audit against T3's MarkdownCodeBlock ────────────────────────

test("SV-12: the block is T3's box — rounded-lg (--radius), no shadow, regular-weight code", () => {
  const html = renderBlock();
  const block = /<div[^>]*data-code-block=""[^>]*>/.exec(html)?.[0] ?? "";
  assert.match(block, /class="[^"]*\brounded-lg\b/);
  assert.doesNotMatch(block, /rounded-2xl|shadow-/);
  assert.match(block, /border-radius:var\(--radius\)/);
  assert.match(CODE_BLOCK_CLASS, /\bfont-normal\b/);
  assert.doesNotMatch(CODE_BLOCK_CLASS, /font-medium/);
  const staticHtml = renderToStaticMarkup(
    React.createElement(StaticCodeBlock, null, "echo hi"),
  );
  assert.match(staticHtml, /<pre[^>]*class="[^"]*\brounded-lg\b/);
});

test("SV-12: a fence that names a file shows the file in the header", () => {
  const html = renderBlock("ts", "src/main.ts");
  assert.match(html, /data-code-block-title="src\/main\.ts"/);
  assert.match(html, /<span class="truncate">src\/main\.ts<\/span>/);
  // The language stays one hover away on the title.
  assert.match(html, /title="src\/main\.ts · ts"/);
});

test("SV-12: fence titles are read the way T3 reads them", () => {
  assert.equal(extractFenceTitle('title="a b.ts"'), "a b.ts");
  assert.equal(extractFenceTitle("filename='x.py' {1,3}"), "x.py");
  assert.equal(extractFenceTitle("file=Cargo.toml"), "Cargo.toml");
  assert.equal(extractFenceTitle("src/main.rs"), "src/main.rs");
  assert.equal(extractFenceTitle("{1,3} showLineNumbers"), null);
  assert.equal(extractFenceTitle(""), null);
  assert.equal(extractFenceTitle(undefined), null);
  const pre = {
    type: "element",
    tagName: "pre",
    children: [
      { type: "text" },
      { type: "element", tagName: "code", data: { meta: "  title=x.ts " } },
    ],
  };
  assert.equal(extractPreCodeMeta(pre), "title=x.ts");
  assert.equal(extractPreCodeMeta({ children: [] }), undefined);
  assert.equal(extractPreCodeMeta(undefined), undefined);
});

test("SV-12: the renderer's pre carries language and file title into the chrome", () => {
  const Pre = createPreComponent(true);
  const code = React.createElement(
    "code",
    { className: "language-python" },
    "print(1)\n",
  );
  const node = {
    type: "element",
    tagName: "pre",
    children: [
      { type: "element", tagName: "code", data: { meta: "title=demo.py" } },
    ],
  };
  const html = renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(Pre, { node }, code),
    ),
  );
  assert.match(html, /data-language="python"/);
  assert.match(html, /data-code-block-title="demo\.py"/);
  // Previews and search rows keep the plain contained block.
  const Static = createPreComponent(false);
  const staticHtml = renderToStaticMarkup(
    React.createElement(Static, { node }, code),
  );
  assert.doesNotMatch(staticHtml, /data-code-block/);
});

test("SV-10: inline code is an inline box that wraps with its sentence, regular weight", () => {
  const markdownCss = css("markdown.css");
  const rule = [
    ...markdownCss.matchAll(
      /\.message-markdown \.inline-code-chip,\s*\.message-markdown :not\(pre\) > code \{[^}]*\}/g,
    ),
  ].find((match) => match[0].includes("font-family: ui-monospace"));
  assert.ok(rule);
  // Declared after the shared chip rule (inline-flex), so it wins.
  assert.ok(
    markdownCss.indexOf(rule[0]) > markdownCss.indexOf("display: inline-flex;"),
  );
  assert.match(rule[0], /display: inline;/);
  assert.match(rule[0], /font-weight: 400;/);
  assert.match(rule[0], /padding: 0\.1rem 0\.35rem;/);
  assert.match(rule[0], /box-decoration-break: slice;/);
});

test("SV-09: bold list leads are bold, markers and inline code take full ink in documents", () => {
  const markdownCss = css("markdown.css");
  const doc = markdownCss.slice(
    markdownCss.indexOf("Answers read as documents (SV-09)"),
    markdownCss.indexOf("Tables (SV-11)"),
  );
  assert.match(doc, /\)\s*strong \{\s*font-weight: 700;/);
  assert.match(doc, /\)\s*li::marker \{\s*color: inherit;/);
  assert.match(
    doc,
    /:is\(strong, h1, h2, h3, h4, h5, th, :not\(pre\) > code\) \{\s*color: hsl\(var\(--foreground\)\);/,
  );
});
