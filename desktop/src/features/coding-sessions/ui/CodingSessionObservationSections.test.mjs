import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import { deriveCodingSessionObservationView } from "../lib/codingSessionObservationView.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const SEAT = "11".repeat(32);
const PROVIDER = "22".repeat(32);

function fold(overrides = {}) {
  return {
    schema: "buzz-coding-session-observation-fold-adapter/v1",
    implementation: "buzz-core",
    inputEventIds: [],
    sessionRef: "dc580cfb-6c80-4fc2-8f4e-dfc328acf222",
    genesisRef: "ce".repeat(32),
    checkpoints: [],
    gates: [],
    findings: [],
    phases: [],
    unresolved: [],
    ignored: [],
    misclaimedObserved: [],
    provenanceChecked: true,
    truncated: {
      checkpoints: 0,
      gates: 0,
      findings: 0,
      phases: 0,
      unresolved: 0,
      ignored: 0,
      misclaimedObserved: 0,
      entryEventIds: 0,
      displacedGates: 0,
      displacedFindings: 0,
    },
    disclosure:
      "an observation is something its author saw, not a decision: it settles nothing",
    ...overrides,
  };
}

function gate(authorPubkey, source, name, outcome, extra = {}) {
  return {
    authorPubkey,
    source,
    eventIds: ["ab".repeat(32)],
    droppedEventIds: 0,
    assignmentRef: null,
    gate: name,
    outcome,
    command: `cargo test -p ${name}`,
    summary: null,
    durationMs: null,
    ...extra,
  };
}

function view(foldValue) {
  return deriveCodingSessionObservationView({
    fold: foldValue,
    resolveLabel: (pubkey) =>
      pubkey === SEAT ? "Bob · Builder" : pubkey === PROVIDER ? null : null,
  });
}

async function render(props) {
  const React = (await import("react")).default;
  const { cleanup, render: renderReact } = await import(
    "@testing-library/react"
  );
  const { CodingSessionObservationSections } = await import(
    "./CodingSessionObservationSections.tsx"
  );
  return {
    cleanup,
    ...renderReact(
      React.createElement(CodingSessionObservationSections, props),
    ),
  };
}

test("two seats and three phases group into two blocks in declared phase order", async () => {
  const checkpoint = (authorPubkey, phase, eventId) => ({
    eventId,
    authorPubkey,
    source: "declared",
    assignmentRef: null,
    phase,
    testsWritten: 4,
    testsRed: 4,
    testsGreen: 2,
    lastCommand: null,
    lastSummary: null,
    note: null,
  });
  const screen = await render({
    view: view(
      fold({
        checkpoints: [
          checkpoint(SEAT, "gates", "01".repeat(32)),
          checkpoint(SEAT, "planning", "02".repeat(32)),
          checkpoint(PROVIDER, "red", "03".repeat(32)),
        ],
      }),
    ),
  });
  try {
    const blocks = screen.getAllByTestId("coding-session-observation-seat");
    assert.equal(blocks.length, 2);
    assert.equal(blocks[0].getAttribute("data-author"), SEAT);
    const phases = [
      ...blocks[0].querySelectorAll(
        "[data-testid='coding-session-checkpoint-row']",
      ),
    ].map((row) => row.getAttribute("data-phase"));
    // §1e's declared order, not the order they arrived in.
    assert.deepEqual(phases, ["planning", "gates"]);
  } finally {
    screen.cleanup();
  }
});

test("a failed row renders the command verbatim and its tail", async () => {
  const screen = await render({
    view: view(
      fold({
        gates: [
          gate(SEAT, "declared", "buzz-cli", "failed", {
            command: "cargo test -p buzz-cli -- subcommand_",
            summary:
              "running 2 tests\ntest result: FAILED. 0 passed; 2 failed; 0 ignored",
            durationMs: 41_000,
          }),
        ],
      }),
    ),
  });
  try {
    const row = screen.getByTestId("coding-session-gate-row");
    assert.equal(row.getAttribute("data-outcome"), "failed");
    assert.match(row.textContent, /cargo test -p buzz-cli -- subcommand_/);
    assert.match(
      screen.getByTestId("coding-session-gate-row-summary").textContent,
      /test result: FAILED\. 0 passed; 2 failed/,
    );
    assert.match(row.textContent, /41s · the author's own measurement/);
    // The word, not only the tint (§8 I9).
    assert.match(
      screen.getByTestId("coding-session-gate-outcome").textContent,
      /failed/,
    );
  } finally {
    screen.cleanup();
  }
});

test("a null durationMs reads as not reported, never as 0s", async () => {
  const screen = await render({
    view: view(
      fold({ gates: [gate(SEAT, "declared", "buzz-core", "passed")] }),
    ),
  });
  try {
    const row = screen.getByTestId("coding-session-gate-row");
    assert.match(row.textContent, /not reported/);
    assert.doesNotMatch(row.textContent, /0s/);
  } finally {
    screen.cleanup();
  }
});

test("an observed row and a declared one are two rows that name their source", async () => {
  const screen = await render({
    view: view(
      fold({
        gates: [
          gate(SEAT, "declared", "buzz-cli", "passed"),
          gate(PROVIDER, "observed", "buzz-cli", "failed"),
        ],
      }),
    ),
  });
  try {
    const rows = screen.getAllByTestId("coding-session-gate-row");
    assert.equal(rows.length, 2);
    const sources = rows.map((row) => row.getAttribute("data-source"));
    assert.ok(sources.includes("observed"));
    assert.ok(sources.includes("declared"));
    // The watcher's block says what it is: a 44246 names the key that signed
    // it and has no field for the seat it watched.
    assert.match(
      screen.container.textContent,
      /The record names the watcher, not the seat whose work it watched\./,
    );
  } finally {
    screen.cleanup();
  }
});

test("each empty section states its own emptiness rather than a blank", async () => {
  const screen = await render({
    view: view(
      fold({ gates: [gate(SEAT, "declared", "buzz-core", "passed")] }),
    ),
  });
  try {
    const text = screen.container.textContent;
    assert.match(text, /No checkpoint yet/);
    assert.match(text, /No finding recorded/);
    assert.match(text, /No phase timing/);
  } finally {
    screen.cleanup();
  }
});

test("a dangling assignmentRef is disclosed and excludes nothing", async () => {
  const screen = await render({
    view: view(
      fold({
        gates: [gate(SEAT, "declared", "buzz-core", "passed")],
        checkpoints: [
          {
            eventId: "01".repeat(32),
            authorPubkey: SEAT,
            source: "declared",
            assignmentRef: "cd".repeat(32),
            phase: "green",
            testsWritten: 1,
            testsRed: 1,
            testsGreen: 1,
            lastCommand: null,
            lastSummary: null,
            note: null,
          },
        ],
        unresolved: [
          { eventId: "01".repeat(32), assignmentRef: "cd".repeat(32) },
        ],
      }),
    ),
  });
  try {
    assert.equal(
      screen.getAllByTestId("coding-session-checkpoint-row").length,
      1,
      "the observation is still rendered",
    );
    assert.match(
      screen.getByTestId("coding-session-observations-unresolved").textContent,
      /names assignment cdcdcdcd, which is not in this session's records/,
    );
    assert.match(screen.container.textContent, /Disclosed, never excluded/);
  } finally {
    screen.cleanup();
  }
});

test("a seat with observations but no assignment still appears", async () => {
  const screen = await render({
    view: view(
      fold({ gates: [gate(SEAT, "declared", "buzz-core", "passed")] }),
    ),
  });
  try {
    assert.equal(
      screen.getAllByTestId("coding-session-observation-seat").length,
      1,
    );
    assert.match(screen.container.textContent, /Bob · Builder/);
  } finally {
    screen.cleanup();
  }
});

test("an errored read is unknown, and says so instead of showing nothing", async () => {
  const screen = await render({
    view: view(null),
    errorMessage: "The relay refused the query.",
  });
  try {
    assert.match(
      screen.getByTestId("coding-session-observations-error").textContent,
      /Nothing here is empty — this read failed/,
    );
  } finally {
    screen.cleanup();
  }
});

test("a fold that holds nothing says which nothing it means", async () => {
  const screen = await render({ view: view(fold()) });
  try {
    assert.match(
      screen.container.textContent,
      /No observation has been published for this session\./,
    );
    assert.match(
      screen.container.textContent,
      /Kind 44246 carries checkpoints, gate rows, findings and phase timing\./,
    );
  } finally {
    screen.cleanup();
  }
});

test("an ignored event is surfaced with its reason, never dropped", async () => {
  const screen = await render({
    view: view(
      fold({
        ignored: [
          {
            eventId: "ff".repeat(32),
            reason: "invalid observation signature: bad",
          },
        ],
      }),
    ),
  });
  try {
    assert.match(
      screen.getByTestId("coding-session-observations-ignored").textContent,
      /ffffffff — invalid observation signature: bad/,
    );
  } finally {
    screen.cleanup();
  }
});

test("a phase timing list is labelled reported by, with no bar", async () => {
  const screen = await render({
    view: view(
      fold({
        phases: [
          {
            eventId: "01".repeat(32),
            authorPubkey: SEAT,
            source: "declared",
            assignmentRef: null,
            phase: "green",
            startedAtMs: 1,
            endedAtMs: 2,
            durationMs: 180_000,
          },
        ],
      }),
    ),
  });
  try {
    const phaseRow = screen.getByTestId("coding-session-phase-row");
    assert.match(phaseRow.textContent, /green.*3m/s);
    // REVIEW-L5 F7: every row says how it was produced, phase timings too.
    assert.match(phaseRow.textContent, /declared/);
    assert.match(screen.container.textContent, /reported by Bob · Builder/);
    assert.equal(screen.container.querySelector("progress"), null);
  } finally {
    screen.cleanup();
  }
});

test("REVIEW-L5 F2: a misclaimed observed row is named on the screen", async () => {
  const screen = await render({
    view: view(
      fold({
        gates: [gate(SEAT, "declared", "buzz-core", "passed")],
        misclaimedObserved: [{ eventId: "ab".repeat(32), authorPubkey: SEAT }],
      }),
    ),
  });
  try {
    assert.match(
      screen.getByTestId("coding-session-observations-misclaimed").textContent,
      /which is not a provider instance for this session\. Shown as declared\./,
    );
  } finally {
    screen.cleanup();
  }
});

test("REVIEW-L5 F2: a view that verified nothing says so", async () => {
  const screen = await render({
    view: view(
      fold({
        gates: [gate(SEAT, "observed", "buzz-core", "passed")],
        provenanceChecked: false,
      }),
    ),
  });
  try {
    assert.match(
      screen.getByTestId("coding-session-observations-provenance-unchecked")
        .textContent,
      /Provenance was not verified in this view/,
    );
  } finally {
    screen.cleanup();
  }
});
