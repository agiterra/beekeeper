import assert from "node:assert/strict";
import test from "node:test";

import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionMissionTransactionCard } from "./CodingSessionMissionTransactionCard.tsx";

const assignmentId = "a".repeat(64);
const reportId = "b".repeat(64);
const dispositionId = "c".repeat(64);

test("acknowledgement-required stays visible with canonical signed chronology", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionMissionTransactionCard, {
      state: {
        kind: "acknowledgement-required",
        sourceEventId: dispositionId,
        assignmentRef: assignmentId,
        requiredAction:
          "The assigned builder seat must acknowledge this approved disposition.",
        heldOn: "builder",
        canonicalChain: [
          {
            type: "assignment",
            sourceEventId: assignmentId,
            authorPubkey: "d".repeat(64),
            createdAt: 1,
            summary: "Implement the Mission projection.",
          },
          {
            type: "report",
            sourceEventId: reportId,
            authorPubkey: "e".repeat(64),
            createdAt: 2,
            summary: "Projection implemented.",
          },
          {
            type: "disposition",
            sourceEventId: dispositionId,
            authorPubkey: "f".repeat(64),
            createdAt: 3,
            summary: "Approved.",
          },
        ],
      },
    }),
  );

  assert.match(markup, /Acknowledgement required/);
  assert.match(
    markup,
    /Required action: The assigned builder seat must acknowledge/,
  );
  assert.match(markup, /Canonical team transaction chronology/);
  assert.match(markup, /Signed team handoff flow/);
  assert.match(markup, /lucide-arrow-right/);
  assert.ok(
    markup.indexOf('capitalize">assignment</span>') <
      markup.indexOf('capitalize">report</span>'),
  );
  assert.ok(
    markup.indexOf('capitalize">report</span>') <
      markup.indexOf('capitalize">disposition</span>'),
  );
  for (const eventId of [assignmentId, reportId, dispositionId]) {
    assert.match(markup, new RegExp(`Signed source ${eventId}`));
  }
});

test("completed Brief preserves disposition, acknowledgement, and required action", () => {
  const acknowledgementId = "d".repeat(64);
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionMissionTransactionCard, {
      state: {
        kind: "completed",
        sourceEventId: "e".repeat(64),
        summary: "The governed mission is complete.",
        landedShas: ["f".repeat(40)],
        followUps: [],
        canonicalChain: [
          {
            type: "assignment",
            sourceEventId: assignmentId,
            authorPubkey: "1".repeat(64),
            createdAt: 1,
            summary: "Implement the Mission projection.",
          },
          {
            type: "report",
            sourceEventId: reportId,
            authorPubkey: "2".repeat(64),
            createdAt: 2,
            summary: "Projection implemented.",
          },
          {
            type: "disposition",
            sourceEventId: dispositionId,
            authorPubkey: "3".repeat(64),
            createdAt: 3,
            summary: "Approved with one follow-up.",
            decision: "approve-with-notes",
            requiredAction: "Publish the signed follow-up note.",
          },
          {
            type: "acknowledgement",
            sourceEventId: acknowledgementId,
            authorPubkey: "4".repeat(64),
            createdAt: 4,
            summary: "Approval received.",
          },
        ],
      },
    }),
  );

  assert.match(markup, /Mission completed/);
  assert.match(markup, /Decision: approve-with-notes/);
  assert.match(markup, /Required action: Publish the signed follow-up note/);
  assert.match(markup, /Approval received/);
  for (const eventId of [
    assignmentId,
    reportId,
    dispositionId,
    acknowledgementId,
  ]) {
    assert.match(markup, new RegExp(`Signed source ${eventId}`));
  }
});
