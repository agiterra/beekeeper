import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import {
  readCodingSessionWorkspaceReuse,
  useCodingSessionWorkspaceReuse,
} from "./useCodingSessionWorkspaceReuse.ts";

/**
 * The menu that consumes this is drawn per session row. What these pin is the
 * read *budget* — nothing runs until a caller resolves, and a resolve costs at
 * most four host calls, fewer when there is nothing to check — and that every
 * failure degrades to a truthful answer instead of a confident one.
 */

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    localStorage: dom.window.localStorage,
    window: dom.window,
  });
});

after(() => dom.window.close());

const SESSION = "9f2c1d40-4a5b-4c6d-8e9f-0a1b2c3d4e5f";
const LOCAL_PROVIDER = "a".repeat(64);
const FOREIGN_PROVIDER = "b".repeat(64);
const CHANNEL = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";

function row(overrides = {}) {
  return {
    key: `${SESSION}/lead`,
    sessionRef: SESSION,
    seatLabel: "lead",
    path: "/Users/x/Code/repo-wt-a",
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

/** Deps that record every host call, so the budget is counted, not claimed. */
function recordingDeps(behaviour = {}) {
  const calls = [];
  const answer = {
    listSeatWorktrees: async () => [row()],
    validateWorkdir: async () => ({ exists: true, isDir: true }),
    listBranches: async () => ({ headBranch: "wt-a-live" }),
    providerStatus: async () => ({ providerPubkey: LOCAL_PROVIDER }),
    ...behaviour,
  };
  // The recording wraps the behaviour rather than replacing it, so a test that
  // overrides one answer still counts the call it made.
  const deps = {
    listSeatWorktrees: (sessions) => {
      calls.push({ call: "listSeatWorktrees", sessions });
      return answer.listSeatWorktrees(sessions);
    },
    validateWorkdir: (path) => {
      calls.push({ call: "validateWorkdir", path });
      return answer.validateWorkdir(path);
    },
    listBranches: (input) => {
      calls.push({ call: "listBranches", workdir: input.workdir });
      return answer.listBranches(input);
    },
    providerStatus: () => {
      calls.push({ call: "providerStatus" });
      return answer.providerStatus();
    },
  };
  return { calls, deps, names: () => calls.map((entry) => entry.call) };
}

test("a resolve costs four host calls and no more", async () => {
  const { deps, names } = recordingDeps();

  const { resolution, error } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  assert.deepEqual(names().sort(), [
    "listBranches",
    "listSeatWorktrees",
    "providerStatus",
    "validateWorkdir",
  ]);
  assert.equal(error, null);
  assert.equal(resolution.availability, "available");
  assert.equal(resolution.path, "/Users/x/Code/repo-wt-a");
  assert.equal(resolution.branch, "wt-a-live");
  assert.equal(resolution.branchSource, "live");
});

test("the seat read carries this session's ref, with conservative facts", async () => {
  const { calls, deps } = recordingDeps();

  await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  const read = calls.find((entry) => entry.call === "listSeatWorktrees");
  assert.deepEqual(read.sessions, [
    {
      sessionRef: SESSION,
      sessionSettled: false,
      executionLive: false,
      tipOnRelay: null,
      settledForSecs: null,
    },
  ]);
});

test("the caller's real relay facts reach the host read when it has them", async () => {
  const { calls, deps } = recordingDeps();

  await readCodingSessionWorkspaceReuse(
    {
      sessionRef: SESSION,
      executionProviderPubkey: LOCAL_PROVIDER,
      facts: { sessionSettled: true, executionLive: true, tipOnRelay: true },
    },
    deps,
  );

  const read = calls.find((entry) => entry.call === "listSeatWorktrees");
  assert.equal(read.sessions[0].sessionSettled, true);
  assert.equal(read.sessions[0].executionLive, true);
  assert.equal(read.sessions[0].tipOnRelay, true);
});

test("no recorded row spends nothing on the disk", async () => {
  const { deps, names } = recordingDeps({
    listSeatWorktrees: async () => [],
  });

  const { resolution } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  assert.deepEqual(names().sort(), ["listSeatWorktrees", "providerStatus"]);
  assert.equal(resolution.availability, "unrecorded");
});

test("a foreign execution never touches this computer's disk", async () => {
  const { deps, names } = recordingDeps();

  const { resolution } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: FOREIGN_PROVIDER },
    deps,
  );

  assert.deepEqual(names().sort(), ["listSeatWorktrees", "providerStatus"]);
  assert.equal(resolution.availability, "elsewhere");
  assert.equal(resolution.path, null);
});

test("a directory that is gone skips the branch read", async () => {
  const { deps, names } = recordingDeps({
    validateWorkdir: async () => ({ exists: false, isDir: false }),
  });

  const { resolution } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  assert.equal(names().includes("listBranches"), false);
  assert.equal(resolution.availability, "missing");
  assert.equal(resolution.branch, "wt-a", "the recorded branch is kept");
  assert.equal(resolution.branchSource, "recorded");
});

test("a failed branch read keeps the recorded branch rather than blanking it", async () => {
  const { deps } = recordingDeps({
    listBranches: async () => {
      throw new Error("not a git checkout");
    },
  });

  const { resolution } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  assert.equal(resolution.availability, "available");
  assert.equal(resolution.branch, "wt-a");
  assert.equal(resolution.branchSource, "recorded");
});

test("a validation that throws is missing, not an unverified offer", async () => {
  const { deps } = recordingDeps({
    validateWorkdir: async () => {
      throw new Error("host is gone");
    },
  });

  const { resolution } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  assert.equal(resolution.availability, "missing");
});

test("a failed seat read says so instead of claiming nothing was recorded", async () => {
  const { deps } = recordingDeps({
    listSeatWorktrees: async () => {
      throw new Error("no tauri host");
    },
  });

  const { resolution, error } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  assert.equal(resolution.availability, "unrecorded");
  assert.equal(resolution.path, null);
  assert.match(error, /could not read/);
});

test("no provider on this computer is unknown, never elsewhere", async () => {
  for (const providerStatus of [
    async () => ({}),
    async () => {
      throw new Error("no provider here");
    },
  ]) {
    const { deps } = recordingDeps({ providerStatus });
    const { resolution } = await readCodingSessionWorkspaceReuse(
      { sessionRef: SESSION, executionProviderPubkey: FOREIGN_PROVIDER },
      deps,
    );
    assert.notEqual(resolution.availability, "elsewhere");
    assert.equal(resolution.availability, "available");
    assert.match(resolution.sentence, /not known here/);
  }
});

test("an execution whose provider the caller does not know is unknown, not elsewhere", async () => {
  const { deps } = recordingDeps();

  const { resolution } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: null },
    deps,
  );

  assert.equal(resolution.availability, "available");
  assert.match(resolution.sentence, /not known here/);
});

test("another session's row is not read as this session's workspace", async () => {
  const { deps, names } = recordingDeps({
    listSeatWorktrees: async () => [
      row({ sessionRef: "c0ffee00-0000-4000-8000-000000000000" }),
    ],
  });

  const { resolution } = await readCodingSessionWorkspaceReuse(
    { sessionRef: SESSION, executionProviderPubkey: LOCAL_PROVIDER },
    deps,
  );

  assert.equal(resolution.availability, "unrecorded");
  assert.equal(names().includes("validateWorkdir"), false);
});

test("the hook reads nothing until a caller resolves, and then holds the answer", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { deps, names } = recordingDeps();

  const mounted = renderHook(() =>
    useCodingSessionWorkspaceReuse(SESSION, CHANNEL, {
      executionProviderPubkey: LOCAL_PROVIDER,
      deps,
    }),
  );

  // Mounting a menu's trigger must cost nothing: rendering is not opening.
  assert.deepEqual(names(), []);
  assert.equal(mounted.result.current.resolution, null);
  assert.equal(mounted.result.current.error, null);
  assert.equal(mounted.result.current.isResolving, false);

  let returned = null;
  await act(async () => {
    returned = await mounted.result.current.resolve();
  });

  assert.equal(names().length, 4);
  assert.equal(mounted.result.current.isResolving, false);
  // What the caller awaits and what the hook holds are the same answer.
  assert.equal(returned.availability, "available");
  assert.equal(mounted.result.current.resolution.availability, "available");
  assert.equal(
    mounted.result.current.resolution.path,
    "/Users/x/Code/repo-wt-a",
  );

  mounted.unmount();
});

test("switching the session drops the previous answer rather than showing it", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { deps } = recordingDeps();

  const mounted = renderHook(
    ({ sessionRef }) =>
      useCodingSessionWorkspaceReuse(sessionRef, CHANNEL, {
        executionProviderPubkey: LOCAL_PROVIDER,
        deps,
      }),
    { initialProps: { sessionRef: SESSION } },
  );

  await act(async () => {
    await mounted.result.current.resolve();
  });
  assert.equal(mounted.result.current.resolution.availability, "available");

  await act(async () => {
    mounted.rerender({ sessionRef: "c0ffee00-0000-4000-8000-000000000000" });
  });

  assert.equal(
    mounted.result.current.resolution,
    null,
    "another session's directory must not appear under this one's menu",
  );

  mounted.unmount();
});

test("the same session reached in another channel drops the held answer", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { deps } = recordingDeps();

  const mounted = renderHook(
    ({ channelId }) =>
      useCodingSessionWorkspaceReuse(SESSION, channelId, {
        executionProviderPubkey: LOCAL_PROVIDER,
        deps,
      }),
    { initialProps: { channelId: CHANNEL } },
  );

  await act(async () => {
    await mounted.result.current.resolve();
  });
  assert.equal(mounted.result.current.resolution.availability, "available");

  await act(async () => {
    mounted.rerender({ channelId: "9c1e0f22-0000-4000-8000-000000000001" });
  });

  assert.equal(mounted.result.current.resolution, null);

  mounted.unmount();
});

test("a caller that names no execution provider still gets an answer, marked unknown", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { deps } = recordingDeps();

  const mounted = renderHook(() =>
    useCodingSessionWorkspaceReuse(SESSION, CHANNEL, { deps }),
  );

  await act(async () => {
    await mounted.result.current.resolve();
  });

  assert.equal(mounted.result.current.resolution.availability, "available");
  assert.match(mounted.result.current.resolution.sentence, /not known here/);

  mounted.unmount();
});

test("a failed seat read surfaces its own sentence beside the resolution", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const { deps } = recordingDeps({
    listSeatWorktrees: async () => {
      throw new Error("no tauri host");
    },
  });

  const mounted = renderHook(() =>
    useCodingSessionWorkspaceReuse(SESSION, CHANNEL, {
      executionProviderPubkey: LOCAL_PROVIDER,
      deps,
    }),
  );

  await act(async () => {
    await mounted.result.current.resolve();
  });

  assert.equal(mounted.result.current.resolution.availability, "unrecorded");
  assert.match(mounted.result.current.error, /could not read/);

  mounted.unmount();
});

test("a resolve that lands after a newer one does not overwrite it", async () => {
  const { act, renderHook } = await import("@testing-library/react");
  const releases = [];
  const { deps } = recordingDeps({
    listSeatWorktrees: async () =>
      new Promise((resolveRows) => {
        releases.push(resolveRows);
      }),
  });

  const mounted = renderHook(() =>
    useCodingSessionWorkspaceReuse(SESSION, CHANNEL, {
      executionProviderPubkey: LOCAL_PROVIDER,
      deps,
    }),
  );

  let first = null;
  let second = null;
  await act(async () => {
    first = mounted.result.current.resolve();
    second = mounted.result.current.resolve();
    await Promise.resolve();
  });

  // The second open answers with a row; the first, released afterwards, has
  // none. The stale answer must not become what the menu shows.
  await act(async () => {
    releases[1]([row()]);
    await second;
    releases[0]([]);
    await first;
  });

  assert.equal(mounted.result.current.resolution.availability, "available");
  assert.equal(mounted.result.current.isResolving, false);

  mounted.unmount();
});

test("the hook keeps no module-level state to leak across communities", async () => {
  const source = await import("node:fs").then((fs) =>
    fs.readFileSync(
      new URL("./useCodingSessionWorkspaceReuse.ts", import.meta.url),
      "utf8",
    ),
  );

  // `resetCommunityState()` is the inventory of community-scoped singletons.
  // Nothing here belongs in it, and this is what keeps that true: a module
  // `let`/`Map`/`Set` added later fails here rather than silently carrying one
  // community's directories into the next.
  for (const pattern of [
    /^let\s/m,
    /^var\s/m,
    /^const\s+\w+\s*=\s*new\s+(?:Map|Set|WeakMap)\b/m,
  ]) {
    assert.doesNotMatch(source, pattern, String(pattern));
  }
});
