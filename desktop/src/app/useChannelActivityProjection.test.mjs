import assert from "node:assert/strict";
import test from "node:test";

import { resolveChannelActivityFeedItemReadAt } from "./useChannelActivityProjection.ts";

test("channel activity read state folds the item's own message and channel markers", () => {
  const markers = new Map([
    ["msg:reply-general", 100],
    ["general", 200],
    ["random", 500],
  ]);

  assert.equal(
    resolveChannelActivityFeedItemReadAt(
      { id: "reply-general", channelId: "general" },
      (contextId) => markers.get(contextId) ?? null,
    ),
    200,
  );
});

test("channel activity read state honors a channel marker without a message marker", () => {
  assert.equal(
    resolveChannelActivityFeedItemReadAt(
      { id: "reply-general", channelId: "general" },
      (contextId) => (contextId === "general" ? 300 : null),
    ),
    300,
  );
});

// The channel pip is a THREAD-level signal resolved from per-reply rows, so it
// must read the aggregate thread:<root> marker as well as the row's own. This
// is what makes opening a thread clear the pip: the open writes the aggregate
// at the thread's newest reply and marks only the replies it revealed, so a
// reply nested in a collapsed branch is covered here without its own marker
// moving — and its in-panel branch badge, which reads only msg:<id>, survives
// (ledger 279(g); before this the pip cleared only from the Inbox row click).
const threadTags = (rootId, parentId) => [
  ["e", rootId, "", "root"],
  ["e", parentId, "", "reply"],
];

test("the pip's read state folds the aggregate thread marker", () => {
  const markers = new Map([["thread:root-1", 400]]);

  assert.equal(
    resolveChannelActivityFeedItemReadAt(
      {
        id: "nested-reply",
        channelId: "general",
        tags: threadTags("root-1", "a1"),
      },
      (contextId) => markers.get(contextId) ?? null,
    ),
    400,
  );
});

test("a reply newer than the aggregate marker is still unread", () => {
  // The caller's predicate is strictly createdAt > this value. A reply that
  // lands after the open (createdAt 500) must out-run the marker the open
  // wrote (400), or the thread could never light the pip again.
  const readAt = resolveChannelActivityFeedItemReadAt(
    {
      id: "late-reply",
      channelId: "general",
      tags: threadTags("root-1", "a1"),
    },
    (contextId) => (contextId === "thread:root-1" ? 400 : null),
  );

  assert.equal(readAt, 400);
  assert.equal(500 > readAt, true);
});

test("a top-level row with no thread root reads its own markers only", () => {
  // No root e-tag, so there is no aggregate to fold; the row falls back to
  // msg:<id> and the channel, exactly as before.
  assert.equal(
    resolveChannelActivityFeedItemReadAt(
      { id: "top-level", channelId: "general", tags: [] },
      (contextId) => (contextId === "msg:top-level" ? 150 : null),
    ),
    150,
  );
});
