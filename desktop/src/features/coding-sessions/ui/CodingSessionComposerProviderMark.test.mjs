import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionComposerProviderMark,
  codingSessionProviderMarkKind,
} from "./CodingSessionComposerProviderMark.tsx";

test("the runtime label picks the vendor mark", () => {
  assert.equal(codingSessionProviderMarkKind("Claude Code"), "claude");
  assert.equal(
    codingSessionProviderMarkKind(null, null, "claude-opus-5"),
    "claude",
  );
  assert.equal(codingSessionProviderMarkKind("Cursor"), "cursor");
  assert.equal(codingSessionProviderMarkKind("goose"), "goose");
});

test("a token match only: a name that merely contains a vendor is not that vendor", () => {
  assert.equal(codingSessionProviderMarkKind("Claudette's box"), null);
  assert.equal(codingSessionProviderMarkKind("Precursor"), null);
});

test("Codex gets the gallery's neutral terminal glyph, never a guessed OpenAI mark", () => {
  // CREDITS.md: the OpenAI mark is deliberately not bundled.
  assert.equal(codingSessionProviderMarkKind("Codex", "gpt-5-codex"), "codex");
  assert.equal(
    codingSessionProviderMarkKind(null, null, "gpt-5-codex"),
    "codex",
  );
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposerProviderMark, { kind: "codex" }),
  );
  assert.match(markup, /data-provider-mark="codex"/);
  assert.match(markup, /<svg/);
  assert.doesNotMatch(markup, /<img/);
});

test("unknown runtimes get the generic glyph", () => {
  assert.equal(codingSessionProviderMarkKind("Gemini CLI"), null);
  assert.equal(codingSessionProviderMarkKind("OpenAI"), null);
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposerProviderMark, { kind: null }),
  );
  assert.match(markup, /data-provider-mark="generic"/);
});

test("preset harnesses with bundled logos render them", () => {
  assert.equal(codingSessionProviderMarkKind("Grok CLI"), "grok");
  assert.equal(codingSessionProviderMarkKind("OpenCode"), "opencode");
  assert.equal(codingSessionProviderMarkKind(null, null, "kimi-k2"), "kimi");
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposerProviderMark, { kind: "grok" }),
  );
  assert.match(markup, /<img[^>]*data-provider-mark="grok"/);
  assert.match(markup, /harness-logos\/grok\.svg/);
});

test("Claude renders its bundled logo, decorative to assistive tech", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposerProviderMark, { kind: "claude" }),
  );
  assert.match(markup, /<img[^>]*data-provider-mark="claude"/);
  assert.match(markup, /alt=""/);
});

test("Cursor and Goose render their inline marks", () => {
  for (const kind of ["cursor", "goose"]) {
    const markup = renderToStaticMarkup(
      React.createElement(CodingSessionComposerProviderMark, { kind }),
    );
    assert.match(markup, new RegExp(`data-provider-mark="${kind}"`));
    assert.match(markup, /<svg/);
  }
});
