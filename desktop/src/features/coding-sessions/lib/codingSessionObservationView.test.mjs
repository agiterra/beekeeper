import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import {
  CODING_SESSION_GATE_SUMMARY_LINE_CEILING,
  CODING_SESSION_OBSERVATION_EMPTY,
  CODING_SESSION_OBSERVATION_SECTION_LIMIT,
  deriveCodingSessionObservationView,
  describeDuration,
} from "./codingSessionObservationView.ts";
import { decodeCodingSessionObservationFold } from "./codingSessionObservationWire.ts";

const FIXTURE = JSON.parse(
  readFileSync(
    fileURLToPath(
      new URL(
        "./codingSessionObservationFoldAdapterResponse.fixture.json",
        import.meta.url,
      ),
    ),
    "utf8",
  ),
);

const SEAT = "11".repeat(32);
const OTHER = "22".repeat(32);
const ASSIGNMENT = "cd".repeat(32);
const DECISION = "9a".repeat(32);

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
    disclosure: "an observation is something its author saw",
    ...overrides,
  };
}

function checkpoint(authorPubkey, phase, eventId) {
  return {
    eventId,
    authorPubkey,
    source: "declared",
    assignmentRef: null,
    phase,
    testsWritten: 3,
    testsRed: 3,
    testsGreen: 1,
    lastCommand: null,
    lastSummary: null,
    note: null,
  };
}

function gate(authorPubkey, source, name, outcome, extra = {}) {
  return {
    authorPubkey,
    source,
    eventIds: [`${name.length}`.padStart(2, "0").repeat(32)],
    droppedEventIds: 0,
    assignmentRef: null,
    gate: name,
    outcome,
    command: `run ${name}`,
    summary: null,
    durationMs: null,
    ...extra,
  };
}

function view(input) {
  return deriveCodingSessionObservationView({
    resolveLabel: (pubkey) => (pubkey === SEAT ? "Bob · Builder" : null),
    ...input,
  });
}

test("a null fold is not an empty one", () => {
  const empty = view({ fold: null });
  assert.deepEqual(empty.seats, []);
  assert.deepEqual(empty.gates, []);
  assert.equal(empty.disclosure, "");
});

test("blocks group by author seat, and a seat with no assignment appears", () => {
  const projected = view({
    fold: fold({
      checkpoints: [
        checkpoint(SEAT, "green", "01".repeat(32)),
        checkpoint(OTHER, "red", "02".repeat(32)),
      ],
    }),
  });
  assert.equal(projected.seats.length, 2);
  assert.equal(projected.seats[0].label, "Bob · Builder");
  // No resolver answer, and no assignment anywhere: the block still exists and
  // is named by the first eight hex of the key that signed it.
  // The canonical `truncatePubkey` form, not a hand-rolled prefix: a truncated
  // prefix is forgeable, which is what the repo's own guard is about.
  assert.equal(
    projected.seats[1].label,
    `${OTHER.slice(0, 8)}…${OTHER.slice(-4)}`,
  );
});

test("phases sort in the declared order, never by arrival", () => {
  const projected = view({
    fold: fold({
      checkpoints: [
        checkpoint(SEAT, "reporting", "01".repeat(32)),
        checkpoint(SEAT, "planning", "02".repeat(32)),
        checkpoint(SEAT, "gates", "03".repeat(32)),
      ],
    }),
  });
  assert.deepEqual(
    projected.seats[0].checkpoints.map((row) => row.phase),
    ["planning", "gates", "reporting"],
  );
});

test("a phase timing whose word is not declared sorts last, and keeps its word", () => {
  const projected = view({
    fold: fold({
      phases: [
        {
          eventId: "01".repeat(32),
          authorPubkey: SEAT,
          source: "declared",
          assignmentRef: null,
          phase: "rebase",
          startedAtMs: 1,
          endedAtMs: 2,
          durationMs: 1_000,
        },
        {
          eventId: "02".repeat(32),
          authorPubkey: SEAT,
          source: "declared",
          assignmentRef: null,
          phase: "red",
          startedAtMs: 1,
          endedAtMs: null,
          durationMs: null,
        },
      ],
    }),
  });
  assert.deepEqual(
    projected.seats[0].phases.map((row) => row.phase),
    ["red", "rebase"],
  );
  // Unknown ≠ zero: a phase that published no duration says so.
  assert.equal(projected.seats[0].phases[0].duration, null);
  assert.equal(projected.seats[0].phases[0].running, true);
  assert.equal(projected.seats[0].phases[1].duration, "1s");
});

test("observed sorts before declared, and neither replaces the other", () => {
  const projected = view({
    fold: fold({
      gates: [
        gate(SEAT, "declared", "cargo test", "passed"),
        gate(OTHER, "observed", "cargo test", "failed"),
      ],
    }),
  });
  assert.equal(projected.gates.length, 2);
  assert.deepEqual(
    projected.gates.map((row) => row.source),
    ["observed", "declared"],
  );
  assert.deepEqual(
    projected.gates.map((row) => row.outcome),
    ["failed", "passed"],
  );
});

test("a block of only observed rows says it is a watcher's, not a seat's", () => {
  const projected = view({
    fold: fold({ gates: [gate(OTHER, "observed", "cargo test", "failed")] }),
  });
  assert.equal(projected.seats[0].watcherOnly, true);

  const mixed = view({
    fold: fold({
      gates: [
        gate(OTHER, "observed", "cargo test", "failed"),
        gate(OTHER, "declared", "just ci", "passed"),
      ],
    }),
  });
  assert.equal(mixed.seats[0].watcherOnly, false);
});

test("a failed row keeps its command verbatim and its tail in lines", () => {
  const projected = view({
    fold: fold({
      gates: [
        gate(SEAT, "declared", "cargo test", "failed", {
          command: "cargo test -p buzz-cli -- --nocapture subcommand_",
          summary:
            "running 2 tests\n\nfailures:\n  subcommand_names_are_stable",
          durationMs: 41_000,
        }),
      ],
    }),
  });
  const row = projected.gates[0];
  assert.equal(
    row.command,
    "cargo test -p buzz-cli -- --nocapture subcommand_",
  );
  assert.deepEqual(row.summaryLines, [
    "running 2 tests",
    "failures:",
    "  subcommand_names_are_stable",
  ]);
  assert.equal(row.duration, "41s");
});

test("a null durationMs renders as absent, never as 0s", () => {
  const projected = view({
    fold: fold({ gates: [gate(SEAT, "declared", "just ci", "passed")] }),
  });
  assert.equal(projected.gates[0].duration, null);
  // A *measured* zero is a different fact and does print.
  assert.equal(describeDuration(0), "0s");
  assert.equal(describeDuration(null), null);
  assert.equal(describeDuration(90_000), "1m 30s");
  assert.equal(describeDuration(3_600_000), "1h");
});

test("a dangling assignmentRef is disclosed and excludes nothing", () => {
  const projected = view({
    fold: fold({
      checkpoints: [
        {
          ...checkpoint(SEAT, "green", "01".repeat(32)),
          assignmentRef: ASSIGNMENT,
        },
      ],
      unresolved: [{ eventId: "01".repeat(32), assignmentRef: ASSIGNMENT }],
    }),
  });
  assert.equal(projected.seats[0].checkpoints.length, 1);
  assert.equal(projected.seats[0].checkpoints[0].assignmentUnresolved, true);
  assert.equal(projected.unresolved.length, 1);
  assert.equal(
    projected.unresolved[0].shortAssignmentRef,
    ASSIGNMENT.slice(0, 8),
  );
});

test("a finding's decisionRef resolves through this surface's fold, and otherwise not", () => {
  const finding = (decisionRef) => ({
    authorPubkey: SEAT,
    source: "declared",
    eventIds: ["03".repeat(32)],
    droppedEventIds: 0,
    assignmentRef: null,
    findingId: "F1",
    title: "the card described the wire instead of reading it",
    disposition: "needs-ruling",
    detail: null,
    refs: [ASSIGNMENT],
    decisionRef,
  });

  const resolved = view({
    fold: fold({ findings: [finding(DECISION)] }),
    knownDecisionRefs: [DECISION],
  });
  assert.equal(resolved.seats[0].findings[0].decisionUnresolved, false);
  assert.equal(
    resolved.seats[0].findings[0].decisionShortRef,
    DECISION.slice(0, 8),
  );
  assert.equal(resolved.seats[0].findings[0].refCount, 1);

  const dangling = view({ fold: fold({ findings: [finding(DECISION)] }) });
  assert.equal(dangling.seats[0].findings[0].decisionUnresolved, true);
});

test("every bound bites in words, and both layers are distinct facts", () => {
  const many = Array.from(
    { length: CODING_SESSION_OBSERVATION_SECTION_LIMIT + 3 },
    (_unused, index) =>
      checkpoint(SEAT, "red", index.toString(16).padStart(64, "0")),
  );
  const projected = view({
    fold: fold({
      checkpoints: many,
      truncated: {
        checkpoints: 12,
        gates: 0,
        findings: 0,
        phases: 0,
        unresolved: 0,
        ignored: 0,
        misclaimedObserved: 0,
        entryEventIds: 4,
        displacedGates: 0,
        displacedFindings: 0,
      },
    }),
  });
  assert.equal(
    projected.seats[0].checkpoints.length,
    CODING_SESSION_OBSERVATION_SECTION_LIMIT,
  );
  assert.equal(projected.seats[0].hidden.checkpoints, 3);
  const notices = projected.truncations.map((entry) => entry.notice);
  assert.ok(
    notices.some((notice) =>
      /12 more checkpoints are on the wire/.test(notice),
    ),
    notices.join(" | "),
  );
  assert.ok(
    notices.some((notice) => /4 older statements are on the wire/.test(notice)),
    notices.join(" | "),
  );
  assert.ok(
    notices.some((notice) => /Bob · Builder: 3 more checkpoints/.test(notice)),
    notices.join(" | "),
  );
});

test("a tail longer than the ceiling is bounded and says so", () => {
  const lines = Array.from(
    { length: CODING_SESSION_GATE_SUMMARY_LINE_CEILING + 5 },
    (_unused, index) => `line ${index}`,
  ).join("\n");
  const projected = view({
    fold: fold({
      gates: [gate(SEAT, "declared", "just ci", "failed", { summary: lines })],
    }),
  });
  assert.equal(
    projected.gates[0].summaryLines.length,
    CODING_SESSION_GATE_SUMMARY_LINE_CEILING,
  );
  assert.equal(projected.gates[0].hiddenSummaryLines, 5);
});

test("the empty copy is the frozen wording each section owes its reader", () => {
  assert.deepEqual(CODING_SESSION_OBSERVATION_EMPTY, {
    checkpoints: "No checkpoint yet",
    gates: "No gate row yet",
    findings: "No finding recorded",
    phases: "No phase timing",
  });
});

test("the adapter's own fixture projects into two blocks, one of them a watcher's", () => {
  const projected = view({
    fold: decodeCodingSessionObservationFold(structuredClone(FIXTURE)),
  });
  assert.equal(projected.seats.length, 2);
  const watcher = projected.seats.find((seat) => seat.watcherOnly);
  assert.ok(watcher, "the provider's observed row is its own block");
  assert.equal(watcher.gates.length, 1);
  assert.equal(watcher.gates[0].outcome, "failed");
});

test("REVIEW-L5 F1: a displaced statement is a notice, never a silence", () => {
  const projected = view({
    fold: fold({
      gates: [gate(SEAT, "observed", "cargo test", "passed")],
      truncated: {
        checkpoints: 0,
        gates: 0,
        findings: 0,
        phases: 0,
        unresolved: 0,
        ignored: 0,
        misclaimedObserved: 0,
        entryEventIds: 0,
        displacedGates: 1,
        displacedFindings: 2,
      },
    }),
  });
  const notices = projected.truncations.map((entry) => entry.notice);
  assert.ok(
    notices.some((notice) =>
      /1 earlier gate statement was replaced by a later one/.test(notice),
    ),
    notices.join(" | "),
  );
  assert.ok(
    notices.some((notice) =>
      /2 earlier findings were replaced by a later disposition/.test(notice),
    ),
    notices.join(" | "),
  );
});

test("REVIEW-L5 F2: a misclaimed observed row is disclosed, not repeated", () => {
  const projected = view({
    fold: fold({
      gates: [gate(SEAT, "declared", "cargo test", "passed")],
      misclaimedObserved: [{ eventId: "ab".repeat(32), authorPubkey: SEAT }],
    }),
  });
  assert.equal(projected.misclaimedObserved.length, 1);
  assert.equal(projected.misclaimedObserved[0].shortEventId, "abababab");
  assert.equal(
    projected.misclaimedObserved[0].shortAuthor,
    `${SEAT.slice(0, 8)}\u2026${SEAT.slice(-4)}`,
  );
  assert.equal(projected.provenanceChecked, true);
});

test("REVIEW-L5 F2: an unchecked fold says so rather than implying it verified", () => {
  const projected = view({
    fold: fold({
      gates: [gate(SEAT, "observed", "cargo test", "passed")],
      provenanceChecked: false,
    }),
  });
  assert.equal(projected.provenanceChecked, false);
  assert.deepEqual(projected.misclaimedObserved, []);
});

test("a null fold has not checked provenance either", () => {
  assert.equal(view({ fold: null }).provenanceChecked, false);
});
