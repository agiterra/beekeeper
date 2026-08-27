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
      canSteer: true,
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
  // The turn-level control is "Interrupt"; "Stop execution" is the terminal
  // one and must not share its verb (asked about live, 2026-08-24).
  assert.match(markup, />Interrupt</);
  assert.doesNotMatch(markup, />Stop</);
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
        model: "claude-opus-5[high]",
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
  assert.match(markup, /rounded-3xl/);
  assert.match(markup, /resize-none/);
  assert.doesNotMatch(markup, /flex-wrap/);
  assert.doesNotMatch(markup, /coding-session-control-status/);
  assert.doesNotMatch(markup, />Working</);
  assert.match(markup, /Anthropic · Claude Opus 5/);
  assert.match(markup, />High</);
  assert.doesNotMatch(markup, /claude-opus-5\[high\]/);
  assert.match(markup, />Can control</);
  assert.match(markup, /coding-session-composer-steer/);
  assert.match(markup, /aria-label="Steer current turn"/);
  assert.match(markup, /coding-session-composer-interrupt/);
  assert.match(markup, /aria-label="Interrupt current turn"/);
  assert.doesNotMatch(markup, /coding-session-composer-primary/);
});

test("immersive idle composer leaves status to the header and shows send", () => {
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

  assert.doesNotMatch(markup, /coding-session-control-status/);
  assert.doesNotMatch(markup, />Idle</);
  assert.match(markup, /OpenAI · GPT-5\.6/);
  assert.match(markup, /coding-session-composer-primary/);
  assert.match(markup, /aria-label="Send message"/);
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
  assert.match(markup, />Stop execution</);
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
  assert.match(markup, />Provider runtime</);
  assert.doesNotMatch(markup, /coding-session-composer-steer/);
  assert.match(markup, /coding-session-composer-queue/);
  assert.match(
    markup,
    /aria-label="Send at the next turn boundary; it cannot be recalled"/,
  );
  assert.match(markup, /Current-turn interrupt is unavailable/);
  assert.match(markup, /coding-session-composer-interrupt[^>]*disabled=""/);
});

test("a running turn without live steer sends at the boundary and says so", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      canSteer: false,
      channelId: "channel-1",
      immersive: true,
      isMember: true,
      isWorking: true,
      target,
      variant: "floating",
    }),
  );

  assert.match(markup, /placeholder="Send the next turn…"/);
  assert.doesNotMatch(
    markup,
    /aria-label="Coding-session instruction"[^>]*disabled=""/,
  );
  assert.match(markup, /coding-session-composer-queue/);
  assert.match(
    markup,
    /aria-label="Send at the next turn boundary; it cannot be recalled"/,
  );
  // The promise the button makes is the one the provider can keep: this
  // execution advertised no native steering, so nothing here says "Steer".
  assert.doesNotMatch(markup, />Steer</);
});

test("a non-steering execution never labels its mid-turn send a steer", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      canSteer: false,
      channelId: "channel-1",
      isMember: true,
      isWorking: true,
      layout: "stacked",
      target,
      variant: "floating",
    }),
  );
  assert.match(markup, />Send next</);
  assert.doesNotMatch(markup, />Steer</);
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
  assert.doesNotMatch(working, /coding-session-composer-authority-gated/);
  assert.match(working, />View only</);
  assert.match(
    working,
    /placeholder="View only — ask for collaborator access\."/,
  );
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

test("legacy sessions retain controls without adding a warning above the editor", () => {
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
  assert.doesNotMatch(markup, /coding-session-ungoverned-hint/);
  assert.match(markup, />Can control</);
  assert.doesNotMatch(markup, /coding-session-composer-authority-gated/);
  assert.doesNotMatch(
    markup,
    /aria-label="Coding-session instruction"[^>]*disabled=""/,
  );
});

test("signed context usage earns a meter; missing telemetry does not", () => {
  const withUsage = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      channelId: "channel-1",
      contextWindow: {
        usedTokens: 75_000,
        maxTokens: 100_000,
        usedPercentage: 75,
      },
      immersive: true,
      isMember: true,
      isWorking: false,
      target,
      variant: "floating",
    }),
  );
  assert.match(withUsage, /coding-session-context-window/);
  assert.match(withUsage, /aria-label="Context window 75% used"/);

  const withoutUsage = renderToStaticMarkup(
    React.createElement(CodingSessionComposer, {
      canInterrupt: true,
      channelId: "channel-1",
      immersive: true,
      isMember: true,
      isWorking: false,
      target,
      variant: "floating",
    }),
  );
  assert.doesNotMatch(withoutUsage, /coding-session-context-window/);
});

/**
 * A seated execution's reconnect has to carry its identity again.
 *
 * The provider consumes a seat's host-local custody entry when it spawns the
 * adapter, and a resume spawns a *new* adapter under a fresh `commandId`. So a
 * Reconnect that stages nothing is refused `ACTOR_UNAVAILABLE` — permanently,
 * on the very host that holds the key — which would make every seated
 * execution single-use.
 */
test("reconnecting a seated execution stages its key material first", async () => {
  const { JSDOM } = await import("jsdom");
  const dom = new JSDOM("<!doctype html><html><body></body></html>", {
    url: "http://localhost",
  });
  const tauriInternals = {
    invoke: async () => {
      throw new Error("no Tauri IPC in this unit test");
    },
    transformCallback: () => Math.random(),
  };
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    self: dom.window,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
  dom.window.__TAURI_INTERNALS__ = tauriInternals;

  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const actor = "aa".repeat(32);
  const calls = [];

  try {
    await act(async () => {
      render(
        React.createElement(
          QueryClientProvider,
          { client: queryClient },
          React.createElement(CodingSessionComposer, {
            canInterrupt: false,
            channelId: "0c8016c8-9483-4426-a4b1-b45c8e21d0a1",
            isMember: true,
            immersive: true,
            isWorking: false,
            lifecycleStatus: "disconnected",
            providerAuthorityPubkey: "bb".repeat(32),
            seatActorPubkey: actor,
            seatCustody: {
              stageSeat: async (input) => {
                calls.push(["stageSeat", input]);
              },
              clearSeat: async (commandId) => {
                calls.push(["clearSeat", commandId]);
              },
            },
            publishResume: async (input) => {
              calls.push(["publishResume", input.commandId]);
              return { eventId: "event-1", kind: 44221 };
            },
            target,
          }),
        ),
      );
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId("coding-session-composer-resume"));
    });

    assert.equal(calls.length, 2, JSON.stringify(calls));
    assert.equal(calls[0][0], "stageSeat");
    assert.equal(calls[0][1].agentPubkey, actor);
    assert.equal(calls[1][0], "publishResume");
    // One command, one custody entry: the seat is staged under the exact
    // `commandId` the resume is published with, or the provider looks for it
    // under a key nothing wrote.
    assert.equal(calls[0][1].commandId, calls[1][1]);
  } finally {
    cleanup();
    dom.window.close();
  }
});

test("reconnecting an unseated execution stages nothing", async () => {
  const { act, cleanup, fireEvent, render, screen } = await import(
    "@testing-library/react"
  );
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const calls = [];
  try {
    await act(async () => {
      render(
        React.createElement(
          QueryClientProvider,
          { client: queryClient },
          React.createElement(CodingSessionComposer, {
            canInterrupt: false,
            channelId: "0c8016c8-9483-4426-a4b1-b45c8e21d0a1",
            isMember: true,
            immersive: true,
            isWorking: false,
            lifecycleStatus: "disconnected",
            providerAuthorityPubkey: "bb".repeat(32),
            seatCustody: {
              stageSeat: async (input) => {
                calls.push(["stageSeat", input]);
              },
              clearSeat: async (commandId) => {
                calls.push(["clearSeat", commandId]);
              },
            },
            publishResume: async (input) => {
              calls.push(["publishResume", input.commandId]);
              return { eventId: "event-2", kind: 44221 };
            },
            target,
          }),
        ),
      );
    });
    await act(async () => {
      fireEvent.click(screen.getByTestId("coding-session-composer-resume"));
    });
    assert.deepEqual(
      calls.map((call) => call[0]),
      ["publishResume"],
    );
  } finally {
    cleanup();
  }
});
