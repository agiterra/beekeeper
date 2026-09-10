import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";
import React from "react";
import { act } from "react";
import { createRoot } from "react-dom/client";

import {
  clearCodingSessionFoundingRequest,
  markCodingSessionFoundingStarted,
  parseNewCodingSessionRequest,
  requestCodingSessionFounding,
  requestCodingSessionFoundingInWorkspace,
  requestProjectCodingSessionFounding,
  resetCodingSessionFoundingRequest,
  useCodingSessionFoundingRequest,
} from "./newCodingSessionDialogStore.ts";

const dom = new JSDOM(
  "<!doctype html><html><body><div id='root'></div></body></html>",
  { url: "http://localhost" },
);

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

afterEach(() => {
  clearCodingSessionFoundingRequest();
});

after(() => dom.window.close());

/** Mount a probe that reads the store the way the founding host does. */
async function mountProbe() {
  const seen = { request: undefined };
  function Probe() {
    seen.request = useCodingSessionFoundingRequest();
    return null;
  }
  const root = createRoot(dom.window.document.getElementById("root"));
  await act(async () => {
    root.render(React.createElement(Probe));
  });
  return {
    request: () => seen.request,
    unmount: async () => {
      await act(async () => root.unmount());
    },
  };
}

// The parser is the executable definition of a request's shape. Nothing
// stores a request any more, so nothing at runtime parses one — but every
// arm's fields are pinned here so an opener that drops or renames one is
// caught against the same definition the host reads.

test("a channel request round-trips, with or without a channel", () => {
  assert.deepEqual(
    parseNewCodingSessionRequest('{"kind":"channel","channelId":"abc"}'),
    { kind: "channel", channelId: "abc" },
  );
  assert.deepEqual(
    parseNewCodingSessionRequest('{"kind":"channel","channelId":null}'),
    { kind: "channel", channelId: null },
  );
});

test("a project request round-trips", () => {
  assert.deepEqual(
    parseNewCodingSessionRequest('{"kind":"project","projectId":"p1"}'),
    { kind: "project", projectId: "p1" },
  );
});

test("nothing means no request", () => {
  assert.equal(parseNewCodingSessionRequest(null), null);
});

test("anything that is not a request is refused rather than half-read", () => {
  for (const raw of [
    "not json",
    "null",
    "[]",
    '"channel"',
    "{}",
    '{"kind":"channel"}',
    '{"kind":"channel","channelId":7}',
    '{"kind":"project"}',
    '{"kind":"project","projectId":""}',
    '{"kind":"project","projectId":42}',
    '{"kind":"elsewhere","channelId":"abc"}',
  ]) {
    assert.equal(parseNewCodingSessionRequest(raw), null, raw);
  }
});

// The workspace arm carries a directory the person explicitly chose. Losing a
// field of it is not a blank form: the founded page's prefill order puts a
// channel's remembered folder above any fallback, so a half-read request
// lands on a *different* directory under a title that says "this workspace".

test("a workspace request round-trips whole", () => {
  assert.deepEqual(
    parseNewCodingSessionRequest(
      '{"kind":"workspace","channelId":"c1","projectId":"30621:owner:p",' +
        '"sessionRef":"s1","workspace":{"path":"/Users/x/Code/repo-wt-a",' +
        '"branch":"wt-a"}}',
    ),
    {
      kind: "workspace",
      channelId: "c1",
      projectId: "30621:owner:p",
      sessionRef: "s1",
      sourceRepoRef: null,
      workspace: {
        path: "/Users/x/Code/repo-wt-a",
        branch: "wt-a",
        branchSource: null,
      },
    },
  );
});

test("a workspace request with no channel, project, or branch still round-trips", () => {
  assert.deepEqual(
    parseNewCodingSessionRequest(
      '{"kind":"workspace","channelId":null,"projectId":null,' +
        '"sessionRef":"s1","workspace":{"path":"/Users/x/Code/repo","branch":null}}',
    ),
    {
      kind: "workspace",
      channelId: null,
      projectId: null,
      sessionRef: "s1",
      sourceRepoRef: null,
      workspace: {
        path: "/Users/x/Code/repo",
        branch: null,
        branchSource: null,
      },
    },
  );
});

test("a malformed workspace request is refused rather than half-read", () => {
  const whole = {
    kind: "workspace",
    channelId: "c1",
    projectId: null,
    sessionRef: "s1",
    sourceRepoRef: null,
    workspace: { path: "/Users/x/Code/repo-wt-a", branch: "wt-a" },
  };
  const broken = [
    // Each field dropped in turn — the shape this parser must not tolerate.
    { ...whole, channelId: undefined },
    { ...whole, projectId: undefined },
    { ...whole, sessionRef: undefined },
    { ...whole, workspace: undefined },
    { ...whole, workspace: { branch: "wt-a" } },
    { ...whole, workspace: { path: "/Users/x/Code/repo-wt-a" } },
    // …and each field with the wrong value.
    { ...whole, sessionRef: "" },
    { ...whole, sessionRef: 7 },
    { ...whole, channelId: 7 },
    { ...whole, projectId: 7 },
    { ...whole, sourceRepoRef: 7 },
    { ...whole, workspace: null },
    { ...whole, workspace: "/Users/x/Code/repo-wt-a" },
    { ...whole, workspace: { path: "", branch: null } },
    { ...whole, workspace: { path: 7, branch: null } },
    { ...whole, workspace: { path: "/Users/x", branch: 7 } },
  ];
  for (const value of broken) {
    const raw = JSON.stringify(value);
    assert.equal(parseNewCodingSessionRequest(raw), null, raw);
  }
});

test("a workspace request never falls through to another arm", () => {
  // `kind` decides, and a workspace request that fails its own checks is
  // refused — it must not be re-read as a channel request that happens to
  // carry a channelId.
  assert.equal(
    parseNewCodingSessionRequest(
      '{"kind":"workspace","channelId":"c1","projectId":null}',
    ),
    null,
  );
});

test("a recorded branch source round-trips; an absent one reads as null", () => {
  assert.deepEqual(
    parseNewCodingSessionRequest(
      '{"kind":"workspace","channelId":null,"projectId":null,' +
        '"sessionRef":"s1","workspace":{"path":"/Users/x/Code/repo-wt-a",' +
        '"branch":"wt-a","branchSource":"recorded"}}',
    ),
    {
      kind: "workspace",
      channelId: null,
      projectId: null,
      sessionRef: "s1",
      sourceRepoRef: null,
      workspace: {
        path: "/Users/x/Code/repo-wt-a",
        branch: "wt-a",
        branchSource: "recorded",
      },
    },
  );
  assert.deepEqual(
    parseNewCodingSessionRequest(
      '{"kind":"workspace","channelId":null,"projectId":null,' +
        '"sessionRef":"s1","workspace":{"path":"/Users/x/Code/repo-wt-a",' +
        '"branch":"wt-a"}}',
    ).workspace.branchSource,
    null,
  );
});

test("a live branch source is refused, not shown", () => {
  // "on disk now" carried from a record would be a claim about the present.
  // The opener never writes it; a request that carries it came from
  // somewhere else and is refused whole.
  for (const value of ["live", "", "recorded ", 7]) {
    const raw = JSON.stringify({
      kind: "workspace",
      channelId: null,
      projectId: null,
      sessionRef: "s1",
      sourceRepoRef: null,
      workspace: {
        path: "/Users/x/Code/repo-wt-a",
        branch: "wt-a",
        branchSource: value,
      },
    });
    assert.equal(parseNewCodingSessionRequest(raw), null, raw);
  }
});

// What the openers admit is what the host reads — through the hook, since
// the store has no other reader.

test("what the opener admits is what the hook reads back, phase requested", async () => {
  const probe = await mountProbe();
  assert.equal(probe.request(), null);
  await act(async () => {
    requestCodingSessionFoundingInWorkspace({
      channelId: "c1",
      projectId: "30621:owner:p",
      sessionRef: "s1",
      sourceRepoRef: null,
      workspace: { path: "/Users/x/Code/repo-wt-a", branch: "wt-a" },
    });
  });
  assert.deepEqual(probe.request(), {
    kind: "workspace",
    phase: "requested",
    channelId: "c1",
    projectId: "30621:owner:p",
    sessionRef: "s1",
    sourceRepoRef: null,
    workspace: {
      path: "/Users/x/Code/repo-wt-a",
      branch: "wt-a",
      branchSource: null,
    },
  });
  // The parser and the opener agree on the shape, phase aside.
  const { phase: _phase, ...stored } = probe.request();
  assert.deepEqual(
    parseNewCodingSessionRequest(JSON.stringify(stored)),
    stored,
  );
  await act(async () => {
    clearCodingSessionFoundingRequest();
  });
  assert.equal(probe.request(), null);
  await probe.unmount();
});

test("the opener defaults an omitted channel and project to null, not undefined", async () => {
  const probe = await mountProbe();
  await act(async () => {
    requestCodingSessionFoundingInWorkspace({
      sessionRef: "s1",
      sourceRepoRef: null,
      workspace: { path: "/Users/x/Code/repo-wt-a", branch: null },
    });
  });
  assert.deepEqual(probe.request(), {
    kind: "workspace",
    phase: "requested",
    channelId: null,
    projectId: null,
    sessionRef: "s1",
    sourceRepoRef: null,
    workspace: {
      path: "/Users/x/Code/repo-wt-a",
      branch: null,
      branchSource: null,
    },
  });
  await probe.unmount();
});

test("the opener carries a recorded source and drops a live one", async () => {
  const probe = await mountProbe();
  await act(async () => {
    requestCodingSessionFoundingInWorkspace({
      sessionRef: "s1",
      workspace: {
        path: "/Users/x/Code/repo-wt-a",
        branch: "wt-a",
        branchSource: "recorded",
      },
    });
  });
  assert.equal(probe.request().workspace.branchSource, "recorded");
  await act(async () => {
    clearCodingSessionFoundingRequest();
    requestCodingSessionFoundingInWorkspace({
      sessionRef: "s1",
      workspace: {
        path: "/Users/x/Code/repo-wt-a",
        branch: "wt-a",
        // A caller handing over a live head gets it dropped rather than carried.
        branchSource: "live",
      },
    });
  });
  assert.equal(probe.request().workspace.branchSource, null);
  await probe.unmount();
});

test("nothing is written to sessionStorage or localStorage", async () => {
  // A request that survived a reload would found a second genesis for the
  // same click. The old dialog mirrored its request; this store must not.
  dom.window.sessionStorage.clear();
  dom.window.localStorage.clear();
  requestCodingSessionFounding("c1");
  markCodingSessionFoundingStarted();
  assert.equal(dom.window.sessionStorage.length, 0);
  assert.equal(dom.window.localStorage.length, 0);
});

test("a second request while one is pending is ignored, in either phase", async () => {
  const probe = await mountProbe();
  await act(async () => {
    requestCodingSessionFounding("c1");
  });
  assert.deepEqual(probe.request(), {
    kind: "channel",
    phase: "requested",
    channelId: "c1",
  });
  // Still "requested": a double click, or two entry points in one tick.
  await act(async () => {
    requestCodingSessionFounding("c2");
    requestProjectCodingSessionFounding("p1");
    requestCodingSessionFoundingInWorkspace({
      sessionRef: "s1",
      workspace: { path: "/Users/x/Code/repo", branch: null },
    });
  });
  assert.deepEqual(probe.request(), {
    kind: "channel",
    phase: "requested",
    channelId: "c1",
  });
  // Now "founding": the host has begun.
  await act(async () => {
    assert.equal(markCodingSessionFoundingStarted(), true);
  });
  assert.equal(probe.request().phase, "founding");
  await act(async () => {
    requestCodingSessionFounding("c2");
    requestProjectCodingSessionFounding("p1");
  });
  assert.deepEqual(probe.request(), {
    kind: "channel",
    phase: "founding",
    channelId: "c1",
  });
  // Cleared: the next click is admitted again.
  await act(async () => {
    clearCodingSessionFoundingRequest();
    requestProjectCodingSessionFounding("p1");
  });
  assert.deepEqual(probe.request(), {
    kind: "project",
    phase: "requested",
    projectId: "p1",
  });
  await probe.unmount();
});

test("markCodingSessionFoundingStarted is synchronous and claims a request once", () => {
  // No request: nothing to claim.
  assert.equal(markCodingSessionFoundingStarted(), false);
  requestCodingSessionFounding("c1");
  // The first caller wins, in the same tick, with no await in between —
  // which is what lets a StrictMode double effect start exactly one founding.
  assert.equal(markCodingSessionFoundingStarted(), true);
  assert.equal(markCodingSessionFoundingStarted(), false);
  assert.equal(markCodingSessionFoundingStarted(), false);
});

test("reset drops a request in either phase", async () => {
  const probe = await mountProbe();
  await act(async () => {
    requestCodingSessionFounding("c1");
    resetCodingSessionFoundingRequest();
  });
  assert.equal(probe.request(), null);
  await act(async () => {
    requestCodingSessionFounding("c1");
    markCodingSessionFoundingStarted();
    resetCodingSessionFoundingRequest();
  });
  assert.equal(probe.request(), null);
  await probe.unmount();
});
