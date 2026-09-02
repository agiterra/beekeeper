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
