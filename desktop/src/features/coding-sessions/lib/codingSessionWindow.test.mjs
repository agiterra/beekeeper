import assert from "node:assert/strict";
import test from "node:test";

import { buildCodingSessionWindowLabel } from "./codingSessionRoute.ts";
import { openCodingSessionPopout } from "./codingSessionWindow.ts";
import { rememberCodingSessionPopoutBootstrap } from "./codingSessionBootstrap.ts";

function windowHandle(label, calls) {
  return {
    label,
    async setFocus() {
      calls.push("focus");
    },
    async show() {
      calls.push("show");
    },
    async unminimize() {
      calls.push("unminimize");
    },
  };
}

test("popout focuses the deterministic existing window", async () => {
  const calls = [];
  const label = buildCodingSessionWindowLabel("channel-1", "generation-1");
  const result = await openCodingSessionPopout("channel-1", "generation-1", {
    async getByLabel(actualLabel) {
      assert.equal(actualLabel, label);
      return windowHandle(actualLabel, calls);
    },
    create() {
      throw new Error("existing window must not be recreated");
    },
  });

  assert.deepEqual(result, { created: false, label });
  assert.deepEqual(calls, ["unminimize", "show", "focus"]);
});

test("popout creates the exact route once and focuses it", async () => {
  const calls = [];
  let createdOptions;
  const result = await openCodingSessionPopout("channel/a", "generation|1", {
    async getByLabel() {
      return null;
    },
    create(label, options) {
      createdOptions = options;
      return {
        ...windowHandle(label, calls),
        async once(event, handler) {
          if (event === "tauri://created") {
            queueMicrotask(() => handler({ payload: null }));
          }
          return () => {};
        },
      };
    },
  });

  assert.equal(result.created, true);
  assert.equal(
    createdOptions.url,
    "/#/coding-sessions/channel%2Fa/generation%7C1?surface=popout",
  );
  assert.equal(createdOptions.title, "Coding session");
  assert.deepEqual(calls, ["unminimize", "show", "focus"]);
});

test("fresh popout stages the accepted exact generation before creating", async () => {
  const calls = [];
  const relayEvent = {
    id: "signed-event",
    pubkey: "a".repeat(64),
    created_at: 1,
    kind: 9,
    tags: [],
    content: "{}",
    sig: "b".repeat(128),
  };
  rememberCodingSessionPopoutBootstrap({
    channelId: "channel-stage",
    generationId: "generation-2",
    authorityIdentity: "authority-1",
    relayEvents: [relayEvent],
  });

  let staged;
  await openCodingSessionPopout("channel-stage", "generation-2", {
    async getByLabel() {
      return null;
    },
    async stageBootstrap(label, bootstrap) {
      calls.push("stage");
      staged = { label, bootstrap };
    },
    create(label) {
      calls.push("create");
      return {
        ...windowHandle(label, calls),
        async once(event, handler) {
          if (event === "tauri://created") {
            queueMicrotask(() => handler({ payload: null }));
          }
          return () => {};
        },
      };
    },
  });

  assert.equal(staged.bootstrap.generationId, "generation-2");
  assert.deepEqual(staged.bootstrap.relayEvents, [relayEvent]);
  assert.deepEqual(calls.slice(0, 2), ["stage", "create"]);
});
