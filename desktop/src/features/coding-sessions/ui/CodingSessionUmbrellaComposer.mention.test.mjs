/**
 * `@mention` sugar, at the surface.
 *
 * The pure rules live in `lib/codingSessionMentionRouting.ts` and are tested
 * there. What has to be true *here* is the part a person can see: the selector
 * follows the handle they typed (so the target is never a guess), the draft
 * they wrote survives the retarget, and the text that would actually be sent
 * is the one with the handle removed.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    CustomEvent: dom.window.CustomEvent,
    document: dom.window.document,
    Element: dom.window.Element,
    Event: dom.window.Event,
    getComputedStyle: dom.window.getComputedStyle.bind(dom.window),
    HTMLElement: dom.window.HTMLElement,
    HTMLInputElement: dom.window.HTMLInputElement,
    HTMLTextAreaElement: dom.window.HTMLTextAreaElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    Node: dom.window.Node,
    NodeFilter: dom.window.NodeFilter,
    window: dom.window,
  });
});

after(() => dom.window.close());

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const CLAUDE_SIGNER = "a".repeat(64);
const CODEX_SIGNER = "b".repeat(64);

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

function record({ target, signerPubkey, runtime, model, lastEventAt }) {
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
    sessionRef: SESSION_REF,
    provider: null,
    runtime,
    model,
    capabilities: null,
  };
}

const CLAUDE_RECORD = record({
  target: CLAUDE_TARGET,
  signerPubkey: CLAUDE_SIGNER,
  runtime: "claude",
  model: "claude-opus-5",
  lastEventAt: "2026-08-12T11:00:00.000Z",
});
const CODEX_RECORD = record({
  target: CODEX_TARGET,
  signerPubkey: CODEX_SIGNER,
  runtime: "codex",
  model: "gpt-5.3-codex",
  lastEventAt: "2026-08-12T10:00:00.000Z",
});

async function mount(records) {
  const React = (await import("react")).default;
  const testing = await import("@testing-library/react");
  const { groupCodingSessionCatalog } = await import(
    "../lib/codingSessionUmbrellaModel.ts"
  );
  const { CodingSessionUmbrellaComposer } = await import(
    "./CodingSessionUmbrellaComposer.tsx"
  );
  const [umbrella] = groupCodingSessionCatalog(records);
  await testing.act(async () => {
    testing.render(
      React.createElement(CodingSessionUmbrellaComposer, {
        channelId: CHANNEL_ID,
        currentUserPubkey: null,
        isMember: true,
        umbrella,
        publishLaneMessage: async () => {},
      }),
    );
  });
  const { fireEvent, screen } = testing;
  return {
    ...testing,
    editor: () => screen.getByLabelText("Coding-session instruction"),
    pressed: () =>
      screen.getByTestId("coding-session-participant-picker-trigger")
        .textContent ?? "",
    hint: () => screen.queryByTestId("coding-session-mention-hint"),
    sendDisabled: () =>
      screen.getByTestId("coding-session-composer-primary").disabled,
    openPicker: async () => {
      await testing.act(async () => {
        fireEvent.click(
          screen.getByTestId("coding-session-participant-picker-trigger"),
        );
      });
    },
    type: async (value) => {
      await testing.act(async () => {
        fireEvent.change(screen.getByLabelText("Coding-session instruction"), {
          target: { value },
        });
      });
    },
  };
}

test("a leading handle moves the selector and keeps the draft it was typed in", async () => {
  const view = await mount([CLAUDE_RECORD, CODEX_RECORD]);
  try {
    // The selector starts on the most recently active execution (Claude).
    assert.match(view.pressed(), /Claude/);
    assert.equal(view.hint(), null);

    await view.type("@codex rerun the failing fixture");
    assert.match(view.pressed(), /Codex/);
    // The editor was not remounted: the words are still there, caret and all.
    assert.equal(view.editor().value, "@codex rerun the failing fixture");
    // And the deck now describes Codex, not Claude — the target really moved.
    assert.match(
      view.screen.getByTestId("coding-session-participant-picker-trigger")
        .textContent,
      /GPT-5\.3 Codex/,
    );
    assert.match(view.hint().textContent, /Sending to Codex/);
    assert.match(view.hint().textContent, /@codex is removed from the prompt/);
    assert.equal(view.sendDisabled(), false);

    // Editing the handle re-routes live, still without losing the draft.
    await view.type("@claude rerun the failing fixture");
    assert.match(view.pressed(), /Claude/);
    assert.equal(view.editor().value, "@claude rerun the failing fixture");
  } finally {
    view.cleanup();
  }
});

test("a handle with nothing after it selects but cannot be sent", async () => {
  const view = await mount([CLAUDE_RECORD, CODEX_RECORD]);
  try {
    await view.type("@codex");
    assert.match(view.pressed(), /Codex/);
    // What would be published is the stripped text — here, nothing at all.
    assert.equal(view.sendDisabled(), true);
    await view.type("@codex go");
    assert.equal(view.sendDisabled(), false);
  } finally {
    view.cleanup();
  }
});

test("a mid-sentence handle is prose and never retargets", async () => {
  const view = await mount([CLAUDE_RECORD, CODEX_RECORD]);
  try {
    await view.type("ask @codex whether the fixture is flaky");
    assert.match(view.pressed(), /Claude/);
    assert.equal(view.sendDisabled(), false);
    assert.equal(view.hint(), null);
  } finally {
    view.cleanup();
  }
});

test("an unknown handle says nothing and changes nothing", async () => {
  const view = await mount([CLAUDE_RECORD, CODEX_RECORD]);
  try {
    await view.type("@gemini take a look");
    assert.match(view.pressed(), /Claude/);
    assert.equal(view.hint(), null);
    assert.equal(view.editor().value, "@gemini take a look");
  } finally {
    view.cleanup();
  }
});

test("a handle two executions answer to is reported, not guessed", async () => {
  const view = await mount([
    CLAUDE_RECORD,
    record({
      target: {
        driver: "claude-agent-acp",
        instanceId: "claude-instance-2",
        sessionId: "33333333-3333-3333-3333-333333333333",
        generation: 1,
      },
      signerPubkey: "c".repeat(64),
      runtime: "claude",
      model: "claude-sonnet-5",
      lastEventAt: "2026-08-12T09:00:00.000Z",
    }),
  ]);
  try {
    const before = view.pressed();
    await view.type("@claude rerun the failing fixture");
    // No silent pick between the two, and no strip: it is still just text.
    assert.equal(view.pressed(), before);
    assert.match(view.hint().textContent, /@claude fits .+ and .+/);
    assert.match(view.hint().textContent, /Pick one above/);
    assert.equal(view.editor().value, "@claude rerun the failing fixture");
    assert.equal(view.sendDisabled(), false);
  } finally {
    view.cleanup();
  }
});

test("picking a participant by hand still starts a clean draft", async () => {
  const view = await mount([CLAUDE_RECORD, CODEX_RECORD]);
  try {
    await view.type("@codex rerun the failing fixture");
    assert.match(view.pressed(), /Codex/);
    await view.openPicker();
    const [first, second] = view.screen.getAllByTestId(
      "coding-session-participant-execution",
    );
    const claudeChip =
      first.textContent.includes("Claude") === true ? first : second;
    await view.act(async () => {
      view.fireEvent.click(claudeChip);
    });
    assert.match(view.pressed(), /Claude/);
    // The Codex-addressed draft does not follow an explicit switch.
    assert.equal(view.editor().value, "");
  } finally {
    view.cleanup();
  }
});

test("a single-execution session has no selector, no hint, and no sugar", async () => {
  const view = await mount([CLAUDE_RECORD]);
  try {
    assert.equal(
      view.screen.queryByTestId("coding-session-participant-selector"),
      null,
    );
    assert.equal(view.hint(), null);
    await view.type("@claude rerun the failing fixture");
    // Plain text: nothing is stripped, so the message stays sendable as typed.
    assert.equal(view.editor().value, "@claude rerun the failing fixture");
    assert.equal(view.sendDisabled(), false);
    await view.type("@claude");
    assert.equal(view.sendDisabled(), false);
  } finally {
    view.cleanup();
  }
});
