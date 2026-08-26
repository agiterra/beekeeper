import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_COMPOSER_DOCK_CLASS,
  CODING_SESSION_COLUMN_CLASS,
  CodingSessionColumn,
} from "./CodingSessionColumn.tsx";
import {
  CODING_SESSION_GUTTER_CLASSES,
  CODING_SESSION_MEASURE_CLASSES,
  CODING_SESSION_WIDTH_OPTIONS,
  DEFAULT_CODING_SESSION_WIDTH,
} from "../lib/codingSessionWidthPreference.ts";

function source(name) {
  return readFileSync(fileURLToPath(new URL(name, import.meta.url)), "utf8");
}

const CALL_SITES = [
  "CodingSessionWorkspace.tsx",
  "CodingSessionUmbrellaWorkspace.tsx",
  "PendingCodingSessionScreen.tsx",
  "CodingSessionGoalPill.tsx",
];

test("every measure cap stays in rem so Cmd +/- widens it with the glyphs", () => {
  // px would freeze the column against the root-font-size zoom in
  // app/useWebviewZoomShortcuts.ts (the PR #891 class of regression); vw would
  // track the window and ignore zoom entirely. `max-w-none` is the Full
  // choice, which caps nothing at all.
  for (const [width, caps] of Object.entries(CODING_SESSION_MEASURE_CLASSES)) {
    for (const [state, cap] of Object.entries(caps)) {
      assert.doesNotMatch(
        cap,
        /max-w-\[[^\]]*(px|vw)\]/,
        `${width}.${state} must not pin the measure to px or vw`,
      );
      assert.match(cap, /^max-w-(none|\d?xl)$/, `${width}.${state}: ${cap}`);
    }
  }
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
  // Server snapshot is the default width, so this renders Narrow's two caps.
  assert.match(expanded, /\bmax-w-6xl\b/);
  assert.doesNotMatch(expanded, /\bmax-w-3xl\b/);
});

test("a side surface open earns a narrower cap at every width", () => {
  // A rail takes real width; a column sized for an empty workspace would be
  // cramped beside one. Full is the exception — it caps nothing either way.
  for (const [width, caps] of Object.entries(CODING_SESSION_MEASURE_CLASSES)) {
    if (width === "full") {
      assert.equal(caps.default, "max-w-none");
      assert.equal(caps.expanded, "max-w-none");
      continue;
    }
    assert.notEqual(
      caps.default,
      caps.expanded,
      `${width}: a rail must not leave the cap unchanged`,
    );
  }
});

test("Wide is wider than Narrow, and Full caps nothing", () => {
  // The order the settings page promises: each step gives back more window.
  const rank = {
    "max-w-3xl": 1,
    "max-w-5xl": 2,
    "max-w-6xl": 3,
    "max-w-7xl": 4,
  };
  const { narrow, wide, full } = CODING_SESSION_MEASURE_CLASSES;
  assert.ok(rank[wide.default] > rank[narrow.default]);
  assert.ok(rank[wide.expanded] > rank[narrow.expanded]);
  assert.equal(full.default, "max-w-none");
});

test("the gutter lives outside the measure, never inside it", () => {
  // `max-w-3xl px-5` silently narrows the measure by the padding, which is how
  // the transcript text and the composer edge drifted out of register.
  assert.doesNotMatch(CODING_SESSION_COLUMN_CLASS, /(^|\s)p[xlrs]?-/);
});

test("Narrow is the default, so the app looks unchanged until asked", () => {
  assert.equal(DEFAULT_CODING_SESSION_WIDTH, "narrow");
  assert.equal(CODING_SESSION_MEASURE_CLASSES.narrow.default, "max-w-3xl");
  assert.equal(CODING_SESSION_MEASURE_CLASSES.narrow.expanded, "max-w-6xl");
  assert.match(CODING_SESSION_GUTTER_CLASSES.narrow, /\bpx-5\b/);
  assert.match(CODING_SESSION_GUTTER_CLASSES.narrow, /\bsm:px-8\b/);
});

test("Full starts where a DM conversation starts", () => {
  // A DM timeline is the app's other uncapped full-width reading surface, and
  // it sits at 20px: `px-5` on the composer dock, and px-2 + mx-1 + px-2 on a
  // message row. A Full transcript beside one must not invent its own margin.
  assert.equal(CODING_SESSION_GUTTER_CLASSES.full, "px-5");
  // No `sm:` step: the capped widths widen their margin on a large window
  // because they have room spare, and Full spends that room on text instead.
  assert.doesNotMatch(CODING_SESSION_GUTTER_CLASSES.full, /\bsm:/);
  assert.equal(
    CODING_SESSION_GUTTER_CLASSES.wide,
    CODING_SESSION_GUTTER_CLASSES.narrow,
  );
});

test("every offered choice is named, described, and has classes to apply", () => {
  const values = CODING_SESSION_WIDTH_OPTIONS.map((option) => option.value);
  assert.deepEqual(values, ["narrow", "wide", "full"]);
  for (const option of CODING_SESSION_WIDTH_OPTIONS) {
    assert.ok(option.label.length > 0, `${option.value} needs a label`);
    assert.ok(
      option.description.length > 0,
      `${option.value} needs a description`,
    );
    // This is a settings dialog, not a layout inspector: the copy describes
    // the reading experience and never quotes a measurement.
    assert.doesNotMatch(
      option.description,
      /\d+\s*(px|rem|em|%)|\bpixels?\b/i,
      `${option.value}: settings copy must not quote measurements`,
    );
    assert.match(CODING_SESSION_GUTTER_CLASSES[option.value], /\bpx-/);
  }
  assert.deepEqual(
    Object.keys(CODING_SESSION_MEASURE_CLASSES).sort(),
    [...values].sort(),
  );
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
  // Any literal cap in a call site is a surface that has drifted back out of
  // the shared column — and now also one that would ignore the person's
  // chosen width, staying put while everything around it moved.
  for (const name of CALL_SITES) {
    // Only the numbered caps — `max-w-none` is a general-purpose utility that
    // legitimately appears on things that are not the reading measure (a
    // SheetContent overriding its own default, for one).
    assert.doesNotMatch(
      source(name),
      /\bmax-w-(3xl|5xl|6xl|7xl)\b/,
      `${name} should take its measure from the width preference, not a literal cap`,
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
