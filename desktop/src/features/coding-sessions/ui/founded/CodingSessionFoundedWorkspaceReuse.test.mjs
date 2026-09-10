import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * What the founded page says about its reused folder, against a host that
 * answers.
 *
 * The founded draft carries a branch recorded when the worktree was cut, and
 * it was written to `localStorage` at the click — so nothing in it may be
 * rendered as a fact about now. The head is therefore read from that exact
 * directory once, when the page opens, and only that read earns "on disk
 * now". These mount the real component against a stubbed host and check
 * what a person would actually see.
 */

const PATH = "/Users/x/Code/repo-wt-a";
const RECORDED_BRANCH = "wt-a";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

/** Every host call the mounted draft made, and what it was told to answer. */
let calls = [];
let answers = {};

const tauriInternals = {
  invoke(command, args) {
    calls.push(command);
    const answer = answers[command];
    if (!answer) return Promise.reject(new Error(`unmocked: ${command}`));
    return answer(args);
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

beforeEach(() => {
  calls = [];
  answers = {};
});

after(() => dom.window.close());

const onDisk = () =>
  Promise.resolve({ exists: true, isDir: true, isAbsolute: true });

async function mountDraft(workspaceReuse) {
  const React = (await import("react")).default;
  const { act, render } = await import("@testing-library/react");
  const { CodingSessionFoundedWorkspaceReuse } = await import(
    "./CodingSessionFoundedWorkspaceReuse.tsx"
  );

  let mounted = null;
  await act(async () => {
    mounted = render(
      React.createElement(CodingSessionFoundedWorkspaceReuse, {
        workspaceReuse,
      }),
    );
  });
  // Let the one directory read resolve.
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 0));
  });
  return {
    rerender: async () => {
      await act(async () => {
        mounted.rerender(
          React.createElement(CodingSessionFoundedWorkspaceReuse, {
            workspaceReuse,
          }),
        );
      });
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, 0));
      });
    },
    text: () => mounted.container.textContent ?? "",
    unmount: () => mounted.unmount(),
    within: (testid) =>
      mounted.container.querySelector(`[data-testid="${testid}"]`),
  };
}

test("a head read off that folder is shown as the branch on disk now", async () => {
  answers = {
    validate_coding_session_workdir: onDisk,
    list_coding_session_worktree_branches: () =>
      Promise.resolve({
        branches: ["wt-a-fixups"],
        defaultBranch: "main",
        headBranch: "wt-a-fixups",
      }),
  };

  const draft = await mountDraft({ path: PATH, branch: RECORDED_BRANCH });

  assert.ok(draft.within("coding-session-workspace-reuse"));
  assert.match(draft.text(), new RegExp(PATH.replace(/\//g, "\\/")));
  assert.match(draft.text(), /wt-a-fixups · on disk now/);
  // The stale recorded branch is not shown beside the live one.
  assert.doesNotMatch(draft.text(), /wt-a · /);
  assert.match(draft.text(), /New conversation; uses these files\./);

  draft.unmount();
});

test("a branch read that throws falls back to the recorded branch, unattributed", async () => {
  answers = {
    validate_coding_session_workdir: onDisk,
    list_coding_session_worktree_branches: () =>
      Promise.reject(new Error("not a git checkout")),
  };

  const draft = await mountDraft({ path: PATH, branch: RECORDED_BRANCH });

  assert.ok(draft.within("coding-session-workspace-reuse"));
  assert.match(draft.text(), /wt-a · source not known here/);
  assert.doesNotMatch(draft.text(), /on disk now/);
  // The folder is still there, so the draft still offers it.
  assert.equal(draft.within("coding-session-workspace-reuse-gone"), null);

  draft.unmount();
});

test("a folder that is gone says so and shows no branch at all", async () => {
  answers = {
    validate_coding_session_workdir: () =>
      Promise.resolve({ exists: false, isDir: false, isAbsolute: true }),
  };

  const draft = await mountDraft({ path: PATH, branch: RECORDED_BRANCH });

  assert.ok(draft.within("coding-session-workspace-reuse-gone"));
  assert.equal(draft.within("coding-session-workspace-reuse"), null);
  assert.match(draft.text(), /No such directory on this computer\./);
  assert.match(
    draft.text(),
    /Pick a folder in the draft to start somewhere else on this computer\./,
  );
  // The path is still named — the person is owed what went missing — but no
  // branch line stands over a folder that is not there.
  assert.match(draft.text(), new RegExp(PATH.replace(/\//g, "\\/")));
  assert.equal(draft.within("coding-session-workspace-reuse-branch"), null);
  // No branch wording of any kind — including the "no branch recorded" line,
  // which would still be a statement about a folder that is not there.
  assert.doesNotMatch(
    draft.text(),
    /on disk now|recorded at creation|source not known here|No branch recorded/,
  );
  // And nothing was spent asking a missing directory for its branches.
  assert.equal(calls.includes("list_coding_session_worktree_branches"), false);

  draft.unmount();
});

test("a validation that throws is not read as a missing folder", async () => {
  answers = {
    validate_coding_session_workdir: () => Promise.reject(new Error("no host")),
  };

  const draft = await mountDraft({ path: PATH, branch: RECORDED_BRANCH });

  assert.equal(draft.within("coding-session-workspace-reuse-gone"), null);
  assert.match(draft.text(), /wt-a · source not known here/);

  draft.unmount();
});

test("the folder is read once per open, not once per render", async () => {
  answers = {
    validate_coding_session_workdir: onDisk,
    list_coding_session_worktree_branches: () =>
      Promise.resolve({
        branches: [],
        defaultBranch: null,
        headBranch: "wt-a-fixups",
      }),
  };

  const draft = await mountDraft({ path: PATH, branch: RECORDED_BRANCH });
  const afterOpen = [...calls];
  assert.deepEqual(afterOpen, [
    "validate_coding_session_workdir",
    "list_coding_session_worktree_branches",
  ]);

  // A form that re-renders (a keystroke, a query settling) must not re-stat
  // the disk. Three more renders, same two calls.
  await draft.rerender();
  await draft.rerender();
  await draft.rerender();

  assert.deepEqual(calls, afterOpen);
  assert.match(draft.text(), /wt-a-fixups · on disk now/);

  draft.unmount();
});

test("an ordinary draft reads nothing and renders nothing", async () => {
  const draft = await mountDraft(null);

  assert.deepEqual(calls, []);
  assert.equal(draft.text(), "");

  draft.unmount();
});

test("a branch recorded at creation is shown as that, without a live read", async () => {
  answers = {
    validate_coding_session_workdir: onDisk,
    list_coding_session_worktree_branches: () =>
      Promise.reject(new Error("not a git checkout")),
  };

  const draft = await mountDraft({
    path: PATH,
    branch: RECORDED_BRANCH,
    branchSource: "recorded",
  });

  // The head could not be read, but the branch the worktree was cut on is a
  // creation-time fact and does not go stale.
  assert.match(draft.text(), /wt-a · recorded at creation/);
  assert.doesNotMatch(draft.text(), /on disk now|source not known here/);

  draft.unmount();
});

test("a live head still outranks a recorded branch in the draft", async () => {
  answers = {
    validate_coding_session_workdir: onDisk,
    list_coding_session_worktree_branches: () =>
      Promise.resolve({
        branches: [],
        defaultBranch: null,
        headBranch: "wt-a-fixups",
      }),
  };

  const draft = await mountDraft({
    path: PATH,
    branch: RECORDED_BRANCH,
    branchSource: "recorded",
  });

  assert.match(draft.text(), /wt-a-fixups · on disk now/);
  assert.doesNotMatch(draft.text(), /recorded at creation/);

  draft.unmount();
});

test("a request that recorded no provenance still says so, not 'recorded'", async () => {
  answers = {
    validate_coding_session_workdir: onDisk,
    list_coding_session_worktree_branches: () =>
      Promise.reject(new Error("not a git checkout")),
  };

  const draft = await mountDraft({ path: PATH, branch: RECORDED_BRANCH });

  assert.match(draft.text(), /wt-a · source not known here/);
  assert.doesNotMatch(draft.text(), /recorded at creation/);

  draft.unmount();
});

test("the page renders the disclosure only once the seam has landed", async () => {
  const React = (await import("react")).default;
  const { renderToStaticMarkup } = await import("react-dom/server");
  const { codingSessionFoundedWorkspaceReuse } = await import(
    "./CodingSessionFoundedWorkspaceReuse.tsx"
  );
  const { WORKSPACE_REUSE_SEAM_LANDED } = await import(
    "@/features/coding-sessions/lib/codingSessionWorkspaceReuse"
  );

  assert.equal(WORKSPACE_REUSE_SEAM_LANDED, true);
  assert.equal(
    codingSessionFoundedWorkspaceReuse(
      { path: PATH, branch: RECORDED_BRANCH },
      false,
    ),
    null,
  );

  const off = renderToStaticMarkup(
    React.createElement(
      "div",
      null,
      codingSessionFoundedWorkspaceReuse(
        { path: PATH, branch: RECORDED_BRANCH },
        false,
      ),
    ),
  );
  assert.doesNotMatch(off, /coding-session-workspace-reuse/);
  assert.doesNotMatch(off, /New conversation/);

  const on = renderToStaticMarkup(
    React.createElement(
      "div",
      null,
      codingSessionFoundedWorkspaceReuse(
        { path: PATH, branch: RECORDED_BRANCH },
        WORKSPACE_REUSE_SEAM_LANDED,
      ),
    ),
  );
  assert.match(on, /data-testid="coding-session-workspace-reuse"/);
  assert.match(on, new RegExp(PATH.replace(/\//g, "\\/")));
  assert.match(on, /New conversation; uses these files\./);

  // And a draft with no reused workspace renders nothing, flag or no flag.
  assert.equal(
    codingSessionFoundedWorkspaceReuse(null, WORKSPACE_REUSE_SEAM_LANDED),
    null,
  );
});
