import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionUmbrellaTimelineView } from "./CodingSessionUmbrellaTimelineView.tsx";

const FOUNDER = "f".repeat(64);
const BUILDER = "b".repeat(64);

const UMBRELLA = {
  umbrellaKey: "session-1",
  sessionRef: "session-1",
  title: "TeamTest",
  executions: [],
  founderPubkey: FOUNDER,
  genesisRef: "g".repeat(64),
  genesisResolution: "governed",
  status: "idle",
  lastEventAt: "2026-09-01T00:00:00.000Z",
  conflictCount: 0,
  foreignAttachmentCount: 0,
};

const LANE = [
  {
    eventId: "lane-1",
    sessionRef: "session-1",
    channelId: "channel-1",
    authorPubkey: FOUNDER,
    content: "Fix the dispatch handling and verify the tests.",
    timestampMs: 1_800_000_000_000,
  },
];

const TRANSACTIONS = [
  {
    sourceEventId: "a".repeat(64),
    type: "assignment",
    authorPubkey: FOUNDER,
    createdAt: 1_800_000_060,
    counterpartyPubkey: BUILDER,
    parentEventId: null,
    summary: "Extract the dispatch arm into a pure function",
    decision: null,
    requiredAction: null,
    fileCount: null,
    testCount: null,
    unseated: false,
  },
  {
    sourceEventId: "d".repeat(64),
    type: "report",
    authorPubkey: BUILDER,
    createdAt: 1_800_000_120,
    counterpartyPubkey: FOUNDER,
    parentEventId: "a".repeat(64),
    summary: "Dispatch extraction",
    decision: null,
    requiredAction: null,
    fileCount: 3,
    testCount: 3,
    unseated: true,
  },
];

function render(props) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaTimelineView, {
      channelId: "channel-1",
      currentUserPubkey: FOUNDER,
      laneMessages: LANE,
      onHandoff() {},
      umbrella: UMBRELLA,
      ...props,
    }),
  );
}

test("U-T4: Conversation renders no transaction rows even when they are passed", () => {
  const bare = render({});
  const withTransactions = render({
    missionTransactions: TRANSACTIONS,
    missionFounderPubkey: FOUNDER,
    resolveMissionActor: () => ({ label: "Bob", executionKey: "builder" }),
  });
  assert.equal(
    withTransactions,
    bare,
    "Conversation's DOM must not move when Mission data is present",
  );
  assert.doesNotMatch(bare, /coding-session-mission-transaction-row/);
  assert.match(bare, /class="rounded-xl bg-muted\/40 px-4 py-2"/);
});

test("U-T4: Mission renders the transaction rows in chronological order", () => {
  const markup = render({
    missionDensity: "live",
    missionFounderPubkey: FOUNDER,
    missionTransactions: TRANSACTIONS,
    resolveMissionActor: (pubkey) =>
      pubkey === FOUNDER
        ? { label: "Keystone", executionKey: "lead" }
        : { label: "Bob", executionKey: "builder" },
  });
  const rows = markup.match(/data-transaction-type="[a-z.]+"/g) ?? [];
  assert.deepEqual(rows, [
    'data-transaction-type="assignment"',
    'data-transaction-type="report"',
  ]);
  assert.ok(
    markup.indexOf("Fix the dispatch handling") <
      markup.indexOf("Keystone → Bob · Assignment"),
    "the founder's lane message precedes the assignment it caused",
  );
  assert.match(markup, /Bob → Keystone · Report/);
  assert.match(markup, /coding-session-unseated-badge/);
});

test("U-T4: Mission wears the shared card grammar on the conversation row", () => {
  const markup = render({
    missionDensity: "live",
    missionFounderPubkey: FOUNDER,
  });
  const row = markup.match(
    /<div class="([^"]*)" data-testid="coding-session-umbrella-conversation"/,
  );
  assert.ok(row, "the conversation row keeps its test id in Mission");
  assert.match(row[1], /border-border\/60/);
  assert.doesNotMatch(row[1], /bg-muted\/40/);
});
