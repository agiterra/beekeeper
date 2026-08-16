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

test("a disconnected execution offers reconnect or durable end instead of send", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      channelId: "channel-1",
      immersive: true,
      isMember: true,
      isWorking: false,
      lifecycleStatus: "disconnected",
      providerAuthorityPubkey: "ab".repeat(32),
      target,
      variant: "floating",
    }),
  );

  assert.match(markup, /coding-session-composer-resume/);
  assert.match(markup, />Reconnect</);
  assert.match(markup, /coding-session-composer-session-stop/);
  assert.match(markup, />End session</);
  assert.doesNotMatch(markup, /coding-session-composer-primary/);
  assert.match(markup, /Reconnect this execution to continue/);
});

test("an ended execution is terminal and offers no further controls", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      channelId: "channel-1",
      immersive: true,
      isMember: true,
      isWorking: false,
      lifecycleStatus: "stopped",
      providerAuthorityPubkey: "ab".repeat(32),
      target,
      variant: "floating",
    }),
  );

  assert.match(markup, /This provider execution has ended/);
  assert.match(markup, /This execution has ended/);
  assert.doesNotMatch(markup, /coding-session-composer-primary/);
  assert.doesNotMatch(markup, /coding-session-composer-resume/);
  assert.doesNotMatch(markup, /coding-session-composer-session-stop/);
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

test("founder authority gates send, interrupt, resume, and stop together", () => {
  const working = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      authorityReason: "Only the session founder can control this session.",
      canControl: false,
      canInterrupt: true,
      canSteer: true,
      channelId: "channel-1",
      immersive: true,
      isMember: true,
      isWorking: true,
      target,
      variant: "floating",
    }),
  );
  assert.match(working, /coding-session-composer-authority-gated/);
  assert.match(working, /Only the session founder/);
  assert.match(working, /coding-session-composer-interrupt[^>]*disabled=""/);
  assert.match(
    working,
    /aria-label="Coding-session instruction"[^>]*disabled=""/,
  );

  const disconnected = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canControl: false,
      canInterrupt: true,
      channelId: "channel-1",
      immersive: true,
      isMember: true,
      isWorking: false,
      lifecycleStatus: "disconnected",
      providerAuthorityPubkey: "ab".repeat(32),
      target,
      variant: "floating",
    }),
  );
  assert.match(disconnected, /coding-session-composer-resume[^>]*disabled=""/);
  assert.match(
    disconnected,
    /coding-session-composer-session-stop[^>]*disabled=""/,
  );
});

test("legacy sessions retain controls with an honest ungoverned hint", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canControl: true,
      canInterrupt: true,
      channelId: "channel-1",
      immersive: true,
      isMember: true,
      isUngovernedSession: true,
      isWorking: false,
      target,
      variant: "floating",
    }),
  );
  assert.match(markup, /coding-session-ungoverned-hint/);
  assert.match(markup, />ungoverned — adopt to govern\.</);
  assert.doesNotMatch(markup, /coding-session-composer-authority-gated/);
  assert.doesNotMatch(
    markup,
    /aria-label="Coding-session instruction"[^>]*disabled=""/,
  );
});
