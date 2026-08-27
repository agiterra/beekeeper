import assert from "node:assert/strict";
import test from "node:test";

import { filterShareRecipientCandidates } from "./PersonaShareRecipients.tsx";

const HUMAN = {
  pubkey: "11".repeat(32),
  displayName: "Ada",
  avatarUrl: null,
  nip05Handle: null,
  ownerPubkey: null,
  isAgent: false,
};
const AGENT = {
  pubkey: "22".repeat(32),
  displayName: "Builder",
  avatarUrl: null,
  nip05Handle: null,
  ownerPubkey: HUMAN.pubkey,
  isAgent: true,
};

function filter(overrides = {}) {
  return filterShareRecipientCandidates({
    allowAgents: false,
    currentPubkey: null,
    excludedPubkeys: new Set(),
    isArchived: () => false,
    selectedPubkeys: new Set(),
    users: [HUMAN, AGENT],
    ...overrides,
  }).map((user) => user.pubkey);
}

test("agents are hidden unless the surface asks for them", () => {
  assert.deepEqual(filter(), [HUMAN.pubkey]);
});

test("a surface that grants to agents gets them by name", () => {
  assert.deepEqual(filter({ allowAgents: true }), [HUMAN.pubkey, AGENT.pubkey]);
});

test("allowing agents does not loosen any other exclusion", () => {
  assert.deepEqual(filter({ allowAgents: true, currentPubkey: HUMAN.pubkey }), [
    AGENT.pubkey,
  ]);
  assert.deepEqual(
    filter({ allowAgents: true, excludedPubkeys: new Set([AGENT.pubkey]) }),
    [HUMAN.pubkey],
  );
  assert.deepEqual(
    filter({ allowAgents: true, selectedPubkeys: new Set([AGENT.pubkey]) }),
    [HUMAN.pubkey],
  );
  assert.deepEqual(
    filter({
      allowAgents: true,
      isArchived: (pubkey) => pubkey === AGENT.pubkey,
    }),
    [HUMAN.pubkey],
  );
});
