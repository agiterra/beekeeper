/**
 * One quiet row for the goal and the founder, in place of a "Founded by" bar
 * plus a bordered goal pill.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

after(() => dom.window.close());

// The communities provider and React Query schedule long timers that outlive
// the assertions and would hold this file's process open. Unreference anything
// long; nothing here waits that long on purpose.
const realSetTimeout = globalThis.setTimeout;
globalThis.setTimeout = (callback, delay, ...rest) => {
  const handle = realSetTimeout(callback, delay, ...rest);
  if (delay > 10_000 && typeof handle?.unref === "function") handle.unref();
  return handle;
};

let React;
let renderToStaticMarkup;
let QueryClient;
let QueryClientProvider;
let CommunitiesProvider;
let CodingSessionWorkspaceGoalRow;
let codingSessionGoalRestatesTitle;

before(async () => {
  React = (await import("react")).default;
  ({ renderToStaticMarkup } = await import("react-dom/server"));
  ({ QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  ));
  ({ CommunitiesProvider } = await import(
    "@/features/communities/useCommunities"
  ));
  ({ CodingSessionWorkspaceGoalRow, codingSessionGoalRestatesTitle } =
    await import("./CodingSessionWorkspaceGoalRow.tsx"));
});

const FOUNDER = "a".repeat(64);
const VIEWER = "b".repeat(64);
const GENESIS = "c".repeat(64);

function render({ goal, title, viewer = VIEWER, genesisRef = GENESIS }) {
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0, enabled: false } },
  });
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(
        CommunitiesProvider,
        null,
        React.createElement(CodingSessionWorkspaceGoalRow, {
          channelId: "channel-1",
          currentUserPubkey: viewer,
          founderPubkey: FOUNDER,
          genesisRef,
          goal: goal === null ? null : { content: goal },
          sessionRef: "session-1",
          title,
          workspaceExpanded: true,
        }),
      ),
    ),
  );
}

test("a goal that only restates the title is not said twice", () => {
  assert.equal(
    codingSessionGoalRestatesTitle(
      "Fix the reconnect bug.",
      "fix the  reconnect bug",
    ),
    true,
  );
  assert.equal(
    codingSessionGoalRestatesTitle("Fix the reconnect bug", "Reconnect"),
    false,
  );
  assert.equal(codingSessionGoalRestatesTitle("", "x"), false);
  assert.equal(codingSessionGoalRestatesTitle("x", null), false);
});

test("goal and founder share one quiet row", () => {
  const markup = render({
    goal: "Make authority visible at every decision point",
    title: "Fix the reconnect bug",
  });
  assert.match(markup, /coding-session-goal-workspace/);
  assert.match(markup, /Make authority visible at every decision point/);
  assert.match(markup, /data-testid="coding-session-founded-by"/);
  assert.match(markup, new RegExp(`data-genesis-ref="${GENESIS}"`));
  assert.match(markup, /Founded by/);
  // Quiet: no pill border or fill.
  assert.doesNotMatch(markup, /border-primary\/20/);
  assert.doesNotMatch(markup, /bg-primary\/8/);
  // One row: the founder is inside the goal row, not a second bar.
  const row = markup.indexOf("coding-session-goal-workspace");
  const founder = markup.indexOf("coding-session-founded-by");
  assert.ok(row >= 0 && founder > row);
  assert.doesNotMatch(markup, /border-b border-border\/50/);
});

test("a restated goal is withheld from a reader, and the row goes with it", () => {
  const markup = render({
    goal: "Fix the reconnect bug",
    title: "Fix the reconnect bug",
  });
  // Nothing left to say here; the founder stays one click away in the
  // header's provenance popover.
  assert.equal(markup, "");
});

test("the founder keeps the edit control when the goal restates the title", () => {
  const markup = render({
    goal: "Fix the reconnect bug",
    title: "Fix the reconnect bug",
    viewer: FOUNDER,
  });
  assert.match(markup, /coding-session-goal-edit-workspace/);
  assert.match(markup, />Edit goal</);
  assert.doesNotMatch(markup, /Goal:/);
  assert.match(markup, /Founded by/);
});

test("without a linked genesis the row makes no founding claim", () => {
  const markup = render({
    goal: "Make authority visible",
    title: "Fix the reconnect bug",
    genesisRef: null,
  });
  assert.match(markup, /Make authority visible/);
  assert.doesNotMatch(markup, /Founded by/);
});
