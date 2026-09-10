/**
 * The founding host, mounted under StrictMode, performing a real click.
 *
 * Everything below the store is production code: the request store, the
 * founding sequence, the draft writer. Only the relay (the three publishes),
 * the session-ref mint and the navigation are injected. StrictMode is not
 * decoration — `main.tsx` mounts it, so in dev every effect runs twice, and
 * the one thing this file must prove is that a click still founds exactly
 * once.
 */
import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";
import { toast } from "sonner";

import {
  codingSessionFoundedDraftStorageKey,
  readCodingSessionFoundedDraft,
} from "../lib/codingSessionFoundedDraft.ts";
import { foundCodingSessionTopic } from "../lib/codingSessionTopicFounding.ts";
import {
  clearCodingSessionFoundingRequest,
  requestCodingSessionFounding,
  requestCodingSessionFoundingInWorkspace,
} from "../newCodingSessionDialogStore.ts";
import {
  CODING_SESSION_FOUNDING_NO_DESTINATION_SENTENCE,
  CodingSessionFoundingRunner,
  resolveCodingSessionFoundingRoute,
} from "./CodingSessionFoundingHost.tsx";

const dom = new JSDOM(
  "<!doctype html><html><body><div id='root'></div></body></html>",
  { url: "http://localhost" },
);

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    // sonner's dismiss schedules its subscribers on a frame.
    requestAnimationFrame: (callback) => setTimeout(callback, 0),
    window: dom.window,
  });
});

afterEach(() => {
  clearCodingSessionFoundingRequest();
  dom.window.localStorage.clear();
});

after(() => dom.window.close());

const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const SESSION_REF = "11111111-1111-4111-8111-111111111111";
const SOURCE_SESSION_REF = "22222222-2222-4222-8222-222222222222";
const SOURCE_REPO_REF = `30617:${"b".repeat(64)}:repo-b`;
const PATH = "/Users/x/Code/repo-wt-a";

/** The relay, recorded. The founding sequence itself is the real one. */
function fakeDeps(overrides = {}) {
  const calls = [];
  const deps = {
    runFounding: foundCodingSessionTopic,
    newSessionRef: () => SESSION_REF,
    publishGenesis: async (input) => {
      calls.push(["genesis", input]);
      return { eventId: "genesis-event" };
    },
    publishGoal: async (input) => {
      calls.push(["goal", input]);
    },
    publishName: async (input) => {
      calls.push(["name", input]);
    },
    ...overrides,
  };
  return { deps, calls };
}

async function mount(deps) {
  const navigations = [];
  const root = createRoot(dom.window.document.getElementById("root"));
  await act(async () => {
    root.render(
      React.createElement(
        React.StrictMode,
        null,
        React.createElement(CodingSessionFoundingRunner, {
          deps,
          goFoundedCodingSession: (channelId, sessionRef) => {
            navigations.push([channelId, sessionRef]);
          },
        }),
      ),
    );
  });
  return {
    navigations,
    /** Let the founding's promise chain run to the end. */
    settle: async () => {
      for (let i = 0; i < 20; i += 1) {
        await act(async () => {
          await new Promise((resolve) => setTimeout(resolve, 0));
        });
      }
    },
    unmount: async () => {
      await act(async () => root.unmount());
    },
  };
}

/** Toasts raised since `mark`, by type and title. */
function toastsSince(mark) {
  return toast
    .getToasts()
    .slice(mark)
    .map((entry) => [entry.type ?? "default", String(entry.title ?? "")]);
}

test("a channel request founds exactly once, writes the draft, navigates, clears", async () => {
  const { deps, calls } = fakeDeps();
  const view = await mount(deps);
  const toastMark = toast.getToasts().length;
  await act(async () => {
    requestCodingSessionFounding(CHANNEL_ID);
  });
  await view.settle();

  // One 44226, and nothing else on the wire: the goal and the name are the
  // page's to publish.
  assert.deepEqual(
    calls.map(([name]) => name),
    ["genesis"],
  );
  assert.deepEqual(calls[0][1], {
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
  });
  assert.deepEqual(view.navigations, [[CHANNEL_ID, SESSION_REF]]);
  assert.deepEqual(readCodingSessionFoundedDraft(SESSION_REF), {
    name: null,
    workdir: null,
    useWorktree: true,
    worktreeName: null,
    worktreeSource: null,
    rememberWorkspace: true,
    repoRef: null,
    workspaceSourcePath: null,
    workspaceSourceBranch: null,
    workspaceSourceBranchSource: null,
  });
  assert.ok(
    dom.window.localStorage.getItem(
      codingSessionFoundedDraftStorageKey(SESSION_REF),
    ),
    "written under the v2 key",
  );
  // The request is cleared, so the next click is admitted again…
  const before = calls.length;
  await act(async () => {
    requestCodingSessionFounding(CHANNEL_ID);
  });
  await view.settle();
  assert.equal(calls.length, before + 1);
  assert.equal(view.navigations.length, 2);
  // …and no error was raised for either.
  assert.deepEqual(
    toastsSince(toastMark).filter(([type]) => type === "error"),
    [],
  );
  await view.unmount();
});

test("a failed founding toasts the publisher's words, clears the request, navigates nowhere", async () => {
  const { deps, calls } = fakeDeps({
    publishGenesis: async (input) => {
      calls.push(["genesis", input]);
      throw new Error("auth-required: not a member");
    },
  });
  const view = await mount(deps);
  const toastMark = toast.getToasts().length;
  await act(async () => {
    requestCodingSessionFounding(CHANNEL_ID);
  });
  await view.settle();

  assert.equal(calls.length, 1, "one attempt, not a retry per effect run");
  assert.deepEqual(view.navigations, []);
  assert.equal(readCodingSessionFoundedDraft(SESSION_REF), null);
  assert.deepEqual(
    toastsSince(toastMark).filter(([type]) => type === "error"),
    [["error", "auth-required: not a member"]],
  );
  // Cleared: a retry click is admitted.
  await act(async () => {
    requestCodingSessionFounding(CHANNEL_ID);
  });
  await view.settle();
  assert.equal(calls.length, 2);
  await view.unmount();
});

test("a workspace request founds in its channel and the draft carries the reuse", async () => {
  const { deps, calls } = fakeDeps();
  const view = await mount(deps);
  await act(async () => {
    requestCodingSessionFoundingInWorkspace({
      channelId: CHANNEL_ID,
      projectId: null,
      sessionRef: SOURCE_SESSION_REF,
      sourceRepoRef: SOURCE_REPO_REF,
      workspace: { path: PATH, branch: "wt-a", branchSource: "recorded" },
    });
  });
  await view.settle();

  assert.deepEqual(
    calls.map(([name]) => name),
    ["genesis"],
  );
  assert.deepEqual(view.navigations, [[CHANNEL_ID, SESSION_REF]]);
  assert.deepEqual(readCodingSessionFoundedDraft(SESSION_REF), {
    name: null,
    workdir: PATH,
    useWorktree: false,
    worktreeName: null,
    worktreeSource: null,
    rememberWorkspace: false,
    repoRef: SOURCE_REPO_REF,
    workspaceSourcePath: PATH,
    workspaceSourceBranch: "wt-a",
    workspaceSourceBranchSource: "recorded",
  });
  await view.unmount();
});

test("a workspace request with an unknown source repository carries null, never a guess", async () => {
  const { deps } = fakeDeps();
  const view = await mount(deps);
  await act(async () => {
    requestCodingSessionFoundingInWorkspace({
      channelId: CHANNEL_ID,
      sessionRef: SOURCE_SESSION_REF,
      sourceRepoRef: null,
      workspace: { path: PATH, branch: null },
    });
  });
  await view.settle();
  const draft = readCodingSessionFoundedDraft(SESSION_REF);
  assert.equal(draft.repoRef, null);
  assert.equal(draft.workspaceSourceBranch, null);
  assert.equal(draft.workspaceSourceBranchSource, null);
  await view.unmount();
});

test("a request with no channel and no project founds nothing and says so", async () => {
  const { deps, calls } = fakeDeps();
  const view = await mount(deps);
  const toastMark = toast.getToasts().length;
  await act(async () => {
    requestCodingSessionFounding(null);
  });
  await view.settle();
  assert.deepEqual(calls, []);
  assert.deepEqual(view.navigations, []);
  // Said once, StrictMode notwithstanding.
  assert.deepEqual(toastsSince(toastMark), [
    ["info", CODING_SESSION_FOUNDING_NO_DESTINATION_SENTENCE],
  ]);
  // And cleared: a real click afterwards is founded.
  await act(async () => {
    requestCodingSessionFounding(CHANNEL_ID);
  });
  await view.settle();
  assert.equal(calls.length, 1);
  await view.unmount();
});

test("routing: a workspace request with a channel is founded there; with only a project it goes through the project founder", () => {
  const withChannel = resolveCodingSessionFoundingRoute({
    kind: "workspace",
    phase: "requested",
    channelId: CHANNEL_ID,
    projectId: "project-alpha",
    sessionRef: SOURCE_SESSION_REF,
    sourceRepoRef: SOURCE_REPO_REF,
    workspace: { path: PATH, branch: "wt-a", branchSource: "recorded" },
  });
  assert.equal(withChannel.kind, "channel");
  assert.equal(withChannel.target.channelId, CHANNEL_ID);
  assert.equal(withChannel.target.repoRef, SOURCE_REPO_REF);
  assert.equal(withChannel.target.workspace.path, PATH);

  const projectOnly = resolveCodingSessionFoundingRoute({
    kind: "workspace",
    phase: "requested",
    channelId: null,
    projectId: "project-alpha",
    sessionRef: SOURCE_SESSION_REF,
    sourceRepoRef: SOURCE_REPO_REF,
    workspace: { path: PATH, branch: "wt-a", branchSource: null },
  });
  assert.deepEqual(projectOnly, {
    kind: "project",
    projectId: "project-alpha",
    sourceRepoRef: SOURCE_REPO_REF,
    workspaceReuse: { path: PATH, branch: "wt-a", branchSource: null },
  });

  assert.deepEqual(
    resolveCodingSessionFoundingRoute({
      kind: "project",
      phase: "requested",
      projectId: "project-alpha",
    }),
    {
      kind: "project",
      projectId: "project-alpha",
      sourceRepoRef: null,
      workspaceReuse: null,
    },
  );
  assert.deepEqual(
    resolveCodingSessionFoundingRoute({
      kind: "workspace",
      phase: "requested",
      channelId: null,
      projectId: null,
      sessionRef: SOURCE_SESSION_REF,
      workspace: { path: PATH, branch: null },
    }),
    { kind: "nowhere" },
  );
});
