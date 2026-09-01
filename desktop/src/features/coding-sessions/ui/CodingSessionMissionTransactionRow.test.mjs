import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { buildCodingSessionMissionTransactionRows } from "../lib/codingSessionMissionTransactionRows.ts";
import { CodingSessionMissionTransactionRow } from "./CodingSessionMissionTransactionRow.tsx";

const FOUNDER = "f".repeat(64);
const BUILDER = "b".repeat(64);

function rowFor(overrides, extra = {}) {
  return buildCodingSessionMissionTransactionRows({
    transactions: [
      {
        sourceEventId: "c".repeat(64),
        type: "report",
        authorPubkey: BUILDER,
        createdAt: 1_800_000_000,
        counterpartyPubkey: FOUNDER,
        parentEventId: null,
        summary: "Dispatch extraction",
        decision: null,
        requiredAction: null,
        fileCount: 3,
        testCount: 3,
        unseated: false,
        ...overrides,
      },
    ],
    resolveActor: (pubkey) =>
      pubkey === FOUNDER
        ? { label: "Keystone", executionKey: "lead" }
        : { label: "Bob", executionKey: "builder" },
    founderPubkey: FOUNDER,
    density: "live",
    ...extra,
  })[0];
}

function render(row) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionMissionTransactionRow, { row }),
  );
}

test("U-T1: a transaction row carries the title, the sentence and its weight", () => {
  const markup = render(rowFor({}));
  assert.match(markup, /data-testid="coding-session-mission-transaction-row"/);
  assert.match(markup, /data-transaction-type="report"/);
  assert.match(markup, /data-weight="standard"/);
  assert.match(markup, /aria-label="Bob to Keystone: report"/);
  assert.match(markup, /Bob → Keystone · Report/);
  assert.match(markup, /Dispatch extraction/);
  assert.match(markup, /3 files · 3 tests/);
});

test("U-T1: the arrow and the monograms are hidden from assistive technology", () => {
  const markup = render(rowFor({}));
  const hidden = markup.search(/<span aria-hidden=[^>]*>/);
  const content = markup.indexOf("Bob → Keystone · Report");
  assert.ok(hidden > 0, "the monogram pair opens an aria-hidden group");
  assert.ok(content > hidden, "the accessible title follows the glyphs");
  const decorative = markup.slice(hidden, content);
  assert.match(decorative, /lucide-arrow-right/);
  assert.match(decorative, />B</);
  assert.match(decorative, />K</);
  // The sentence, not the glyphs, is what assistive technology reads.
  assert.match(markup, /aria-label="Bob to Keystone: report"/);
});

test("U-T1: a refutation and a blocked terminal render at attention weight", () => {
  assert.match(
    render(rowFor({ type: "refutation" })),
    /data-weight="attention"/,
  );
  const blocked = render(
    rowFor({
      type: "mission.blocked",
      counterpartyPubkey: null,
      summary: "Mission cannot proceed.",
      fileCount: null,
      testCount: null,
    }),
  );
  assert.match(blocked, /data-weight="attention"/);
  assert.match(blocked, /border-destructive\/40/);
  assert.match(blocked, /Mission blocked · Bob/);
});

test("U-T1: a failed delivery turns its own report row into an attention row", () => {
  const markup = render(
    rowFor(
      {},
      {
        deliveries: [
          {
            sourceEventId: "c".repeat(64),
            operationType: "report",
            sourceActorPubkey: BUILDER,
            leadTargetKey: "lead-target",
            kind: "failed",
            owningCommandId: "cmd-1",
            duplicateRefusedCommandIds: [],
            failures: [],
            reArmCount: 1,
            observedAtMs: 5,
            detail:
              "Wake delivery failed — the lead dropped it and no Desktop was present to cover",
          },
        ],
      },
    ),
  );
  assert.match(markup, /data-weight="attention"/);
  assert.match(markup, /data-testid="coding-session-delivery-badge"/);
  assert.match(markup, /data-kind="failed"/);
  assert.match(markup, />failed</);
  assert.match(
    markup,
    /title="Wake delivery failed — the lead dropped it and no Desktop was present to cover"/,
  );
});

test("U-T1: an unseated report says so on the row, in words", () => {
  const markup = render(rowFor({ unseated: true }));
  assert.match(markup, /data-testid="coding-session-unseated-badge"/);
  assert.match(markup, />unseated</);
  assert.match(
    markup,
    /title="Report author holds no seat for the assigned role"/,
  );
  assert.doesNotMatch(markup, /Seat created, not granted/);
});

test("U-T1: the full signed id appears only under Trace's disclosure", () => {
  const live = render(rowFor({}));
  assert.doesNotMatch(live, new RegExp("c".repeat(64)));
  const trace = render(rowFor({}, { density: "trace" }));
  assert.match(trace, /<summary[^>]*>Signed source<\/summary>/);
  assert.match(trace, new RegExp("c".repeat(64)));
});

test("U-T1: a required action is stated on the row that carries it", () => {
  const markup = render(
    rowFor({
      type: "disposition",
      authorPubkey: FOUNDER,
      counterpartyPubkey: BUILDER,
      decision: "changes-requested",
      requiredAction: "Publish the signed follow-up note.",
    }),
  );
  assert.match(markup, /Keystone → Bob · Verdict: changes-requested/);
  assert.match(markup, /Required action: Publish the signed follow-up note\./);
});

test("R2 §8: the row reads at chat weight — 24px monograms, text-sm body", () => {
  const markup = render(rowFor({}));
  // Two monogram discs, both 24px.
  assert.equal(
    (
      markup.match(
        /size-6 shrink-0 items-center justify-center rounded-full/g,
      ) ?? []
    ).length,
    2,
  );
  assert.doesNotMatch(markup, /inline-flex size-5 shrink-0 items-center/);
  // The signed summary is a message, not fine print.
  const body = markup.match(/<p class="([^"]*)"[^>]*>Dispatch extraction<\/p>/);
  assert.ok(body, "the signed summary renders in its own paragraph");
  assert.match(body[1], /\btext-sm\b/);
  assert.doesNotMatch(body[1], /\btext-xs\b/);
  // Meta stays on the 2xs step.
  assert.match(markup, /text-2xs[^"]*"[^>]*>3 files · 3 tests/);
});

test("R2 §8: an attention row prints the delivery sentence, not just a title", () => {
  const residual =
    "Wake delivery failed — the lead dropped it and no Desktop was present to cover";
  const markup = render(
    rowFor(
      {},
      {
        deliveries: [
          {
            sourceEventId: "c".repeat(64),
            operationType: "report",
            sourceActorPubkey: BUILDER,
            leadTargetKey: "lead-target",
            kind: "failed",
            owningCommandId: "cmd-1",
            duplicateRefusedCommandIds: [],
            failures: [],
            reArmCount: 1,
            observedAtMs: 5,
            detail: residual,
          },
        ],
      },
    ),
  );
  assert.match(markup, /data-weight="attention"/);
  assert.match(markup, /data-testid="coding-session-delivery-detail"/);
  // A visible text node, not only the badge's hover title.
  const detail = markup.match(
    /data-testid="coding-session-delivery-detail"[^>]*>([^<]*)</,
  );
  assert.ok(detail, "the detail line renders");
  assert.equal(detail[1], residual);
  assert.match(markup, /text-destructive/);
});

test("R2 §8: a standard row keeps the sentence in the badge title only", () => {
  const markup = render(
    rowFor(
      {},
      {
        deliveries: [
          {
            sourceEventId: "c".repeat(64),
            operationType: "report",
            sourceActorPubkey: BUILDER,
            leadTargetKey: "lead-target",
            kind: "provider-queued",
            owningCommandId: "cmd-1",
            duplicateRefusedCommandIds: [],
            failures: [],
            reArmCount: 0,
            observedAtMs: 5,
            detail: "Provider wake queued",
          },
        ],
      },
    ),
  );
  assert.match(markup, /data-weight="standard"/);
  assert.doesNotMatch(markup, /data-testid="coding-session-delivery-detail"/);
  assert.match(markup, /title="Provider wake queued"/);
});
