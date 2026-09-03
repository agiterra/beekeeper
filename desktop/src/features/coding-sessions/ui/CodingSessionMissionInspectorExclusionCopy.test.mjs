/**
 * §1k's exclusion copy, rendered.
 *
 * A separate file from `CodingSessionMissionInspector.test.mjs` on purpose:
 * two other lanes are editing that suite in parallel, and a new file is the
 * one change that cannot collide with them.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import { deriveCodingSessionMissionInspectorModel } from "../lib/codingSessionMissionInspectorModel.ts";

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

const COMPLETION = "cd".repeat(32);
const ASSIGNMENT = "11".repeat(32);
const REPORT = "22".repeat(32);

/** The fold's own reason, verbatim from `buzz-core` (§1k). */
const REASON =
  "mission.completed requires a verifier's ruling while the policy sets " +
  `gates.verifierRequired: assignment ${ASSIGNMENT} settled on report ` +
  `${REPORT}, and no active verifier seat has ruled on that report`;

function model(rejectedReasons) {
  return deriveCodingSessionMissionInspectorModel({
    goal: { kind: "absent" },
    acceptedPlan: { kind: "absent" },
    seatPlans: [],
    reports: [],
    observedChanges: { files: [], unreportedEditCount: 0 },
    participants: [],
    contextLoads: new Map(),
    missionState: { kind: "unknown", detail: null },
    usage: null,
    rejectedEventCount: rejectedReasons.length,
    rejectionsTruncated: false,
    rejectedReasons,
    conflicts: [],
  });
}

async function renderInspector(props) {
  const React = (await import("react")).default;
  const { cleanup, render } = await import("@testing-library/react");
  const { CodingSessionMissionInspector } = await import(
    "./CodingSessionMissionInspector.tsx"
  );
  return {
    cleanup,
    ...render(React.createElement(CodingSessionMissionInspector, props)),
  };
}

test("a completion the fold refused for want of a verifier reads as words", async () => {
  const view = await renderInspector({
    model: model([
      {
        code: "completion_not_verified",
        summary: REASON,
        eventIds: [COMPLETION],
      },
    ]),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const row = view.getByTestId("exclusion-copy-completion_not_verified");
    assert.equal(
      row.textContent,
      "Completion not verified · the policy requires a verifier's ruling",
    );
    // The raw wire token is not what the founder reads...
    assert.equal(view.queryByText("completion_not_verified"), null);
    // ...but the fold's own reason, with the ids, is still the row's evidence.
    assert.ok(row.closest("p")?.textContent?.includes(REASON));
  } finally {
    view.cleanup();
  }
});

test("a code nobody wrote copy for still shows its wire token", async () => {
  const view = await renderInspector({
    model: model([
      {
        code: "dangling_reference",
        summary: "reference is absent from the supplied transaction set",
        eventIds: [COMPLETION],
      },
    ]),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    assert.ok(view.getByText("dangling_reference"));
    assert.equal(view.queryByTestId("exclusion-copy-dangling_reference"), null);
  } finally {
    view.cleanup();
  }
});

test("a mission whose completion the fold refused does not read as running", async () => {
  // REVIEW-L7 F5: §1k froze this line and nothing rendered it, so a mission
  // the fold had refused to complete still said `Mission running`.
  const view = await renderInspector({
    model: model([
      {
        code: "completion_not_verified",
        summary: REASON,
        eventIds: [COMPLETION],
      },
    ]),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const panel = view.getByTestId("mission-state-summary");
    assert.match(panel.textContent, /Completion refused · no verifier ruling/);
    assert.doesNotMatch(panel.textContent, /Mission running/);
  } finally {
    view.cleanup();
  }
});

test("a mission with no such exclusion keeps its own state word", async () => {
  const view = await renderInspector({
    model: model([
      {
        code: "dangling_reference",
        summary: "reference is absent from the supplied transaction set",
        eventIds: [COMPLETION],
      },
    ]),
    variant: "panel",
    focusedExecutionKey: null,
  });
  try {
    const panel = view.getByTestId("mission-state-summary");
    assert.doesNotMatch(panel.textContent, /Completion refused/);
    assert.match(panel.textContent, /Mission state unknown/);
  } finally {
    view.cleanup();
  }
});
