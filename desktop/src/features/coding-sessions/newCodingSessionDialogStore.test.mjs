import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

import {
  closeNewCodingSessionDialog,
  openNewCodingSessionDialogInWorkspace,
  parseNewCodingSessionRequest,
} from "./newCodingSessionDialogStore.ts";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, { window: dom.window });
});

after(() => dom.window.close());

// What survives a reload is what this parses. The route this dialog replaced
// pinned its channel into the URL so a mid-create refresh re-attached to the
// durable transaction; the stored request is what does that job now, so a
// shape it silently mis-reads is a create that quietly loses its dialog.

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

test("nothing stored means no dialog", () => {
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
// field of it on reload is not a blank form: the launcher's prefill order puts
// a channel's remembered folder above any fallback, so a half-read request
// reopens on a *different* directory under a title that says "this workspace".

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

test("what the opener stores is what the parser reads back", () => {
  // The reload path end to end: the opener writes into sessionStorage and the
  // parser is what survives the refresh. Asserting the parser alone would miss
  // an opener that stores a field under another name.
  openNewCodingSessionDialogInWorkspace({
    channelId: "c1",
    projectId: "30621:owner:p",
    sessionRef: "s1",
    sourceRepoRef: null,
    workspace: { path: "/Users/x/Code/repo-wt-a", branch: "wt-a" },
  });

  const raw = dom.window.sessionStorage.getItem(
    "buzz.new-coding-session-dialog.v1",
  );
  assert.deepEqual(parseNewCodingSessionRequest(raw), {
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
  });

  closeNewCodingSessionDialog();
  assert.equal(
    dom.window.sessionStorage.getItem("buzz.new-coding-session-dialog.v1"),
    null,
  );
});

test("the opener defaults an omitted channel and project to null, not undefined", () => {
  // `JSON.stringify` drops an undefined field, and a dropped field is one the
  // parser refuses — an opener called with only a session and a workspace has
  // to store nulls or the request would not survive its own reload.
  openNewCodingSessionDialogInWorkspace({
    sessionRef: "s1",
    sourceRepoRef: null,
    workspace: { path: "/Users/x/Code/repo-wt-a", branch: null },
  });

  const raw = dom.window.sessionStorage.getItem(
    "buzz.new-coding-session-dialog.v1",
  );
  assert.deepEqual(parseNewCodingSessionRequest(raw), {
    kind: "workspace",
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

  closeNewCodingSessionDialog();
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

test("a stored live branch source is refused, not shown", () => {
  // "on disk now" restored from storage would be a claim about the present
  // made from a record. The opener never writes it; a request that carries it
  // came from somewhere else and is refused whole.
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

test("the opener stores a recorded source and drops a live one", () => {
  openNewCodingSessionDialogInWorkspace({
    sessionRef: "s1",
    sourceRepoRef: null,
    workspace: {
      path: "/Users/x/Code/repo-wt-a",
      branch: "wt-a",
      branchSource: "recorded",
    },
  });
  assert.equal(
    parseNewCodingSessionRequest(
      dom.window.sessionStorage.getItem("buzz.new-coding-session-dialog.v1"),
    ).workspace.branchSource,
    "recorded",
  );

  openNewCodingSessionDialogInWorkspace({
    sessionRef: "s1",
    sourceRepoRef: null,
    workspace: {
      path: "/Users/x/Code/repo-wt-a",
      branch: "wt-a",
      // A caller handing over a live head gets it dropped rather than stored.
      branchSource: "live",
    },
  });
  assert.equal(
    parseNewCodingSessionRequest(
      dom.window.sessionStorage.getItem("buzz.new-coding-session-dialog.v1"),
    ).workspace.branchSource,
    null,
  );

  closeNewCodingSessionDialog();
});
