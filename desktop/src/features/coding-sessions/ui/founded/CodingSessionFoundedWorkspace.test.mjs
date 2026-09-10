/**
 * The founded screen, from its facts.
 *
 * `CodingSessionFoundedView` is the workspace minus its hooks: it is handed
 * the route resolution (with closures layered on top), the viewer, the
 * founder's name and the goal, and renders what the founder and everybody
 * else sees — and owns the one effect on this screen, the hand-off to the
 * generation route once a create for the umbrella has produced a generation.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    Node: dom.window.Node,
    MutationObserver: dom.window.MutationObserver,
    getComputedStyle: dom.window.getComputedStyle,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

afterEach(async () => {
  // A failed assertion must not leak its tree into the next mount.
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "22222222-2222-4222-8222-222222222222";
const FOUNDER = "ab12cd34".repeat(8);
const OTHER = "9876fedc".repeat(8);

const FOUNDED = {
  channelId: CHANNEL_ID,
  sessionRef: SESSION_REF,
  genesisRef: "f".repeat(64),
  founderPubkey: FOUNDER,
  foundedAt: 1_700_000_000,
};

async function mount({
  currentUserPubkey = FOUNDER,
  goal = { kind: "available", text: "Close ledger item 104." },
  resolution = { kind: "founded", founded: FOUNDED },
} = {}) {
  const React = (await import("react")).default;
  const { act, cleanup, render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { CodingSessionFoundedView } = await import(
    "./CodingSessionFoundedWorkspace.tsx"
  );
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const navigations = [];
  const channelNavigations = [];
  const cards = [];
  const cardStartingFlags = [];
  const props = (next) => ({
    channelId: CHANNEL_ID,
    channelName: "Hive Sessions",
    currentUserPubkey,
    founderName: "Andrew",
    goChannel: (id) => channelNavigations.push(id),
    goCodingSession: (...args) => navigations.push(args),
    goal,
    resolution,
    sessionName: "Founded first",
    sessionRef: SESSION_REF,
    setupCard: (founded, starting) => {
      cards.push(founded);
      cardStartingFlags.push(starting);
      return React.createElement(
        "button",
        { "data-testid": "coding-session-founded-start", type: "button" },
        "Start",
      );
    },
    ...next,
  });
  const tree = (next) =>
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(CodingSessionFoundedView, props(next)),
    );
  let view = null;
  await act(async () => {
    view = render(tree({}));
  });
  return {
    cards,
    cardStartingFlags,
    channelNavigations,
    navigations,
    markup: () => document.body.innerHTML,
    rerender: async (next) => {
      await act(async () => {
        view.rerender(tree(next));
      });
    },
    teardown: () => cleanup(),
  };
}

test("the founder sees the header, the founder line, and the setup card with Start — no second title or goal", async () => {
  const screen = await mount();
  const markup = screen.markup();
  assert.match(
    markup,
    /data-testid="coding-session-founded-workspace-founded"/,
  );
  // No founder line for the founder: the header says "not started" and the
  // card says the rest.
  assert.doesNotMatch(markup, /coding-session-founded-founder/);
  assert.doesNotMatch(markup, /Founded by you/);
  assert.match(markup, /data-testid="coding-session-founded-start"/);
  assert.doesNotMatch(markup, /coding-session-founded-setup-readonly/);
  // The card is the form: the goal is its prompt field, not a paragraph
  // above it, and the one title on the page is the header's.
  assert.doesNotMatch(markup, /data-testid="coding-session-founded-goal"/);
  assert.equal(markup.match(/<h1/g).length, 1, "only the header's title");
  assert.deepEqual(screen.cards, [FOUNDED]);
  assert.deepEqual(screen.navigations, []);
  screen.teardown();
});

test("a member who did not found it sees the name, the goal, the read-only sentence and no Start", async () => {
  const screen = await mount({ currentUserPubkey: OTHER });
  const markup = screen.markup();
  assert.equal(markup.match(/<h1/g).length, 2, "the header's and the page's");
  assert.match(markup, /data-testid="coding-session-founded-goal"/);
  assert.match(markup, /Close ledger item 104\./);
  assert.match(markup, /Founded by .*Andrew.* — only they can start it\./);
  assert.match(markup, /data-testid="coding-session-founded-setup-readonly"/);
  assert.match(
    markup,
    /Not set up yet\. Solo or team, the runtime and where it runs are picked by .*Andrew.* when they start it; nothing about that is on the relay before then\./,
  );
  assert.doesNotMatch(markup, /data-testid="coding-session-founded-start"/);
  assert.doesNotMatch(markup, /data-testid="coding-session-founded-discard"/);
  assert.deepEqual(
    screen.cards,
    [],
    "the setup card is never mounted for them",
  );
  screen.teardown();
});

test("a goal this screen could not read is said to a non-founder, never shown blank", async () => {
  const screen = await mount({
    currentUserPubkey: OTHER,
    goal: { kind: "errored", message: "relay closed the subscription" },
  });
  const markup = screen.markup();
  assert.match(markup, /data-testid="coding-session-founded-goal-missing"/);
  assert.match(
    markup,
    /The goal for this session could not be read — relay closed the subscription/,
  );
  screen.teardown();
});

test("an execution appearing under the umbrella hands off to the generation route, once", async () => {
  const screen = await mount();
  assert.deepEqual(screen.navigations, []);
  const started = { kind: "started", generationId: "gen-1" };
  await screen.rerender({ resolution: started });
  assert.match(
    screen.markup(),
    /data-testid="coding-session-founded-workspace-started"/,
  );
  assert.deepEqual(screen.navigations, [
    [CHANNEL_ID, "gen-1", { replace: true }],
  ]);
  // The catalog keeps reporting the same fact; the screen does not keep
  // navigating on it.
  await screen.rerender({ resolution: { ...started } });
  await screen.rerender({ resolution: { ...started } });
  assert.equal(screen.navigations.length, 1);
  screen.teardown();
});

test("a create observed but no generation yet reads as starting, with Start withheld", async () => {
  const screen = await mount({
    resolution: { kind: "starting", founded: FOUNDED },
  });
  const markup = screen.markup();
  assert.match(
    markup,
    /data-testid="coding-session-founded-workspace-starting"/,
  );
  // The founder's own view has no line of its own; the card is still
  // mounted (the founder may need its error line) and is told a create
  // already claims the umbrella, which holds its Start with a sentence.
  assert.doesNotMatch(markup, /coding-session-founded-founder/);
  assert.deepEqual(screen.cardStartingFlags.at(-1), true);
  screen.teardown();
});

test("a non-founder in the starting state is told the create is on the relay, not that nothing is", async () => {
  const screen = await mount({
    currentUserPubkey: OTHER,
    resolution: { kind: "starting", founded: FOUNDED },
  });
  const markup = screen.markup();
  assert.match(markup, /data-testid="coding-session-founded-setup-readonly"/);
  assert.match(
    markup,
    /A create for this session is on the relay; its first report is still to come\./,
  );
  assert.doesNotMatch(markup, /nothing about that is on the relay before then/);
  screen.teardown();
});

test("loading and missing use the non-ready layout", async () => {
  const loading = await mount({ resolution: { kind: "loading" } });
  assert.match(
    loading.markup(),
    /data-testid="coding-session-founded-workspace-loading"/,
  );
  assert.match(loading.markup(), /Loading coding session/);
  loading.teardown();
  const missing = await mount({
    resolution: {
      kind: "missing",
      description:
        "No genesis for this session is available in the relay catalog.",
    },
  });
  assert.match(
    missing.markup(),
    /data-testid="coding-session-founded-workspace-missing"/,
  );
  assert.match(missing.markup(), /Session not found/);
  assert.match(missing.markup(), /No genesis for this session/);
  missing.teardown();
});

test("a closed founded session renders the closed state, no Start, and a way back", async () => {
  const screen = await mount({
    resolution: { kind: "closed", founded: FOUNDED },
  });
  const markup = screen.markup();
  assert.match(markup, /data-testid="coding-session-founded-workspace-closed"/);
  assert.match(markup, /Closed before it started/);
  assert.match(markup, /Reopen it from its row in the sidebar/);
  assert.doesNotMatch(markup, /data-testid="coding-session-founded-start"/);
  assert.deepEqual(screen.cards, [], "nothing to set up");
  // The header carries the name and reads Closed, not Not started.
  assert.match(markup, /Founded first/);
  assert.match(markup, /Closed/);
  assert.doesNotMatch(markup, /Not started/);
  const back = document.querySelector(
    '[data-testid="coding-session-founded-back"]',
  );
  assert.ok(back);
  assert.match(back.textContent, /Back to the channel/);
  const { act } = await import("@testing-library/react");
  await act(async () => {
    back.click();
  });
  assert.deepEqual(screen.channelNavigations, [CHANNEL_ID]);
  assert.deepEqual(screen.navigations, []);
  screen.teardown();
});

test("closures layer on top of the route: started wins, then closed, then founded", async () => {
  const { resolveFoundedCodingSessionView } = await import(
    "./CodingSessionFoundedWorkspace.tsx"
  );
  const started = { kind: "started", generationId: "gen-1" };
  assert.deepEqual(
    resolveFoundedCodingSessionView({ resolution: started, closed: true }),
    started,
    "a generation exists; the closure does not un-start it here",
  );
  assert.deepEqual(
    resolveFoundedCodingSessionView({
      resolution: { kind: "founded", founded: FOUNDED },
      closed: true,
    }),
    { kind: "closed", founded: FOUNDED },
  );
  assert.deepEqual(
    resolveFoundedCodingSessionView({
      resolution: { kind: "starting", founded: FOUNDED },
      closed: true,
    }),
    { kind: "closed", founded: FOUNDED },
  );
  for (const resolution of [
    { kind: "founded", founded: FOUNDED },
    { kind: "loading" },
    { kind: "missing", description: "gone" },
  ]) {
    assert.deepEqual(
      resolveFoundedCodingSessionView({ resolution, closed: false }),
      resolution,
    );
  }
  // A closure for a session the catalog has not resolved yet is not a
  // closed page: there is no umbrella to say was closed.
  assert.deepEqual(
    resolveFoundedCodingSessionView({
      resolution: { kind: "loading" },
      closed: true,
    }),
    { kind: "loading" },
  );
});
