import assert from "node:assert/strict";
import { afterEach, test } from "node:test";
import { JSDOM } from "jsdom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import { codingSessionSubagentScopeOf } from "../lib/codingSessionSubagentNavigation.ts";
import {
  openCodingSessionSubagentPage,
  readCodingSessionSubagentPage,
  resetCodingSessionSubagentPages,
} from "../lib/codingSessionSubagentPageStore.ts";
import { deriveCodingSessionSubagentPanel } from "../lib/codingSessionSubagents.ts";
import { projectCodingSessionTranscript } from "../lib/codingSessionTranscriptProjection.ts";
import { CodingSessionSubagentPage } from "./CodingSessionSubagentPage.tsx";
import { CodingSessionSurfaceCtxProvider } from "./surfaces/codingSessionSurfaceContext.tsx";

const saved = {
  document: globalThis.document,
  window: globalThis.window,
  HTMLElement: globalThis.HTMLElement,
  Element: globalThis.Element,
  KeyboardEvent: globalThis.KeyboardEvent,
  requestAnimationFrame: globalThis.requestAnimationFrame,
  act: globalThis.IS_REACT_ACT_ENVIRONMENT,
};

afterEach(() => {
  resetCodingSessionSubagentPages();
  for (const [key, value] of Object.entries(saved)) {
    const name = key === "act" ? "IS_REACT_ACT_ENVIRONMENT" : key;
    if (value === undefined) delete globalThis[name];
    else globalThis[name] = value;
  }
});

function installDom() {
  const dom = new JSDOM("<!doctype html><html><body></body></html>");
  globalThis.window = dom.window;
  globalThis.document = dom.window.document;
  globalThis.HTMLElement = dom.window.HTMLElement;
  globalThis.Element = dom.window.Element;
  globalThis.KeyboardEvent = dom.window.KeyboardEvent;
  globalThis.requestAnimationFrame = (callback) => setTimeout(callback, 0);
  globalThis.IS_REACT_ACT_ENVIRONMENT = true;
  return dom;
}

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

function project(items) {
  return projectCodingSessionTranscript(
    items.map((item, index) => ({
      target: TARGET,
      eventSeq: index + 1,
      timestamp: 1_700_000_000_000 + index * 1_000,
      turnId: "turn-1",
      item,
    })),
    { channelId: "channel-1", generationId: "generation-1" },
  );
}

const FOCUSED = {
  executionKey: "execution-1",
  priorGenerations: [],
  activeGeneration: { generationId: "generation-1", transcript: [] },
};

function ctxFor(transcript, settlementOf) {
  return {
    layout: "single",
    communityScope: "wss://hive",
    channelId: "channel-1",
    sessionKey: "session-1",
    focusedRecord: { generationId: "generation-1" },
    focusedExecution: FOCUSED,
    executions: [
      { execution: FOCUSED, status: { kind: "working", label: "Working" } },
    ],
    sessionClosed: false,
    umbrella: { executions: [FOCUSED], title: "Parent session" },
    transcript,
    subagents: deriveCodingSessionSubagentPanel([transcript], settlementOf),
    currentUserPubkey: null,
    extensions: {},
  };
}

const TASK = {
  kind: "tool_call",
  tool: {
    toolName: "Map the call sites",
    toolKind: "think",
    toolId: "task-1",
    input: {
      description: "Map the call sites",
      prompt: "Find every caller.",
      subagent_type: "Explore",
    },
  },
};

async function render(ctx) {
  const dom = installDom();
  const container = dom.window.document.createElement("div");
  dom.window.document.body.append(container);
  const root = createRoot(container);
  await act(async () => {
    root.render(
      React.createElement(
        CodingSessionSurfaceCtxProvider,
        { value: ctx },
        React.createElement(CodingSessionSubagentPage),
      ),
    );
  });
  return { container, dom, root };
}

test("nothing renders until a row opens a subagent", async () => {
  const transcript = project([{ kind: "user_prompt", content: "Go" }, TASK]);
  const { container, root } = await render(ctxFor(transcript));
  assert.equal(container.innerHTML, "");
  await act(async () => root.unmount());
});

test("an opened spawn with no steps yet shows its bar, its prompt and says so", async () => {
  const transcript = project([{ kind: "user_prompt", content: "Go" }, TASK]);
  const ctx = ctxFor(transcript);
  const scope = codingSessionSubagentScopeOf(ctx);
  openCodingSessionSubagentPage(scope, "task-1");
  const { container, root } = await render(ctx);
  const q = (id) => container.querySelector(`[data-testid="${id}"]`);
  assert.ok(q("coding-session-subagent-page"));
  assert.equal(
    q("coding-session-subagent-bar-title").textContent,
    "Map the call sites",
  );
  assert.equal(q("coding-session-subagent-bar-status").textContent, "Running");
  assert.equal(q("coding-session-subagent-bar-elapsed").dataset.live, "true");
  assert.match(
    q("coding-session-subagent-page-prompt").textContent,
    /Find every caller\./,
  );
  assert.match(
    q("coding-session-subagent-page-empty").textContent,
    /No activity from this subagent yet/,
  );
  await act(async () => root.unmount());
});

test("a spawn whose turn settled reads Stopped with no live timer", async () => {
  const transcript = project([{ kind: "user_prompt", content: "Go" }, TASK]);
  const ctx = ctxFor(transcript, () => "settled");
  openCodingSessionSubagentPage(codingSessionSubagentScopeOf(ctx), "task-1");
  const { container, root } = await render(ctx);
  const bar = container.querySelector(
    '[data-testid="coding-session-subagent-bar"]',
  );
  assert.equal(bar.dataset.status, "stopped");
  assert.equal(
    container.querySelector('[data-live="true"]'),
    null,
    "a settled subagent never ticks",
  );
  await act(async () => root.unmount());
});

test("a page whose owning call is gone says so", async () => {
  const transcript = project([{ kind: "user_prompt", content: "Go" }]);
  const ctx = ctxFor(transcript);
  openCodingSessionSubagentPage(codingSessionSubagentScopeOf(ctx), "task-9");
  const { container, root } = await render(ctx);
  assert.match(
    container.querySelector(
      '[data-testid="coding-session-subagent-page-missing-call"]',
    ).textContent,
    /not in this transcript/,
  );
  await act(async () => root.unmount());
});

test("Open parent and Escape both close the page", async () => {
  const transcript = project([{ kind: "user_prompt", content: "Go" }, TASK]);
  const ctx = ctxFor(transcript);
  const scope = codingSessionSubagentScopeOf(ctx);
  openCodingSessionSubagentPage(scope, "task-1");
  const { container, dom, root } = await render(ctx);
  const row = dom.window.document.createElement("details");
  row.dataset.subagentCallIds = ctx.subagents.rows[0].id;
  let scrolled = 0;
  row.scrollIntoView = () => {
    scrolled += 1;
  };
  dom.window.document.body.append(row);
  await act(async () => {
    container
      .querySelector('[data-testid="coding-session-subagent-open-parent"]')
      .dispatchEvent(new dom.window.MouseEvent("click", { bubbles: true }));
  });
  assert.equal(readCodingSessionSubagentPage(scope), null);
  assert.equal(container.innerHTML, "");
  await act(async () => new Promise((resolve) => setTimeout(resolve, 5)));
  assert.equal(scrolled, 1, "the parent scrolls back to the subagent's row");

  await act(async () => openCodingSessionSubagentPage(scope, "task-1"));
  assert.ok(
    container.querySelector('[data-testid="coding-session-subagent-page"]'),
  );
  await act(async () => {
    dom.window.dispatchEvent(
      new dom.window.KeyboardEvent("keydown", { key: "Escape" }),
    );
  });
  assert.equal(readCodingSessionSubagentPage(scope), null);
  await act(async () => new Promise((resolve) => setTimeout(resolve, 5)));
  await act(async () => root.unmount());
});

test("SV-98: a lineage divider heads the page and the facts bar sits where the composer is", async () => {
  // Kept to a spawn with no published items: rendering items or a result
  // needs the app router (Markdown). The SV-97 de-duplication of the page's
  // items is pinned in codingSessionSubagentPageModel.test.mjs.
  const transcript = project([{ kind: "user_prompt", content: "Go" }, TASK]);
  const ctx = ctxFor(transcript);
  openCodingSessionSubagentPage(codingSessionSubagentScopeOf(ctx), "task-1");
  const { container, root } = await render(ctx);
  const q = (id) => container.querySelector(`[data-testid="${id}"]`);
  const page = q("coding-session-subagent-page");
  const lineage = q("coding-session-subagent-page-lineage");
  assert.match(lineage.textContent, /Subagent of·Parent session/);
  assert.equal(
    q("coding-session-subagent-page-parent-status").dataset.parentStatus,
    "working",
  );
  assert.match(lineage.textContent, /Working/);
  // The divider heads the transcript column; the prompt follows it.
  const scroll = q("coding-session-subagent-page-scroll");
  assert.equal(
    scroll.firstElementChild.firstElementChild.dataset.testid,
    "coding-session-subagent-page-lineage",
  );
  // The bar is docked after the scroller, at the bottom of the page.
  assert.equal(
    page.lastElementChild.dataset.testid,
    "coding-session-subagent-bar-dock",
  );
  assert.ok(page.lastElementChild.contains(q("coding-session-subagent-bar")));
  assert.match(
    q("coding-session-subagent-open-parent").title,
    /Working/,
    "the way back says what the parent is doing",
  );
  await act(async () => root.unmount());
});

test("SV-98: a closed session's parent reads Closed, never Working", async () => {
  const transcript = project([{ kind: "user_prompt", content: "Go" }, TASK]);
  const ctx = { ...ctxFor(transcript), sessionClosed: true };
  openCodingSessionSubagentPage(codingSessionSubagentScopeOf(ctx), "task-1");
  const { container, root } = await render(ctx);
  const status = container.querySelector(
    '[data-testid="coding-session-subagent-page-parent-status"]',
  );
  assert.equal(status.textContent, "Closed");
  assert.doesNotMatch(status.innerHTML, /animate-pulse/);
  await act(async () => root.unmount());
});
