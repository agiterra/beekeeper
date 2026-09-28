import assert from "node:assert/strict";
import test from "node:test";

import {
  buildCreatedAtByMessageId,
  buildDirectReplyIdsByParentId,
  buildRepliesByRootId,
  collectReplyDescendantIds,
} from "./subtreeCreatedAt.ts";
import { computeThreadBadgeCounts } from "./threadBadgeCounts.ts";
import { threadOpenReadTargets } from "./threadOpenReadTargets.ts";

// Opening a thread marks the WHOLE thread read (Andy, 2026-09-28): the shape
// that stayed unread forever under open-at-level was an agent's reply nested
// under the human's own reply, in a branch the human never expanded.
const msg = (id, parentId, createdAt, pubkey = "agent", rootId) => ({
  id,
  parentId,
  rootId: rootId ?? parentId ?? id,
  createdAt,
  pubkey,
});

// root ─ andyReply ─ agentNested1, agentNested2   (the Beekeeper #bee-keeper case)
//      └ agentDirect
const thread = [
  msg("root", null, 100, "andy"),
  msg("andyReply", "root", 200, "andy", "root"),
  msg("agentNested1", "andyReply", 300, "agent", "root"),
  msg("agentNested2", "andyReply", 400, "agent", "root"),
  msg("agentDirect", "root", 350, "agent", "root"),
];

function targetsFor(messages, rootId) {
  const byParent = buildDirectReplyIdsByParentId(messages);
  return threadOpenReadTargets(
    rootId,
    (id) => collectReplyDescendantIds(id, byParent),
    buildCreatedAtByMessageId(messages),
  );
}

// The on-open effect: every target gets msg:<id> = its createdAt.
function readAfterOpen(targets) {
  const read = new Map(targets.replies);
  return (id) => read.get(id) ?? null;
}

// The root summary badge as the channel shows it, read by Andy.
const rootBadge = (messages, getReadAt) =>
  computeThreadBadgeCounts(
    messages,
    buildRepliesByRootId(messages),
    getReadAt,
    () => true,
    "andy",
  ).get("root") ?? 0;

test("openingThread_targetsNestedRepliesInCollapsedBranches", () => {
  const { replies, latest } = targetsFor(thread, "root");
  assert.deepEqual(replies.map(([id]) => id).sort(), [
    "agentDirect",
    "agentNested1",
    "agentNested2",
    "andyReply",
  ]);
  assert.equal(latest, 400);
});

test("openingThread_clearsTheRootBadgeIncludingNestedReplies", () => {
  assert.equal(
    rootBadge(thread, () => null),
    3,
    "precondition: the 3 agent replies start unread",
  );
  assert.equal(rootBadge(thread, readAfterOpen(targetsFor(thread, "root"))), 0);
});

test("newerReplyAfterOpen_stillRelightsTheBadge", () => {
  const getReadAt = readAfterOpen(targetsFor(thread, "root"));
  const withNewer = [
    ...thread,
    msg("agentLater", "andyReply", 500, "agent", "root"),
  ];
  assert.equal(rootBadge(withNewer, getReadAt), 1);
});

test("threadWithNoReplies_hasNoThreadMarker", () => {
  const { replies, latest } = targetsFor(
    [msg("lonely", null, 100, "andy")],
    "lonely",
  );
  assert.deepEqual(replies, []);
  assert.equal(latest, null);
});

test("replyWithUnknownCreatedAt_isSkippedNotMarkedAtZero", () => {
  const byParent = buildDirectReplyIdsByParentId(thread);
  const createdAt = buildCreatedAtByMessageId(thread);
  createdAt.delete("agentNested2");
  const { replies, latest } = threadOpenReadTargets(
    "root",
    (id) => collectReplyDescendantIds(id, byParent),
    createdAt,
  );
  assert.ok(!replies.some(([id]) => id === "agentNested2"));
  assert.equal(latest, 350);
});
