import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import { CodingSessionUmbrellaComposer } from "./CodingSessionUmbrellaComposer.tsx";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CLAUDE_SIGNER = "a".repeat(64);
const CODEX_SIGNER = "b".repeat(64);
const FOUNDER = "f".repeat(64);
const TEAMMATE = "e".repeat(64);

const CLAUDE_TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};
const CODEX_TARGET = {
  driver: "codex-acp",
  instanceId: "codex-instance",
  sessionId: "22222222-2222-2222-2222-222222222222",
  generation: 1,
};

function record({
  target,
  signerPubkey,
  runtime,
  model,
  sessionRef = SESSION_REF,
  lastEventAt = "2026-08-12T10:00:00.000Z",
}) {
  return {
    generationId: `gen-${signerPubkey.slice(0, 4)}`,
    label: `${target.driver} · generation ${target.generation}`,
    title: "Advance Buzz live sessions",
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt,
    status: "completed",
    transcript: [],
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef,
    provider: null,
    runtime,
    model,
    capabilities: null,
  };
}

function multiExecutionUmbrella({ creates } = {}) {
  const umbrellas = groupCodingSessionCatalog(
    [
      record({
        target: CLAUDE_TARGET,
        signerPubkey: CLAUDE_SIGNER,
        runtime: "claude",
        model: "claude-opus-5",
        lastEventAt: "2026-08-12T11:00:00.000Z",
      }),
      record({
        target: CODEX_TARGET,
        signerPubkey: CODEX_SIGNER,
        runtime: "codex",
        model: "gpt-5.3-codex",
      }),
    ],
    creates ?? [],
  );
  assert.equal(umbrellas.length, 1);
  return umbrellas[0];
}

function singleExecutionUmbrella() {
  const umbrellas = groupCodingSessionCatalog([
    record({
      target: CLAUDE_TARGET,
      signerPubkey: CLAUDE_SIGNER,
      runtime: "claude",
      model: "claude-opus-5",
      sessionRef: null,
    }),
  ]);
  assert.equal(umbrellas.length, 1);
  return umbrellas[0];
}

function foundedCreates(founderPubkey) {
  return [
    {
      sessionRef: SESSION_REF,
      signerPubkey: founderPubkey,
      createdAt: 1_770_000_000,
      eventId: "create-1",
      target: CLAUDE_TARGET,
    },
    {
      sessionRef: SESSION_REF,
      signerPubkey: founderPubkey,
      createdAt: 1_770_000_100,
      eventId: "create-2",
      target: CODEX_TARGET,
    },
  ];
}

function render(props) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionUmbrellaComposer, {
      channelId: "channel-1",
      isMember: true,
      ...props,
    }),
  );
}

test("N=1: no participant selector, exactly today's single-target composer", () => {
  const markup = render({
    currentUserPubkey: FOUNDER,
    umbrella: singleExecutionUmbrella(),
  });
  assert.doesNotMatch(markup, /coding-session-participant-selector/);
  assert.doesNotMatch(markup, />Session</);
  assert.match(markup, /data-testid="coding-session-composer"/);
});

test("N>1: selector lists each execution plus the Session lane", () => {
  const markup = render({
    currentUserPubkey: FOUNDER,
    umbrella: multiExecutionUmbrella(),
  });
  assert.match(markup, /data-testid="coding-session-participant-selector"/);
  assert.match(markup, /Claude · claude-opus-5/);
  assert.match(markup, /Codex · gpt-5\.3-codex/);
  assert.match(markup, />Session</);
  // Default selection is the most recently active execution, not the lane.
  assert.match(markup, /aria-pressed="true"[^>]*data-participant="execution:/);
});

test("founder keeps the execution composer; the selection defaults sticky to the live execution", () => {
  const markup = render({
    currentUserPubkey: FOUNDER,
    umbrella: multiExecutionUmbrella({ creates: foundedCreates(FOUNDER) }),
  });
  assert.match(markup, /data-testid="coding-session-composer"/);
  assert.doesNotMatch(markup, /coding-session-umbrella-composer-gated/);
});

test("non-founders get honestly disabled execution targets, with the lane still open", () => {
  const markup = render({
    currentUserPubkey: TEAMMATE,
    umbrella: multiExecutionUmbrella({ creates: foundedCreates(FOUNDER) }),
  });
  // Execution chips disabled with the reason; Session chip enabled.
  assert.match(
    markup,
    /data-testid="coding-session-participant-execution"[^>]*disabled/,
  );
  assert.doesNotMatch(
    markup,
    /data-testid="coding-session-participant-session"[^>]*disabled/,
  );
  assert.match(markup, /data-testid="coding-session-umbrella-composer-gated"/);
  assert.match(markup, /Only the session founder/);
});

test("an unresolved founder leaves prompting to the relay's membership gate", () => {
  const markup = render({
    currentUserPubkey: TEAMMATE,
    umbrella: multiExecutionUmbrella(),
  });
  assert.doesNotMatch(markup, /coding-session-umbrella-composer-gated/);
  assert.match(markup, /data-testid="coding-session-composer"/);
});

test("a session prefill selects the lane target's execution and stages its text", () => {
  const umbrella = multiExecutionUmbrella();
  const codexKey = umbrella.executions.find(
    (execution) => execution.signerPubkey === CODEX_SIGNER,
  ).executionKey;
  const markup = render({
    currentUserPubkey: FOUNDER,
    prefill: {
      id: "handoff-1",
      participantKey: `execution:${codexKey}`,
      text: "> From Claude (this session)\n> finding\n\n",
    },
    umbrella,
  });
  assert.match(
    markup,
    new RegExp(
      `aria-pressed="true"[^>]*data-participant="execution:${escapeRegExp(codexKey)}"`,
    ),
  );
  assert.match(markup, /From Claude \(this session\)/);
});

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

// §2 item 41 — the composer must not offer Send into a provider that has no
// live lease, and it must say so rather than leaving the button mysteriously
// dead. Brian's prod app quit at 21:17 and this composer kept offering Send
// and Stop for two hours.
test("an unreachable provider disables the editor and explains why", () => {
  const markup = render({
    currentUserPubkey: FOUNDER,
    umbrella: singleExecutionUmbrella(),
    resolveReachability: () => ({ known: true, reachable: false }),
  });
  assert.match(markup, /data-testid="coding-session-composer-unreachable"/);
  assert.match(markup, /No provider is answering for this execution/);
  assert.match(markup, /Add a provider to the session to continue the work/);
  const editor = markup.match(
    /<textarea[^>]*aria-label="Coding-session instruction"[^>]*>/,
  );
  assert.ok(editor, "the composer editor must render");
  assert.match(editor[0], /disabled/);
});

test("a live lease leaves the composer exactly as it was", () => {
  const markup = render({
    currentUserPubkey: FOUNDER,
    umbrella: singleExecutionUmbrella(),
    resolveReachability: () => ({ known: true, reachable: true }),
  });
  assert.doesNotMatch(markup, /coding-session-composer-unreachable/);
});

test("no coordination read yet claims nothing about the provider", () => {
  const markup = render({
    currentUserPubkey: FOUNDER,
    umbrella: singleExecutionUmbrella(),
  });
  assert.doesNotMatch(markup, /coding-session-composer-unreachable/);
});
