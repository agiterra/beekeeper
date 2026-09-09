import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";

import { workspaceReuseRepoRef } from "../lib/codingSessionWorkspaceReuse.ts";
import { selectLaunchRepoRef } from "../lib/codingSessionLaunchRepoRef.ts";
import { resolveNewCodingSessionDialogRoute } from "../ui/NewCodingSessionDialogHost.tsx";
import { useNewSessionInWorkspaceAction } from "./useNewSessionInWorkspaceAction.ts";
import {
  closeNewCodingSessionDialog,
  parseNewCodingSessionRequest,
  useNewCodingSessionRequest,
} from "../newCodingSessionDialogStore.ts";

/**
 * What the menu item does when it is chosen, in the three states that decide
 * it: the execution runs on this computer's provider, on somebody else's, or
 * on a provider nobody here can name.
 *
 * These are behaviour assertions against the store the launcher reads — what
 * draft opened, seeded with what — not an inspection of the wiring. The one
 * thing that must never happen is a draft seeded with a directory this
 * computer could not verify, or a foreign execution's answer dressed as a
 * local one.
 */

const dom = new JSDOM(
  "<!doctype html><html><body><div id='root'></div></body></html>",
  { url: "http://localhost" },
);

const originalDocument = globalThis.document;
const originalWindow = globalThis.window;
const originalHTMLElement = globalThis.HTMLElement;

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

afterEach(() => {
  closeNewCodingSessionDialog();
});

after(() => {
  if (originalDocument === undefined) delete globalThis.document;
  else globalThis.document = originalDocument;
  if (originalWindow === undefined) delete globalThis.window;
  else globalThis.window = originalWindow;
  if (originalHTMLElement === undefined) delete globalThis.HTMLElement;
  else globalThis.HTMLElement = originalHTMLElement;
  dom.window.close();
});

const SESSION = "9f2c1d40-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const CHANNEL = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const PROJECT = "project-alpha";
const LOCAL_PROVIDER = "a".repeat(64);
const FOREIGN_PROVIDER = "b".repeat(64);
const PATH = "/Users/x/Code/repo-wt-a";

function row(overrides = {}) {
  return {
    key: `${SESSION}/lead`,
    sessionRef: SESSION,
    seatLabel: "lead",
    path: PATH,
    branch: "wt-a",
    repoRoot: "/Users/x/Code/repo",
    disposition: "held",
    dirtyFiles: 0,
    reclaimableBytes: null,
    reclaimableLabel: "unknown",
    reclaimableNow: false,
    graceRemainingSecs: null,
    exists: true,
    tipOnRelayKnown: true,
    detail: "Held.",
    ...overrides,
  };
}

function deps(behaviour = {}) {
  const calls = [];
  const answers = {
    listSeatWorktrees: async () => [row()],
    validateWorkdir: async () => ({ exists: true, isDir: true }),
    listBranches: async () => ({ headBranch: "wt-a-live" }),
    providerStatus: async () => ({ providerPubkey: LOCAL_PROVIDER }),
    ...behaviour,
  };
  return {
    calls,
    deps: {
      listSeatWorktrees: (sessions) => {
        calls.push("listSeatWorktrees");
        return answers.listSeatWorktrees(sessions);
      },
      validateWorkdir: (path) => {
        calls.push("validateWorkdir");
        return answers.validateWorkdir(path);
      },
      listBranches: (input) => {
        calls.push("listBranches");
        return answers.listBranches(input);
      },
      providerStatus: () => {
        calls.push("providerStatus");
        return answers.providerStatus();
      },
    },
  };
}

async function mount(props) {
  const seen = { action: null, request: null };
  function Probe() {
    seen.action = useNewSessionInWorkspaceAction(props);
    seen.request = useNewCodingSessionRequest();
    return React.createElement("div", null, seen.action.detail);
  }
  const host = dom.window.document.getElementById("root");
  const root = createRoot(host);
  await act(async () => {
    root.render(React.createElement(Probe));
  });
  return {
    detail: () => host.textContent,
    request: () => seen.request,
    resolveNow: async () => {
      await act(async () => {
        seen.action.resolveNow();
      });
    },
    start: async () => {
      await act(async () => {
        seen.action.start();
      });
    },
    unmount: async () => {
      await act(async () => root.unmount());
    },
  };
}

test("nothing is read until the menu asks", async () => {
  // A sidebar draws these rows on every scroll tick. A read per render would
  // be a filesystem walk per tick.
  const host = deps();
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    executionProviderPubkey: LOCAL_PROVIDER,
    sessionRef: SESSION,
  });
  assert.deepEqual(host.calls, []);
  assert.equal(
    view.detail(),
    "Opens a draft. This computer looks up the session's folder first.",
  );
  await view.resolveNow();
  assert.ok(host.calls.includes("listSeatWorktrees"));
  assert.equal(view.detail(), "repo-wt-a · wt-a-live");
  assert.equal(view.request(), null, "a menu opening opens no dialog");
  await view.unmount();
});

test("local execution: the draft opens seeded on that exact directory", async () => {
  const host = deps();
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    executionProviderPubkey: LOCAL_PROVIDER,
    projectId: PROJECT,
    sessionRef: SESSION,
  });
  await view.start();
  assert.deepEqual(view.request(), {
    kind: "workspace",
    channelId: CHANNEL,
    projectId: PROJECT,
    sessionRef: SESSION,
    sourceRepoRef: null,
    // The head read off disk right now. It travels as the branch, but never
    // as a *provenance* the draft could still be showing after a reload.
    workspace: { path: PATH, branch: "wt-a-live", branchSource: null },
  });
  await view.unmount();
});

test("only a recorded branch carries its provenance into the draft", async () => {
  // Cut-at-creation is a fact that cannot go stale, so it is labelled.
  const recorded = deps({
    listBranches: async () => ({ headBranch: null }),
  });
  const view = await mount({
    channelId: CHANNEL,
    deps: recorded.deps,
    executionProviderPubkey: LOCAL_PROVIDER,
    projectId: PROJECT,
    sessionRef: SESSION,
  });
  await view.start();
  assert.deepEqual(view.request().workspace, {
    path: PATH,
    branch: "wt-a",
    branchSource: "recorded",
  });
  await view.unmount();

  // A live head is a reading of one moment. It is never relabelled
  // "recorded", and never persisted as "live" either — the draft re-reads it.
  const live = await mount({
    channelId: CHANNEL,
    deps: deps().deps,
    executionProviderPubkey: LOCAL_PROVIDER,
    sessionRef: SESSION,
  });
  await live.start();
  assert.equal(live.request().workspace.branchSource, null);
  assert.notEqual(live.request().workspace.branchSource, "live");
  await live.unmount();

  // Nothing resolved, nothing seeded, and so no provenance at all.
  const unresolved = await mount({
    channelId: CHANNEL,
    deps: deps({ listSeatWorktrees: async () => [] }).deps,
    executionProviderPubkey: LOCAL_PROVIDER,
    sessionRef: SESSION,
  });
  await unresolved.start();
  assert.deepEqual(unresolved.request(), {
    kind: "channel",
    channelId: CHANNEL,
  });
  assert.equal("workspace" in unresolved.request(), false);
  await unresolved.unmount();
});

test("foreign execution: unseeded, and it says whose body it is not", async () => {
  const host = deps({
    providerStatus: async () => ({ providerPubkey: LOCAL_PROVIDER }),
  });
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    executionProviderPubkey: FOREIGN_PROVIDER,
    projectId: PROJECT,
    sessionRef: SESSION,
  });
  await view.start();
  // The ordinary launcher, with nothing carried over — never this computer's
  // copy of a path standing in for another machine's checkout.
  assert.deepEqual(view.request(), { kind: "channel", channelId: CHANNEL });
  assert.equal(
    view.detail(),
    "This session's execution runs on a provider this computer does not hold.",
  );
  await view.unmount();
});

test("unknown location keeps the verified local folder, and says it is unknown", async () => {
  // Fail-closed applies to the *claim*, not to the offer: an unnamed
  // execution provider is not evidence the work runs elsewhere, and the
  // directory here was still validated on this computer. So the draft is
  // seeded and the uncertainty is stated rather than hidden.
  const host = deps();
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    projectId: PROJECT,
    sessionRef: SESSION,
  });
  await view.resolveNow();
  await view.start();
  assert.equal(view.request().kind, "workspace");
  assert.equal(view.request().workspace.path, PATH);
  await view.unmount();
});

test("unknown location with nothing recorded stays unseeded", async () => {
  const host = deps({ listSeatWorktrees: async () => [] });
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    projectId: PROJECT,
    sessionRef: SESSION,
  });
  await view.start();
  assert.deepEqual(view.request(), { kind: "channel", channelId: CHANNEL });
  assert.match(
    view.detail(),
    /This computer recorded no directory for this session\./,
  );
  assert.match(view.detail(), /Where its execution runs is not known here\./);
  await view.unmount();
});

test("a missing directory is never seeded, whatever the record says", async () => {
  const host = deps({
    validateWorkdir: async () => ({ exists: false, isDir: false }),
  });
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    executionProviderPubkey: LOCAL_PROVIDER,
    sessionRef: SESSION,
  });
  await view.start();
  assert.deepEqual(view.request(), { kind: "channel", channelId: CHANNEL });
  assert.equal(view.detail(), "No such directory on this computer.");
  await view.unmount();
});

test("a failed read says so instead of claiming an absence", async () => {
  const host = deps({
    listSeatWorktrees: async () => {
      throw new Error("host is not answering");
    },
  });
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    executionProviderPubkey: LOCAL_PROVIDER,
    sessionRef: SESSION,
  });
  await view.resolveNow();
  // Not "this computer recorded no directory" — that is a fact the read
  // never established.
  assert.equal(
    view.detail(),
    "This computer could not read its record of this session's directories.",
  );
  await view.start();
  assert.deepEqual(view.request(), { kind: "channel", channelId: CHANNEL });
  await view.unmount();
});

test("a row with no session ref opens the ordinary draft and reads nothing", async () => {
  const host = deps();
  const view = await mount({
    channelId: CHANNEL,
    deps: host.deps,
    sessionRef: null,
  });
  await view.resolveNow();
  await view.start();
  assert.deepEqual(host.calls, []);
  assert.deepEqual(view.request(), { kind: "channel", channelId: CHANNEL });
  await view.unmount();
});

test("a contextual draft in a multi-repository project keeps repository B through reload", async () => {
  const sourceRepoRef = `30617:${"b".repeat(64)}:repo-b`;
  const otherRepoRef = `30617:${"a".repeat(64)}:repo-a`;
  const firstProjectRepo = selectLaunchRepoRef({
    repos: [
      { name: "repo-a", dtag: "repo-a", repoAddress: otherRepoRef },
      { name: "repo-b", dtag: "repo-b", repoAddress: sourceRepoRef },
    ],
    localRepos: [
      { name: "repo-a", path: "/src/repo-a" },
      { name: "repo-b", path: "/src/repo-b" },
    ],
  });
  assert.equal(
    firstProjectRepo,
    otherRepoRef,
    "ordinary project lookup selects A",
  );
  const view = await mount({
    channelId: CHANNEL,
    projectId: PROJECT,
    sourceRepoRef,
    sessionRef: SESSION,
    executionProviderPubkey: LOCAL_PROVIDER,
    deps: deps({
      listSeatWorktrees: async () => [
        row({ path: "/src/repo-b-wt-lane", repoRoot: "/src/repo-b" }),
      ],
    }).deps,
  });
  try {
    await view.start();
    const restored = parseNewCodingSessionRequest(
      dom.window.sessionStorage.getItem("buzz.new-coding-session-dialog.v1"),
    );
    const route = resolveNewCodingSessionDialogRoute(restored);
    assert.equal(route.projectId, PROJECT);
    assert.equal(route.workspaceReuse.path, "/src/repo-b-wt-lane");
    assert.equal(route.sourceRepoRef, sourceRepoRef);
    assert.equal(
      workspaceReuseRepoRef({
        contextual: route.workspaceReuse !== null,
        sourceRepoRef: route.sourceRepoRef,
        projectRepoRef: firstProjectRepo,
      }),
      sourceRepoRef,
    );
    assert.equal(
      workspaceReuseRepoRef({
        contextual: true,
        sourceRepoRef: null,
        projectRepoRef: firstProjectRepo,
      }),
      null,
      "unknown source never borrows A",
    );
    assert.equal(
      workspaceReuseRepoRef({
        contextual: false,
        sourceRepoRef,
        projectRepoRef: firstProjectRepo,
      }),
      firstProjectRepo,
      "ordinary starts still use their project resolver",
    );
  } finally {
    await view.unmount();
  }
});

test("multiple seat directories open an unseeded draft instead of borrowing one", async () => {
  const host = deps({
    listSeatWorktrees: async () => [
      row(),
      row({ key: `${SESSION}/builder`, path: "/src/other-worktree" }),
    ],
  });
  const view = await mount({
    channelId: CHANNEL,
    projectId: PROJECT,
    sessionRef: SESSION,
    executionProviderPubkey: LOCAL_PROVIDER,
    deps: host.deps,
  });
  try {
    await view.resolveNow();
    assert.match(view.detail(), /multiple recorded workspaces/);
    await view.start();
    assert.deepEqual(view.request(), { kind: "channel", channelId: CHANNEL });
    assert.ok(!host.calls.includes("validateWorkdir"));
  } finally {
    await view.unmount();
  }
});
