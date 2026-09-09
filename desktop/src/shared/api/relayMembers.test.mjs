import assert from "node:assert/strict";
import test from "node:test";
import { relayClient } from "./relayClient.ts";

import {
  canManageCommunityMembers,
  loadRelayMembershipLookup,
  shouldWarnMissingMembershipSnapshot,
} from "./relayMembers.ts";

test("open relays skip the membership snapshot request", async () => {
  const lookup = await loadRelayMembershipLookup("a".repeat(64), false, () =>
    assert.fail("open relays must not request a membership snapshot"),
  );

  assert.deepEqual(lookup, {
    snapshotFound: false,
    membershipRequired: false,
    membership: null,
  });
});

test("membership relays request their membership snapshot", async () => {
  const pubkey = "a".repeat(64);
  let requestCount = 0;
  const lookup = await loadRelayMembershipLookup(pubkey, true, async () => {
    requestCount += 1;
    return {
      created_at: 1,
      tags: [["member", pubkey, "admin"]],
    };
  });

  assert.equal(requestCount, 1);
  assert.equal(lookup.snapshotFound, true);
  assert.equal(lookup.membershipRequired, true);
  assert.equal(lookup.membership?.role, "admin");
});

test("community member management is hidden on open relays", () => {
  assert.equal(
    canManageCommunityMembers({
      snapshotFound: false,
      membershipRequired: false,
      membership: null,
    }),
    false,
  );
});

test("community member management is visible to closed-relay admins and owners", () => {
  for (const role of ["admin", "owner"]) {
    assert.equal(
      canManageCommunityMembers({
        snapshotFound: true,
        membershipRequired: true,
        membership: { pubkey: "a".repeat(64), role },
      }),
      true,
    );
  }
});

test("community member management stays hidden while loading and from closed-relay non-admins", () => {
  assert.equal(canManageCommunityMembers(undefined), false);
  assert.equal(
    canManageCommunityMembers({
      snapshotFound: true,
      membershipRequired: true,
      membership: { pubkey: "a".repeat(64), role: "member" },
    }),
    false,
  );
  assert.equal(
    canManageCommunityMembers({
      snapshotFound: true,
      membershipRequired: true,
      membership: null,
    }),
    false,
  );
});

test("missing snapshot warns when the relay requires membership", () => {
  assert.equal(
    shouldWarnMissingMembershipSnapshot({
      snapshotFound: false,
      membershipRequired: true,
      membership: null,
    }),
    true,
  );
});

test("missing snapshot is normal on an open relay", () => {
  assert.equal(
    shouldWarnMissingMembershipSnapshot({
      snapshotFound: false,
      membershipRequired: false,
      membership: null,
    }),
    false,
  );
});

test("an available snapshot never warns", () => {
  assert.equal(
    shouldWarnMissingMembershipSnapshot({
      snapshotFound: true,
      membershipRequired: true,
      membership: null,
    }),
    false,
  );
});

test("the default membership read uses a bounded authenticated snapshot and fails closed", async () => {
  const original = relayClient.fetchEventsBatch;
  const pubkey = "a".repeat(64);
  const reads = [];
  try {
    relayClient.fetchEventsBatch = async (filters) => {
      reads.push(filters);
      return [{ created_at: 1, tags: [["member", pubkey, "admin"]] }];
    };
    const lookup = await loadRelayMembershipLookup(pubkey, true);
    assert.equal(lookup.membership?.role, "admin");
    assert.deepEqual(reads, [[{ kinds: [13534], limit: 1 }]]);
    relayClient.fetchEventsBatch = async () => [];
    assert.equal(
      (await loadRelayMembershipLookup(pubkey, true)).membership,
      null,
    );
    relayClient.fetchEventsBatch = async () => {
      throw new Error("auth refused");
    };
    await assert.rejects(
      loadRelayMembershipLookup(pubkey, true),
      /auth refused/,
    );
  } finally {
    relayClient.fetchEventsBatch = original;
  }
});
