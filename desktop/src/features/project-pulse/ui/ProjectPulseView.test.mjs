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
  const targetKey =
    overrides.targetKey ?? "coding-session/v1|3:acp6:inst-16:sess-11:1";
  const sessionRef = Object.hasOwn(overrides, "sessionRef")
    ? overrides.sessionRef
    : "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const statusAt = overrides.statusAt ?? NOW - 60;
  const lifecycle =
    overrides.lifecycle ?? (overrides.closed ? "closed" : "open");
  const coordinationState =
    overrides.coordinationState ??
    (lifecycle === "closed"
      ? "closed"
      : overrides.activity === "stale"
        ? "open_unverified"
        : "provider_reachable");
  const status = overrides.status ?? "running";
  const reachability =
    overrides.reachability ??
    (status === "stopped" || status === "disconnected"
      ? "terminal"
      : coordinationState === "provider_reachable"
        ? "provider_reachable"
        : "unverified");
  return {
    sessionKey: sessionRef ?? `implicit:${targetKey}`,
    sessionRef,
    name: overrides.name ?? "Pulse plumbing",
    goal: overrides.goal ?? null,
    lifecycle,
    coordinationState,
    latestObservationAt: statusAt,
    observedAgeSeconds: overrides.observedAgeSeconds ?? 60,
    generations: overrides.generations ?? [
      {
        targetKey,
        executionKey: `coding-execution/v1|${targetKey}`,
        providerAuthorityPubkey: "d4".repeat(32),
        current: true,
        reachability,
        status,
        statusAt,
        branch: overrides.branch ?? "wip/project-pulse",
        observedCommit: overrides.observedCommit ?? null,
        dirty: overrides.dirty ?? null,
        relayReachable: overrides.relayReachable ?? null,
        verifiedAt: overrides.verifiedAt ?? null,
        commitConfirmation:
          overrides.commitConfirmation ?? "Commit not checked",
        leaseState: reachability === "provider_reachable" ? "live" : null,
        leaseIssuedAt: reachability === "provider_reachable" ? NOW - 30 : null,
        leaseAcceptedAt: null,
        leaseExpiresAt:
          reachability === "provider_reachable" ? NOW + 150 : null,
        leaseSigner:
          reachability === "provider_reachable" ? "d4".repeat(32) : null,
        leaseSourceEventId:
          reachability === "provider_reachable" ? id("l1") : null,
        leaseSequence: reachability === "provider_reachable" ? 1 : null,
        lifecycleCommandEventId: id("c1"),
        lifecycleReceiptEventId: id("r1"),
        sourceEventIds: [id("c1"), id("r1"), id("s1")],
      },
    ],
    sourceEventIds: [id("c1"), id("r1"), id("s1")],
  };
}

function digest(overrides = {}) {
  return {
    schema: "buzz-project-pulse-digest/v2",
    source: "client-composed",
    project: PROJECT,
    asOf: NOW,
    complete: true,
    sessionsScope: "project channels",
    sessions: [],
    providerReachableSessions: [],
    openUnverifiedSessions: [],
    closedSessions: [],
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

test("session presentation separates provider reachability, open uncertainty, and closed history", async () => {
  const { groupPulseSessions } = await import(
    "@/features/project-pulse/lib/pulseFormat"
  );
  const reachable = session();
  const openUnverified = session({
    targetKey: "coding-session/v1|3:acp6:inst-26:sess-21:1",
    coordinationState: "open_unverified",
    observedAgeSeconds: 10_800,
  });
  const closed = session({
    targetKey: "coding-session/v1|3:acp6:inst-36:sess-31:1",
    coordinationState: "closed",
    lifecycle: "closed",
  });

  const groups = groupPulseSessions(
    digest({ sessions: [reachable, openUnverified, closed] }),
  );

  assert.deepEqual(groups.providerReachable, [reachable]);
  assert.deepEqual(groups.openUnverified, [openUnverified]);
  assert.deepEqual(groups.closed, [closed]);
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
    [
      { kind: "ready", digest: digest() },
      "pulse-empty",
      /^No Pulse observations\./,
      false,
    ],
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
          errors: [
            {
              scope: "invalid-event",
              message: `event ${id("c3")} (kind 44223) failed signature validation and was excluded`,
            },
          ],
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
 * An observation-empty state that offers no control reads as an invitation to
 * a button that does not exist. Desktop cannot write a 44240 in v1, and the
 * empty state has to say so.
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
  assert.equal(
    cached.queryByTestId("pulse-empty"),
    null,
    "a refreshing cached empty digest is not a confirmed current absence",
  );
  assert.doesNotMatch(
    cached.container.textContent,
    /No sessions are currently verified live/,
    "no current-tense absence claim may come from the last completed read",
  );
  assert.match(
    cached.getByTestId("pulse-sessions-empty").textContent,
    /last completed read.*refreshing/i,
  );
});

/**
 * A digest can be `complete` — every query answered — and still have lost
 * individual events: an entry the fold could not validate, or a 44223 the
 * client could not decode. Turning those omissions into a settled absence is a
 * null rendered as a confirmed negative, which is exactly what this screen
 * exists to prevent.
 */
test("a complete read that excluded events never presents absence as settled", async () => {
  const { getByTestId, queryByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      errors: [
        {
          scope: "invalid-event",
          message: `event ${id("c3")} (kind 44223) carried undecodable coding-session metadata and was excluded`,
        },
      ],
    }),
  });
  assert.equal(queryByTestId("pulse-empty"), null);
  const excluded = getByTestId("pulse-excluded");
  assert.match(excluded.textContent, /Some events were excluded/);
  // What was lost, in the reader's vocabulary — not `invalid-event: event
  // 0000…c3 (kind 44223) …`, which names an internal scope and an id the
  // reader cannot look up anywhere on this screen.
  assert.match(
    excluded.textContent,
    /A coding-session update was left out — its session details could not be read\./,
  );
  assert.equal(excluded.textContent.includes(id("c3")), false);
  assert.equal(excluded.textContent.includes("invalid-event"), false);
  assert.match(
    getByTestId("pulse-error-note").getAttribute("title"),
    new RegExp(`^invalid-event: event ${id("c3")} `),
  );
});

/**
 * Three events lost the same way is one fact, and the card says it once with a
 * count. Repeating an identical sentence per wire record would read as three
 * different problems and bury the one that is different.
 */
test("repeats of one loss collapse into a single counted sentence", async () => {
  const { getByTestId, getAllByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      errors: [
        {
          scope: "invalid-entry",
          message: `entry ${id("71")} failed validation and was excluded`,
        },
        {
          scope: "invalid-entry",
          message: `entry ${id("72")} failed validation and was excluded`,
        },
        {
          scope: "invalid-event",
          message: `event ${id("c3")} (kind 44227) failed signature validation and was excluded`,
        },
      ],
    }),
  });
  const notes = getAllByTestId("pulse-error-note");
  assert.equal(notes.length, 2);
  assert.match(
    notes[0].textContent,
    /^2 entries were left out — they did not pass validation\.$/,
  );
  assert.match(
    notes[1].textContent,
    /^A coding-session update was left out — its signature did not check out\.$/,
  );
  // Nothing is dropped on the way to the shorter sentence: both wire records
  // are still readable behind the note that stands for them.
  const title = notes[0].getAttribute("title");
  assert.ok(title.includes(id("71")));
  assert.ok(title.includes(id("72")));
  assert.equal(
    getByTestId("pulse-excluded").textContent.includes(id("71")),
    false,
  );
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

test("provider-reachable, open-unverified, and closed sessions render as separate honest groups", async () => {
  const { container, getByTestId, queryByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [
        session(),
        session({
          targetKey: "coding-session/v1|3:acp6:inst-26:sess-21:1",
          coordinationState: "open_unverified",
          status: "disconnected",
          statusAt: NOW - 10_800,
          observedAgeSeconds: 10_800,
          name: "Orphaned run",
        }),
        session({
          targetKey: "coding-session/v1|3:acp6:inst-36:sess-31:1",
          coordinationState: "closed",
          status: "stopped",
          statusAt: NOW - 21_600,
          observedAgeSeconds: 21_600,
          name: "Finished run",
          lifecycle: "closed",
        }),
      ],
    }),
  });
  assert.equal(
    getByTestId("pulse-provider-reachable").querySelectorAll("li").length,
    1,
  );
  assert.match(
    getByTestId("pulse-provider-reachable").textContent,
    /Provider-reachable sessions/,
  );
  const openUnverified = getByTestId("pulse-open-unverified");
  assert.equal(openUnverified.querySelectorAll("li").length, 1);
  assert.match(openUnverified.textContent, /Open · liveness unverified/);
  assert.match(
    openUnverified.textContent,
    /Disconnected · last observed 3h ago/,
  );
  const closed = getByTestId("pulse-closed");
  assert.equal(closed.querySelectorAll("li").length, 1);
  assert.match(closed.textContent, /Closed\/history/);
  assert.equal(
    /active work|\blive\b|currently connected/i.test(container.textContent),
    false,
  );
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
  assert.match(
    getByTestId("pulse-session-status").textContent,
    /observed 1m ago/,
  );
  assert.equal(
    document.querySelector("[data-testid='pulse-session-observed-age']"),
    null,
    "the header already carries the age, so the metadata row does not repeat it",
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
  // Not on the row and not on the error card either: the digest's `errors[]`
  // is a wire record, and the card that stands for it says the same sentence
  // in the same words, naming the entry by its text. The verbatim record —
  // ids and all — survives in the note's `title`, one hover away.
  const note = getByTestId("pulse-error-note");
  assert.match(note.textContent, /“Refactoring session creation\.”/);
  assert.match(note.textContent, /not visible in this read/);
  assert.match(note.textContent, /nothing was replaced/);
  assert.equal(container.textContent.includes(id("ff")), false);
  assert.equal(container.textContent.includes(id("e1")), false);
  assert.match(note.getAttribute("title"), new RegExp(id("ff")));
  assert.match(note.getAttribute("title"), /^unresolved-supersedes: /);
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
    getByTestId("pulse-provider-reachable").contains(reference),
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
 * admission in exactly the state where a reader might otherwise treat absence
 * as a settled liveness conclusion.
 */
test("a readable digest with no sessions still says so, with the scope caveat", async () => {
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({ entries: [entry()] }),
  });
  assert.equal(
    getByTestId("pulse-sessions-empty").textContent,
    "No sessions are currently verified live.",
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
 * Regression: the chips are enumerated over **every** generation's branch
 * (`pulseDigestBranches`), so a branch that only ever hosted a superseded
 * generation still gets a chip. If the session rows matched on the display
 * generation's branch alone, that chip would count zero and show zero cards —
 * a control offering a view of work it then refuses to show. The shipped fold
 * settled this: filter executions first, then collapse.
 */
test("a chip for a branch only a superseded generation ran on still shows its session", async () => {
  const current = session({
    targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:2",
  }).generations[0];
  const superseded = session({
    targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:1",
    branch: "wip/earlier-attempt",
    status: "stopped",
    statusAt: NOW - 7_200,
  }).generations[0];
  superseded.current = false;
  assert.equal(current.branch, "wip/project-pulse");

  const { getAllByTestId, getByTestId, queryByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [session({ generations: [current, superseded] })],
    }),
  });

  // The chip exists, and its count already promises the row.
  const chips = getAllByTestId("pulse-branch-chip").map(
    (chip) => chip.textContent,
  );
  assert.deepEqual(chips, ["wip/earlier-attempt1", "wip/project-pulse1"]);

  // Selecting it shows the umbrella, rather than an empty state.
  const supersededChip = getAllByTestId("pulse-branch-chip").find((chip) =>
    chip.textContent.startsWith("wip/earlier-attempt"),
  );
  const { act } = await import("react");
  await act(async () => {
    supersededChip.dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true }),
    );
  });
  assert.equal(queryByTestId("pulse-sessions-empty"), null);
  assert.equal(getAllByTestId("pulse-session-card").length, 1);
  assert.equal(
    getByTestId("pulse-count-sessions").textContent,
    "1 session · 1 provider-reachable",
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

test("generations of one umbrella collapse into one disclosed session card", async () => {
  const opened = [];
  const newest = session({
    targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:3",
  }).generations[0];
  const generation2 = session({
    targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:2",
    status: "stopped",
    statusAt: NOW - 7_200,
    observedCommit: "50dee7a75a99abcd",
  }).generations[0];
  generation2.current = false;
  const generation1 = session({
    targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:1",
    status: "stopped",
    statusAt: NOW - 10_800,
  }).generations[0];
  generation1.current = false;

  const { getAllByTestId, getByTestId, queryByTestId } = await renderView(
    {
      kind: "ready",
      digest: digest({
        sessions: [
          session({
            targetKey: newest.targetKey,
            name: "dedupe_test",
            generations: [newest, generation2, generation1],
          }),
        ],
      }),
    },
    { onOpenSession: (key) => opened.push(key) },
  );

  assert.equal(getAllByTestId("pulse-session-card").length, 1);
  assert.equal(
    getByTestId("pulse-session-card").getAttribute("data-execution-count"),
    "3",
  );
  assert.ok(getByTestId("pulse-provider-reachable"));
  assert.equal(queryByTestId("pulse-open-unverified"), null);
  assert.equal(queryByTestId("pulse-closed"), null);

  const toggle = getByTestId("pulse-session-executions-toggle");
  assert.match(toggle.textContent, /3 executions/);
  assert.equal(queryByTestId("pulse-session-execution-list"), null);
  const { act } = await import("react");
  await act(async () => {
    toggle.dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  const history = getAllByTestId("pulse-session-execution");
  assert.equal(history.length, 2);
  assert.match(history[0].textContent, /Ended · last observed 2h ago/);
  assert.match(history[0].textContent, /50dee7a75a99/);

  await act(async () => {
    history[0]
      .querySelector("[data-testid='pulse-session-execution-open']")
      .dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.deepEqual(opened, [generation2.targetKey]);
});

test("sessions without an umbrella reference remain separate cards", async () => {
  const { getAllByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [
        session({ sessionRef: null }),
        session({
          targetKey: "coding-session/v1|3:acp6:inst-26:sess-21:1",
          sessionRef: null,
        }),
      ],
    }),
  });
  assert.equal(getAllByTestId("pulse-session-card").length, 2);
});

test("entries and their superseded disclosure render above session observations", async () => {
  const original = entry({ eventId: id("d1"), active: false });
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [
        entry({
          eventId: id("d2"),
          createdAt: NOW - 300,
          supersedes: original.eventId,
        }),
        original,
      ],
      sessions: [session()],
    }),
  });
  const sessions = getByTestId("pulse-sessions");
  for (const preceding of [
    getByTestId("pulse-entries"),
    getByTestId("pulse-superseded"),
  ]) {
    assert.equal(
      preceding.compareDocumentPosition(sessions) &
        dom.window.Node.DOCUMENT_POSITION_FOLLOWING,
      dom.window.Node.DOCUMENT_POSITION_FOLLOWING,
    );
  }
});

test("the summary counts rendered umbrellas and names verified reachability", async () => {
  const current = session().generations[0];
  const old = session({
    targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:0",
    status: "stopped",
  }).generations[0];
  old.current = false;
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({
      entries: [entry()],
      sessions: [
        session({ generations: [current, old] }),
        session({
          targetKey: "coding-session/v1|3:acp6:inst-26:sess-21:1",
          sessionRef: "1d2e3f4a-5b6c-4d7e-8f90-a1b2c3d4e5f6",
          coordinationState: "open_unverified",
          status: "disconnected",
          statusAt: NOW - 10_800,
          observedAgeSeconds: 10_800,
        }),
      ],
    }),
  });
  assert.equal(getByTestId("pulse-count-entries").textContent, "1 entry");
  assert.equal(
    getByTestId("pulse-count-sessions").textContent,
    "2 sessions · 1 provider-reachable",
  );
});

test("counts over incomplete reads are explicit floors", async () => {
  const partial = await renderView({
    kind: "partial",
    digest: digest({
      complete: false,
      entries: [entry()],
      sessions: [session()],
      errors: [{ scope: "sessions", message: "relay unavailable" }],
    }),
  });
  assert.equal(
    partial.getByTestId("pulse-count-entries").textContent,
    "at least 1 entry",
  );
  assert.equal(
    partial.getByTestId("pulse-count-sessions").textContent,
    "at least 1 session · 1 provider-reachable",
  );
  assert.match(
    partial.getByTestId("pulse-counts-incomplete").textContent,
    /floors, not totals/,
  );
  partial.unmount();

  const empty = await renderView({
    kind: "partial",
    digest: digest({
      complete: false,
      errors: [{ scope: "entries", message: "relay unavailable" }],
    }),
  });
  assert.equal(
    empty.getByTestId("pulse-count-entries").textContent,
    "no entries in what this read returned",
  );
  assert.equal(
    empty.getByTestId("pulse-count-sessions").textContent,
    "no sessions in what this read returned",
  );
  empty.unmount();
});

test("confirmed empty renders one teaching state without a second summary", async () => {
  const { getAllByTestId, queryByTestId } = await renderView({
    kind: "ready",
    digest: digest(),
  });
  assert.equal(queryByTestId("pulse-counts"), null);
  assert.equal(queryByTestId("pulse-entries"), null);
  assert.equal(getAllByTestId("pulse-write-hint").length, 1);
});

test("entry absence distinguishes settled, incomplete, and branch-hidden reads", async () => {
  const settled = await renderView({
    kind: "ready",
    digest: digest({ sessions: [session()] }),
  });
  assert.match(
    settled.getByTestId("pulse-entries-empty").textContent,
    /No entries posted for this project yet/,
  );
  settled.unmount();

  const incomplete = await renderView({
    kind: "partial",
    digest: digest({
      complete: false,
      sessions: [session()],
      errors: [{ scope: "entries", message: "relay unavailable" }],
    }),
  });
  assert.match(
    incomplete.getByTestId("pulse-entries-empty").textContent,
    /not a project-wide answer/,
  );
  incomplete.unmount();

  const filtered = await renderView({
    kind: "ready",
    digest: digest({
      entries: [entry({ branch: "wip/project-pulse" })],
      sessions: [session({ branch: "main" })],
    }),
  });
  const mainChip = filtered
    .getAllByTestId("pulse-branch-chip")
    .find((chip) => chip.textContent.startsWith("main"));
  const { act } = await import("react");
  await act(async () => {
    mainChip.dispatchEvent(
      new dom.window.MouseEvent("click", { bubbles: true }),
    );
  });
  assert.match(
    filtered.getByTestId("pulse-entries-empty").textContent,
    /No entries on this branch/,
  );
  assert.equal(filtered.queryByTestId("pulse-write-hint"), null);
  filtered.unmount();
});

test("commit confirmation stays inline with the commit observation", async () => {
  const { getByTestId } = await renderView({
    kind: "ready",
    digest: digest({ sessions: [session()] }),
  });
  const confirmation = getByTestId("pulse-session-commit-confirmation");
  assert.equal(confirmation.tagName, "SPAN");
  assert.equal(
    confirmation.parentElement,
    getByTestId("pulse-session-commit").parentElement,
  );
  assert.match(confirmation.className, /\btext-xs\b/);
});

test("Closed is suppressed only when the status already says Ended", async () => {
  const ended = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [
        session({
          lifecycle: "closed",
          coordinationState: "closed",
          status: "stopped",
          statusAt: NOW - 1_800,
          observedAgeSeconds: 1_800,
        }),
      ],
    }),
  });
  assert.match(
    ended.getByTestId("pulse-session-status").textContent,
    /^Ended · last observed 30m ago$/,
  );
  assert.equal(ended.queryByTestId("pulse-session-closed"), null);
  ended.unmount();

  const disconnected = await renderView({
    kind: "ready",
    digest: digest({
      sessions: [
        session({
          lifecycle: "closed",
          coordinationState: "closed",
          status: "disconnected",
          statusAt: NOW - 10_800,
          observedAgeSeconds: 10_800,
        }),
      ],
    }),
  });
  assert.match(
    disconnected.getByTestId("pulse-session-status").textContent,
    /^Disconnected · last observed 3h ago$/,
  );
  assert.equal(
    disconnected.getByTestId("pulse-session-closed").textContent,
    "Closed",
  );
});
