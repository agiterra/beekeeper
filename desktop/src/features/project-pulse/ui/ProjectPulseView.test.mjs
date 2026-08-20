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

/** Names the real screen resolves through `useUsersBatchQuery`. */
const AUTHOR_NAMES = new Map([
  [ALICE, "alice"],
  [BOB, "bob"],
]);

async function renderView(state, props = {}) {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { ProjectPulseView } = await import(
    "@/features/project-pulse/ui/ProjectPulseView"
  );
  return render(
    createElement(ProjectPulseView, {
      state,
      nowSeconds: NOW,
      authorNames: AUTHOR_NAMES,
      ...props,
    }),
  );
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
 * Five honesty states in one identical grey card means the reader has to parse
 * a paragraph to learn which of five worlds they are in. Each one leads with a
 * short verdict, and the two that mean "this is not the whole answer" carry the
 * warning treatment the other three do not.
 */
test("each state leads with its own verdict, and only the incomplete ones warn", async () => {
  const cases = [
    [{ kind: "loading" }, "pulse-loading", /^Reading…/, false],
    [{ kind: "unavailable" }, "pulse-unavailable", /^Not readable\./, false],
    [{ kind: "ready", digest: digest() }, "pulse-empty", /^Quiet\./, false],
    [
      {
        kind: "partial",
        digest: digest({
          complete: false,
          errors: [{ scope: "sessions", message: "relay unavailable" }],
        }),
      },
      "pulse-partial",
      /^Partial read\./,
      true,
    ],
    [
      {
        kind: "ready",
        digest: digest({
          errors: [{ scope: "invalid-event", message: "event abc excluded" }],
        }),
      },
      "pulse-excluded",
      /^Some events were excluded\./,
      true,
    ],
  ];
  for (const [state, testId, verdict, warns] of cases) {
    const view = await renderView(state);
    const card = view.getByTestId(testId);
    assert.match(card.textContent, verdict, testId);
    assert.equal(
      card.className.includes("amber"),
      warns,
      `${testId} warning treatment`,
    );
    view.unmount();
  }
});

test("the loading card shows skeleton rows, not just a sentence", async () => {
  const { getByTestId } = await renderView({ kind: "loading" });
  assert.ok(getByTestId("pulse-loading-skeleton"));
});

test("the screen names the project it describes and offers a way back", async () => {
  const clicks = [];
  const { getByTestId } = await renderView(
    { kind: "ready", digest: digest() },
    { projectName: "Pulse Demo", onBack: () => clicks.push("back") },
  );
  assert.equal(
    getByTestId("pulse-header-title").textContent,
    "Pulse · Pulse Demo",
  );
  const back = getByTestId("pulse-back");
  assert.match(back.textContent, /Pulse Demo/);
  const { act } = await import("react");
  await act(async () => {
    back.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.deepEqual(clicks, ["back"]);
});

/**
 * A quiet state that says "nobody has posted" and offers no control reads as
 * an invitation to a button that does not exist. Desktop cannot write a 44240
 * in v1, and the empty state has to say so.
 */
test("the empty state teaches the CLI first action and admits it is read-only", async () => {
  const { getByTestId } = await renderView({ kind: "ready", digest: digest() });
  const hint = getByTestId("pulse-write-hint");
  assert.match(hint.textContent, /buzz pulse update --project/);
  assert.match(hint.textContent, /cannot post one for you yet/);
});

/**
 * The read's own age, and the admission that a cached digest is the last
 * complete read rather than the current one.
 */
test("the header dates the read and marks a cached one as cached", async () => {
  const fresh = await renderView({ kind: "ready", digest: digest() });
  assert.equal(
    fresh.getByTestId("pulse-read-age").textContent,
    "read just now",
  );
  assert.equal(fresh.queryByTestId("pulse-stale-read"), null);
  fresh.unmount();

  const cached = await renderView({
    kind: "ready",
    digest: digest({ asOf: NOW - 600 }),
    refreshing: true,
  });
  assert.equal(
    cached.getByTestId("pulse-read-age").textContent,
    "read 10m ago",
  );
  assert.match(
    cached.getByTestId("pulse-stale-read").textContent,
    /showing last complete read/,
  );
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

/**
 * Both halves of one refused claim say the same thing in the same words, and
 * neither says "supersession" — a term of art from the plan, not a word a
 * reader of this screen would use. The claim names the entry it points at by
 * that entry's own content, because no other row on the screen shows an id a
 * reader could match a hash against.
 */
test("a cross-author claim reads as one relationship, in plain language, on both rows", async () => {
  const target = entry({
    eventId: id("c1"),
    type: "blocker",
    text: "Do not touch pool.rs; the creation path is half-migrated.",
  });
  const claimant = entry({
    eventId: id("c2"),
    pubkey: BOB,
    createdAt: NOW - 300,
    text: "Picking pool.rs back up.",
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
  const { container, getAllByTestId, getByTestId } = await renderView({
    kind: "ready",
    digest: digest({ entries: [claimant, target] }),
  });
  assert.equal(getAllByTestId("pulse-entry-row").length, 2);

  const onTarget = getByTestId("pulse-entry-supersession-claimed");
  assert.match(onTarget.textContent, /^bob says this is resolved/);
  assert.match(onTarget.textContent, /this one stays active/);

  const onClaimant = getByTestId("pulse-entry-unhonored-claim");
  assert.match(onClaimant.textContent, /Do not touch pool\.rs/);
  assert.match(onClaimant.textContent, /Blocker · 10m ago/);
  assert.match(onClaimant.textContent, /by alice/);
  assert.match(onClaimant.textContent, /the original stays active/);

  assert.equal(
    /supersession|supersedes/i.test(container.textContent),
    false,
    "no user-facing copy uses the plan's term of art",
  );
  assert.equal(
    container.textContent.includes(id("c1")),
    false,
    "the raw event id never renders as body text",
  );
  assert.match(onClaimant.getAttribute("title"), new RegExp(id("c1")));
});

test("an unresolved claim is echoed, not resolved away, and prints no hash", async () => {
  const { container, getByTestId } = await renderView({
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
  const claim = getByTestId("pulse-entry-unhonored-claim");
  assert.match(claim.textContent, /not visible in this read/);
  assert.match(claim.textContent, /nothing was replaced/);
  assert.equal(claim.textContent.includes(id("ff")), false);
  assert.match(claim.getAttribute("title"), new RegExp(id("ff")));
  // The digest's own `errors[]` still carries the raw ids — that card is the
  // audit trail, and it is a different surface from the row's sentence.
  assert.ok(container.textContent.includes(id("ff")));
});

test("superseded entries stay reachable, and say they are retired and by whom", async () => {
  const { getByTestId, queryAllByTestId, getAllByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [
        entry({
          eventId: id("d2"),
          createdAt: NOW - 100,
          text: "Wire contract landed; starting relay ingest.",
        }),
        entry({
          eventId: id("d1"),
          createdAt: NOW - 1_800,
          text: "First pass at the wire contract.",
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
  assert.ok(
    getByTestId("pulse-entry-superseded-badge"),
    "a retired entry is labelled, not only dimmed",
  );
  const replaced = getByTestId("pulse-entry-replaced-by");
  assert.match(replaced.textContent, /^Replaced by alice, 1m ago/);
  assert.match(replaced.textContent, /Wire contract landed/);
});

/**
 * Salience tracks consequence: the entry that can stop somebody else's work
 * leads the list, whatever the fold's newest-first order says.
 */
test("blockers lead the entries list and carry their own treatment", async () => {
  const { getAllByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [
        entry({ eventId: id("b3"), type: "note", createdAt: NOW - 10 }),
        entry({ eventId: id("b2"), type: "handoff", createdAt: NOW - 20 }),
        entry({ eventId: id("b1"), type: "blocker", createdAt: NOW - 3_000 }),
      ],
    }),
  });
  const rows = getAllByTestId("pulse-entry-row");
  assert.deepEqual(
    rows.map((row) => row.getAttribute("data-entry-type")),
    ["blocker", "handoff", "note"],
  );
  assert.ok(
    rows[0].className.includes("destructive"),
    "a blocker is not the same grey as a plan",
  );
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
  assert.ok(
    chips.some((label) => label.startsWith("no branch")),
    chips.join(","),
  );
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
  // The session's name, not its uuid — the uuid resolves to nothing on screen.
  assert.equal(reference.textContent, "references session “Pulse plumbing”");
  assert.match(reference.getAttribute("title"), /5b7e1c2a/);
  assert.equal(
    getByTestId("pulse-active-work").contains(reference),
    false,
    "an unverified pu-session claim never renders inside that session's card",
  );
});

test("a session reference this digest cannot resolve says so in words", async () => {
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [entry({ sessionRef: "0f0f0f0f-0000-4000-8000-000000000000" })],
      sessions: [session()],
    }),
  });
  assert.equal(
    getByTestId("pulse-entry-session-reference").textContent,
    "references a session outside this project",
  );
});

/**
 * The sessions block is the one place this surface admits its own
 * non-exhaustiveness. Gating it on `sessions.length > 0` removed that
 * admission in exactly the state where a reader would otherwise conclude
 * "nobody is running anything here."
 */
test("a readable digest with no sessions still says so, with the scope caveat", async () => {
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({ entries: [entry()] }),
  });
  assert.equal(
    getByTestId("pulse-sessions-empty").textContent,
    "No coding sessions observed in this project's channels.",
  );
  assert.match(
    getByTestId("pulse-sessions-scope").textContent,
    /channels only/,
  );
});

test("an incomplete read never reports 'no sessions' as a finding", async () => {
  const { getByTestId } = await renderView({
    kind: "partial",
    digest: digest({
      complete: false,
      entries: [entry()],
      errors: [{ scope: "sessions", message: "relay unavailable" }],
    }),
  });
  assert.match(
    getByTestId("pulse-sessions-empty").textContent,
    /not a confirmed answer/,
  );
});

test("a branch filter that hides every session says that, not that there are none", async () => {
  const { getAllByTestId, getByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [session({ branch: "wip/project-pulse" })],
      entries: [entry({ branch: "main" })],
    }),
  });
  const chips = getAllByTestId("pulse-branch-chip");
  const mainChip = chips.find((chip) => chip.textContent.startsWith("main"));
  const { act } = await import("react");
  await act(async () => {
    mainChip.dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true }),
    );
  });
  assert.match(
    getByTestId("pulse-sessions-empty").textContent,
    /No coding sessions on this branch/,
  );
});

/**
 * The chips are a control, so they read as one — a labelled segmented group
 * whose counts agree with the rows behind them (sessions plus active entries;
 * superseded history stays behind its own disclosure count).
 */
test("branch chips are a labelled control whose counts match the rows", async () => {
  const { getAllByTestId, getByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [session({ branch: "wip/project-pulse" })],
      entries: [
        entry({ eventId: id("f1"), branch: "wip/project-pulse" }),
        entry({ eventId: id("f2"), branch: null }),
      ],
    }),
  });
  assert.match(getByTestId("pulse-branch-chips").textContent, /^Branch/);
  assert.equal(
    getByTestId("pulse-branch-chip-all").textContent,
    "All branches3",
  );
  const chips = getAllByTestId("pulse-branch-chip").map(
    (chip) => chip.textContent,
  );
  assert.deepEqual(chips, ["wip/project-pulse2", "no branch1"]);
  assert.equal(
    getByTestId("pulse-branch-chip-all").getAttribute("aria-pressed"),
    "true",
  );
});

/**
 * `VISION_ACTIVITY.md`: "the reader thinks in names, not hashes." A pubkey may
 * appear in a `title` for copying, never as the label of a person.
 */
test("authors render as names, with the hex only in a copyable title", async () => {
  const { container, getAllByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [entry(), entry({ eventId: id("a2"), pubkey: BOB })],
    }),
  });
  const rows = getAllByTestId("pulse-entry-row");
  assert.match(rows[0].textContent, /alice/);
  assert.match(rows[1].textContent, /bob/);
  assert.equal(container.textContent.includes(ALICE), false);
  assert.equal(
    container.querySelector(`[title="${ALICE}"]`).textContent,
    "alice",
  );
});
