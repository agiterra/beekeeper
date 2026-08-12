import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionComposer } from "./CodingSessionComposer.tsx";

const target = {
  driver: "provider-neutral",
  instanceId: "instance-1",
  sessionId: "session-1",
  generation: 2,
};

test("stacked composer keeps the editor and authorized actions in separate rows", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      channelId: "channel-1",
      isMember: true,
      isWorking: true,
      layout: "stacked",
      target,
      variant: "floating",
    }),
  );

  assert.match(markup, /data-layout="stacked"/);
  assert.match(markup, /class="flex gap-2 flex-col"/);
  assert.match(markup, /aria-label="Coding-session instruction"/);
  assert.match(markup, /data-testid="coding-session-composer-actions"/);
  assert.match(markup, /data-testid="coding-session-composer-primary"/);
  assert.match(markup, />Steer</);
  assert.match(markup, /data-testid="coding-session-composer-stop"/);
  assert.match(markup, />Stop</);
});

test("inline composer remains the default for existing panel surfaces", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: false,
      channelId: "channel-1",
      isMember: true,
      isWorking: false,
      target,
    }),
  );

  assert.match(markup, /data-layout="inline"/);
  assert.match(markup, />Send</);
  assert.doesNotMatch(markup, /coding-session-composer-stop/);
  assert.doesNotMatch(markup, /coding-session-control-deck/);
});

test("immersive working composer uses one compact truthful control row", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      canSteer: true,
      channelId: "channel-1",
      controlContext: {
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: true,
          threadSteer: true,
          context: true,
          diff: true,
          plan: true,
        },
        model: "claude-opus-5",
        providerLabel: "Anthropic",
        runtimeLabel: "Claude Code",
        status: { kind: "working", label: "Working" },
      },
      immersive: true,
      isMember: true,
      isWorking: true,
      target,
      variant: "floating",
    }),
  );

  assert.match(markup, /data-mode="immersive"/);
  assert.match(markup, /data-testid="coding-session-control-deck"/);
  assert.match(markup, /whitespace-nowrap/);
  assert.doesNotMatch(markup, /flex-wrap/);
  assert.match(markup, /data-status="working"/);
  assert.match(markup, />Working</);
  assert.match(markup, />Anthropic</);
  assert.match(markup, />claude-opus-5</);
  assert.match(markup, /Runtime: Claude Code/);
  assert.match(markup, /Signed channel member/);
  assert.match(markup, />Member</);
  assert.match(
    markup,
    /Provider capabilities: Turns, Steer, Interrupt, Context, Diff, Plan/,
  );
  assert.match(markup, /coding-session-composer-steer/);
  assert.match(markup, />Steer</);
  assert.match(markup, /coding-session-composer-interrupt/);
  assert.match(markup, />Interrupt</);
  assert.doesNotMatch(markup, /coding-session-composer-primary/);
});

test("immersive idle composer shows send and distinct idle status without stop", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      canSteer: true,
      channelId: "channel-1",
      controlContext: {
        capabilities: null,
        model: "gpt-5.6",
        providerLabel: "OpenAI",
        runtimeLabel: "Codex",
        status: { kind: "idle", label: "Idle" },
      },
      immersive: true,
      isMember: true,
      isWorking: false,
      target,
      variant: "floating",
    }),
  );

  assert.match(markup, /data-status="idle"/);
  assert.match(markup, />Idle</);
  assert.match(
    markup,
    /Live steer and interrupt capabilities have not been declared/,
  );
  assert.match(markup, /coding-session-composer-primary/);
  assert.match(markup, />Send</);
  assert.doesNotMatch(markup, /coding-session-composer-interrupt/);
});

test("immersive controls fail closed when authority and interrupt capability are absent", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: false,
      canSteer: false,
      channelId: "channel-1",
      controlContext: {
        capabilities: {
          threadTurnStart: true,
          threadTurnInterrupt: false,
          threadSteer: false,
          context: false,
          diff: true,
          plan: false,
        },
        model: null,
        providerLabel: null,
        runtimeLabel: "Provider runtime",
        status: { kind: "unknown", label: "Status unknown" },
      },
      immersive: true,
      isMember: false,
      isWorking: true,
      target,
      variant: "floating",
    }),
  );

  assert.match(markup, />View only</);
  assert.match(markup, /Provider capabilities: Turns, Diff/);
  assert.doesNotMatch(markup, /Provider capabilities: Turns, Diff, Plan/);
  assert.doesNotMatch(markup, /coding-session-composer-steer/);
  assert.match(markup, />Interrupt</);
  assert.match(markup, /Current-turn interrupt is unavailable/);
  assert.match(markup, /coding-session-composer-interrupt[^>]*disabled=""/);
});
