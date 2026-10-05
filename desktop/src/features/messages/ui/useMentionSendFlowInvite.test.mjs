import assert from "node:assert/strict";
import { test } from "node:test";

import {
  membershipForMentionSend,
  nonMemberMentionPromptPubkeys,
  resolveMentionMembership,
} from "@/features/messages/ui/useMentionSendFlowInvite";

const ALICE = "a".repeat(64);
const BOB = "b".repeat(64);
const CAROL = "c".repeat(64);

function composer(overrides = {}) {
  return {
    channelId: "chan-1",
    hasResolvedMembers: false,
    memberPubkeys: new Set(),
    ...overrides,
  };
}

/**
 * SV-50. The old send path read "members not resolved yet" as "nobody to ask
 * about" and sent without an Invite prompt. A mention sent before the member
 * read answers must wait for that read and then prompt for whoever is outside.
 */
test("a mention sent before members resolve waits for the read, then asks", async () => {
  let release;
  const read = new Promise((resolve) => {
    release = resolve;
  });
  const fetched = [];
  const pending = resolveMentionMembership({
    targetChannelId: "chan-1",
    composer: composer(),
    fetchMemberPubkeys: (channelId) => {
      fetched.push(channelId);
      return read;
    },
  });
  let settled = false;
  void pending.then(() => {
    settled = true;
  });
  await Promise.resolve();
  assert.equal(settled, false, "the decision waits on the member read");
  release([ALICE.toUpperCase()]);
  const membership = await pending;
  assert.deepEqual(fetched, ["chan-1"]);
  assert.equal(membership.kind, "resolved");
  assert.deepEqual(
    nonMemberMentionPromptPubkeys({
      channelType: "stream",
      mentionPubkeys: [ALICE, BOB],
      membership,
    }),
    [BOB],
  );
});

test("a failed member read is unknown, and unknown asks about every mention", async () => {
  const membership = await resolveMentionMembership({
    targetChannelId: "chan-1",
    composer: composer(),
    fetchMemberPubkeys: async () => {
      throw new Error("relay timed out");
    },
  });
  assert.deepEqual(membership, { kind: "unknown", reason: "relay timed out" });
  assert.deepEqual(
    nonMemberMentionPromptPubkeys({
      channelType: "stream",
      mentionPubkeys: [ALICE, BOB, ALICE],
      membership,
    }),
    [ALICE, BOB],
  );
});

test("the composer's settled list is reused only for its own channel", async () => {
  const members = new Set([ALICE]);
  const reused = await resolveMentionMembership({
    targetChannelId: "chan-1",
    composer: composer({ hasResolvedMembers: true, memberPubkeys: members }),
    fetchMemberPubkeys: async () => {
      throw new Error("must not read");
    },
  });
  assert.equal(reused.kind, "resolved");
  assert.equal(reused.memberPubkeys, members);

  const other = await resolveMentionMembership({
    targetChannelId: "chan-2",
    composer: composer({ hasResolvedMembers: true, memberPubkeys: members }),
    fetchMemberPubkeys: async () => [CAROL],
  });
  assert.deepEqual([...other.memberPubkeys], [CAROL]);
});

test("no channel and no settled list is unknown, not an empty channel", async () => {
  const membership = await resolveMentionMembership({
    targetChannelId: null,
    composer: composer({ channelId: null }),
    fetchMemberPubkeys: async () => [],
  });
  assert.equal(membership.kind, "unknown");
});

test("DMs never prompt, whatever the membership read says", () => {
  for (const membership of [
    { kind: "unknown", reason: "x" },
    { kind: "resolved", memberPubkeys: new Set() },
  ]) {
    assert.deepEqual(
      nonMemberMentionPromptPubkeys({
        channelType: "dm",
        mentionPubkeys: [ALICE],
        membership,
      }),
      [],
    );
  }
});

test("a member read that never answers becomes unknown at the deadline", async () => {
  const membership = await resolveMentionMembership({
    targetChannelId: "chan-1",
    composer: composer(),
    fetchMemberPubkeys: () => new Promise(() => {}),
    timeoutMs: 5,
  });
  assert.equal(membership.kind, "unknown");
  assert.match(membership.reason, /did not answer/);
});

/**
 * SV-50 must not slow the plain send: a message that mentions nobody, or only
 * agents the prompt never asks about, goes out without reading members.
 */
test("a send with no mentions never calls the member fetch", async () => {
  let fetches = 0;
  const membership = await membershipForMentionSend({
    channelType: "stream",
    mentionPubkeys: [],
    isExemptFromPrompt: () => false,
    resolve: () =>
      resolveMentionMembership({
        targetChannelId: "chan-1",
        composer: composer(),
        fetchMemberPubkeys: async () => {
          fetches += 1;
          return [];
        },
      }),
  });
  assert.equal(membership, null);
  assert.equal(fetches, 0);
});

test("only exempt agent mentions skip the read; one person mention waits for it", async () => {
  let reads = 0;
  const resolve = async () => {
    reads += 1;
    return { kind: "resolved", memberPubkeys: new Set([ALICE]) };
  };
  const isExemptFromPrompt = (pubkey) => pubkey === CAROL;
  assert.equal(
    await membershipForMentionSend({
      channelType: "stream",
      mentionPubkeys: [CAROL],
      isExemptFromPrompt,
      resolve,
    }),
    null,
  );
  assert.equal(
    await membershipForMentionSend({
      channelType: "dm",
      mentionPubkeys: [BOB],
      isExemptFromPrompt,
      resolve,
    }),
    null,
  );
  assert.equal(reads, 0);
  const membership = await membershipForMentionSend({
    channelType: "stream",
    mentionPubkeys: [CAROL, BOB],
    isExemptFromPrompt,
    resolve,
  });
  assert.equal(reads, 1);
  assert.equal(membership?.kind, "resolved");
});
