import assert from "node:assert/strict";
import { after, before, beforeEach, test } from "node:test";

import { JSDOM } from "jsdom";

/**
 * What the draft says about its folder, against a host that answers.
 *
 * The request that opens a reuse draft carries a branch recorded when the
 * worktree was cut, and it is written to `sessionStorage` — so nothing in it
 * may be rendered as a fact about now. The head is therefore read from that
 * exact directory once, when the draft opens, and only that read earns "on
 * disk now". These mount the real component against a stubbed host and check
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
  const { NewCodingSessionWorkspaceDisclosure } = await import(
    "./NewCodingSessionDialog.tsx"
  );

  let mounted = null;
  await act(async () => {
    mounted = render(
      React.createElement(NewCodingSessionWorkspaceDisclosure, {
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
          React.createElement(NewCodingSessionWorkspaceDisclosure, {
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

test("the dialog mounts no disclosure until root's launcher seam lands", async () => {
  const React = (await import("react")).default;
  const { renderToStaticMarkup } = await import("react-dom/server");
  const {
    WORKSPACE_REUSE_SEAM_LANDED,
    newCodingSessionDialogTitle,
    newCodingSessionWorkspaceDisclosure,
  } = await import("./NewCodingSessionDialog.tsx");

  // While the form still starts empty with "Use a worktree" ticked, a block
  // naming one folder over a field about to use another is a draft that lies
  // about what Start will do.
  assert.equal(WORKSPACE_REUSE_SEAM_LANDED, false);
  assert.equal(
    newCodingSessionWorkspaceDisclosure({
      path: PATH,
      branch: RECORDED_BRANCH,
    }),
    null,
  );

  const off = renderToStaticMarkup(
    React.createElement(
      "div",
      null,
      newCodingSessionWorkspaceDisclosure({
        path: PATH,
        branch: RECORDED_BRANCH,
      }),
    ),
  );
  assert.doesNotMatch(off, /coding-session-workspace-reuse/);
  assert.doesNotMatch(off, /New conversation/);

  // Forced on, the same call site renders the disclosure — so the component
  // stays covered while the flag is off, and flipping the constant is the
  // only change needed once root's three edits land.
  const on = renderToStaticMarkup(
    React.createElement(
      "div",
      null,
      newCodingSessionWorkspaceDisclosure(
        { path: PATH, branch: RECORDED_BRANCH },
        true,
      ),
    ),
  );
  assert.match(on, /data-testid="coding-session-workspace-reuse"/);
  assert.match(on, new RegExp(PATH.replace(/\//g, "\\/")));
  assert.match(on, /New conversation; uses these files\./);

  // The heading is the same claim in fewer words, so it moves with the
  // disclosure and not with the prop.
  const reuse = { path: PATH, branch: RECORDED_BRANCH };
  assert.equal(
    newCodingSessionDialogTitle({
      projectContext: null,
      workspaceReuse: reuse,
    }),
    "New coding session",
  );
  assert.equal(
    newCodingSessionDialogTitle({
      projectContext: { projectName: "Buzz Glue" },
      workspaceReuse: reuse,
    }),
    "New coding session in Buzz Glue",
  );
  assert.equal(
    newCodingSessionDialogTitle({
      projectContext: null,
      workspaceReuse: reuse,
      seamLanded: true,
    }),
    "New session in this workspace",
  );
  // A project draft with a seeded workspace is still a workspace draft.
  assert.equal(
    newCodingSessionDialogTitle({
      projectContext: { projectName: "Buzz Glue" },
      workspaceReuse: reuse,
      seamLanded: true,
    }),
    "New session in this workspace",
  );
  // …and an ordinary draft never takes that title, flag or no flag.
  assert.equal(
    newCodingSessionDialogTitle({
      projectContext: null,
      workspaceReuse: null,
      seamLanded: true,
    }),
    "New coding session",
  );
});

test("the seam constant lives where a browser spec can import it", async () => {
  // Importing a UI module into a Playwright spec drags a CSS import through
  // node's transform and kills the run, so the flag lives in the CSS-free lib
  // and is re-exported for the app's own callers.
  const lib = await import(
    "@/features/coding-sessions/lib/codingSessionWorkspaceReuse"
  );
  const ui = await import("./NewCodingSessionDialog.tsx");

  assert.equal(typeof lib.WORKSPACE_REUSE_SEAM_LANDED, "boolean");
  assert.equal(ui.WORKSPACE_REUSE_SEAM_LANDED, lib.WORKSPACE_REUSE_SEAM_LANDED);
});
