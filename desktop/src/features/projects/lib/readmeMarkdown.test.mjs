import assert from "node:assert/strict";
import { test } from "node:test";

import { normalizeReadmeMarkdown } from "./readmeMarkdown.ts";

test("top-level anchors and <br/> convert without block wrappers", () => {
  const readme = [
    '<a href="/">',
    "# TankLoop - Septic Service Management System",
    "</a>",
    "<br/>",
    "",
    "Some intro text.",
  ].join("\n");

  const normalized = normalizeReadmeMarkdown(readme);
  assert.match(
    normalized,
    /^# \[TankLoop - Septic Service Management System\]\(\/\)/,
  );
  assert.ok(!normalized.includes("<a"), normalized);
  assert.ok(!normalized.includes("<br"), normalized);
  assert.ok(normalized.includes("Some intro text."));
});

test("plain top-level anchors become markdown links", () => {
  const normalized = normalizeReadmeMarkdown(
    'See <a href="https://example.com">the docs</a> for details.',
  );
  assert.equal(normalized, "See [the docs](https://example.com) for details.");
});

test("fenced code and inline code pass through untouched", () => {
  const readme = [
    "## Install",
    "",
    "```bash",
    "git clone https://git.example.com/tankloop/tankloop.git",
    "cd tankloop",
    "corepack enable",
    "pnpm install",
    "```",
    "",
    'Use `<br/>` sparingly, and `<a href="x">` never.',
  ].join("\n");

  const normalized = normalizeReadmeMarkdown(readme);
  // Every fence line survives on its own line.
  assert.ok(
    normalized.includes(
      "git clone https://git.example.com/tankloop/tankloop.git\ncd tankloop\ncorepack enable\npnpm install",
    ),
  );
  // HTML inside inline code spans is not converted.
  assert.ok(normalized.includes("`<br/>`"));
  assert.ok(normalized.includes('`<a href="x">`'));
});

test("block wrappers still convert as before", () => {
  const readme = [
    "<h2>Features</h2>",
    '<p>Fast <strong>and</strong> <a href="/d">documented</a>.</p>',
    "<center><img src='/logo.png' alt='Logo'></center>",
  ].join("\n");

  const normalized = normalizeReadmeMarkdown(readme);
  assert.ok(normalized.includes("## Features"));
  assert.ok(normalized.includes("Fast **and** [documented](/d)."));
  assert.ok(normalized.includes("![Logo](/logo.png)"));
});

test("markdown autolinks survive the top-level pass", () => {
  const normalized = normalizeReadmeMarkdown(
    "Visit <https://example.com> for more.",
  );
  assert.equal(normalized, "Visit <https://example.com> for more.");
});
