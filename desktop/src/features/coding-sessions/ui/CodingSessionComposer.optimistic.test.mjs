/**
 * A sent turn has to be on screen before the relay answers.
 *
 * The transcript is fact-driven — the words only appear once the provider
 * publishes its own signed `user_prompt` and the client verifies it — so
 * without an optimistic row the message a person just wrote is simply absent
 * for two relay hops plus however long the provider takes. These tests pin the
 * two halves of that: the editor clears and the pending row exists *during* the
 * publish, and a publish that fails puts the words back rather than eating them.
 */
import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const CHANNEL_ID = "0c8016c8-9483-4426-a4b1-b45c8e21d0a1";
const OPERATOR = "c".repeat(64);
const TARGET = {
  driver: "claude-agent-acp",
  instanceId: "claude-instance",
  sessionId: "11111111-1111-1111-1111-111111111111",
  generation: 1,
};

async function loadStore() {
  return import("../lib/codingSessionPendingTurns.ts");
}

beforeEach(async () => {
  const store = await loadStore();
  store.resetPendingCodingSessionTurns();
});

test("the editor clears and the turn is pending before the relay answers", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const { buildCodingSessionTargetKey } = await import(
    "../lib/codingSessionCommand.ts"
  );
  const store = await loadStore();

  let release;
  const inFlight = new Promise((resolve) => {
    release = resolve;
  });

  try {
    await act(async () => {
      render(
        React.createElement(CodingSessionComposer, {
          canInterrupt: false,
          channelId: CHANNEL_ID,
          currentUserPubkey: OPERATOR,
          isMember: true,
          isWorking: false,
          target: TARGET,
          publishCommand: async (input) => {
            await inFlight;
            return {
              eventId: "event-1",
              kind: 44220,
              commandId: input.commandId,
            };
          },
        }),
      );
    });

    const editor = () => screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.change(editor(), { target: { value: "run the tests" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("coding-session-composer-primary"));
    });

    // Still in flight: the relay has said nothing, and yet the person's words
    // have left the editor and are recorded as a pending turn.
    assert.equal(editor().value, "");
    const pending = store.readPendingCodingSessionTurns();
    assert.equal(pending.length, 1);
    assert.equal(pending[0].text, "run the tests");
    assert.equal(pending[0].channelId, CHANNEL_ID);
    assert.equal(pending[0].targetKey, buildCodingSessionTargetKey(TARGET));
    assert.equal(pending[0].operatorPubkey, OPERATOR);
    assert.equal(
      pending[0].published,
      false,
      "an unanswered publish must not be reported as sent",
    );
    assert.equal(
      store.pendingCodingSessionTurnState(pending[0], pending[0].recordedAt),
      "sending",
    );

    await act(async () => {
      release();
      await inFlight;
    });

    const settled = store.readPendingCodingSessionTurns();
    assert.equal(settled.length, 1, "the provider has still not echoed it");
    assert.equal(settled[0].published, true);
    assert.equal(
      store.pendingCodingSessionTurnState(settled[0], settled[0].recordedAt),
      "waiting",
    );
  } finally {
    cleanup();
  }
});

test("a failed publish returns the words and retires the pending row", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const store = await loadStore();

  try {
    await act(async () => {
      render(
        React.createElement(CodingSessionComposer, {
          canInterrupt: false,
          channelId: CHANNEL_ID,
          currentUserPubkey: OPERATOR,
          isMember: true,
          isWorking: false,
          target: TARGET,
          publishCommand: async () => {
            throw new Error(
              "Timed out while sending the coding-session command.",
            );
          },
        }),
      );
    });

    const editor = () => screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.change(editor(), { target: { value: "run the tests" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("coding-session-composer-primary"));
    });

    assert.equal(
      editor().value,
      "run the tests",
      "words that never left this machine belong back in the editor",
    );
    assert.equal(store.readPendingCodingSessionTurns().length, 0);
    assert.match(
      screen.getByTestId("coding-session-composer-error").textContent,
      /Timed out/,
    );
  } finally {
    cleanup();
  }
});
