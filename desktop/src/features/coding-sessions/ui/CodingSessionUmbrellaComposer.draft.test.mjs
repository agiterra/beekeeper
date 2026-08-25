/**
 * Drafts must not cross participants.
 *
 * The per-target composer holds its draft in local React state. Without a
 * `key` per execution, switching the participant selector reuses the same
 * component instance — so a half-written prompt for Claude stays in the box
 * addressed to Codex and is *sent* to Codex on the next Enter. That is a
 * wrong-agent send, not a cosmetic glitch, so it gets a mounted test rather
 * than a markup assertion.
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

test("a draft typed for one execution never survives a participant switch", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { groupCodingSessionCatalog } = await import(
    "../lib/codingSessionUmbrellaModel.ts"
  );
  const { CodingSessionUmbrellaComposer } = await import(
    "./CodingSessionUmbrellaComposer.tsx"
  );

  const [umbrella] = groupCodingSessionCatalog([
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
      lastEventAt: "2026-08-12T10:00:00.000Z",
    }),
  ]);
  assert.equal(umbrella.executions.length, 2);

  try {
    await act(async () => {
      render(
        React.createElement(CodingSessionUmbrellaComposer, {
          channelId: CHANNEL_ID,
          currentUserPubkey: null,
          isMember: true,
          umbrella,
          publishLaneMessage: async () => {},
        }),
      );
    });

    const editor = () => screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.click(
        screen.getByTestId("coding-session-participant-picker-trigger"),
      );
    });
    const participants = screen.getAllByTestId(
      "coding-session-participant-execution",
    );
    assert.equal(participants.length, 2);
    const [first, second] = participants;
    // The selector starts on the most recently active execution (Claude).
    assert.equal(
      screen
        .getByTestId("coding-session-participant-picker-trigger")
        .textContent?.includes("Claude"),
      true,
    );

    await act(async () => {
      fireEvent.change(editor(), {
        target: { value: "Claude, rerun the failing fixture." },
      });
    });
    assert.equal(editor().value, "Claude, rerun the failing fixture.");

    await act(async () => {
      fireEvent.click(
        second.getAttribute("aria-pressed") === "true" ? first : second,
      );
    });
    // The other execution's composer is a fresh instance: an empty box.
    assert.equal(editor().value, "");

    await act(async () => {
      fireEvent.click(
        screen.getByTestId("coding-session-participant-picker-trigger"),
      );
    });
    const reopenedParticipants = screen.getAllByTestId(
      "coding-session-participant-execution",
    );
    await act(async () => {
      fireEvent.click(
        reopenedParticipants.find(
          (participant) => participant.getAttribute("aria-pressed") !== "true",
        ),
      );
    });
    // And switching back does not resurrect the abandoned draft either — the
    // only text that can be sent is text typed for the selected execution.
    assert.equal(editor().value, "");
  } finally {
    cleanup();
  }
});
