import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_BOUNDARY_TITLE,
  codingSessionBoundaryText,
} from "../lib/codingSessionBoundaryStatus.ts";
import {
  groupCodingSessionCatalog,
  listCodingSessionUmbrellaParticipants,
} from "../lib/codingSessionUmbrellaModel.ts";
import { codingSessionSeatSandboxWarning } from "./CodingSessionComposerSeatSandbox.tsx";
import { CodingSessionUmbrellaComposer } from "./CodingSessionUmbrellaComposer.tsx";
import {
  CodingSessionUmbrellaClosedSandboxFooter,
  codingSessionUmbrellaOtherSeatWarnings,
} from "./CodingSessionUmbrellaSessionFacts.tsx";
import {
  codingSessionExecutionSandbox,
  codingSessionExecutionSandboxes,
} from "./codingSessionUmbrellaExecutionSandbox.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CLAUDE_SIGNER = "a".repeat(64);
const CODEX_SIGNER = "b".repeat(64);
const FOUNDER = "f".repeat(64);

function boundary(id, status, reason) {
  return {
    id,
    type: "lifecycle",
    renderClass: "status",
    title: CODING_SESSION_BOUNDARY_TITLE,
    text: codingSessionBoundaryText(status, reason),
  };
}

const full = (id) =>
  boundary(id, "execution_boundary_not_enforced", "full-access");
const safe = (id) =>
  boundary(id, "execution_boundary_enforced", "macos-seatbelt");

function target(driver, instanceId, sessionId, generation) {
  return { driver, instanceId, sessionId, generation };
}

function record({ signer, runtime, commandTarget, transcript, lastEventAt }) {
  return {
    generationId: `gen-${signer.slice(0, 4)}-${commandTarget.generation}`,
    label: `${commandTarget.driver} · generation ${commandTarget.generation}`,
    title: "Resumed mission",
    providerAuthorityPubkey: signer,
    metadataAuthorityPubkey: signer,
    lastEventAt,
    status: "completed",
    transcript,
    conflictCount: 0,
    commandTarget,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: null,
    runtime,
    model: null,
    capabilities: null,
  };
}

/**
 * Two seats, each with full access in generation 1 and resumed sandboxed in
 * generation 2: the running generation alone reads "Sandboxed".
 */
function resumedUmbrella() {
  const claude = (generation, transcript, lastEventAt) =>
    record({
      signer: CLAUDE_SIGNER,
      runtime: "claude",
      commandTarget: target(
        "claude-agent-acp",
        "claude-instance",
        "11111111-1111-1111-1111-111111111111",
        generation,
      ),
      transcript,
      lastEventAt,
    });
  const codex = (generation, transcript, lastEventAt) =>
    record({
      signer: CODEX_SIGNER,
      runtime: "codex",
      commandTarget: target(
        "codex-acp",
        "codex-instance",
        "22222222-2222-2222-2222-222222222222",
        generation,
      ),
      transcript,
      lastEventAt,
    });
  const umbrellas = groupCodingSessionCatalog([
    claude(1, [full("claude-f")], "2026-10-04T10:00:00.000Z"),
    claude(2, [safe("claude-s")], "2026-10-04T12:00:00.000Z"),
    codex(1, [full("codex-f")], "2026-10-04T10:00:00.000Z"),
    codex(2, [safe("codex-s")], "2026-10-04T11:00:00.000Z"),
  ]);
  assert.equal(umbrellas.length, 1);
  return umbrellas[0];
}

test("a seat full-access in generation 1 and resumed sandboxed in generation 2 tells the same history open and closed", () => {
  const umbrella = resumedUmbrella();
  const participants = listCodingSessionUmbrellaParticipants(umbrella);
  const executions = participants.filter((p) => p.kind === "execution");
  assert.equal(executions.length, 2);
  for (const seat of executions) {
    assert.equal(seat.execution.priorGenerations.length, 1);
    // The running generation alone would read plain "Sandboxed".
    assert.equal(seat.execution.activeGeneration.transcript.length, 1);
  }

  // The open reading (cached, every generation) and the closed reading
  // (uncached, every generation) are one report per seat.
  const open = codingSessionExecutionSandboxes(participants, null).reports;
  for (const seat of executions) {
    const report = open.get(seat.executionKey);
    assert.deepEqual(report, codingSessionExecutionSandbox(seat.execution));
    assert.equal(report.state, "sandboxed");
    assert.equal(
      codingSessionSeatSandboxWarning(report)?.label,
      "Was full access",
    );
  }

  const [focused, other] = executions;
  const openMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaComposer, {
      channelId: "channel-1",
      channelAccess: { kind: "member" },
      currentUserPubkey: FOUNDER,
      umbrella,
    }),
  );
  const closedMarkup = renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaClosedSandboxFooter, {
      focusedExecution: focused.execution,
      participants,
    }),
  );
  // The open composer addresses the most recently active seat first.
  assert.match(openMarkup, new RegExp(`Send to ${focused.label}`));
  // Focused chip: the same label open and closed.
  assert.match(openMarkup, /Sandboxed · was full access/);
  assert.match(closedMarkup, /Sandboxed · was full access/);
  // The other seat: counted on the open trigger, named in the closed footer.
  assert.match(openMarkup, /1 other seat is not sandboxed/);
  assert.deepEqual(
    codingSessionUmbrellaOtherSeatWarnings(
      participants,
      focused.executionKey,
    ).map((seat) => [seat.executionKey, seat.warning.label]),
    [[other.executionKey, "Was full access"]],
  );
  assert.match(closedMarkup, /Was full access/);
});

/** A transcript that counts every element read from it. */
function counted(items) {
  const reads = { count: 0 };
  const proxy = new Proxy(items, {
    get(target, property, receiver) {
      if (typeof property === "string" && /^\d+$/.test(property)) {
        reads.count += 1;
      }
      return Reflect.get(target, property, receiver);
    },
  });
  return { proxy, reads };
}

function seat(executionKey, prior, active) {
  return {
    kind: "execution",
    executionKey,
    label: executionKey,
    execution: {
      executionKey,
      signerPubkey: CLAUDE_SIGNER,
      activeGeneration: { transcript: active },
      priorGenerations: prior.map((transcript) => ({ transcript })),
      operatorPubkey: null,
    },
  };
}

test("a burst of streamed items re-reads no unchanged generation and keeps every report", () => {
  const priorA = counted([full("a-f"), { id: "a-m", type: "message" }]);
  const priorB = counted([full("b-f")]);
  const activeB = counted([safe("b-s")]);
  let activeA = [safe("a-s")];
  let result = codingSessionExecutionSandboxes(
    [
      seat("A", [priorA.proxy], activeA),
      seat("B", [priorB.proxy], activeB.proxy),
    ],
    null,
  );
  const firstReports = result.reports;
  const firstReportA = firstReports.get("A");
  assert.equal(
    codingSessionSeatSandboxWarning(firstReportA)?.label,
    "Was full access",
  );
  priorA.reads.count = 0;
  priorB.reads.count = 0;
  activeB.reads.count = 0;

  for (let index = 0; index < 50; index += 1) {
    // Each streamed item is a new running transcript and new participant
    // objects, as the catalog produces them.
    activeA = [...activeA, { id: `m${index}`, type: "message", text: "x" }];
    result = codingSessionExecutionSandboxes(
      [
        seat("A", [priorA.proxy], activeA),
        seat("B", [priorB.proxy], activeB.proxy),
      ],
      result,
    );
  }
  assert.equal(priorA.reads.count, 0, "seat A's prior generation re-read");
  assert.equal(priorB.reads.count, 0, "seat B's prior generation re-read");
  assert.equal(activeB.reads.count, 0, "seat B's unchanged running re-read");
  // No fact arrived, so no report and not the map changed.
  assert.equal(result.reports, firstReports);
  assert.equal(result.reports.get("A"), firstReportA);

  // A boundary row is a fact: the report follows it.
  activeA = [...activeA, full("a-f2")];
  result = codingSessionExecutionSandboxes(
    [
      seat("A", [priorA.proxy], activeA),
      seat("B", [priorB.proxy], activeB.proxy),
    ],
    result,
  );
  assert.notEqual(result.reports, firstReports);
  assert.equal(result.reports.get("A").state, "full-access");
  assert.equal(result.reports.get("B"), firstReports.get("B"));
  assert.equal(priorA.reads.count, 0);
});

test("a seat that leaves drops out of the reports", () => {
  const first = codingSessionExecutionSandboxes(
    [seat("A", [], [full("a")]), seat("B", [], [full("b")])],
    null,
  );
  const next = codingSessionExecutionSandboxes(
    [seat("A", [], first.cache.get("A").generations.generations[0].transcript)],
    first,
  );
  assert.deepEqual([...next.reports.keys()], ["A"]);
});
