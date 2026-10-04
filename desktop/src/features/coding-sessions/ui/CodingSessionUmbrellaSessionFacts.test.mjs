import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_BOUNDARY_TITLE,
  codingSessionBoundaryText,
} from "../lib/codingSessionBoundaryStatus.ts";
import {
  CODING_SESSION_CONTINUITY_STATUSES,
  CODING_SESSION_CONTINUITY_TITLE,
} from "../lib/codingSessionTranscriptItems.ts";
import { CodingSessionHeaderDetails } from "./CodingSessionHeaderDetails.tsx";
import { CodingSessionDetailsContinuityProvider } from "./CodingSessionHeaderDetailsContinuity.tsx";
import {
  CodingSessionUmbrellaClosedSandboxFooter,
  codingSessionExecutionSandbox,
  codingSessionUmbrellaContinuityRows,
  codingSessionUmbrellaGenerationFacts,
  codingSessionUmbrellaOtherSeatWarnings,
} from "./CodingSessionUmbrellaSessionFacts.tsx";

const RESUMED = CODING_SESSION_CONTINUITY_STATUSES.get("session_resumed");
const RESTARTED = CODING_SESSION_CONTINUITY_STATUSES.get(
  "session_restarted_without_context",
);

function continuity(id, text, timestamp) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: CODING_SESSION_CONTINUITY_TITLE,
    text,
    timestamp,
  };
}

function boundary(id, status, reason) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: CODING_SESSION_BOUNDARY_TITLE,
    text: codingSessionBoundaryText(status, reason),
  };
}

const SAFE = boundary("s", "execution_boundary_enforced", "macos-seatbelt");
const FULL = boundary("f", "execution_boundary_not_enforced", "full-access");

function record(transcript) {
  return { transcript };
}

function execution(executionKey, active, prior = []) {
  return {
    executionKey,
    signerPubkey: "a".repeat(64),
    activeGeneration: record(active),
    priorGenerations: prior.map(record),
    operatorPubkey: null,
  };
}

function participant(executionKey, label, active, prior = []) {
  return {
    kind: "execution",
    executionKey,
    label,
    execution: execution(executionKey, active, prior),
  };
}

test("the focused execution's continuity spans every generation, newest first", () => {
  const rows = codingSessionUmbrellaContinuityRows(
    execution(
      "A",
      [continuity("c2", RESTARTED, "2026-10-04T11:00:00.000Z")],
      [
        [
          { id: "m", type: "message", text: "hello" },
          continuity("c1", RESUMED, "2026-10-04T10:00:00.000Z"),
        ],
      ],
    ),
  );
  assert.deepEqual(
    rows.map((row) => [row.id, row.lost]),
    [
      ["c2", true],
      ["c1", false],
    ],
  );
});

test("the umbrella header's Details shows the focused execution's continuity (SV-16)", () => {
  const rows = codingSessionUmbrellaContinuityRows(
    execution("A", [continuity("c", RESTARTED, "2026-10-04T11:00:00.000Z")]),
  );
  const markup = renderToStaticMarkup(
    React.createElement(
      CodingSessionDetailsContinuityProvider,
      { value: rows },
      React.createElement(CodingSessionHeaderDetails, {
        channelName: null,
        compact: false,
        generationLabel: "gen 1",
        peopleCount: 0,
        projectName: null,
        providerAuthorityPubkey: null,
      }),
    ),
  );
  assert.match(markup, /data-testid="coding-session-details-continuity-dot"/);

  // The umbrella workspace wraps its header row in that provider, fed from
  // the focused execution.
  const source = readFileSync(
    fileURLToPath(
      new URL("./CodingSessionUmbrellaWorkspace.tsx", import.meta.url),
    ),
    "utf8",
  );
  assert.match(
    source,
    /useCodingSessionUmbrellaContinuity\(\s*focusedExecution\s*\)/,
  );
  assert.match(
    source,
    /<CodingSessionDetailsContinuityProvider value=\{focusedContinuity\}>\s*<CodingSessionUmbrellaHeaderRow/,
  );
});

test("a closed Mission names every other seat outside a boundary, not only the focused one", () => {
  const participants = [
    participant("A", "Claude", [SAFE]),
    participant("B", "Codex", [FULL]),
    { kind: "session", sessionRef: "s", label: "Session" },
  ];
  const others = codingSessionUmbrellaOtherSeatWarnings(participants, "A");
  assert.deepEqual(
    others.map((seat) => [
      seat.executionKey,
      seat.seatLabel,
      seat.warning.label,
    ]),
    [["B", "Codex", "Full access"]],
  );
  // The focused seat is the chip; it is not listed twice.
  assert.deepEqual(
    codingSessionUmbrellaOtherSeatWarnings(participants, "B"),
    [],
  );

  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaClosedSandboxFooter, {
      focusedExecution: participants[0].execution,
      participants,
    }),
  );
  assert.match(markup, /data-testid="coding-session-sandbox-footer"/);
  assert.match(
    markup,
    /data-testid="coding-session-closed-other-seats-sandbox"/,
  );
  assert.match(markup, /data-execution-key="B"/);
  assert.match(markup, /Codex/);
  assert.match(markup, /Full access/);
  assert.match(markup, /Session closed/);
  // The focused seat is named beside its chip so the two are not confused.
  assert.match(markup, /Claude/);
});

test("a closed Mission with every other seat sandboxed draws no seat list", () => {
  const participants = [
    participant("A", "Claude", [FULL]),
    participant("B", "Codex", [SAFE]),
  ];
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaClosedSandboxFooter, {
      focusedExecution: participants[0].execution,
      participants,
    }),
  );
  assert.doesNotMatch(markup, /coding-session-closed-other-seats-sandbox/);
  assert.match(markup, /Session closed/);
});

test("a seat that ran with full access in an earlier generation and resumed sandboxed is still flagged", () => {
  const SAFE_AGAIN = boundary(
    "s2",
    "execution_boundary_enforced",
    "macos-seatbelt",
  );
  const participants = [
    participant("A", "Claude", [SAFE_AGAIN], [[FULL]]),
    participant("B", "Codex", [SAFE_AGAIN], [[FULL]]),
  ];
  assert.deepEqual(
    codingSessionUmbrellaOtherSeatWarnings(participants, "A").map((seat) => [
      seat.executionKey,
      seat.warning.label,
    ]),
    [["B", "Was full access"]],
  );
  // The focused seat's own chip reads across generations too.
  const report = codingSessionExecutionSandbox(participants[0].execution);
  assert.equal(report.state, "sandboxed");
  assert.deepEqual(
    (report.earlier ?? []).map((period) => [period.id, period.state]),
    [["f", "full-access"]],
  );
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaClosedSandboxFooter, {
      focusedExecution: participants[0].execution,
      participants,
    }),
  );
  assert.match(markup, /Was full access/);
});

test("generation facts hold their reference across streamed items that are not facts", () => {
  const prior = [continuity("c1", RESUMED, "2026-10-04T10:00:00.000Z")];
  const first = codingSessionUmbrellaGenerationFacts(
    [prior, [{ id: "m1", type: "message", text: "a" }]],
    null,
  );
  const streamed = codingSessionUmbrellaGenerationFacts(
    [
      prior,
      [
        { id: "m1", type: "message", text: "a" },
        { id: "m2", type: "message", text: "b" },
      ],
    ],
    first,
  );
  assert.equal(streamed.facts, first.facts);
  // The unchanged prior generation is reused, not re-read.
  assert.equal(streamed.generations[0], first.generations[0]);

  const withFact = codingSessionUmbrellaGenerationFacts(
    [prior, [continuity("c2", RESTARTED, "2026-10-04T11:00:00.000Z")]],
    streamed,
  );
  assert.notEqual(withFact.facts, streamed.facts);
  assert.deepEqual(
    withFact.facts.map((item) => item.id),
    ["c1", "c2"],
  );
});
