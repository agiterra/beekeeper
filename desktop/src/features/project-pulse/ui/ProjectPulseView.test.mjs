import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const PROJECT =
  "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo";
const ALICE = "a1".repeat(32);
const BOB = "b2".repeat(32);
const NOW = 1_785_513_037;

const id = (suffix) => suffix.padStart(64, "0");

function entry(overrides = {}) {
  return {
    eventId: id("e1"),
    pubkey: ALICE,
    createdAt: NOW - 600,
    type: "plan",
    text: "Refactoring session creation.",
    claimedAreas: [],
    branch: null,
    sessionRef: null,
    supersedes: null,
    supersededBy: [],
    active: true,
    ...overrides,
  };
}

function session(overrides = {}) {
  return {
    targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:1",
    sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    name: "Pulse plumbing",
    goal: null,
    status: "running",
    statusAt: NOW - 60,
    closed: false,
    activity: "active",
    branch: "wip/project-pulse",
    observedCommit: null,
    dirty: null,
    relayReachable: null,
    verifiedAt: null,
    commitConfirmation: "Commit not checked",
    observedAgeSeconds: 60,
    sourceEventIds: [id("s1")],
    ...overrides,
  };
}

function digest(overrides = {}) {
  return {
    schema: "buzz-project-pulse-digest/v1",
    source: "client-composed",
    project: PROJECT,
    asOf: NOW,
    complete: true,
    sessionsScope: "project channels",
    sessions: [],
    entries: [],
    errors: [],
    ...overrides,
  };
}

async function renderView(state) {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { ProjectPulseView } = await import(
    "@/features/project-pulse/ui/ProjectPulseView"
  );
  return render(createElement(ProjectPulseView, { state, nowSeconds: NOW }));
}

test("the header sentence never claims more than the screen shows", async () => {
  const { getByTestId, queryByText } = await renderView({ kind: "loading" });
  assert.equal(
    getByTestId("pulse-header-subtitle").textContent,
    "Explicit updates and observed session state.",
  );
  assert.equal(queryByText(/Live summaries/i), null);
  assert.equal(queryByText(/Automatic summary/i), null);
});

test("loading, confirmed-empty, and unavailable are three different cards", async () => {
  const loading = await renderView({ kind: "loading" });
  assert.ok(loading.getByTestId("pulse-loading"));
  assert.equal(loading.queryByTestId("pulse-empty"), null);
  loading.unmount();

  const unavailable = await renderView({ kind: "unavailable" });
  assert.ok(unavailable.getByTestId("pulse-unavailable"));
  assert.match(
    unavailable.getByTestId("pulse-unavailable").textContent,
    /not a claim that the project is empty/,
  );
  assert.equal(unavailable.queryByTestId("pulse-empty"), null);
  unavailable.unmount();

  const empty = await renderView({ kind: "ready", digest: digest() });
  assert.ok(empty.getByTestId("pulse-empty"));
  assert.match(empty.getByTestId("pulse-empty").textContent, /read completed/);
  assert.equal(empty.queryByTestId("pulse-loading"), null);
});

/**
 * A digest can be `complete` — every query answered — and still have lost
 * individual events: an entry the fold could not validate, or a 44223 the
 * client could not decode. Saying "the project is quiet" over those is a null
 * rendered as a confirmed negative, which is exactly what this screen exists
 * to prevent.
 */
test("a complete read that excluded events never says the project is quiet", async () => {
  const { getByTestId, queryByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      errors: [
        {
          scope: "invalid-event",
          message: "event abc (kind 44223) failed client validation",
        },
      ],
    }),
  });
  assert.equal(queryByTestId("pulse-empty"), null);
  const excluded = getByTestId("pulse-excluded");
  assert.match(excluded.textContent, /Some events were excluded/);
  assert.match(excluded.textContent, /failed client validation/);
});

test("a partial read says so and still shows what it read", async () => {
  const { getByTestId } = await renderView({
    kind: "partial",
    digest: digest({
      complete: false,
      entries: [entry()],
      errors: [{ scope: "sessions", message: "relay unavailable" }],
    }),
  });
  assert.match(getByTestId("pulse-partial").textContent, /Partial read/);
  assert.match(getByTestId("pulse-partial").textContent, /relay unavailable/);
  assert.equal(getByTestId("pulse-entries").querySelectorAll("li").length, 1);
});

test("Active work and Last seen are separate groups", async () => {
  const { getByTestId, queryByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [
        session(),
        session({
          targetKey: "coding-session/v1|3:acp6:inst-26:sess-21:1",
          activity: "stale",
          status: "disconnected",
          statusAt: NOW - 10_800,
          observedAgeSeconds: 10_800,
          name: "Orphaned run",
        }),
      ],
    }),
  });
  assert.equal(
    getByTestId("pulse-active-work").querySelectorAll("li").length,
    1,
  );
  const lastSeen = getByTestId("pulse-last-seen");
  assert.equal(lastSeen.querySelectorAll("li").length, 1);
  assert.match(lastSeen.textContent, /Disconnected · last observed 3h ago/);
  assert.equal(queryByTestId("pulse-empty"), null);
  assert.match(
    getByTestId("pulse-sessions-scope").textContent,
    /channels only/,
  );
});

test("the three commit-confirmation strings render verbatim", async () => {
  const cases = [
    [true, NOW - 120, "Commit confirmed on relay · 2m ago"],
    [false, NOW - 120, "Commit not found on relay · 2m ago"],
    [null, null, "Commit not checked"],
  ];
  for (const [relayReachable, verifiedAt, expected] of cases) {
    const view = await renderView({
      kind: "ready",
      digest: digest({
        sessions: [
          session({
            relayReachable,
            verifiedAt,
            commitConfirmation:
              relayReachable === true
                ? "Commit confirmed on relay"
                : relayReachable === false
                  ? "Commit not found on relay"
                  : "Commit not checked",
          }),
        ],
      }),
    });
    assert.equal(
      view.getByTestId("pulse-session-commit-confirmation").textContent,
      expected,
    );
    assert.equal(
      view.queryByText(/relay reachable|relay unreachable/i),
      null,
      "the connection words are never rendered",
    );
    view.unmount();
  }
});

test("null observations render as unknown, never as false", async () => {
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({ sessions: [session()] }),
  });
  assert.equal(
    getByTestId("pulse-session-dirty").textContent,
    "Worktree not observed",
  );
  assert.equal(
    getByTestId("pulse-session-commit").textContent,
    "Commit unknown",
  );
  assert.equal(
    getByTestId("pulse-session-observed-age").textContent,
    "observed 1m ago",
  );
});

test("a cross-author supersession leaves the target active and names the claimant", async () => {
  const target = entry({ eventId: id("c1"), type: "blocker" });
  const claimant = entry({
    eventId: id("c2"),
    pubkey: BOB,
    createdAt: NOW - 300,
    supersedes: id("c1"),
    supersededBy: [
      {
        eventId: id("c1"),
        pubkey: ALICE,
        honored: false,
        reason: "cross-author",
      },
    ],
  });
  const { getAllByTestId, getByTestId } = await renderView({
    kind: "ready",
    digest: digest({ entries: [claimant, target] }),
  });
  assert.equal(getAllByTestId("pulse-entry-row").length, 2);
  assert.match(
    getByTestId("pulse-entry-supersession-claimed").textContent,
    /supersession claimed by/,
  );
  assert.match(
    getByTestId("pulse-entry-unhonored-claim").textContent,
    /not honored \(different author\)/,
  );
});

test("an unresolved supersedes is echoed, not resolved away", async () => {
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [
        entry({
          supersedes: id("ff"),
          supersededBy: [
            {
              eventId: id("ff"),
              pubkey: null,
              honored: false,
              reason: "unresolved",
            },
          ],
        }),
      ],
      errors: [
        {
          scope: "unresolved-supersedes",
          message: `entry ${id("e1")} supersedes ${id("ff")}, which is not in the visible result set`,
        },
      ],
    }),
  });
  assert.match(
    getByTestId("pulse-entry-unhonored-claim").textContent,
    /\(not visible\)$/,
  );
});

test("superseded entries stay reachable behind progressive disclosure", async () => {
  const { getByTestId, queryAllByTestId, getAllByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [
        entry({ eventId: id("d2"), createdAt: NOW - 100 }),
        entry({
          eventId: id("d1"),
          active: false,
          supersededBy: [
            { eventId: id("d2"), pubkey: ALICE, honored: true, reason: null },
          ],
        }),
      ],
    }),
  });
  assert.equal(queryAllByTestId("pulse-entry-row").length, 1);
  const toggle = getByTestId("pulse-superseded-toggle");
  assert.match(toggle.textContent, /1 superseded entry/);
  const { act } = await import("react");
  await act(async () => {
    toggle.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.equal(getAllByTestId("pulse-entry-row").length, 2);
});

test("claimed areas are labelled as claims and 'no branch' is a real group", async () => {
  const { getByTestId, getAllByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [entry({ claimedAreas: ["crates/buzz-acp/src/pool.rs"] })],
      sessions: [session({ branch: null })],
    }),
  });
  assert.match(getByTestId("pulse-entries").textContent, /Claimed areas:/);
  assert.match(
    getAllByTestId("pulse-entry-claimed-area")[0].getAttribute("title"),
    /claims this area; nothing here was observed/,
  );
  const chips = getAllByTestId("pulse-branch-chip").map(
    (chip) => chip.textContent,
  );
  assert.ok(chips.includes("no branch"), chips.join(","));
});

test("an entry naming a session renders at project level, never inside the card", async () => {
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [entry({ sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10" })],
      sessions: [session()],
    }),
  });
  const reference = getByTestId("pulse-entry-session-reference");
  assert.match(reference.textContent, /references session/);
  assert.equal(
    getByTestId("pulse-active-work").contains(reference),
    false,
    "an unverified pu-session claim never renders inside that session's card",
  );
});
