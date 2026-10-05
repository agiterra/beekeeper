import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionTreeDefaultNotice,
  codingSessionTreeElsewhereReason,
} from "./CodingSessionSurfaceTerminal.tsx";
import {
  codingSessionLocalProviderState,
  codingSessionProviderAuthority,
  codingSessionSurfaceTreeState,
} from "./useCodingSessionSurfacePanelsShell.ts";

const MINE = "a".repeat(64);
const OTHER = "b".repeat(64);
const QUERY = {
  sessionId: "sess-1",
  channelId: "channel-1",
  projectRef: null,
  isLocalProvider: false,
};

test("an unloaded provider status is unknown, not another computer", () => {
  assert.deepEqual(
    codingSessionLocalProviderState({ data: undefined, isError: false }, MINE),
    { isLocalProvider: null, statusUnread: false, authorityUnknown: false },
  );
});

test("an unreadable provider status is unknown and says it was not read", () => {
  assert.deepEqual(
    codingSessionLocalProviderState({ data: undefined, isError: true }, MINE),
    { isLocalProvider: null, statusUnread: true, authorityUnknown: false },
  );
});

test("a read status answers local or remote", () => {
  const status = (providerPubkey) => ({
    data: { providerPubkey },
    isError: false,
  });
  assert.equal(
    codingSessionLocalProviderState(status(MINE.toUpperCase()), MINE)
      .isLocalProvider,
    true,
  );
  assert.equal(
    codingSessionLocalProviderState(status(MINE), OTHER).isLocalProvider,
    false,
  );
  // No provider key read: this machine runs no provider here, so not local.
  assert.equal(
    codingSessionLocalProviderState(status(undefined), MINE).isLocalProvider,
    false,
  );
});

test("while locality is pending the tree is loading and claims no refusal", () => {
  const tree = codingSessionSurfaceTreeState({
    query: QUERY,
    isLocalProvider: null,
    statusUnread: false,
    resolution: undefined,
    resolutionFailed: false,
  });
  assert.equal(tree.state, "loading");
  assert.equal(tree.refusal, null);
  assert.equal(tree.available, false);
});

test("an unread status turns a notLocal answer into an error, never `another computer`", () => {
  const tree = codingSessionSurfaceTreeState({
    query: QUERY,
    isLocalProvider: null,
    statusUnread: true,
    resolution: {
      available: false,
      source: null,
      label: "no working tree",
      reason: "The working tree is on another computer.",
      refusal: "notLocal",
    },
    resolutionFailed: false,
  });
  assert.equal(tree.state, "error");
  assert.equal(tree.refusal, null);
  assert.doesNotMatch(tree.reason, /another computer/);
  assert.match(tree.reason, /could not say whether it runs this session/);
});

test("an unread status still opens a worktree this host cut for the session", () => {
  const tree = codingSessionSurfaceTreeState({
    query: QUERY,
    isLocalProvider: null,
    statusUnread: true,
    resolution: {
      available: true,
      source: "session",
      label: "this session's worktree",
      reason: null,
      refusal: null,
    },
    resolutionFailed: false,
  });
  assert.equal(tree.state, "resolved");
  assert.equal(tree.available, true);
});

test("only a read status naming another provider yields notLocal", () => {
  const tree = codingSessionSurfaceTreeState({
    query: QUERY,
    isLocalProvider: false,
    statusUnread: false,
    resolution: {
      available: false,
      source: null,
      label: "no working tree",
      reason: "The working tree is on another computer.",
      refusal: "notLocal",
    },
    resolutionFailed: false,
  });
  assert.equal(tree.state, "resolved");
  assert.equal(tree.refusal, "notLocal");
});

// ---------------------------------------------------------------------------
// A record whose provider authority is not known yet (no trusted transcript)
// is never "on another computer".
// ---------------------------------------------------------------------------

test("the authority falls back to metadata, then the execution's signer", () => {
  const record = (providerAuthorityPubkey, metadataAuthorityPubkey) => ({
    providerAuthorityPubkey,
    metadataAuthorityPubkey,
  });
  assert.equal(
    codingSessionProviderAuthority(record(MINE, OTHER), {
      signerPubkey: OTHER,
    }),
    MINE,
  );
  assert.equal(
    codingSessionProviderAuthority(record(null, MINE.toUpperCase()), {
      signerPubkey: OTHER,
    }),
    MINE,
  );
  assert.equal(
    codingSessionProviderAuthority(record(null, null), { signerPubkey: MINE }),
    MINE,
  );
  assert.equal(
    codingSessionProviderAuthority(record(null, null), { signerPubkey: " " }),
    null,
  );
  assert.equal(codingSessionProviderAuthority(null, null), null);
});

test("a record with null authority never yields notLocal or isLocalProvider false", () => {
  const authority = codingSessionProviderAuthority(
    { providerAuthorityPubkey: null, metadataAuthorityPubkey: null },
    null,
  );
  assert.equal(authority, null);
  // Whatever the provider status says — loaded, naming this machine, naming
  // nobody, unread — an unknown authority stays unknown.
  const statuses = [
    { data: { providerPubkey: MINE }, isError: false },
    { data: { providerPubkey: undefined }, isError: false },
    { data: undefined, isError: false },
    { data: undefined, isError: true },
  ];
  const notLocalAnswer = {
    available: false,
    source: null,
    label: "no working tree",
    reason: "The working tree is on another computer.",
    refusal: "notLocal",
  };
  for (const status of statuses) {
    const local = codingSessionLocalProviderState(status, authority);
    assert.equal(local.isLocalProvider, null);
    assert.equal(local.authorityUnknown, true);
    for (const resolution of [undefined, notLocalAnswer]) {
      const tree = codingSessionSurfaceTreeState({
        query: QUERY,
        ...local,
        resolution,
        resolutionFailed: false,
      });
      assert.notEqual(tree.refusal, "notLocal");
      if (tree.reason !== null) {
        assert.doesNotMatch(tree.reason, /another computer/);
      }
    }
    const refused = codingSessionSurfaceTreeState({
      query: QUERY,
      ...local,
      resolution: notLocalAnswer,
      resolutionFailed: false,
    });
    assert.equal(
      refused.reason,
      "This session has not said which provider runs it yet.",
    );
  }
});

test("a session worktree this host cut still opens while the authority is unknown", () => {
  const tree = codingSessionSurfaceTreeState({
    query: QUERY,
    isLocalProvider: null,
    statusUnread: false,
    authorityUnknown: true,
    resolution: {
      available: true,
      source: "session",
      label: "this session's worktree",
      reason: null,
      refusal: null,
    },
    resolutionFailed: false,
  });
  assert.equal(tree.state, "resolved");
  assert.equal(tree.available, true);
});

// ---------------------------------------------------------------------------
// The Terminal's "elsewhere" sentence names the machine's provider, never the
// session's creator.
// ---------------------------------------------------------------------------

function elsewhereCtx({ operator, provider, names }) {
  return {
    focusedExecution: { operatorPubkey: operator, signerPubkey: provider },
    focusedRecord: { providerAuthorityPubkey: provider },
    resolveActorName: (pubkey) => names[pubkey] ?? null,
  };
}

test("creator differs from the machine's owner: the provider is named, not the creator", () => {
  const reason = codingSessionTreeElsewhereReason(
    elsewhereCtx({
      operator: MINE,
      provider: OTHER,
      names: { [MINE]: "Brian", [OTHER]: "Andy's Mac" },
    }),
  );
  assert.equal(
    reason,
    "The working tree is on another computer (Andy's Mac's provider runs this session), and no terminal there is shared.",
  );
  assert.doesNotMatch(reason, /Brian/);
});

test("an unresolved provider names nobody", () => {
  const reason = codingSessionTreeElsewhereReason(
    elsewhereCtx({ operator: OTHER, provider: OTHER, names: {} }),
  );
  assert.equal(
    reason,
    "The working tree is on another computer, and no terminal there is shared.",
  );
});

test("the viewer's own other machine is never called another person's", () => {
  const reason = codingSessionTreeElsewhereReason(
    elsewhereCtx({ operator: MINE, provider: OTHER, names: {} }),
  );
  assert.doesNotMatch(reason, /person|your computer|another of your/);
  assert.match(reason, /^The working tree is on another computer/);
});

test("a default tree says it is not the session's own worktree", () => {
  const tree = (source) => ({ tree: { available: true, source } });
  assert.equal(codingSessionTreeDefaultNotice(tree("session")), null);
  assert.match(
    codingSessionTreeDefaultNotice(tree("project")),
    /this project's checkout, not a worktree recorded for this session/,
  );
  assert.match(
    codingSessionTreeDefaultNotice(tree("channel")),
    /this channel's folder/,
  );
});
