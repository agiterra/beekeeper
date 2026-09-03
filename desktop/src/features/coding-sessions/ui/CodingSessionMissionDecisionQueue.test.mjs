import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionMissionDecisionQueue } from "./CodingSessionMissionDecisionQueue.tsx";
import { CodingSessionMissionStatePanel } from "./CodingSessionMissionStatePanel.tsx";

const NOW = 1_788_400_000_000;
const REQUEST = "2099cdb3".repeat(8);
const ACTOR_REQUEST = "215525d1".repeat(8);

function openRow(overrides = {}) {
  return {
    requestId: REQUEST,
    shortId: "2099cdb3",
    question: "Should the JSON also carry the seat role?",
    state: "open",
    stateWord: "Open · held on the founder",
    blocksWord: "holds up no assignment yet",
    blocks: [],
    askedAtMs: NOW - 12 * 60_000,
    // L8's row fields. The three refs are null here on purpose: a surface that
    // does not know which umbrella it is in cannot publish an answer, so the
    // control is absent and every L2 assertion below renders what it always
    // did.
    options: [],
    recommendation: null,
    heldOn: "founder",
    viewerIsHolder: null,
    heldElsewhereSentence: null,
    answerChoiceWord: null,
    answerCondition: null,
    channelRef: null,
    sessionRef: null,
    genesisRef: null,
    ...overrides,
  };
}

test("L2.2: an open founder-held row says the question, who holds it, and what it blocks", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [openRow()],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
    }),
  );
  assert.match(html, /Should the JSON also carry the seat role\?/);
  assert.match(html, /Open · held on the founder/);
  assert.match(html, /holds up no assignment yet/);
  assert.match(html, /asked 12m/);
  assert.match(html, /data-decision-state="open"/);
});

test("L2.2: an answered row is answered by a name and carries no waiting word", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [
        openRow({
          state: "answered",
          stateWord: "Answered by the founder",
          askedAtMs: null,
        }),
      ],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
    }),
  );
  assert.match(html, /Answered by the founder/);
  assert.doesNotMatch(html, /Open · held on/);
  // No stamp means no ` · asked …` at all — never `0m` (§1g).
  assert.doesNotMatch(html, /asked/);
});

test("L2.2: an empty fold is the §1g empty state, and an absent fold is unknown", () => {
  const empty = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
    }),
  );
  assert.match(empty, /No decisions on the wire/);
  const unknown = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [],
      decisionsKnown: false,
      decisionsTruncated: 0,
      nowMs: NOW,
    }),
  );
  assert.match(unknown, /Decisions unknown/);
  assert.doesNotMatch(unknown, /No decisions on the wire/);
});

test("L2.2: a truncated queue says how many rows it is not showing", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [openRow()],
      decisionsKnown: true,
      decisionsTruncated: 13,
      decisionsTruncatedNotice: "13 older answered decisions not shown",
      nowMs: NOW,
    }),
  );
  assert.match(html, /13 older answered decisions not shown/);
});

test("L2.2: an actor-held row names the assignments it holds up", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [
        openRow({
          requestId: ACTOR_REQUEST,
          shortId: "215525d1",
          stateWord: "Open · held on Keystone",
          blocksWord: "holds up 1 assignment: 3234382f",
          blocks: ["3234382f"],
        }),
      ],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
    }),
  );
  assert.match(html, /Open · held on Keystone/);
  assert.match(html, /holds up 1 assignment: 3234382f/);
});

test("L2.2: with no open lead turn the waiting fact is the state line", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: NOW,
      state: {
        kind: "running",
        sourceEventId: "a".repeat(64),
        phase: "assigned",
        detail: "Assigned.",
        canonicalChain: [],
      },
      waiting: {
        requestId: REQUEST,
        shortId: "2099cdb3",
        line: "Waiting on the founder",
        askedAtMs: NOW - 12 * 60_000,
        placement: "state-line",
      },
    }),
  );
  assert.match(html, /data-mission-waiting="state-line"/);
  assert.match(html, /Waiting on the founder · asked 12m/);
});

test("L2.2: with the lead working both facts are present and neither overwrites the other", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: NOW,
      state: {
        kind: "running",
        sourceEventId: "a".repeat(64),
        phase: "assigned",
        detail: "Assigned.",
        canonicalChain: [],
      },
      waiting: {
        requestId: REQUEST,
        shortId: "2099cdb3",
        line: "Waiting on the founder",
        askedAtMs: null,
        placement: "beside",
      },
    }),
  );
  assert.match(html, /Mission running/);
  assert.match(html, /Waiting on the founder/);
  assert.match(html, /data-mission-waiting="beside"/);
  assert.doesNotMatch(html, /asked/);
});

test("L2.2: no waiting fact leaves the state panel exactly as it was", () => {
  const state = {
    kind: "running",
    sourceEventId: "a".repeat(64),
    phase: "assigned",
    detail: "Assigned.",
    canonicalChain: [],
  };
  const before = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, { state }),
  );
  const withNull = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: NOW,
      state,
      waiting: null,
    }),
  );
  assert.equal(withNull, before);
  assert.doesNotMatch(before, /data-mission-waiting/);
});

test("F4: a completed mission keeps its word and gains the waiting clause", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: NOW,
      state: {
        kind: "completed",
        sourceEventId: "a".repeat(64),
        summary: "Done.",
        landedShas: [],
        followUps: [],
        canonicalChain: [],
      },
      waiting: {
        requestId: REQUEST,
        shortId: "2099cdb3",
        line: "Waiting on the founder",
        askedAtMs: null,
        placement: "appended",
      },
    }),
  );
  assert.match(html, /Mission completed · waiting on the founder/);
  assert.match(html, /data-mission-waiting="appended"/);
  // The terminal word is still the state, in the visible line and in the data.
  assert.match(html, /data-mission-state="completed"/);
});

// ── L8: the Answer control, on the screen that shows the question ────────────

const CHANNEL = "e0d3f1b8-8c66-4c62-9ef1-3fa933b32f86";
const SESSION = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS = "ab".repeat(32);

function answerableRow(overrides = {}) {
  return {
    ...openRow(),
    options: ["now", "after the rebuild", "neither"],
    recommendation: null,
    heldOn: "founder",
    viewerIsHolder: true,
    heldElsewhereSentence: null,
    answerChoiceWord: null,
    answerCondition: null,
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    ...overrides,
  };
}

/** A probe that never resolves: SSR runs no effects, so this is never called. */
const NO_PROBE = { capabilities: async () => new Promise(() => {}) };

test("L8.1: a founder-held row with three options renders three buttons and the Answer control", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow()],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  const options = html.match(/data-testid="decision-answer-option"/g) ?? [];
  assert.equal(options.length, 3);
  assert.match(html, /data-decision-option-index="0"/);
  assert.match(html, /data-decision-option-index="2"/);
  assert.match(html, /after the rebuild/);
  assert.match(html, /data-testid="decision-answer-submit"/);
  assert.ok(!/decision-answer-held-elsewhere/.test(html));
});

test("L8.1: a row held on somebody else disables every control and prints §1l's sentence", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [
        answerableRow({
          requestId: ACTOR_REQUEST,
          shortId: "215525d1",
          stateWord: "Open · held on Bob",
          viewerIsHolder: false,
          heldElsewhereSentence:
            "This ruling is held on Bob, so only they can answer it. You can read it here.",
        }),
      ],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  assert.match(
    html,
    /This ruling is held on Bob, so only they can answer it\. You can read it here\./,
  );
  // Disabled, never hidden: the control is present and every button is off.
  assert.match(html, /data-testid="decision-answer-form"/);
  const buttons = html.match(/<button[^>]*disabled=""/g) ?? [];
  assert.ok(
    buttons.length >= 4,
    `expected disabled controls, got ${buttons.length}`,
  );
});

test("L8.1: a request with no options offers free text alone and says so", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow({ options: [] })],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  assert.ok(!/decision-answer-option/.test(html));
  assert.match(html, /This request declared no options/);
  assert.match(html, /data-testid="decision-answer-choice"/);
});

test("L8.4: the condition field appears with its hint and counter only where the wire carries it", () => {
  const supported = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow()],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      supportsCondition: true,
      answerDeps: NO_PROBE,
    }),
  );
  assert.match(supported, /Condition \(optional\)/);
  assert.match(
    supported,
    /Name the class this ruling covers, so it does not have to be asked again for the next commit\./,
  );
  assert.match(supported, /0\/512 bytes/);

  const unsupported = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow()],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      supportsCondition: false,
      answerDeps: NO_PROBE,
    }),
  );
  assert.ok(!/Condition \(optional\)/.test(unsupported));
  assert.match(
    unsupported,
    /This build&#x27;s wire has no condition on an answer yet, so this ruling covers only the question it answers, and the same question will have to be asked again\./,
  );
});

test("L8.4: an answered row reads its choice and its condition, and discloses the clamp", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [
        answerableRow({
          state: "answered",
          stateWord: "Answered by the founder",
          answerChoiceWord: "after the rebuild",
          answerCondition: {
            text: "every commit on this branch…",
            truncated: 41,
          },
        }),
      ],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  assert.match(
    html,
    /Answered by the founder · after the rebuild · condition: every commit on this branch/,
  );
  assert.match(html, /41 more characters of this condition are not shown/);
  // An answered row carries no form: the ruling is on the wire.
  assert.ok(!/data-testid="decision-answer-form"/.test(html));
});

test("L8.1: nothing reads `Answered` until a fold carrying the answer arrives", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow()],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  assert.match(html, /data-decision-state="open"/);
  assert.ok(!/Answered by/.test(html));
});

// ── Fix round 2: the review's findings, each with its own assertion ──────────

test("F8: a published answer says so, names the event, and stops a second publish", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow()],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: {
        ...NO_PROBE,
        publish: async () => ({
          eventId: "ab".repeat(32),
          conditionSent: false,
        }),
      },
      publishedForTest: { [REQUEST]: "ab".repeat(32) },
    }),
  );
  assert.match(html, /Answer published · abababab · the fold will show it/);
  // The row is still open — the fold has not carried it yet — and every
  // control is off, so the same ruling cannot be signed twice.
  assert.match(html, /data-decision-state="open"/);
  assert.ok(!/Answered by/.test(html));
  const enabled =
    html.match(/<button(?![^>]*disabled)[^>]*data-testid="decision-answer/g) ??
    [];
  assert.equal(
    enabled.length,
    0,
    "no answer control stays live after a publish",
  );
});

test("F9: a row whose holder this surface cannot resolve is disabled, with §1l's sentence", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [
        answerableRow({
          viewerIsHolder: null,
          heldElsewhereSentence:
            "This ruling is held on the founder, so only they can answer it. You can read it here.",
        }),
      ],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  assert.match(
    html,
    /This ruling is held on the founder, so only they can answer it\./,
  );
  const enabled =
    html.match(/<button(?![^>]*disabled)[^>]*data-testid="decision-answer/g) ??
    [];
  assert.equal(enabled.length, 0, "unknown is never rendered as permitted");
});

test("F10: an open row with no session anchor says so instead of hiding the control", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow({ genesisRef: null })],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  assert.match(
    html,
    /This view does not know which session this ruling belongs to, so it cannot be answered from here\./,
  );
  assert.ok(!/data-testid="decision-answer-form"/.test(html));
});

test("F11: two identical declared options render two distinct buttons", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDecisionQueue, {
      decisions: [answerableRow({ options: ["now", "now"] })],
      decisionsKnown: true,
      decisionsTruncated: 0,
      nowMs: NOW,
      answerDeps: NO_PROBE,
    }),
  );
  assert.match(html, /data-decision-option-index="0"/);
  assert.match(html, /data-decision-option-index="1"/);
});
