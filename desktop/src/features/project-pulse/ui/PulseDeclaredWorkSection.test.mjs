import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

/**
 * Every model below is built literally, from the projection contract's §5
 * shape, so this file proves the surface and never the projection module.
 *
 * `fetch` is replaced with a spy that fails the test if it is ever called: the
 * section's contract is that it renders a model and reads nothing.
 */
let fetchCalls = 0;

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
    fetch: (...args) => {
      fetchCalls += 1;
      throw new Error(`declared-work section fetched: ${String(args[0])}`);
    },
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
  assert.equal(fetchCalls, 0, "the section must never fetch");
});

after(() => dom.window.close());

const NOW = 1_785_513_037;
const HUMAN = "a1".repeat(32);
const AGENT = "b2".repeat(32);
const ASSIGNER = "c3".repeat(32);
const SESSION_OPEN = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const SESSION_CLOSED = "8c1d2e3f-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const CHANNEL = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
/** The umbrella's immutable genesis event id, as the wire now carries it. */
const GENESIS = "aa".repeat(32);

const id = (suffix) => suffix.padStart(64, "0");

const NAMES = new Map([
  [HUMAN, "Brian"],
  [AGENT, "Ada"],
  [ASSIGNER, "Astra"],
]);

function planEntry(overrides = {}) {
  return {
    eventId: id("e1"),
    pubkey: HUMAN,
    createdAt: NOW - 900,
    type: "plan",
    text: "Splitting the pulse read into pages.",
    claimedAreas: ["desktop/src/features/project-pulse/lib"],
    branch: "work/declared-work",
    sessionRef: null,
    supersedes: null,
    supersededBy: [],
    active: true,
    ...overrides,
  };
}

function planRow(overrides = {}) {
  const entry = planEntry(overrides);
  return {
    kind: "plan",
    label: "Plan posted",
    entry,
    createdAt: entry.createdAt,
    dedupeKey: `plan:${entry.eventId}`,
  };
}

function assignment(overrides = {}) {
  return {
    sourceEventId: id("a1"),
    createdAt: NOW - 600,
    assignerPubkey: ASSIGNER,
    assigneeActor: AGENT,
    assigneeRole: "builder",
    objective: "Render the declared-work section.",
    brief: "Ship the surface with every test id the contract names.",
    branch: "work/declared-work-fable",
    baseSha: "6a59eb45e6a59eb45e6a59eb45e6a59eb45e6a59",
    fileOwnership: ["desktop/src/features/project-pulse/ui"],
    acceptanceSteps: ["Unit tests pass", "No fetch on render"],
    supersedes: null,
    reports: [],
    dispositions: [],
    settlement: {
      settled: false,
      awaiting: { link: "disposition", owedByRole: "lead", owedByActor: null },
      governedReportEventId: null,
      dispositionEventId: null,
      acknowledgementEventId: null,
    },
    status: "unresolved",
    ...overrides,
  };
}

function assignmentRow(overrides = {}) {
  const session = {
    sessionKey: SESSION_OPEN,
    sessionRef: SESSION_OPEN,
    channelId: CHANNEL,
    name: "Declared work",
    lifecycle: "open",
    genesisRef: GENESIS,
    founderPubkey: "d4".repeat(32),
    ...(overrides.session ?? {}),
  };
  const payload = assignment(overrides.assignment ?? {});
  return {
    kind: "assignment",
    label: "Assigned",
    session,
    assignment: payload,
    createdAt: payload.createdAt,
    evidence: overrides.evidence ?? [],
    responsible: overrides.responsible ?? {
      pubkey: payload.assigneeActor,
      role: payload.assigneeRole,
    },
    assignedBy: overrides.assignedBy ?? payload.assignerPubkey,
    dedupeKey: `${session.channelId}:${session.sessionRef}:${payload.sourceEventId}`,
  };
}

function model(overrides = {}) {
  const current = overrides.current ?? [];
  const settled = overrides.settled ?? [];
  return {
    current,
    settled,
    scan: {
      visibleSessions: 13,
      scannedSessions: 8,
      unreadableSessions: 0,
      morePages: true,
      capped: false,
      sentence:
        "Scanned 8 of 13 visible sessions, newest first; 5 older sessions not read yet.",
      ...(overrides.scan ?? {}),
    },
    limitations: overrides.limitations ?? [],
    // The projection's own two verdicts: did the read cover its stated scope,
    // and did it earn the words "No declared work".
    readIsComplete:
      overrides.readIsComplete ?? (overrides.limitations ?? []).length === 0,
    noDeclaredWork:
      overrides.noDeclaredWork ??
      (current.length === 0 &&
        settled.length === 0 &&
        (overrides.limitations ?? []).length === 0),
  };
}

async function renderSection(props = {}) {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { PulseDeclaredWorkSection } = await import(
    "@/features/project-pulse/ui/PulseDeclaredWorkSection"
  );
  const element = (overrides = {}) =>
    createElement(PulseDeclaredWorkSection, {
      authorNames: NAMES,
      entriesById: new Map(),
      hasNextPage: false,
      isFetchingNextPage: false,
      message: null,
      model: null,
      nowSeconds: NOW,
      onLoadMore: () => {},
      onOpenSession: () => {},
      onRecheck: () => {},
      refreshing: false,
      sessionOpenable: () => true,
      sessionsByRef: new Map(),
      state: "ready",
      ...props,
      ...overrides,
    });
  const screen = render(element());
  return {
    ...screen,
    rerenderSection: (overrides) => screen.rerender(element(overrides)),
  };
}

for (const empty of [false, true]) {
  test(`a failed refresh retains the previous ${empty ? "empty read" : "assignment"} and discloses stale results until recovery`, async () => {
    const previous = model({
      current: empty ? [] : [assignmentRow()],
      scan: {
        visibleSessions: 1,
        scannedSessions: 1,
        morePages: false,
        sentence: "Scanned the 1 visible session.",
      },
    });
    const screen = await renderSection({ model: previous });
    const row = screen.queryByTestId("pulse-declared-row");
    assert.equal(screen.queryByTestId("pulse-declared-update-failed"), null);
    if (empty) {
      assert.equal(
        screen.getByTestId("pulse-declared-empty").textContent,
        "No declared work in this project's visible sessions.",
      );
    } else {
      assert.match(row.textContent, /Render the declared-work section/);
    }

    screen.rerenderSection({ refreshing: true });
    assert.ok(screen.getByTestId("pulse-declared-refreshing"));
    screen.rerenderSection({
      refreshing: false,
      message: "Relay connection closed during refresh",
    });
    const failure = screen.getByTestId("pulse-declared-update-failed");
    assert.match(failure.textContent, /could not be updated/);
    assert.match(failure.textContent, /previously read results/);
    assert.match(failure.textContent, /may be out of date/);
    assert.match(failure.textContent, /Relay connection closed during refresh/);
    assert.equal(screen.queryByTestId("pulse-declared-refreshing"), null);
    assert.match(
      screen.getByTestId("pulse-declared-scan").textContent,
      /^Previous read: Scanned the 1 visible session/,
    );
    if (empty) {
      const sentence = screen.getByTestId("pulse-declared-empty").textContent;
      assert.match(sentence, /current work is unknown/);
      assert.doesNotMatch(sentence, /^No declared work in/);
    } else {
      assert.equal(screen.getByTestId("pulse-declared-row"), row);
      assert.equal(screen.queryByTestId("pulse-declared-empty"), null);
    }

    screen.rerenderSection({ message: null, refreshing: false });
    assert.equal(screen.queryByTestId("pulse-declared-update-failed"), null);
    assert.equal(
      screen.getByTestId("pulse-declared-scan").textContent,
      "Scanned the 1 visible session.",
    );
    if (empty) {
      assert.equal(
        screen.getByTestId("pulse-declared-empty").textContent,
        "No declared work in this project's visible sessions.",
      );
    }
  });
}

test("a loading read says it is reading, never that there is no work", async () => {
  const screen = await renderSection({ state: "loading", model: null });
  assert.equal(
    screen.getByTestId("pulse-declared-work").dataset.state,
    "loading",
  );
  assert.match(
    screen.getByTestId("pulse-declared-loading").textContent,
    /Nothing below is an answer yet/,
  );
  assert.equal(screen.queryByTestId("pulse-declared-empty"), null);
  assert.doesNotMatch(screen.container.textContent, /No declared work/);
});

test("an unreadable read is disclosed as a failure, never as no work", async () => {
  const screen = await renderSection({
    state: "unreadable",
    model: null,
    message: "pulse declared work: response missing key `sessions`",
  });
  const failure = screen.getByTestId("pulse-declared-unreadable");
  assert.match(failure.textContent, /could not be read/);
  assert.match(failure.textContent, /missing key `sessions`/);
  assert.doesNotMatch(screen.container.textContent, /No declared work/);
});

test("a stale read keeps its rows and shows what it could not read", async () => {
  const screen = await renderSection({
    state: "unreadable",
    message: "page 2: relay read failed",
    model: model({
      current: [assignmentRow()],
      limitations: [
        "Page 2 could not be read: relay read failed.",
        "1 session's records were unreadable.",
      ],
      scan: { unreadableSessions: 1 },
    }),
  });
  assert.equal(screen.getAllByTestId("pulse-declared-row").length, 1);
  assert.equal(screen.getAllByTestId("pulse-declared-limitation").length, 2);
  assert.ok(screen.getByTestId("pulse-declared-limitations"));
  assert.doesNotMatch(screen.container.textContent, /No declared work/);
});

test("two owners in different sessions render distinct responsible lines", async () => {
  const screen = await renderSection({
    model: model({
      current: [
        planRow(),
        assignmentRow({
          session: {
            sessionKey: SESSION_CLOSED,
            sessionRef: SESSION_CLOSED,
            name: "Agent lane",
          },
        }),
      ],
    }),
  });
  const rows = screen.getAllByTestId("pulse-declared-row");
  assert.equal(rows.length, 2);
  assert.deepEqual(
    rows.map((row) => row.dataset.kind),
    ["plan", "assignment"],
  );
  // The plan's owner is its author, rendered by the unchanged entry row.
  assert.match(screen.getByTestId("pulse-entry-row").textContent, /Brian/);
  // The assignment's owner is the assigned actor; the assigner stays visible.
  const responsible = screen.getByTestId("pulse-declared-responsible");
  assert.match(responsible.textContent, /Assigned to Ada as builder/);
  assert.match(
    screen.getByTestId("pulse-declared-assigner").textContent,
    /by Astra/,
  );
  assert.equal(
    screen.getByTestId("pulse-declared-assigner").getAttribute("title"),
    ASSIGNER,
  );
  const labels = screen
    .getAllByTestId("pulse-declared-label")
    .map((node) => node.textContent);
  assert.deepEqual(labels, ["Plan posted", "Assigned"]);
});

test("every test id the contract names is on the screen", async () => {
  const screen = await renderSection({
    hasNextPage: true,
    model: model({
      current: [
        assignmentRow({
          evidence: [
            {
              label: "Report submitted",
              detail: "Ada reported 4m ago.",
              eventId: id("f1"),
            },
          ],
        }),
      ],
      settled: [
        assignmentRow({
          assignment: {
            sourceEventId: id("a2"),
            status: "settled",
            settlement: {
              settled: true,
              awaiting: null,
              governedReportEventId: id("f1"),
              dispositionEventId: id("f2"),
              acknowledgementEventId: id("f3"),
            },
          },
        }),
      ],
      limitations: ["Read stopped at the 4-page cap."],
    }),
  });
  for (const testId of [
    "pulse-declared-work",
    "pulse-declared-scan",
    "pulse-declared-row",
    "pulse-declared-label",
    "pulse-declared-responsible",
    "pulse-declared-scope",
    "pulse-declared-branch",
    "pulse-declared-evidence",
    "pulse-declared-details",
    "pulse-declared-open-session",
    "pulse-declared-settled",
    "pulse-declared-more",
    "pulse-declared-limitations",
    "pulse-declared-recheck",
  ]) {
    assert.ok(
      screen.getAllByTestId(testId).length > 0,
      `${testId} is missing from the section`,
    );
  }
  assert.match(
    screen.getByTestId("pulse-declared-scan").textContent,
    /Scanned 8 of 13 visible sessions/,
  );
  const evidence = screen.getAllByTestId("pulse-declared-evidence")[0];
  assert.equal(evidence.dataset.evidenceLabel, "Report submitted");
  assert.match(evidence.textContent, /Report submitted Ada reported 4m ago\./);
  assert.equal(evidence.querySelector("span").textContent, "Report submitted");
});

test("declared paths render verbatim, and their absence is said in words", async () => {
  const withPaths = await renderSection({
    model: model({ current: [assignmentRow()] }),
  });
  const paths = withPaths
    .getAllByTestId("pulse-declared-path")
    .map((node) => node.textContent);
  assert.deepEqual(paths, ["desktop/src/features/project-pulse/ui"]);
  assert.match(
    withPaths.getByTestId("pulse-declared-scope").textContent,
    /Declared paths/,
  );
  const { cleanup } = await import("@testing-library/react");
  cleanup();

  const without = await renderSection({
    model: model({
      current: [
        assignmentRow({ assignment: { fileOwnership: [], branch: null } }),
      ],
    }),
  });
  assert.match(
    without.getByTestId("pulse-declared-scope").textContent,
    /No declared paths/,
  );
  assert.equal(
    without.getByTestId("pulse-declared-branch").textContent,
    "Branch not reported",
  );
});

test("a reported branch renders with its base, shortened but inspectable", async () => {
  const screen = await renderSection({
    model: model({ current: [assignmentRow()] }),
  });
  const branch = screen.getByTestId("pulse-declared-branch").textContent;
  assert.match(branch, /Branch work\/declared-work-fable/);
  // One sha convention with the model's evidence lines: the first eight.
  assert.match(branch, /base 6a59eb45\b/);
  assert.doesNotMatch(branch, /…/);
});

test("an unresolved assignment in a closed session stays current and says so", async () => {
  const screen = await renderSection({
    model: model({
      current: [
        assignmentRow({
          session: {
            sessionKey: SESSION_CLOSED,
            sessionRef: SESSION_CLOSED,
            name: "Ended lane",
            lifecycle: "closed",
          },
          evidence: [
            {
              label: "Session closed",
              detail: "Closing the session settled nothing.",
              eventId: null,
            },
          ],
        }),
      ],
    }),
  });
  const row = screen.getByTestId("pulse-declared-row");
  assert.equal(row.dataset.status, "unresolved");
  assert.match(
    screen.getByTestId("pulse-declared-session").textContent,
    /Session closed/,
  );
  assert.equal(
    screen.getByTestId("pulse-declared-evidence").dataset.evidenceLabel,
    "Session closed",
  );
  assert.equal(
    screen.getByTestId("pulse-declared-status").textContent,
    "Unresolved",
  );
  assert.equal(screen.queryByTestId("pulse-declared-settled"), null);
});

test("settled assignments live in a collapsed group titled with their count", async () => {
  const screen = await renderSection({
    model: model({
      current: [assignmentRow()],
      settled: [
        assignmentRow({
          assignment: { sourceEventId: id("a2"), status: "settled" },
        }),
        assignmentRow({
          assignment: { sourceEventId: id("a3"), status: "settled" },
        }),
      ],
    }),
  });
  const group = screen.getByTestId("pulse-declared-settled");
  assert.equal(group.tagName, "DETAILS");
  assert.equal(group.open, false);
  assert.equal(group.querySelector("summary").textContent, "Settled (2)");
  assert.equal(
    group.querySelectorAll('[data-testid="pulse-declared-row"]').length,
    2,
  );
  const statuses = group.querySelectorAll(
    '[data-testid="pulse-declared-status"]',
  ).length;
  assert.equal(statuses, 2);
});

test("the details block is collapsed and carries the brief, steps and ids", async () => {
  const screen = await renderSection({
    model: model({ current: [assignmentRow()] }),
  });
  const details = screen.getByTestId("pulse-declared-details");
  assert.equal(details.open, false);
  assert.ok(details.querySelector("summary"));
  assert.match(
    screen.getByTestId("pulse-declared-brief").textContent,
    /Ship the surface/,
  );
  assert.deepEqual(
    screen
      .getAllByTestId("pulse-declared-acceptance-step")
      .map((node) => node.textContent),
    ["Unit tests pass", "No fetch on render"],
  );
  assert.match(
    screen.getByTestId("pulse-declared-source-id").textContent,
    new RegExp(id("a1")),
  );
  const ids = screen.getByTestId("pulse-declared-session-ids").textContent;
  assert.match(ids, new RegExp(SESSION_OPEN));
  assert.match(ids, new RegExp(CHANNEL));
  // The genesis id is rendered in full, inside the collapsed details block.
  const genesis = screen.getByTestId("pulse-declared-genesis");
  assert.equal(genesis.textContent, `Genesis ${GENESIS}`);
  assert.ok(
    details.contains(genesis),
    "the genesis id belongs behind the disclosure, with the other ids",
  );
});

test("a session with no recorded execution offers no control, and says why", async () => {
  const screen = await renderSection({
    sessionOpenable: () => false,
    model: model({ current: [assignmentRow()] }),
  });
  assert.equal(screen.queryByTestId("pulse-declared-open-session"), null);
  assert.equal(
    screen.getByTestId("pulse-declared-open-session-missing").textContent,
    "No execution recorded to open",
  );
});

test("Open session hands the session key back and nothing else happens", async () => {
  const opened = [];
  const screen = await renderSection({
    onOpenSession: (key) => opened.push(key),
    sessionOpenable: (key) => key === SESSION_OPEN,
    model: model({ current: [assignmentRow()] }),
  });
  const { fireEvent } = await import("@testing-library/react");
  fireEvent.click(screen.getByTestId("pulse-declared-open-session"));
  assert.deepEqual(opened, [SESSION_OPEN]);
});

test("load more is offered only when a page exists, and blocks while reading", async () => {
  let more = 0;
  const none = await renderSection({
    hasNextPage: false,
    onLoadMore: () => {
      more += 1;
    },
    model: model({ current: [assignmentRow()] }),
  });
  assert.equal(none.queryByTestId("pulse-declared-more"), null);
  const { cleanup, fireEvent } = await import("@testing-library/react");
  cleanup();

  const screen = await renderSection({
    hasNextPage: true,
    onLoadMore: () => {
      more += 1;
    },
    model: model({ current: [assignmentRow()] }),
  });
  const button = screen.getByTestId("pulse-declared-more");
  assert.equal(button.textContent, "Show older sessions");
  fireEvent.click(button);
  assert.equal(more, 1);
  cleanup();

  const fetching = await renderSection({
    hasNextPage: true,
    isFetchingNextPage: true,
    onLoadMore: () => {
      more += 1;
    },
    model: model({ current: [assignmentRow()] }),
  });
  const busy = fetching.getByTestId("pulse-declared-more");
  assert.equal(busy.disabled, true);
  assert.equal(busy.textContent, "Reading older sessions…");
  fireEvent.click(busy);
  assert.equal(more, 1);
});

test("Check again re-reads and starts nothing else", async () => {
  let rechecks = 0;
  let more = 0;
  let opened = 0;
  const screen = await renderSection({
    hasNextPage: true,
    onLoadMore: () => {
      more += 1;
    },
    onOpenSession: () => {
      opened += 1;
    },
    onRecheck: () => {
      rechecks += 1;
    },
    model: model({ current: [assignmentRow()] }),
  });
  const { fireEvent } = await import("@testing-library/react");
  const recheck = screen.getByTestId("pulse-declared-recheck");
  assert.equal(recheck.textContent, "Check again");
  fireEvent.click(recheck);
  assert.equal(rechecks, 1);
  assert.equal(more, 0);
  assert.equal(opened, 0);
});

test("a complete empty read may say so; an incomplete one may not", async () => {
  const complete = await renderSection({ model: model({}) });
  assert.match(
    complete.getByTestId("pulse-declared-empty").textContent,
    /No declared work in this project's visible sessions\./,
  );
  const { cleanup } = await import("@testing-library/react");
  cleanup();

  const capped = await renderSection({
    model: model({
      readIsComplete: false,
      limitations: ["Read stopped at the 4-page cap."],
    }),
  });
  const sentence = capped.getByTestId("pulse-declared-empty").textContent;
  assert.doesNotMatch(sentence, /^No declared work in/);
  assert.match(sentence, /not a project-wide answer/);
});

test("a complete read whose only work is settled is never called incomplete", async () => {
  // The regression: "the read is incomplete" printed directly above a
  // "Settled (1)" group that was showing everything this read found.
  const screen = await renderSection({
    model: model({
      readIsComplete: true,
      settled: [
        assignmentRow({
          assignment: { sourceEventId: id("a7"), status: "settled" },
        }),
      ],
    }),
  });
  const sentence = screen.getByTestId("pulse-declared-empty").textContent;
  assert.equal(sentence, "No unresolved declared work; 1 settled below.");
  assert.doesNotMatch(sentence, /incomplete/);
  assert.equal(
    screen.getByTestId("pulse-declared-settled").querySelector("summary")
      .textContent,
    "Settled (1)",
  );
});

test("an incomplete read with settled work still refuses the settled sentence", async () => {
  const screen = await renderSection({
    model: model({
      readIsComplete: false,
      limitations: ["Page 2 could not be read: relay read failed."],
      settled: [
        assignmentRow({
          assignment: { sourceEventId: id("a8"), status: "settled" },
        }),
      ],
    }),
  });
  const sentence = screen.getByTestId("pulse-declared-empty").textContent;
  assert.match(sentence, /the read is incomplete/);
  assert.doesNotMatch(sentence, /No unresolved declared work/);
});

test("the section is labelled, and every control carries its own words", async () => {
  const screen = await renderSection({
    hasNextPage: true,
    refreshing: true,
    model: model({
      current: [assignmentRow()],
      settled: [
        assignmentRow({
          assignment: { sourceEventId: id("a9"), status: "settled" },
        }),
      ],
    }),
  });
  const section = screen.getByTestId("pulse-declared-work");
  assert.equal(section.tagName, "SECTION");
  const labelledBy = section.getAttribute("aria-labelledby");
  assert.ok(labelledBy);
  assert.equal(
    dom.window.document.getElementById(labelledBy).textContent,
    "Declared work",
  );
  for (const button of section.querySelectorAll("button")) {
    assert.ok(
      (button.textContent ?? "").trim().length > 0,
      "a control with no words is not addressable",
    );
  }
  for (const details of section.querySelectorAll("details")) {
    assert.ok(details.querySelector("summary"), "a disclosure needs a summary");
  }
  assert.match(
    screen.getByTestId("pulse-declared-refreshing").textContent,
    /last complete read/,
  );
});

test("the founder is shortened and the genesis id is not", async () => {
  const screen = await renderSection({
    model: model({ current: [assignmentRow()] }),
  });
  assert.match(
    screen.getByTestId("pulse-declared-session-ids").textContent,
    /founder d4d4d4d4…d4d4/,
  );
  assert.equal(
    screen.getByTestId("pulse-declared-genesis").textContent,
    `Genesis ${GENESIS}`,
  );
});

test("a regrouped plan keeps the refused cross-author claim against it", async () => {
  const plan = planEntry();
  const claimant = planEntry({
    eventId: id("e9"),
    pubkey: AGENT,
    text: "Picking this back up.",
  });
  const screen = await renderSection({
    claimedBy: new Map([[plan.eventId, [claimant]]]),
    model: model({ current: [planRow()] }),
  });
  // The same disclosure the Entries list would have shown: a plan that moved
  // into this section must not quietly lose the claim made against it.
  assert.match(
    screen.getByTestId("pulse-entry-supersession-claimed").textContent,
    /says this is resolved/,
  );
});
