import assert from "node:assert/strict";
import test from "node:test";

import { ensureProviderChannelMembership } from "./providerChannelMembership.ts";

const PROVIDER = "d".repeat(64);
const CHANNEL = "channel-1";

function member(pubkey) {
  return {
    pubkey,
    role: "bot",
    isAgent: true,
    joinedAt: "2026-08-17T00:00:00Z",
    displayName: null,
  };
}

function okAdd() {
  return async () => ({ added: [PROVIDER], errors: [] });
}

test("a verified add resolves without retrying", async () => {
  const reads = [];
  await ensureProviderChannelMembership({
    channelId: CHANNEL,
    providerPubkey: PROVIDER,
    deps: {
      addChannelMembers: okAdd(),
      getChannelMembers: async (channelId) => {
        reads.push(channelId);
        return [member(PROVIDER)];
      },
      delayMs: 0,
    },
  });
  assert.deepEqual(reads, [CHANNEL]);
});

test("an 'already a member' echo is tolerated, everything else throws with a remedy", async () => {
  await ensureProviderChannelMembership({
    channelId: CHANNEL,
    providerPubkey: PROVIDER,
    deps: {
      addChannelMembers: async () => ({
        added: [],
        errors: [{ pubkey: PROVIDER, error: "already a member" }],
      }),
      getChannelMembers: async () => [member(PROVIDER)],
      delayMs: 0,
    },
  });

  await assert.rejects(
    ensureProviderChannelMembership({
      channelId: CHANNEL,
      providerPubkey: PROVIDER,
      channelLabel: "tankloop",
      deps: {
        addChannelMembers: async () => ({
          added: [],
          errors: [{ pubkey: PROVIDER, error: "relay rejected event" }],
        }),
        getChannelMembers: async () => [member(PROVIDER)],
        delayMs: 0,
      },
    }),
    /Could not add the session provider to #tankloop: relay rejected event/,
  );
});

test("an accepted add that never reaches the roster is a hard failure, not a wait", async () => {
  // The relay's accepted:true means "stored", not "applied" — a membership
  // write whose side effect fails leaves the roster unchanged with no error
  // on the wire. The read-back is the only place that lie becomes visible.
  let reads = 0;
  await assert.rejects(
    ensureProviderChannelMembership({
      channelId: CHANNEL,
      providerPubkey: PROVIDER,
      deps: {
        addChannelMembers: okAdd(),
        getChannelMembers: async () => {
          reads += 1;
          return [member("a".repeat(64))];
        },
        delayMs: 0,
        attempts: 3,
      },
    }),
    /reported added .* but never appeared in its member list/s,
  );
  assert.equal(reads, 3);
});

test("a slow roster is retried until the provider appears", async () => {
  let reads = 0;
  await ensureProviderChannelMembership({
    channelId: CHANNEL,
    providerPubkey: PROVIDER,
    deps: {
      addChannelMembers: okAdd(),
      getChannelMembers: async () => {
        reads += 1;
        return reads < 2 ? [] : [member(PROVIDER.toUpperCase())];
      },
      delayMs: 0,
    },
  });
  assert.equal(reads, 2);
});

test("a roster read that throws counts as a miss, not a crash", async () => {
  let reads = 0;
  await assert.rejects(
    ensureProviderChannelMembership({
      channelId: CHANNEL,
      providerPubkey: PROVIDER,
      deps: {
        addChannelMembers: okAdd(),
        getChannelMembers: async () => {
          reads += 1;
          throw new Error("relay unreachable");
        },
        delayMs: 0,
        attempts: 2,
      },
    }),
    /never appeared in its member list/,
  );
  assert.equal(reads, 2);
});
