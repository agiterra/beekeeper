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
    // The router reads `self` when it constructs; the pending row renders
    // Markdown, which reads the router.
    self: dom.window,
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
          channelAccess: { kind: "member" },
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
    // The raw draft rides the row so a composer that remounts while the
    // provider still holds this turn can hand these words back.
    assert.equal(pending[0].draft, "run the tests");
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
          channelAccess: { kind: "member" },
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

test("a mid-turn prompt is published at once, held by the provider not this client", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const store = await loadStore();
  const published = [];
  const baseProps = {
    canInterrupt: true,
    canSteer: false,
    channelId: CHANNEL_ID,
    currentUserPubkey: OPERATOR,
    immersive: true,
    channelAccess: { kind: "member" },
    target: TARGET,
    variant: "floating",
    publishCommand: async (input) => {
      published.push(input);
      return {
        eventId: "queued-event",
        kind: 44220,
        commandId: input.commandId,
      };
    },
  };

  try {
    await act(async () => {
      render(
        React.createElement(CodingSessionComposer, {
          ...baseProps,
          isWorking: true,
        }),
      );
    });
    const editor = screen.getByLabelText("Coding-session instruction");
    assert.equal(editor.disabled, false);
    await act(async () => {
      fireEvent.change(editor, {
        target: { value: "check the second failure next" },
      });
      fireEvent.click(screen.getByTestId("coding-session-composer-queue"));
    });

    // The relay is the mailbox: the turn is on it now, addressed with the
    // delivery class that says "wait for the boundary". Nothing is held in
    // this client's memory, so closing the window does not lose it.
    assert.equal(published.length, 1);
    assert.equal(published[0].text, "check the second failure next");
    assert.equal(published[0].deliver, "boundary");
    assert.equal(
      screen.queryByTestId("coding-session-composer-queued"),
      null,
      "the local next-turn draft is retired",
    );
    const pending = store.readPendingCodingSessionTurns();
    assert.equal(pending.length, 1);
    assert.equal(pending[0].published, true);
    assert.equal(
      pending[0].queuedByProvider,
      undefined,
      "only the provider's own receipt may say it is queued",
    );
    assert.equal(editor.value, "");
  } finally {
    cleanup();
  }
});

test("a steerable execution is asked to steer; anything else is a boundary", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const published = [];
  const baseProps = {
    canInterrupt: true,
    channelId: CHANNEL_ID,
    currentUserPubkey: OPERATOR,
    immersive: true,
    channelAccess: { kind: "member" },
    target: TARGET,
    variant: "floating",
    publishCommand: async (input) => {
      published.push(input);
      return { eventId: "e", kind: 44220, commandId: input.commandId };
    },
  };

  try {
    let view;
    await act(async () => {
      view = render(
        React.createElement(CodingSessionComposer, {
          ...baseProps,
          canSteer: true,
          isWorking: true,
        }),
      );
    });
    const editor = () => screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.change(editor(), { target: { value: "stop and re-read it" } });
      fireEvent.click(screen.getByTestId("coding-session-composer-steer"));
    });
    assert.equal(published.length, 1);
    assert.equal(published[0].deliver, "steer");

    // Idle: there is no running turn to steer, so asking to steer one would be
    // a claim about the provider that nothing supports.
    await act(async () => {
      view.rerender(
        React.createElement(CodingSessionComposer, {
          ...baseProps,
          canSteer: true,
          isWorking: false,
        }),
      );
    });
    await act(async () => {
      fireEvent.change(editor(), { target: { value: "and now the tests" } });
      fireEvent.click(screen.getByTestId("coding-session-composer-primary"));
    });
    assert.equal(published.length, 2);
    assert.equal(published[1].deliver, "boundary");
  } finally {
    cleanup();
  }
});

// Validation 1: a working Claude execution can be told "not into this turn —
// after it", explicitly, and the command says so. Before this the composer
// decided for the person: any mid-turn send on a steering execution became a
// steer, so "when you're done, run the gate" landed in the middle of the run.
test("a steering execution can be asked for the boundary instead, explicitly", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const published = [];
  const props = {
    canInterrupt: true,
    canSteer: true,
    channelId: CHANNEL_ID,
    currentUserPubkey: OPERATOR,
    immersive: true,
    channelAccess: { kind: "member" },
    isWorking: true,
    target: TARGET,
    variant: "floating",
    publishCommand: async (input) => {
      published.push(input);
      return { eventId: "e", kind: 44220, commandId: input.commandId };
    },
  };

  try {
    await act(async () => {
      render(React.createElement(CodingSessionComposer, props));
    });
    // Both classes are offered, and the one that joins the running turn is
    // still the primary.
    assert.ok(screen.getByTestId("coding-session-composer-steer"));
    const queueNext = screen.getByTestId("coding-session-composer-queue-next");
    assert.equal(queueNext.textContent, "Queue next");
    // And the difference is stated on screen, not only in a tooltip.
    assert.equal(
      screen.getByTestId("coding-session-composer-delivery-hint").textContent,
      "Steer joins the turn that is running. Queue next runs after it.",
    );

    const editor = screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.change(editor, {
        target: { value: "when this finishes, run the gate" },
      });
      fireEvent.click(screen.getByTestId("coding-session-composer-queue-next"));
    });
    assert.equal(published.length, 1);
    assert.equal(published[0].deliver, "boundary");
    assert.equal(published[0].text, "when this finishes, run the gate");
    assert.equal(editor.value, "");
  } finally {
    cleanup();
  }
});

// An execution that never advertised steering must not be offered a second
// control implying there is a class it is not using: its primary already
// queues, and the sentence beside it says what that means.
test("an execution that cannot steer offers one control and explains it", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");

  try {
    await act(async () => {
      render(
        React.createElement(CodingSessionComposer, {
          canInterrupt: true,
          canSteer: false,
          channelId: CHANNEL_ID,
          currentUserPubkey: OPERATOR,
          immersive: true,
          channelAccess: { kind: "member" },
          isWorking: true,
          target: TARGET,
          variant: "floating",
          publishCommand: async (input) => ({
            eventId: "e",
            kind: 44220,
            commandId: input.commandId,
          }),
        }),
      );
    });
    assert.equal(screen.queryByTestId("coding-session-composer-steer"), null);
    assert.equal(
      screen.queryByTestId("coding-session-composer-queue-next"),
      null,
      "the primary already queues; a second button would be the same act twice",
    );
    assert.equal(
      screen.getByTestId("coding-session-composer-delivery-hint").textContent,
      "Runs after the current turn — this execution cannot steer.",
    );
  } finally {
    cleanup();
  }
});

// Validation 2: the delivery class is resolved against the execution in front
// of the person at the moment they press, not against whatever was true when
// the control was drawn. A capability that arrives late must not leave a
// steer aimed at an execution that no longer offers one.
test("a capability that changes under the composer cannot leak the old class", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const published = [];
  const props = {
    canInterrupt: true,
    channelId: CHANNEL_ID,
    currentUserPubkey: OPERATOR,
    immersive: true,
    channelAccess: { kind: "member" },
    isWorking: true,
    target: TARGET,
    variant: "floating",
    publishCommand: async (input) => {
      published.push(input);
      return { eventId: "e", kind: 44220, commandId: input.commandId };
    },
  };

  try {
    let view;
    await act(async () => {
      view = render(
        React.createElement(CodingSessionComposer, {
          ...props,
          canSteer: true,
        }),
      );
    });
    // The execution's own 44223 arrives and says it cannot steer after all.
    await act(async () => {
      view.rerender(
        React.createElement(CodingSessionComposer, {
          ...props,
          canSteer: false,
        }),
      );
    });
    assert.equal(
      screen.queryByTestId("coding-session-composer-steer"),
      null,
      "the steer control goes with the capability",
    );
    const editor = screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.change(editor, { target: { value: "look again" } });
      fireEvent.click(screen.getByTestId("coding-session-composer-queue"));
    });
    assert.equal(published.length, 1);
    assert.equal(published[0].deliver, "boundary");
  } finally {
    cleanup();
  }
});

// A second press while the first publish is still in flight must not sign a
// second command for the same words.
test("a double press publishes one command, not two", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const published = [];
  let release;
  const inFlight = new Promise((resolve) => {
    release = resolve;
  });

  try {
    await act(async () => {
      render(
        React.createElement(CodingSessionComposer, {
          canInterrupt: true,
          canSteer: true,
          channelId: CHANNEL_ID,
          currentUserPubkey: OPERATOR,
          immersive: true,
          channelAccess: { kind: "member" },
          isWorking: true,
          target: TARGET,
          variant: "floating",
          publishCommand: async (input) => {
            published.push(input);
            await inFlight;
            return { eventId: "e", kind: 44220, commandId: input.commandId };
          },
        }),
      );
    });
    const editor = screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.change(editor, { target: { value: "twice" } });
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("coding-session-composer-steer"));
    });
    // The editor is already empty and the send is in flight; pressing the
    // other class now must not publish the same words under a second command.
    await act(async () => {
      fireEvent.click(screen.getByTestId("coding-session-composer-queue-next"));
    });
    assert.equal(published.length, 1);
    assert.equal(published[0].deliver, "steer");
    await act(async () => {
      release();
      await inFlight;
    });
  } finally {
    cleanup();
  }
});

// Validation 4: the person gets their words back without anything being
// claimed about the turn they came from. The original row stays exactly as it
// was — the provider could not say whether the input arrived, and a client
// that quietly retired the row would be making that claim on its behalf.
test("copy to draft recovers the words, sends nothing, and keeps the unknown row", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { CodingSessionComposer } = await import("./CodingSessionComposer.tsx");
  const { CodingSessionPendingTurnList } = await import(
    "./CodingSessionPendingTurns.tsx"
  );
  const { buildCodingSessionTargetKey } = await import(
    "../lib/codingSessionCommand.ts"
  );
  const store = await loadStore();
  const targetKey = buildCodingSessionTargetKey(TARGET);
  const published = [];

  store.recordPendingCodingSessionTurn({
    channelId: CHANNEL_ID,
    targetKey,
    commandId: "csc-unknown",
    text: "re-read the failing case",
    draft: "re-read the failing case",
    operatorPubkey: OPERATOR,
    recordedAt: Date.now(),
    published: true,
    attachmentCount: 2,
  });
  store.markPendingCodingSessionTurnDeliveryUnknown(CHANNEL_ID, "csc-unknown", {
    code: "STEER_ACK_LOST",
    message: "the runtime never acknowledged the write",
  });

  function Both() {
    const turns = store.usePendingCodingSessionTurns();
    return React.createElement(
      "div",
      null,
      React.createElement(CodingSessionPendingTurnList, {
        now: Date.now(),
        turns,
      }),
      React.createElement(CodingSessionComposer, {
        canInterrupt: true,
        canSteer: true,
        channelId: CHANNEL_ID,
        currentUserPubkey: OPERATOR,
        immersive: true,
        channelAccess: { kind: "member" },
        isWorking: true,
        target: TARGET,
        variant: "floating",
        publishCommand: async (input) => {
          published.push(input);
          return { eventId: "e", kind: 44220, commandId: input.commandId };
        },
      }),
    );
  }

  // The pending row renders Markdown, which reads the router for link
  // handling. Same wrapper the other UI tests here use.
  const { createMemoryHistory, createRootRoute, createRouter, RouterProvider } =
    await import("@tanstack/react-router");
  const rootRoute = createRootRoute({
    component: () => React.createElement(Both),
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute,
  });
  await router.load();

  try {
    await act(async () => {
      render(React.createElement(RouterProvider, { router }));
    });
    // The person is already writing something else. Recovery must not cost
    // them that.
    const editor = screen.getByLabelText("Coding-session instruction");
    await act(async () => {
      fireEvent.change(editor, { target: { value: "meanwhile, check CI" } });
    });

    // The row discloses what it will and will not bring back before it is
    // pressed, on screen rather than in a tooltip.
    const row = screen.getByTestId("coding-session-pending-turn");
    assert.match(row.textContent, /may already have reached the running turn/);
    assert.match(row.textContent, /Its 2 images are not copied\./);

    await act(async () => {
      fireEvent.click(
        screen.getByTestId("coding-session-pending-turn-copy-draft"),
      );
    });

    // The words are back, beside what was already being written, not instead
    // of it.
    assert.equal(
      editor.value,
      "re-read the failing case\n\nmeanwhile, check CI",
    );
    // Nothing was sent, and nothing about the original changed.
    assert.equal(published.length, 0);
    const [pending] = store.readPendingCodingSessionTurns();
    assert.equal(pending.commandId, "csc-unknown");
    assert.deepEqual(pending.deliveryUnknown, {
      code: "STEER_ACK_LOST",
      message: "the runtime never acknowledged the write",
    });
    assert.ok(
      screen.getByTestId("coding-session-pending-turn"),
      "the row stays until the person dismisses it",
    );

    // A second press is the same request, not a second copy of the words.
    await act(async () => {
      fireEvent.click(
        screen.getByTestId("coding-session-pending-turn-copy-draft"),
      );
    });
    assert.equal(
      editor.value,
      "re-read the failing case\n\nmeanwhile, check CI",
    );
    assert.equal(published.length, 0);
  } finally {
    store.resetPendingCodingSessionTurns();
    cleanup();
  }
});
