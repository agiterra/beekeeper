import assert from "node:assert/strict";
import { after, afterEach, before, mock, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});
const clients = [];
let invoke;

before(() => {
  dom.window.__TAURI_INTERNALS__ = {
    invoke: (command, payload) => invoke(command, payload),
  };
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});
afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
  for (const client of clients.splice(0)) client.clear();
  mock.restoreAll();
});
after(() => dom.window.close());

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}

async function renderActions(refetchRepoState) {
  const React = await import("react");
  const { renderHook } = await import("@testing-library/react");
  const { QueryClient, QueryClientProvider, QueryObserver } = await import(
    "@tanstack/react-query"
  );
  const { toast } = await import("sonner");
  const { useProjectBranchActions } = await import("./branchMutations.ts");
  const success = mock.method(toast, "success", () => {});
  const warning = mock.method(toast, "warning", () => {});
  const client = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity },
      mutations: { gcTime: Infinity },
    },
  });
  clients.push(client);
  const unrelated = deferred();
  const observer = new QueryObserver(client, {
    queryKey: ["project", "repo-id", "issues"],
    queryFn: () => unrelated.promise,
    initialData: [],
    staleTime: Infinity,
  });
  const unsubscribe = observer.subscribe(() => {});
  const calls = [];
  const input = {
    project: { id: "repo-id", cloneUrls: ["https://relay/git/repo"] },
    activeBranch: "feature/work",
    activeBranchCommit: "a".repeat(40),
    activeRemoteBranch: { name: "feature/work", commit: "a".repeat(40) },
    defaultBranch: "main",
    deleteBranchReason: null,
    refetchRepoState,
    rememberBranch: (branch) => calls.push(["remember", branch]),
    forgetBranch: (branch) => calls.push(["forget", branch]),
    selectBranch: (branch) => calls.push(["select", branch]),
  };
  const rendered = renderHook(() => useProjectBranchActions(input), {
    wrapper: ({ children }) =>
      React.createElement(QueryClientProvider, { client }, children),
  });
  return {
    ...rendered,
    calls,
    client,
    unrelated,
    unsubscribe,
    success,
    warning,
  };
}

for (const action of ["create", "delete"]) {
  test(`${action} acknowledges the remote result while project reads remain pending`, {
    timeout: 2000,
  }, async () => {
    const { act } = await import("@testing-library/react");
    const refresh = deferred();
    const refreshCall = mock.fn(() => refresh.promise);
    const native = mock.fn(async () => ({
      branch: "feature/work",
      commit: "a".repeat(40),
      message: `Remote branch ${action} succeeded.`,
    }));
    invoke = native;
    const hook = await renderActions(refreshCall);
    await act(async () => {
      if (action === "create")
        await hook.result.current.handleCreate("feature/work");
      else await hook.result.current.handleDelete();
      await new Promise((resolve) => setTimeout(resolve, 0));
    });
    assert.equal(
      native.mock.calls[0].arguments[0],
      `${action}_project_remote_branch`,
    );
    assert.equal(
      hook.client.getQueryState(["project", "repo-id", "issues"]).fetchStatus,
      "fetching",
    );
    assert.equal(hook.result.current[`${action}Pending`], false);
    assert.equal(refreshCall.mock.callCount(), 1);
    assert.deepEqual(
      hook.calls,
      action === "create"
        ? [
            ["remember", { name: "feature/work", commit: "a".repeat(40) }],
            ["select", "feature/work"],
          ]
        : [
            ["forget", "feature/work"],
            ["select", "main"],
          ],
    );
    assert.equal(
      hook.success.mock.calls[0].arguments[0],
      `Remote branch ${action} succeeded.`,
    );
    await act(async () => {
      // React Query resolves read failures in its result; rejected readers
      // must also stay distinct from the already successful remote mutation.
      if (action === "create")
        refresh.resolve({ error: new Error("relay offline") });
      else refresh.reject(new Error("relay offline"));
      hook.unrelated.resolve([]);
    });
    assert.equal(
      hook.warning.mock.calls[0].arguments[0],
      `Branch ${action === "create" ? "created" : "deleted"}, but repository refresh failed: relay offline`,
    );
    hook.unsubscribe();
  });

  test(`${action} refusal never reports success or changes the branch list`, async () => {
    const { act } = await import("@testing-library/react");
    invoke = async () => {
      throw new Error("Remote branch changed.");
    };
    const refresh = mock.fn(async () => ({ error: null }));
    const hook = await renderActions(refresh);
    await act(async () => {
      await assert.rejects(
        action === "create"
          ? hook.result.current.handleCreate("feature/work")
          : hook.result.current.handleDelete(),
        /Remote branch changed/,
      );
    });
    assert.deepEqual(hook.calls, []);
    assert.equal(refresh.mock.callCount(), 0);
    assert.equal(hook.success.mock.callCount(), 0);
    assert.equal(hook.warning.mock.callCount(), 0);
    hook.unsubscribe();
  });
}
