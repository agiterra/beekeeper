/**
 * Item 167: a write to the workdir store here must invalidate the query key
 * the hire host, the Roles tab redirect and `useRolePacksProject` all read
 * (`["coding-session-workdir-state"]`), so those views pick up the write
 * without their own reload. Live on 2026-09-19: Brian set Pivot Test's
 * repository folder here and the record was correct on disk within a
 * second, but nothing told any query cache the store had changed.
 *
 * `CodingSessionHireHost.tsx` itself no longer reads that cache at all — its
 * `checkoutForHire` now re-reads the native command fresh on every hire
 * (see `CodingSessionHireHost.test.mjs`) — so this file is not proving the
 * hire path directly. It proves the other half of the fix: this screen's
 * writes still keep the cache honest for whoever else renders from it.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

const PROJECT = {
  id: "proj-1",
  dtag: "pivot-test",
  address: "30621:owner:pivot-test",
  name: "Pivot Test",
};

let workdirState = {
  version: 1,
  byProject: {},
  byChannel: {},
  mru: [],
  pending: {},
};

const tauriInternals = {
  invoke(command, args) {
    if (command === "get_coding_session_workdir_state") {
      return Promise.resolve(workdirState);
    }
    if (command === "validate_coding_session_workdir") {
      return Promise.resolve({ exists: true, isDir: true, isAbsolute: true });
    }
    if (command === "set_coding_session_workdir") {
      workdirState = {
        ...workdirState,
        byProject: {
          ...workdirState.byProject,
          [args.key]: { path: args.path, updatedAt: "2026-09-19T10:56:53Z" },
        },
      };
      return Promise.resolve(workdirState);
    }
    if (command === "clear_coding_session_workdir") {
      const next = { ...workdirState.byProject };
      delete next[args.key];
      workdirState = { ...workdirState, byProject: next };
      return Promise.resolve(workdirState);
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

async function mountField() {
  const React = (await import("react")).default;
  const { render } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider } = await import(
    "@tanstack/react-query"
  );
  const { ProjectWorkdirField } = await import(
    "./ProjectSettingsLocalSection.tsx"
  );

  // `gcTime: 0` matches the convention every other interactive-render test
  // in this package uses (`CodingSessionPeoplePopover.test.mjs`,
  // `useCodingSessionMissionSurface.memo.test.mjs`, …): the default 5-minute
  // garbage-collect timer is an unref'd-by-nobody `setTimeout` that keeps
  // this file's isolated test process alive long after both tests pass.
  const client = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  const invalidated = [];
  const originalInvalidate = client.invalidateQueries.bind(client);
  client.invalidateQueries = (options) => {
    invalidated.push(options?.queryKey);
    return originalInvalidate(options);
  };
  // Seed the cache the way the hire host and the Roles tab redirect would
  // have, at the moment this field is mounted, with the *old* answer — the
  // point of the test is that a write here does not leave that seed stale.
  client.setQueryData(["coding-session-workdir-state"], workdirState);

  const view = render(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(ProjectWorkdirField, { project: PROJECT }),
    ),
  );

  return { invalidated, view };
}

test("saving a repository folder invalidates the workdir-state query key", async () => {
  workdirState = {
    version: 1,
    byProject: {},
    byChannel: {},
    mru: [],
    pending: {},
  };
  const { fireEvent, waitFor } = await import("@testing-library/react");
  const { invalidated, view } = await mountField();

  const input = view.getByTestId("project-settings-workdir-input");
  fireEvent.change(input, {
    target: { value: "/Users/brian/Projects/pivot-test/pivot-test" },
  });
  fireEvent.blur(input);

  await waitFor(() => {
    assert.ok(
      invalidated.some(
        (key) =>
          Array.isArray(key) &&
          key.length === 1 &&
          key[0] === "coding-session-workdir-state",
      ),
      'the write never invalidated ["coding-session-workdir-state"], so ' +
        "the hire host's and the Roles tab's cached reads stay stale",
    );
  });
  view.unmount();
});

test("clearing a repository folder invalidates the workdir-state query key", async () => {
  workdirState = {
    version: 1,
    byProject: {
      [PROJECT.address]: {
        path: "/Users/brian/Projects/pivot-test/pivot-test",
        updatedAt: "2026-09-19T10:56:53Z",
      },
    },
    byChannel: {},
    mru: [],
    pending: {},
  };
  const { fireEvent, waitFor } = await import("@testing-library/react");
  const { invalidated, view } = await mountField();

  await waitFor(() => {
    view.getByTestId("project-settings-workdir-clear");
  });
  fireEvent.click(view.getByTestId("project-settings-workdir-clear"));

  await waitFor(() => {
    assert.ok(
      invalidated.some(
        (key) =>
          Array.isArray(key) &&
          key.length === 1 &&
          key[0] === "coding-session-workdir-state",
      ),
      "clearing the folder never invalidated the workdir-state query key",
    );
  });
  view.unmount();
});
