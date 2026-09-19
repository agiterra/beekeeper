import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * The seeded directory must survive the project's own remembered checkout.
 *
 * A "New session in this workspace" draft seeds the form's directory from the
 * session's verified workspace, and this field then runs its prefill effect
 * against the *project's* remembered directory — which outranks everything
 * else in `preferredWorkdirPrefill`. The only thing standing between the two
 * is the effect's early return for a value it did not auto-fill itself
 * (`NewCodingSessionWorkdirField.tsx`, the `current !== autoFilledRef.current`
 * guard). Lose that guard and the feature breaks silently: the draft opens on
 * the project's canonical checkout under a title that says "this workspace".
 *
 * The second test is the control. Without it this file would still pass if the
 * effect never ran at all, which would prove nothing.
 */

const PROJECT_KEY = "30621:owner:buzz-glue";
const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const PROJECT_CHECKOUT = "/Users/x/Code/repo";
const SEEDED_WORKSPACE = "/Users/x/Code/repo-wt-a";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

const tauriInternals = {
  invoke(command) {
    if (command === "get_coding_session_workdir_state") {
      return Promise.resolve({
        version: 1,
        byProject: {
          [PROJECT_KEY]: { path: PROJECT_CHECKOUT, updatedAt: "2026-09-09" },
        },
        byChannel: {
          [CHANNEL_ID]: {
            path: "/Users/x/Code/other",
            updatedAt: "2026-09-09",
          },
        },
        mru: [{ path: "/Users/x/Code/elsewhere", lastUsedAt: "2026-09-09" }],
        pending: {},
      });
    }
    if (command === "validate_coding_session_workdir") {
      return Promise.resolve({ exists: true, isDir: true, isAbsolute: true });
    }
    return Promise.reject(new Error(`unmocked: ${command}`));
  },
  transformCallback: () => Math.random(),
};

before(() => {
  dom.window.__TAURI_INTERNALS__ = tauriInternals;
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
    __TAURI_INTERNALS__: tauriInternals,
  });
});

after(() => dom.window.close());

async function mountField(value) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { NewCodingSessionWorkdirField } = await import(
    "./NewCodingSessionWorkdirField.tsx"
  );

  const changes = [];
  let mounted = null;
  await act(async () => {
    mounted = render(
      React.createElement(NewCodingSessionWorkdirField, {
        channelId: CHANNEL_ID,
        // The project's local checkout, exactly as the project wrapper
        // supplies it.
        fallbackPath: PROJECT_CHECKOUT,
        onChange: (next) => changes.push(next),
        projectKey: PROJECT_KEY,
        value,
      }),
    );
  });
  // Let the workdir-state read resolve and the prefill effect run.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return { changes, unmount: () => mounted.unmount() };
}

test("a project session prefills the project's recorded folder and nothing else", async () => {
  const { preferredWorkdirPrefill } = await import(
    "./NewCodingSessionWorkdirField.tsx"
  );
  const state = {
    byProject: {
      [PROJECT_KEY]: { path: PROJECT_CHECKOUT, updatedAt: "2026-09-19" },
    },
    byChannel: {
      [CHANNEL_ID]: { path: "/Users/x/Code/other", updatedAt: "2026-09-19" },
    },
    mru: [{ path: "/Users/x/Code/tankloop", lastUsedAt: "2026-09-19" }],
  };
  assert.equal(
    preferredWorkdirPrefill({
      channelId: CHANNEL_ID,
      fallbackPath: "/Users/x/Code/scanned",
      projectKey: PROJECT_KEY,
      state,
    }),
    PROJECT_CHECKOUT,
  );
  // No folder recorded for the project: an empty field, not the channel's
  // folder, not the caller's guess, not the MRU — which on 2026-09-19 was
  // TankLoop's checkout and became the RPG Test lead's working tree.
  assert.equal(
    preferredWorkdirPrefill({
      channelId: CHANNEL_ID,
      fallbackPath: "/Users/x/Code/scanned",
      projectKey: "30621:owner:rpg-test",
      state,
    }),
    "",
  );
  // Without a project the old order stands.
  assert.equal(
    preferredWorkdirPrefill({
      channelId: null,
      fallbackPath: null,
      projectKey: null,
      state,
    }),
    "/Users/x/Code/tankloop",
  );
});

test("a seeded workspace is never overwritten by the project's remembered checkout", async () => {
  const { changes, unmount } = await mountField(SEEDED_WORKSPACE);

  assert.deepEqual(
    changes,
    [],
    "the field asked to change a directory the caller had already chosen",
  );

  unmount();
});

test("an empty field still prefills from the project — the guard is what differs", async () => {
  const { changes, unmount } = await mountField("");

  assert.deepEqual(changes, [PROJECT_CHECKOUT]);

  unmount();
});
