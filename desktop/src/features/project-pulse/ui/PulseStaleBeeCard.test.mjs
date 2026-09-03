import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { PulseStaleBeeCard } from "./PulseStaleBeeCard.tsx";

/** Server markup escapes apostrophes; the copy is frozen in its read form. */
const render = (props) =>
  renderToStaticMarkup(React.createElement(PulseStaleBeeCard, props))
    .replaceAll("&#x27;", "'")
    .replaceAll("&amp;", "&");

const reading = (overrides) => ({
  rows: [],
  uncomparedCount: 0,
  truncatedCount: 0,
  ...overrides,
});

const builderRow = {
  seatKey: "a",
  label: "Bob · Builder",
  sha7: "07c470b",
  behind: 2,
};

test("the card names each seat, its build, and how far behind main it is", () => {
  const markup = render({
    reading: reading({
      rows: [
        { seatKey: "a", label: "Bob · Builder", sha7: "07c470b", behind: 3 },
        { seatKey: "b", label: "Cleo · Verifier", sha7: "2372822", behind: 1 },
      ],
    }),
  });
  assert.match(markup, /data-testid="pulse-stale-bee"/);
  assert.match(markup, /seats running an older bee/);
  assert.match(markup, /Bob · Builder · bee 07c470b — 3 commits behind main/);
  assert.match(markup, /Cleo · Verifier · bee 2372822 — 1 commit behind main/);
});

test("a read that could compare nothing says exactly that, and nothing else", () => {
  const markup = render({ reading: reading({ uncomparedCount: 4 }) });
  assert.match(markup, /no seat's build could be compared/);
  assert.doesNotMatch(markup, /commits behind main/);
  assert.doesNotMatch(markup, /more seats' builds could not be compared/);
});

test("rows and uncompared seats are two facts, both disclosed", () => {
  const markup = render({
    reading: reading({ rows: [builderRow], uncomparedCount: 3 }),
  });
  assert.match(markup, /Bob · Builder · bee 07c470b — 2 commits behind main/);
  assert.match(markup, /3 more seats' builds could not be compared/);
  assert.doesNotMatch(markup, /no seat's build could be compared/);
});

test("one uncompared seat is disclosed in the singular", () => {
  const markup = render({
    reading: reading({ rows: [builderRow], uncomparedCount: 1 }),
  });
  assert.match(markup, /1 more seat's build could not be compared/);
});

test("a capped list says how many rows it is not showing", () => {
  assert.match(
    render({ reading: reading({ rows: [builderRow], truncatedCount: 1 }) }),
    /1 more row not shown/,
  );
  assert.match(
    render({ reading: reading({ rows: [builderRow], truncatedCount: 7 }) }),
    /7 more rows not shown/,
  );
});

test("nothing observed is no card at all, not an empty one", () => {
  assert.equal(render({ reading: reading({}) }), "");
});

test("the card carries no hex colour and no frozen text size", () => {
  const markup = render({
    reading: reading({ rows: [builderRow], uncomparedCount: 1 }),
  });
  assert.doesNotMatch(markup, /#[0-9a-fA-F]{3,8}\b/);
  assert.doesNotMatch(markup, /text-\[/);
});
