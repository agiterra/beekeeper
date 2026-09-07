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

test("a mounted composer distinguishes unresolved authority from denied access, then enables the resolved founder", async () => {
  const React = await import("react");
  const { act, cleanup, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const { resolveCodingSessionUmbrellaComposerAuthority } = await import(
    "../lib/codingSessionUmbrellaComposerModel.ts"
  );
  const founder = "a".repeat(64);
  const viewer = "b".repeat(64);
  const genesisRef = "c".repeat(64);
  const target = {
    driver: "provider-neutral",
    instanceId: "instance",
    sessionId: "session",
    generation: 1,
  };
  function composer(founderPubkey, currentUserPubkey) {
    const authority = resolveCodingSessionUmbrellaComposerAuthority({
      umbrella: { founderPubkey, genesisRef },
      currentUserPubkey,
      acceptedOperators: new Set(),
    });
    return React.createElement(CodingSessionComposer, {
      authorityReason: authority.reason,
      authorityUnresolved: authority.isUnresolved,
      canControl: authority.canPromptExecutions,
      canInterrupt: true,
      channelId: "channel",
      immersive: true,
      isMember: true,
      isWorking: false,
      target,
    });
  }
  try {
    let mounted;
    await act(async () => {
      mounted = render(composer(null, founder));
    });
    const editor = () => screen.getByLabelText("Coding-session instruction");
    assert.equal(editor().disabled, true);
    assert.match(
      screen.getByTestId("coding-session-control-deck").textContent,
      /Access unresolved/,
    );
    assert.match(
      screen.getByTestId("coding-session-composer-authority-unresolved")
        .textContent,
      /authority could not be resolved/,
    );
    assert.doesNotMatch(editor().placeholder, /collaborator/);

    await act(async () => {
      mounted.rerender(composer(founder, null));
    });
    assert.equal(editor().disabled, true);
    assert.match(editor().placeholder, /identity is available/);

    await act(async () => {
      mounted.rerender(composer(founder, founder));
    });
    assert.equal(editor().disabled, false);
    assert.match(
      screen.getByTestId("coding-session-control-deck").textContent,
      /Can control/,
    );
    assert.equal(
      screen.queryByTestId("coding-session-composer-authority-unresolved"),
      null,
    );

    await act(async () => {
      mounted.rerender(composer(founder, viewer));
    });
    assert.equal(editor().disabled, true);
    assert.match(
      screen.getByTestId("coding-session-control-deck").textContent,
      /View only/,
    );
    assert.match(editor().placeholder, /collaborator access/);
    assert.equal(
      screen.queryByTestId("coding-session-composer-authority-unresolved"),
      null,
    );
  } finally {
    cleanup();
  }
});
