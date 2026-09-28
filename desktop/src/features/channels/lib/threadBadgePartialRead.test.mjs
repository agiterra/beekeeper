import assert from "node:assert/strict";
import test from "node:test";

import { computeThreadBadgeCounts } from "./threadBadgeCounts.ts";
import { buildRepliesByRootId } from "./subtreeCreatedAt.ts";

// Badge arithmetic under a PARTIAL read set (LP4 v3's per-message badge model).
//
// These cases were written for LP4 v3's open-at-level rule, where opening a
// thread marked read only the replies it revealed. That rule is gone: opening a
// thread now reads the whole thread, nested branches included, and
// useChannelUnreadState.test.mjs is the spec for that path. What survives — and
// what these cases actually exercise — is computeThreadBadgeCounts against a
// read set covering some replies and not others, still reached by expanding one
// branch (useChannelPaneHandlers' reveal path, which is still open-at-level),
// by a reply that arrives after the open, and by a thread nobody opened. The
// root summary badge reads effective(msg:<id>) live: a reply counts iff
// createdAt > readAt, so reading one reply never clears another.

// rootId travels with every reply (getThreadReference's `root` e-tag), so a
// nested reply rolls up to the thread root even when an ancestor is collapsed.
const msg = (id, parentId, createdAt = 100, pubkey = "author", rootId) => ({
  id,
  parentId,
  rootId: rootId ?? parentId ?? id,
  createdAt,
  pubkey,
});

const countAll = () => true;

// The live per-message getReadAt resolver for a partial read set: each id in
// `readIds` carries its own createdAt as its msg:<id> marker — the shape
// markMessageRead(id, createdAt) writes — and every other reply resolves to
// null, so it was never read and stays unread.
function markersFor(messages, readIds) {
  const read = new Set(readIds);
  const createdAtById = new Map(messages.map((m) => [m.id, m.createdAt]));
  return (id) => (read.has(id) ? (createdAtById.get(id) ?? null) : null);
}

const rootBadge = (messages, getReadAt, currentPubkey) =>
  computeThreadBadgeCounts(
    messages,
    buildRepliesByRootId(messages),
    getReadAt,
    countAll,
    currentPubkey,
  ).get("root");

test("partialRead_keepsUnreadGrandchildBadge", () => {
  // root -> a -> b, with only a read (expanding a's branch alone, or a
  // reply that landed after the read). b was never marked, so it keeps the
  // root badge lit: one reply's marker never speaks for another's.
  const messages = [
    msg("root", null, 50),
    msg("a", "root", 100),
    msg("b", "a", 200, "author", "root"),
  ];
  assert.equal(rootBadge(messages, markersFor(messages, ["a"])), 1);
});

test("wholeSubtreeRead_clearsRootBadge", () => {
  // When every reply is read the badge clears. This is the state a thread
  // open now always lands in (threadOpenReadTargets marks every descendant),
  // so it is also the post-open assertion.
  const messages = [
    msg("root", null, 50),
    msg("a", "root", 100),
    msg("b", "a", 200, "author", "root"),
  ];
  assert.equal(
    rootBadge(messages, markersFor(messages, ["a", "b"])),
    undefined,
  );
});

test("oneBranchRead_keepsOtherBranchBadge", () => {
  // root -> {a -> a1, c -> c1}, with branch a read and branch c not. The two
  // unread replies under c keep the root badge.
  const messages = [
    msg("root", null, 50),
    msg("a", "root", 100),
    msg("a1", "a", 110, "author", "root"),
    msg("c", "root", 120),
    msg("c1", "c", 130, "author", "root"),
  ];
  assert.equal(rootBadge(messages, markersFor(messages, ["a", "a1"])), 2);
});

test("newerReplyAfterOpen_relightsRootBadge", () => {
  // a(100) is read at its own createdAt. A newer reply b(200) arrives; the
  // predicate is strictly createdAt > readAt, so b is unread against its own
  // absent marker and the badge relights. This is why the open marks each
  // reply at its OWN createdAt rather than at the thread's newest.
  const messages = [
    msg("root", null, 50),
    msg("a", "root", 100),
    msg("b", "root", 200),
  ];
  // Only a was read; b is unread (never marked).
  assert.equal(rootBadge(messages, markersFor(messages, ["a"])), 1);
});

test("ownReplyIsTheOnlyUnread_neverShowsBadge", () => {
  // A nested reply authored by the current user. Self-authored replies are
  // excluded from the count, so no badge shows regardless of read state —
  // whatever the open path marks is inert here.
  const messages = [
    msg("root", null, 50),
    msg("a", "root", 100, "other"),
    msg("b", "a", 200, "ME", "root"),
  ];
  // Nothing revealed (never read), only "other"'s reply a could count.
  assert.equal(
    rootBadge(messages, () => null, "me"),
    1,
  );
  // After revealing a, only the self-authored b remains — no badge.
  assert.equal(
    rootBadge(messages, markersFor(messages, ["a"]), "me"),
    undefined,
  );
});

test("openThreadWhereEveryUnreadIsOwnReply_inertNoBadgeEver", () => {
  // Every reply is the user's own → no badge before OR after open.
  const messages = [
    msg("root", null, 50),
    msg("a", "root", 100, "ME"),
    msg("b", "a", 200, "ME", "root"),
  ];
  assert.equal(
    rootBadge(messages, () => null, "me"),
    undefined,
  );
  assert.equal(
    rootBadge(messages, markersFor(messages, ["a", "b"]), "me"),
    undefined,
  );
});
