import assert from "node:assert/strict";
import { after, afterEach, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
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
});

after(() => dom.window.close());

// Andy's exact shape (2026-09-28): the agent's replies are nested under HIS
// reply, so they sit in a branch that is collapsed when the thread opens. Under
// the old open-at-level rule the open effect marked only the revealed set — a1
// — and g1/g2 stayed unread through every open and every reload, which is the
// channel pip nobody could clear by looking. The thread now reads whole.
const message = (id, parentId, createdAt, author = "agent") => ({
  id,
  createdAt,
  author,
  time: "12:00",
  body: id,
  parentId: parentId ?? null,
  rootId: parentId ? "root" : null,
  depth: parentId ? 1 : 0,
});

const timelineMessages = [
  message("root", null, 100, "andy"),
  message("a1", "root", 200, "andy"),
  message("g1", "a1", 300),
  message("g2", "a1", 400),
];

async function renderUnreadState(overrides = {}) {
  const { renderHook } = await import("@testing-library/react");
  const { useChannelUnreadState } = await import("./useChannelUnreadState.ts");

  const markedRead = [];
  const threadsMarkedRead = [];
  const options = {
    activeChannelId: "channel-a",
    timelineMessages,
    currentPubkey: "andy",
    openThreadHeadId: "root",
    threadReplyTargetId: null,
    expandedThreadReplyIds: new Set(),
    getChannelReadAt: () => null,
    getMessageReadAt: () => null,
    clearChannelUnreadSource: () => {},
    markChannelUnread: () => {},
    markMessageRead: (id, timestamp) => markedRead.push([id, timestamp]),
    markThreadRead: (rootId, timestamp) =>
      threadsMarkedRead.push([rootId, timestamp]),
    isThreadMuted: () => false,
    readStateVersion: 0,
    ...overrides,
  };

  renderHook(() => useChannelUnreadState(options));
  return { markedRead, threadsMarkedRead };
}

test("opening a thread reads replies nested in a collapsed branch", async () => {
  const { markedRead } = await renderUnreadState();

  // The visible set on open is a1 alone (no reply id is expanded), so marking
  // g1 and g2 can only come from the descendant walk. Each marker carries the
  // reply's OWN createdAt, never the newest — a later reply must still be able
  // to re-raise the badge, because the predicate is createdAt > read.
  const readAtById = new Map(markedRead);
  assert.equal(readAtById.get("a1"), 200);
  assert.equal(readAtById.get("g1"), 300);
  assert.equal(readAtById.get("g2"), 400);
  // The head is not a reply to itself and is never marked by this path.
  assert.equal(readAtById.has("root"), false);
});

test("opening a thread writes the aggregate marker at the newest reply", async () => {
  const { threadsMarkedRead } = await renderUnreadState();

  // Inbox parity: the row click writes thread:<root> at the thread's latest
  // activity, and the open must write the same marker at the same timestamp,
  // or the pip returns on the next reload from the surface that reads it.
  assert.deepEqual(threadsMarkedRead, [["root", 400]]);
});

test("a muted thread is not read by opening it", async () => {
  const { markedRead, threadsMarkedRead } = await renderUnreadState({
    isThreadMuted: () => true,
  });

  assert.deepEqual(markedRead, []);
  assert.deepEqual(threadsMarkedRead, []);
});

test("no open thread marks nothing read", async () => {
  const { markedRead, threadsMarkedRead } = await renderUnreadState({
    openThreadHeadId: null,
  });

  assert.deepEqual(markedRead, []);
  assert.deepEqual(threadsMarkedRead, []);
});

test("a thread with no replies writes no aggregate marker", async () => {
  const { markedRead, threadsMarkedRead } = await renderUnreadState({
    timelineMessages: [message("root", null, 100, "andy")],
  });

  assert.deepEqual(markedRead, []);
  // latest is null, so thread:<root> is never written backwards to the head's
  // own createdAt — a first reply must still arrive unread.
  assert.deepEqual(threadsMarkedRead, []);
});

test("the aggregate marker is optional: replies still read without it", async () => {
  // markThreadRead is an optional option (older callers do not plumb it), so
  // the open path must not depend on it being supplied.
  const { markedRead } = await renderUnreadState({ markThreadRead: undefined });

  assert.deepEqual(new Map(markedRead).get("g2"), 400);
});
