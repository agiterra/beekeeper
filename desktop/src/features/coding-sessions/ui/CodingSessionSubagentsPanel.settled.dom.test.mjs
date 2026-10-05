import assert from "node:assert/strict";
import { after, test } from "node:test";
import { JSDOM } from "jsdom";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

// SV-58: everywhere the Agents panel can show a spawn running — the dot, the
// icon, the footer tally and the expanded detail — it reads the spawn's
// settled status, never the transcript-only `running`.

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const saved = {
  document: globalThis.document,
  window: globalThis.window,
  HTMLElement: globalThis.HTMLElement,
  IS_REACT_ACT_ENVIRONMENT: globalThis.IS_REACT_ACT_ENVIRONMENT,
};
Object.assign(globalThis, {
  document: dom.window.document,
  window: dom.window,
  HTMLElement: dom.window.HTMLElement,
  IS_REACT_ACT_ENVIRONMENT: true,
});
after(() => {
  for (const [key, value] of Object.entries(saved)) {
    if (value === undefined) delete globalThis[key];
    else globalThis[key] = value;
  }
  dom.window.close();
});

const { deriveCodingSessionSubagentPanel } = await import(
  "../lib/codingSessionSubagents.ts"
);
const { projectCodingSessionTranscript } = await import(
  "../lib/codingSessionTranscriptProjection.ts"
);
const { CodingSessionSubagentsSection, settledCodingSessionSubagentRowSpawn } =
  await import("./CodingSessionSubagentsPanel.tsx");

const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "0123456789abcdef",
  sessionId: "11111111-2222-3333-4444-555555555555",
  generation: 1,
};

/** A turn a provider abandoned: an open Task call, no result. */
function abandonedTranscript() {
  return projectCodingSessionTranscript(
    [
      { kind: "user_prompt", content: "Go" },
      {
        kind: "tool_call",
        tool: {
          toolName: "Abandoned",
          toolKind: "think",
          toolId: "task-1",
          input: {
            description: "Abandoned",
            prompt: "Look around",
            subagent_type: "Explore",
          },
        },
      },
    ].map((item, index) => ({
      target: TARGET,
      eventSeq: index + 1,
      timestamp: 1_700_000_000_000 + index * 1_000,
      turnId: "turn-a",
      item,
    })),
    { channelId: "channel-1", generationId: "generation-1" },
  );
}

async function renderExpanded(panel) {
  const container = document.createElement("div");
  document.body.append(container);
  const root = createRoot(container);
  await act(async () =>
    root.render(React.createElement(CodingSessionSubagentsSection, { panel })),
  );
  const toggle = container.querySelector("button[aria-expanded]");
  await act(async () => toggle.click());
  return {
    container,
    unmount: async () => {
      await act(async () => root.unmount());
      container.remove();
    },
  };
}

for (const [settlement, status, label, detail, steps] of [
  [
    "settled",
    "stopped",
    "Stopped",
    /Stopped — its turn ended/,
    "No steps of this subagent were published.",
  ],
  [
    "unknown",
    "unknown",
    "Status unknown",
    /Status unknown — no result yet/,
    "No steps published yet.",
  ],
]) {
  test(`a ${settlement} turn's open spawn shows no spinner anywhere in the panel`, async () => {
    const transcript = abandonedTranscript();
    const panel = deriveCodingSessionSubagentPanel(
      [transcript],
      () => settlement,
    );
    assert.equal(panel.rows[0].spawn.status, "running", "transcript-only read");
    assert.equal(panel.rows[0].status, status);
    assert.equal(panel.running, 0);

    const { container, unmount } = await renderExpanded(panel);
    try {
      const row = container.querySelector(
        '[data-testid="coding-session-subagent-row"]',
      );
      assert.equal(row.getAttribute("data-status"), status);
      assert.equal(container.querySelector('[aria-label="Running"]'), null);
      assert.ok(container.querySelector(`[aria-label="${label}"]`));
      const footer = container.querySelector(
        '[data-testid="coding-session-subagents-footer"]',
      );
      assert.doesNotMatch(footer.textContent, /running/);
      // The expanded detail says what the row says, not "still running".
      assert.match(container.textContent, detail);
      assert.ok(container.textContent.includes(steps));
    } finally {
      await unmount();
    }
  });
}

test("a live turn's open spawn still reads running, spinner and all", async () => {
  const panel = deriveCodingSessionSubagentPanel(
    [abandonedTranscript()],
    () => "live",
  );
  assert.equal(panel.running, 1);
  const { container, unmount } = await renderExpanded(panel);
  try {
    assert.ok(container.querySelector('[aria-label="Running"]'));
    assert.match(container.textContent, /No steps published yet/);
    assert.match(container.textContent, /1 running/);
  } finally {
    await unmount();
  }
});

test("the row's spawn is the same object when nothing settled it", () => {
  const panel = deriveCodingSessionSubagentPanel([abandonedTranscript()]);
  assert.equal(
    settledCodingSessionSubagentRowSpawn(panel.rows[0]),
    panel.rows[0].spawn,
  );
});
