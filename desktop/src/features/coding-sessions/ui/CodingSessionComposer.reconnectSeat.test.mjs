import assert from "node:assert/strict";
import test from "node:test";
import React from "react";

import { CodingSessionComposer } from "./CodingSessionComposer.tsx";

const target = {
  driver: "provider-neutral",
  instanceId: "instance-1",
  sessionId: "session-74495ca8",
  generation: 2,
};

/**
 * Ledger 187, as the composer's half of it.
 *
 * The lead of the kettle session held a `write` grant on the project's agents
 * repository, and after a relaunch its Reconnect answered: *"this role is
 * granted the agents repository (Write) but the seat has no worktree to put a
 * clone beside, or no project source to clone"*. The host had the path — it
 * wrote it down under this execution's provider session id when it cut the
 * tree — and the composer was sending nothing it could look it up by.
 *
 * So this pins the fact, not the prose: the reconnect's staging call carries
 * the execution's `sessionId`. Asserted through the real
 * `buildCodingSessionResumeInput` mapping and the real publisher, with only
 * the custody seam and the publish stubbed, so a mapping that drops the field
 * on the way fails here.
 */
test("the composer's reconnect hands staging the execution's session id", async () => {
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
  const projectRef = `30621:${"bb".repeat(32)}:pivot-test`;
  const calls = [];

  try {
    await act(async () => {
      render(
        React.createElement(
          QueryClientProvider,
          { client: queryClient },
          React.createElement(CodingSessionComposer, {
            canInterrupt: false,
            channelId: "85b8db75-9483-4426-a4b1-b45c8e21d0a1",
            channelAccess: { kind: "member" },
            immersive: true,
            isWorking: false,
            lifecycleStatus: "disconnected",
            providerAuthorityPubkey: "cc".repeat(32),
            seatActorPubkey: actor,
            seatRole: "lead",
            projectRef,
            seatCustody: {
              stageSeat: async (input) => {
                calls.push(["stageSeat", input]);
              },
              clearSeat: async (commandId) => {
                calls.push(["clearSeat", commandId]);
              },
              fetchPackSource: async (ref) => {
                calls.push(["fetchPackSource", ref]);
                return { repo: "30617:owner:pivot-test-beekeeper-agents" };
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

    const staged = calls.find(([name]) => name === "stageSeat")?.[1];
    assert.ok(staged, `no staging call: ${JSON.stringify(calls)}`);
    assert.equal(staged.sessionId, target.sessionId);
    // The rest of the seat's facts still travel; this is an addition, not a
    // replacement for the role and project the reconnect already carried.
    assert.equal(staged.agentPubkey, actor);
    assert.equal(staged.role, "lead");
    assert.deepEqual(staged.packSource, {
      repo: "30617:owner:pivot-test-beekeeper-agents",
    });
    // A reconnect does not know the seat's worktree and must not invent one:
    // the host resolves it from its own record by the id above.
    assert.equal(staged.worktree, undefined);
  } finally {
    cleanup();
    dom.window.close();
  }
});
