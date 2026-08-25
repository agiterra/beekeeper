import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_COMPOSER_DOCK_CLASS,
  CODING_SESSION_COLUMN_CLASS,
  CODING_SESSION_COLUMN_EXPANDED_CLASS,
  CodingSessionColumn,
} from "./CodingSessionColumn.tsx";
import {
  CODING_SESSION_GUTTER_CLASSES,
  CODING_SESSION_GUTTER_OPTIONS,
  DEFAULT_CODING_SESSION_GUTTER,
} from "../lib/codingSessionGutterPreference.ts";

function source(name) {
  return readFileSync(fileURLToPath(new URL(name, import.meta.url)), "utf8");
}

const CALL_SITES = [
  "CodingSessionWorkspace.tsx",
  "CodingSessionUmbrellaWorkspace.tsx",
  "PendingCodingSessionScreen.tsx",
  "CodingSessionGoalPill.tsx",
];

test("the measure stays in rem so Cmd +/- widens it with the glyphs", () => {
  // px would freeze the column against the root-font-size zoom in
  // app/useWebviewZoomShortcuts.ts (the PR #891 class of regression); vw would
  // track the window and ignore zoom entirely.
  assert.match(CODING_SESSION_COLUMN_CLASS, /\bmax-w-3xl\b/);
  assert.doesNotMatch(CODING_SESSION_COLUMN_CLASS, /max-w-\[[^\]]*(px|vw)\]/);
});

test("the measure box carries min-w-0 so wide content scrolls instead of clipping", () => {
  // Without min-w-0 a `pre` wider than the column pushes the box past its
  // parent and the nearest overflow-hidden ancestor cuts it off mid-word,
  // instead of the `pre`'s own overflow-x-auto engaging.
  assert.match(CODING_SESSION_COLUMN_CLASS, /\bmin-w-0\b/);
  assert.match(CODING_SESSION_COLUMN_CLASS, /\bw-full\b/);
  assert.match(CODING_SESSION_COLUMN_CLASS, /\bmx-auto\b/);
});

test("a hidden side surface gives the narrative the wider workspace measure", () => {
  const expanded = renderToStaticMarkup(
    React.createElement(
      CodingSessionColumn,
      { expanded: true, "data-testid": "column" },
      "body",
    ),
  );
  assert.match(CODING_SESSION_COLUMN_EXPANDED_CLASS, /\bmax-w-6xl\b/);
  assert.match(expanded, /\bmax-w-6xl\b/);
  assert.doesNotMatch(expanded, /\bmax-w-3xl\b/);
});

test("the gutter lives outside the measure, never inside it", () => {
  // `max-w-3xl px-5` silently narrows the measure by the padding, which is how
  // the transcript text and the composer edge drifted out of register.
  assert.doesNotMatch(CODING_SESSION_COLUMN_CLASS, /(^|\s)p[xlrs]?-/);
});

test("Full is the default gutter, unchanged from before the setting existed", () => {
  assert.equal(DEFAULT_CODING_SESSION_GUTTER, "full");
  assert.match(CODING_SESSION_GUTTER_CLASSES.full, /\bpx-5\b/);
  assert.match(CODING_SESSION_GUTTER_CLASSES.full, /\bsm:px-8\b/);
});

test("Light is half of Full, and None is a flat 10px at every width", () => {
  // px-2.5 is 0.625rem — 10px at default zoom, and rem so it still tracks
  // Cmd +/- like the measure does.
  assert.equal(CODING_SESSION_GUTTER_CLASSES.light, "px-2.5 sm:px-4");
  assert.equal(CODING_SESSION_GUTTER_CLASSES.none, "px-2.5");
  assert.doesNotMatch(CODING_SESSION_GUTTER_CLASSES.none, /\bsm:/);
});

test("every offered choice is named, described, and has classes to apply", () => {
  // A choice with no description is the failure this setting exists to avoid:
  // the difference between the three is a measurement, stated in the page.
  const values = CODING_SESSION_GUTTER_OPTIONS.map((option) => option.value);
  assert.deepEqual(values, ["full", "light", "none"]);
  for (const option of CODING_SESSION_GUTTER_OPTIONS) {
    assert.ok(option.label.length > 0, `${option.value} needs a label`);
    assert.ok(
      option.description.length > 0,
      `${option.value} needs a description`,
    );
    assert.match(CODING_SESSION_GUTTER_CLASSES[option.value], /\bpx-/);
  }
  assert.deepEqual(
    Object.keys(CODING_SESSION_GUTTER_CLASSES).sort(),
    [...values].sort(),
  );
});

test("the column composes an extra className onto the measure", () => {
  const html = renderToStaticMarkup(
    React.createElement(
      CodingSessionColumn,
      { className: "pt-7 pb-64", "data-testid": "column" },
      "body",
    ),
  );
  assert.match(html, /data-testid="column"/);
  for (const token of `${CODING_SESSION_COLUMN_CLASS} pt-7 pb-64`.split(" ")) {
    assert.ok(
      html.includes(token),
      `expected rendered class list to include ${token}: ${html}`,
    );
  }
});

test("every coding-session surface routes its measure through the primitive", () => {
  // A literal max-w-3xl anywhere in the feature is a call site that has
  // drifted back out of the shared column.
  for (const name of CALL_SITES) {
    assert.doesNotMatch(
      source(name),
      /max-w-3xl/,
      `${name} should use CodingSessionColumn, not a literal max-w-3xl`,
    );
    assert.match(source(name), /CodingSessionColumn/);
  }
});

test("transcript and composer sit in the same gutter, so their edges register", () => {
  for (const name of [
    "CodingSessionWorkspace.tsx",
    "CodingSessionUmbrellaWorkspace.tsx",
  ]) {
    const text = source(name);
    // Transcript scroller and composer overlay both take the chosen gutter;
    // neither hardcodes its own horizontal padding. One `const gutter =` plus
    // the three surfaces that spread it.
    assert.match(text, /useCodingSessionColumnGutter\(\)/);
    const gutterUses = text.match(/\bgutter\b/g) ?? [];
    assert.ok(
      gutterUses.length >= 4,
      `${name}: expected transcript, composer, and goal row to share the gutter (found ${gutterUses.length - 1} uses)`,
    );
    assert.doesNotMatch(
      text,
      /absolute inset-x-0 bottom-0 z-20[^"]*\bpx-4\b/,
      `${name}: the composer overlay must not keep its own px-4 gutter`,
    );
  }
});

test("the composer dock is opaque before any chip or control begins", () => {
  // §2 item 52.1: the previous 85%-opaque gradient covered the composer
  // itself, so transcript text showed through the participant chip row. The
  // fade now lives entirely above a solid dock.
  assert.match(CODING_SESSION_COMPOSER_DOCK_CLASS, /\bbg-background\b/);
  assert.match(CODING_SESSION_COMPOSER_DOCK_CLASS, /before:bottom-full/);
  assert.match(CODING_SESSION_COMPOSER_DOCK_CLASS, /before:h-8/);
  assert.match(CODING_SESSION_COMPOSER_DOCK_CLASS, /before:from-transparent/);
  assert.match(CODING_SESSION_COMPOSER_DOCK_CLASS, /before:to-background/);
  assert.doesNotMatch(
    CODING_SESSION_COMPOSER_DOCK_CLASS,
    /via-background|bg-background\//,
  );
  for (const name of [
    "CodingSessionWorkspace.tsx",
    "CodingSessionUmbrellaWorkspace.tsx",
  ]) {
    assert.match(source(name), /CODING_SESSION_COMPOSER_DOCK_CLASS/);
    assert.doesNotMatch(source(name), /via-background\/85/);
  }
});

test("no session surface keeps a private copy of the Full gutter", () => {
  // CodingSessionFounderLine spelled out `px-5 … sm:px-8` instead of taking
  // the constant, so it would have stayed at Full while everything below it
  // moved — a stripe of margin nothing else shared.
  for (const name of [...CALL_SITES, "CodingSessionFounderLine.tsx"]) {
    const text = source(name);
    assert.ok(
      !(/\bpx-5\b/.test(text) && /\bsm:px-8\b/.test(text)),
      `${name}: use useCodingSessionColumnGutter(), not a literal px-5 sm:px-8`,
    );
  }
});

test("the new-session form keeps its own narrower measure", () => {
  // A form wants a shorter measure than a transcript; t3code's analogue is
  // narrower still. Deliberately not routed through the column.
  const text = source("NewCodingSessionDialog.tsx");
  assert.match(text, /max-w-2xl/);
});
