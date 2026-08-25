import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  createMemoryHistory,
  createRootRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";

import { buildCodingSessionTargetKey } from "../lib/codingSessionCommand.ts";
import { buildCodingSessionHandoffPrefill } from "../lib/codingSessionHandoff.ts";
import { encodeStructuredKey } from "../lib/codingSessionKeys.ts";
import { groupCodingSessionCatalog } from "../lib/codingSessionUmbrellaModel.ts";
import {
  buildUmbrellaTimeline,
  codingSessionUmbrellaEntryKey,
} from "../lib/codingSessionUmbrellaTimeline.ts";
import {
  CodingSessionUmbrellaTimelineView,
  buildUmbrellaTurnBlockHandoff,
  shouldShowTurnBlockProvenance,
  umbrellaWorkspaceStatus,
} from "./CodingSessionUmbrellaWorkspace.tsx";
import { CodingSessionExecutionRail } from "./CodingSessionExecutionRail.tsx";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CLAUDE_SIGNER = "a".repeat(64);
const CODEX_SIGNER = "b".repeat(64);
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";

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

function itemId(seq) {
  return encodeStructuredKey(
    "coding-session-transcript-item/v1",
    "generation-scope",
    "target-key",
    String(seq),
  );
}

function message(seq, role, text, timestamp, turnId = "turn-1") {
  return {
    id: itemId(seq),
    type: "message",
    renderClass: "message",
    role,
    title: role === "user" ? "Brian" : "Assistant",
    text,
    timestamp,
    turnId,
  };
}

function turnResult(seq, timestamp, turnId = "turn-1") {
  return {
    id: itemId(seq),
    type: "lifecycle",
    renderClass: "status",
    title: "Turn result",
    text: "Done",
    timestamp,
    turnId,
  };
}

function record({
  target,
  signerPubkey,
  runtime,
  model,
  transcript,
  status = "completed",
  lastEventAt = "2026-08-12T10:10:00.000Z",
}) {
  return {
    generationId: `gen-${signerPubkey.slice(0, 4)}`,
    label: `${target.driver} · generation ${target.generation}`,
    title: "Advance Buzz live sessions",
    providerAuthorityPubkey: signerPubkey,
    metadataAuthorityPubkey: signerPubkey,
    lastEventAt,
    status,
    transcript,
    conflictCount: 0,
    commandTarget: target,
    projectRef: null,
    repoRef: null,
    sessionRef: SESSION_REF,
    provider: null,
    runtime,
    model,
    capabilities: null,
  };
}

function buildUmbrella({ claudePrompt, codexPrompt } = {}) {
  const claude = record({
    target: CLAUDE_TARGET,
    signerPubkey: CLAUDE_SIGNER,
    runtime: "claude",
    model: "claude-opus-5",
    transcript: [
      ...(claudePrompt
        ? [message(1, "user", claudePrompt, "2026-08-12T10:00:00.000Z")]
        : []),
      message(
        2,
        "assistant",
        "The failing test is fixtures/relay.rs:88.",
        "2026-08-12T10:00:30.000Z",
      ),
      turnResult(3, "2026-08-12T10:01:00.000Z"),
    ],
  });
  const codex = record({
    target: CODEX_TARGET,
    signerPubkey: CODEX_SIGNER,
    runtime: "codex",
    model: "gpt-5.3-codex",
    transcript: [
      ...(codexPrompt
        ? [
            message(
              1,
              "user",
              codexPrompt,
              "2026-08-12T10:04:00.000Z",
              "turn-9",
            ),
          ]
        : []),
      message(
        2,
        "assistant",
        "Regenerated the fixture as requested.",
        "2026-08-12T10:05:00.000Z",
        "turn-9",
      ),
      turnResult(3, "2026-08-12T10:06:00.000Z", "turn-9"),
    ],
  });
  const umbrellas = groupCodingSessionCatalog([claude, codex]);
  assert.equal(umbrellas.length, 1);
  assert.equal(umbrellas[0].executions.length, 2);
  return umbrellas[0];
}

test("execution rail offers a consolidated overview and one tab per execution", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionExecutionRail, {
      umbrella: buildUmbrella(),
    }),
  );

  assert.match(markup, /Participants/);
  assert.match(markup, />All agents</);
  assert.match(markup, />Claude</);
  assert.match(markup, />Codex</);
  assert.match(markup, /All idle/);
  assert.equal(
    markup.match(/data-testid="coding-session-execution-card"/g)?.length,
    2,
  );
});

async function renderTimeline(props) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(CodingSessionUmbrellaTimelineView, props),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("N>1 interleaves per-execution turn blocks with per-block provenance", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: buildUmbrella(),
  });
  const blocks = markup.match(
    /data-testid="coding-session-umbrella-turn-block"/g,
  );
  assert.equal(blocks?.length, 2);
  // Each block carries its own signer, never the other stream's.
  assert.match(markup, new RegExp(`data-signer="${CLAUDE_SIGNER}"`));
  assert.match(markup, new RegExp(`data-signer="${CODEX_SIGNER}"`));
  assert.match(markup, /Claude · claude-opus-5/);
  assert.match(markup, /Codex · gpt-5\.3-codex/);
  // Interleave order follows block timestamps: Claude's turn precedes Codex's.
  assert.ok(
    markup.indexOf("fixtures/relay.rs:88") <
      markup.indexOf("Regenerated the fixture"),
  );
});

test("a joining execution gets a named seam row; the founder gets none", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: buildUmbrella(),
  });
  const rows = markup.match(/data-testid="coding-session-umbrella-lifecycle"/g);
  assert.equal(rows?.length, 1, "only the second execution announces itself");
  assert.match(markup, /data-lifecycle-event="execution-joined"/);
  assert.match(markup, /Codex · gpt-5\.3-codex joined this session/);
  assert.doesNotMatch(markup, /Claude · claude-opus-5 joined this session/);
  // The seam introduces the work it precedes.
  assert.ok(
    markup.indexOf("joined this session") <
      markup.indexOf("Regenerated the fixture"),
  );
});

test("conversation-lane messages interleave between blocks by time", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [
      {
        eventId: "lane-1",
        sessionRef: SESSION_REF,
        channelId: CHANNEL_ID,
        authorPubkey: "c".repeat(64),
        content: "Codex, take the failing test Claude found.",
        timestampMs: Date.parse("2026-08-12T10:03:00.000Z"),
      },
    ],
    onHandoff: () => {},
    umbrella: buildUmbrella(),
  });
  assert.match(markup, /data-testid="coding-session-umbrella-conversation"/);
  const laneIndex = markup.indexOf("Codex, take the failing test");
  assert.ok(markup.indexOf("fixtures/relay.rs:88") < laneIndex);
  assert.ok(laneIndex < markup.indexOf("Regenerated the fixture"));
});

test("completed blocks offer Send to the other execution", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: buildUmbrella(),
  });
  assert.match(markup, /data-testid="coding-session-umbrella-send-to"/);
  assert.match(markup, /Send to.*Codex · gpt-5\.3-codex/s);
  assert.match(markup, /Send to.*Claude · claude-opus-5/s);
  assert.match(markup, /group-hover\/turn:opacity-100/);
  assert.match(markup, /opacity-0/);
});

function handoffPromptFor(link) {
  return `${buildCodingSessionHandoffPrefill({
    sourceLabel: "Claude · claude-opus-5",
    link,
    quote: "The failing test is fixtures/relay.rs:88.",
  })}Regenerate the fixture.`;
}

test("a recognized handoff prompt renders a chip; edited text degrades to plain quote", async () => {
  const chipMarkup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: buildUmbrella({
      codexPrompt: handoffPromptFor({
        channelId: CHANNEL_ID,
        targetKey: buildCodingSessionTargetKey(CLAUDE_TARGET),
        eventSeq: 2,
      }),
    }),
  });
  assert.match(
    chipMarkup,
    /data-testid="coding-session-umbrella-handoff-chip"/,
  );
  assert.match(chipMarkup, /Handoff from Claude · claude-opus-5/);
  // The quoted fact is in this view, so the control is a real one that jumps
  // to it — never a bare beekeeper:// anchor, which nothing in the app parses.
  assert.match(chipMarkup, /data-testid="coding-session-umbrella-view-source"/);
  assert.doesNotMatch(chipMarkup, /<a[^>]+href="beekeeper:\/\//);

  const plainMarkup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: buildUmbrella({ claudePrompt: "Just a plain prompt." }),
  });
  assert.doesNotMatch(
    plainMarkup,
    /data-testid="coding-session-umbrella-handoff-chip"/,
  );
});

test("a handoff link this view cannot resolve renders inert provenance, not a dead link", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: buildUmbrella({
      // Same channel and target, but a seq no ingested fact carries.
      codexPrompt: handoffPromptFor({
        channelId: CHANNEL_ID,
        targetKey: buildCodingSessionTargetKey(CLAUDE_TARGET),
        eventSeq: 998,
      }),
    }),
  });
  assert.match(
    markup,
    /data-testid="coding-session-umbrella-source-unavailable"/,
  );
  assert.doesNotMatch(
    markup,
    /data-testid="coding-session-umbrella-view-source"/,
  );
  assert.doesNotMatch(markup, /<a[^>]+href="beekeeper:\/\//);
  // The durable provenance still lives in the prompt text itself.
  assert.match(markup, /beekeeper:\/\/coding-session\?/);
});

test("a handoff link from another channel never resolves to a lookalike block", async () => {
  const markup = await renderTimeline({
    channelId: CHANNEL_ID,
    laneMessages: [],
    onHandoff: () => {},
    umbrella: buildUmbrella({
      codexPrompt: handoffPromptFor({
        channelId: "11111111-2222-3333-4444-555555555555",
        targetKey: buildCodingSessionTargetKey(CLAUDE_TARGET),
        eventSeq: 2,
      }),
    }),
  });
  assert.match(
    markup,
    /data-testid="coding-session-umbrella-source-unavailable"/,
  );
});

test("turn-block keys stay unique and stable when a lane message interleaves", () => {
  const umbrella = buildUmbrella();
  const withoutLane = buildUmbrellaTimeline(umbrella, []).map(
    codingSessionUmbrellaEntryKey,
  );
  const withLane = buildUmbrellaTimeline(umbrella, [
    {
      eventId: "lane-1",
      sessionRef: SESSION_REF,
      channelId: CHANNEL_ID,
      authorPubkey: "c".repeat(64),
      content: "Codex, take the failing test Claude found.",
      timestampMs: Date.parse("2026-08-12T10:03:00.000Z"),
    },
  ]).map(codingSessionUmbrellaEntryKey);

  assert.equal(new Set(withoutLane).size, withoutLane.length);
  assert.equal(new Set(withLane).size, withLane.length);
  // The lane row is additive: every block key it interleaves between is the
  // key it had before, so no block remounts (and no draft/disclosure state is
  // lost) when chat arrives mid-session.
  assert.deepEqual(
    withLane.filter((key) => !key.startsWith("conversation:")),
    withoutLane,
  );
});

test("provenance labels an execution run once and yields to generation lifecycle rows", () => {
  const first = {
    kind: "turn-block",
    executionKey: "claude",
    signerPubkey: CLAUDE_SIGNER,
    generation: 1,
  };
  const entries = [
    first,
    { ...first, blockSeq: 1 },
    {
      kind: "lifecycle",
      executionKey: "claude",
      signerPubkey: CLAUDE_SIGNER,
      event: "generation-started",
      generation: 2,
    },
    { ...first, generation: 2, blockSeq: 2 },
    { ...first, executionKey: "codex", signerPubkey: CODEX_SIGNER },
  ];

  assert.equal(shouldShowTurnBlockProvenance(entries, 0), true);
  assert.equal(shouldShowTurnBlockProvenance(entries, 1), false);
  assert.equal(shouldShowTurnBlockProvenance(entries, 3), false);
  assert.equal(shouldShowTurnBlockProvenance(entries, 4), true);
});

test("a turn split by a mid-turn ungrouped item yields two distinct block keys", () => {
  const split = groupCodingSessionCatalog([
    {
      generationId: "gen-split",
      label: "claude-agent-acp · generation 1",
      title: "Advance Buzz live sessions",
      providerAuthorityPubkey: CLAUDE_SIGNER,
      metadataAuthorityPubkey: CLAUDE_SIGNER,
      lastEventAt: "2026-08-12T10:10:00.000Z",
      status: "completed",
      transcript: [
        message(1, "user", "Do the thing.", "2026-08-12T10:00:00.000Z"),
        // No turn identity: an ungrouped item splits the turn in two.
        {
          id: itemId(2),
          type: "lifecycle",
          renderClass: "status",
          title: "Status",
          text: "running",
          timestamp: "2026-08-12T10:00:10.000Z",
        },
        message(3, "assistant", "Did the thing.", "2026-08-12T10:00:20.000Z"),
      ],
      conflictCount: 0,
      commandTarget: CLAUDE_TARGET,
      projectRef: null,
      repoRef: null,
      sessionRef: SESSION_REF,
      provider: null,
      runtime: "claude",
      model: "claude-opus-5",
      capabilities: null,
    },
  ])[0];
  const keys = buildUmbrellaTimeline(split, []).map(
    codingSessionUmbrellaEntryKey,
  );
  // Three blocks, two of them carrying the same turnId — keying on turnId
  // alone would collide and React would drop one of them.
  assert.equal(keys.length, 3);
  assert.equal(new Set(keys).size, 3);
});

test("the handoff prefill quotes the source and addresses the chosen execution", () => {
  const umbrella = buildUmbrella();
  const [claude, codex] = umbrella.executions;
  const prefill = buildUmbrellaTurnBlockHandoff({
    block: {
      kind: "turn-block",
      executionKey: claude.executionKey,
      signerPubkey: claude.signerPubkey,
      generation: 1,
      generationId: claude.activeGeneration.generationId,
      turnId: "turn-1",
      items: claude.activeGeneration.transcript,
      timestampMs: 0,
    },
    channelId: CHANNEL_ID,
    quote: "The failing test is fixtures/relay.rs:88.",
    eventSeq: 2,
    record: claude.activeGeneration,
    sourceLabel: "Claude · claude-opus-5",
    targetExecutionKey: codex.executionKey,
  });
  assert.equal(prefill.participantKey, `execution:${codex.executionKey}`);
  assert.match(
    prefill.text,
    /^> From Claude · claude-opus-5 \(this session\) — beekeeper:\/\/coding-session\?/,
  );
  assert.match(prefill.text, /seq=2/);
  assert.match(prefill.text, /> The failing test is fixtures\/relay\.rs:88\./);
  assert.match(prefill.text, /\n\n$/);
});

test("without a resolvable signed fact the prefill degrades to a plain quote", () => {
  const umbrella = buildUmbrella();
  const [claude, codex] = umbrella.executions;
  const prefill = buildUmbrellaTurnBlockHandoff({
    block: {
      kind: "turn-block",
      executionKey: claude.executionKey,
      signerPubkey: claude.signerPubkey,
      generation: 1,
      generationId: claude.activeGeneration.generationId,
      turnId: "turn-1",
      items: claude.activeGeneration.transcript,
      timestampMs: 0,
    },
    channelId: CHANNEL_ID,
    quote: "The failing test is fixtures/relay.rs:88.",
    eventSeq: null,
    record: claude.activeGeneration,
    sourceLabel: "Claude · claude-opus-5",
    targetExecutionKey: codex.executionKey,
  });
  assert.doesNotMatch(prefill.text, /beekeeper:\/\//);
  assert.match(
    prefill.text,
    /^> From Claude · claude-opus-5 \(this session\)\n/,
  );
  assert.match(prefill.text, /> The failing test/);
});

test("umbrella status maps onto the header's honest states", () => {
  assert.deepEqual(umbrellaWorkspaceStatus({ status: "running" }), {
    kind: "working",
    label: "Working",
  });
  assert.deepEqual(umbrellaWorkspaceStatus({ status: "stopped" }), {
    kind: "ended",
    label: "Ended",
  });
  assert.deepEqual(umbrellaWorkspaceStatus({ status: "unknown" }), {
    kind: "unknown",
    label: "Status unknown",
  });
});
