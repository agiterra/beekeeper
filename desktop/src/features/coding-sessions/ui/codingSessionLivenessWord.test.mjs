import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionDispositionStrip } from "./CodingSessionHeader.tsx";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail.tsx";

/**
 * D6's red-first test: three voices, one word.
 *
 * The rail row, the participant bar (the disposition strip) and the rail
 * *footer count* are three separate readers of W1, and the walk found each of
 * them answering differently for one execution in one window
 * (`WALK-2026-08-29.md` findings 1 and 2). Every case here asserts all three,
 * because two panels agreeing on the wrong word is the failure mode, not the
 * pass.
 */

const AGENT = "1a".repeat(32);
const NOW = Date.parse("2026-08-12T10:10:00.000Z");
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "liveness-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};

function execution({ status, transcript = [], statusAt = null }) {
  return {
    executionKey: "execution-poker",
    signerPubkey: "a".repeat(64),
    operatorPubkey: null,
    priorGenerations: [],
    activeGeneration: {
      generationId: "execution-poker",
      label: "execution-poker",
      title: "Team session",
      providerAuthorityPubkey: "a".repeat(64),
      metadataAuthorityPubkey: "a".repeat(64),
      lastEventAt: "2026-08-12T10:06:00.000Z",
      status,
      statusAt,
      transcript,
      conflictCount: 0,
      commandTarget: TARGET,
      projectRef: null,
      repoRef: null,
      sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
      provider: "claude-agent-acp",
      runtime: "claude-agent-acp",
      model: "claude-opus-5",
      agentRef: AGENT,
      role: "poker",
      turnBudget: null,
      capabilities: null,
    },
  };
}

function umbrellaOf(executions) {
  return {
    umbrellaKey: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    title: "Team session",
    founderPubkey: null,
    genesisRef: null,
    genesisResolution: "legacy",
    status: "idle",
    lastEventAt: "2026-08-12T10:06:00.000Z",
    conflictCount: 0,
    foreignAttachmentCount: 0,
    executions,
  };
}

/** An open turn: the newest item is not a terminator and carries a turn id. */
const OPEN_TURN = [
  {
    id: "result-1",
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Done",
    timestamp: "2026-08-12T10:00:00.000Z",
    turnId: "turn-1",
  },
  {
    id: "msg-2",
    type: "message",
    renderClass: "message",
    role: "user",
    title: "Prompt",
    text: "next task",
    timestamp: "2026-08-12T10:05:00.000Z",
    turnId: "turn-2",
  },
];

function render({ status, transcript, reachability, canSteer }) {
  const umbrella = umbrellaOf([execution({ status, transcript })]);
  const resolveReachability = () => reachability;
  return {
    rail: renderToStaticMarkup(
      React.createElement(CodingSessionExecutionRail, {
        actorNames: () => "Poker",
        canSteer,
        resolveReachability,
        umbrella,
      }),
    ),
    strip: renderToStaticMarkup(
      React.createElement(CodingSessionDispositionStrip, {
        actorNames: () => "Poker",
        canSteer,
        nowMs: NOW,
        resolveReachability,
        umbrella,
      }),
    ),
  };
}

/** The word inside the rail's status chip. */
function railWord(markup) {
  const match = markup.match(
    /data-testid="coding-session-execution-status"[^>]*>([^<]*)</,
  );
  if (!match) throw new Error(`no rail status chip in markup: ${markup}`);
  return match[1];
}

/** The footer tally, exactly as a person reads it. */
function footerText(markup) {
  const match = markup.match(
    /data-testid="coding-session-execution-rail-footer"[^>]*>(.*?)<\/div>/s,
  );
  if (!match) throw new Error(`no rail footer in markup: ${markup}`);
  return match[1].replace(/<[^>]*>/g, "").trim();
}

/** The word inside the strip's line, between the label and the age clause. */
function stripWord(markup) {
  const match = markup.match(
    /data-testid="coding-session-disposition-row"[^>]*>([^<]*)</,
  );
  if (!match) throw new Error(`no disposition row in markup: ${markup}`);
  const parts = match[1].split(" · ");
  return parts[parts.length - 2];
}

test("a completed execution with an open-looking turn reads idle in all three voices", () => {
  // Walk finding 2: signed `completed`, no terminator on the newest turn,
  // lease fresh. The strip printed `live` off a transcript inference while
  // the rail printed `Idle` and the footer `All idle`.
  const { rail, strip } = render({
    status: "completed",
    transcript: OPEN_TURN,
    reachability: { known: true, reachable: true },
  });

  assert.equal(railWord(rail), "idle");
  assert.equal(stripWord(strip), "idle");
  assert.equal(footerText(rail), "1 idle");
  assert.doesNotMatch(rail, /live|working|Working/);
  assert.doesNotMatch(strip, /live|working|Working/);
});

test("a running execution with an aged-out lease reads no provider answering in all three voices", () => {
  // Walk finding 1's exact frame: the rail said `Working` in blue with a
  // footer of `1 working` while the strip said `no provider answering`.
  const { rail, strip } = render({
    status: "running",
    transcript: OPEN_TURN,
    reachability: { known: true, reachable: false },
  });

  assert.equal(railWord(rail), "no provider answering");
  assert.equal(stripWord(strip), "no provider answering");
  assert.equal(footerText(rail), "1 no provider answering");
  assert.doesNotMatch(rail, /1 working|1 live|>Working</);
});

test("a waiting_for_input execution with a fresh lease reads the waiting word, counted apart from idle", () => {
  const { rail, strip } = render({
    status: "waiting_for_input",
    transcript: OPEN_TURN,
    reachability: { known: true, reachable: true },
    canSteer: true,
  });

  assert.equal(railWord(rail), "waiting for you");
  assert.equal(stripWord(strip), "waiting for you");
  assert.equal(footerText(rail), "1 waiting for you");
  // The count the old footer got right by accident, from a second read of the
  // raw status — a naive W1 fold would have deleted it into `idle`.
  assert.doesNotMatch(footerText(rail), /idle/);
});

test("an unreachable waiting_for_input execution never reports waiting on a person", () => {
  // Reachability outranks stage (§2a): telling an operator a dead seat is
  // waiting on them invites them to type into something nobody will read.
  const { rail, strip } = render({
    status: "waiting_for_input",
    transcript: OPEN_TURN,
    reachability: { known: true, reachable: false },
    canSteer: true,
  });

  assert.equal(railWord(rail), "no provider answering");
  assert.equal(stripWord(strip), "no provider answering");
  assert.equal(footerText(rail), "1 no provider answering");
  assert.doesNotMatch(rail, /waiting for/);
  assert.doesNotMatch(strip, /waiting for/);
});

test("a live seat with an open turn still reads live everywhere", () => {
  // The heuristic keeps its one legitimate job: narrowing, never promoting.
  const { rail, strip } = render({
    status: "running",
    transcript: OPEN_TURN,
    reachability: { known: true, reachable: true },
  });

  assert.equal(railWord(rail), "live");
  assert.equal(stripWord(strip), "live");
  assert.equal(footerText(rail), "1 live");
});

test("unknown reachability is printed as the signed status, never demoted and never promoted", () => {
  // `{known:false}` is the honest default and the common one: it is not
  // evidence of absence, so it may not demote — but it may not hide a resting
  // signed status behind `live` either.
  const live = render({
    status: "running",
    transcript: OPEN_TURN,
    reachability: { known: false },
  });
  assert.equal(railWord(live.rail), "live");
  assert.equal(stripWord(live.strip), "live");

  const resting = render({
    status: "completed",
    transcript: OPEN_TURN,
    reachability: { known: false },
  });
  assert.equal(railWord(resting.rail), "idle");
  assert.equal(stripWord(resting.strip), "idle");
  assert.equal(footerText(resting.rail), "1 idle");
});

test("the footer tallies each word once, from the same function the rows read", () => {
  const umbrella = umbrellaOf([
    { ...execution({ status: "running" }), executionKey: "execution-lead" },
    {
      ...execution({ status: "waiting_for_input" }),
      executionKey: "execution-builder",
    },
    {
      ...execution({ status: "completed" }),
      executionKey: "execution-reviewer",
    },
  ]);
  const rail = renderToStaticMarkup(
    React.createElement(CodingSessionExecutionRail, {
      canSteer: true,
      resolveReachability: () => ({ known: true, reachable: true }),
      umbrella,
    }),
  );

  assert.equal(footerText(rail), "1 live · 1 waiting for you · 1 idle");
  assert.doesNotMatch(rail, /All idle/);
});

test("without steer authority the waiting word never claims the reader can answer", () => {
  // W4 gates prompting; a view-only observer told "waiting for you" will try,
  // and be refused by the composer's own view-only notice (F2).
  const { rail, strip } = render({
    status: "waiting_for_input",
    transcript: [],
    reachability: { known: true, reachable: true },
    canSteer: false,
  });

  assert.equal(railWord(rail), "waiting for an operator");
  assert.equal(stripWord(strip), "waiting for an operator");
  assert.doesNotMatch(rail, /waiting for you/);
  assert.doesNotMatch(strip, /waiting for you/);
});
