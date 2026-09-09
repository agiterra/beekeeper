import assert from "node:assert/strict";
import test from "node:test";

import {
  filterShareRecipientCandidates,
  isShareRecipientAgent,
} from "./PersonaShareRecipients.tsx";

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
/** A managed/relay agent whose owner never attested: the profile flag is false. */
const UNATTESTED_AGENT = {
  pubkey: "33".repeat(32),
  displayName: "Designer",
  avatarUrl: null,
  nip05Handle: null,
  ownerPubkey: null,
  isAgent: false,
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

test("omitting kind reproduces today's allowAgents behaviour exactly", () => {
  for (const allowAgents of [false, true]) {
    const derived = filter({ allowAgents, kind: undefined });
    const explicit = filter({
      allowAgents,
      kind: allowAgents ? "all" : "people",
    });
    assert.deepEqual(derived, explicit);
  }
  assert.deepEqual(filter({ allowAgents: false, kind: undefined }), [
    HUMAN.pubkey,
  ]);
  assert.deepEqual(filter({ allowAgents: true, kind: undefined }), [
    HUMAN.pubkey,
    AGENT.pubkey,
  ]);
});

test("each kind selects its own slice of the same fixture", () => {
  assert.deepEqual(filter({ allowAgents: true, kind: "all" }), [
    HUMAN.pubkey,
    AGENT.pubkey,
  ]);
  assert.deepEqual(filter({ allowAgents: true, kind: "people" }), [
    HUMAN.pubkey,
  ]);
  assert.deepEqual(filter({ allowAgents: true, kind: "agents" }), [
    AGENT.pubkey,
  ]);
});

test("an explicit kind wins over allowAgents in both directions", () => {
  // allowAgents off but the caller asked for agents: it gets agents.
  assert.deepEqual(filter({ allowAgents: false, kind: "agents" }), [
    AGENT.pubkey,
  ]);
  // allowAgents on but the caller asked for people: agents are hidden.
  assert.deepEqual(filter({ allowAgents: true, kind: "people" }), [
    HUMAN.pubkey,
  ]);
});

test("a locally known agent is an agent even when its profile says otherwise", () => {
  const users = [HUMAN, UNATTESTED_AGENT];
  const isKnownAgent = (pubkey) => pubkey === UNATTESTED_AGENT.pubkey;

  assert.deepEqual(filter({ kind: "agents", isKnownAgent, users }), [
    UNATTESTED_AGENT.pubkey,
  ]);
  assert.deepEqual(filter({ kind: "people", isKnownAgent, users }), [
    HUMAN.pubkey,
  ]);
  // Without the local baseline it still reads as "no agent evidence".
  assert.deepEqual(filter({ kind: "people", users }), [
    HUMAN.pubkey,
    UNATTESTED_AGENT.pubkey,
  ]);
});

test("the known-agent predicate only ever widens the profile flag", () => {
  const never = () => false;
  assert.equal(isShareRecipientAgent(AGENT, never), true);
  assert.equal(isShareRecipientAgent(AGENT), true);
  assert.equal(isShareRecipientAgent(UNATTESTED_AGENT), false);
  assert.equal(
    isShareRecipientAgent(UNATTESTED_AGENT, () => true),
    true,
  );
});

test("the known-agent predicate is asked with a normalised key", () => {
  const asked = [];
  const upper = {
    ...UNATTESTED_AGENT,
    pubkey: UNATTESTED_AGENT.pubkey.toUpperCase(),
  };
  isShareRecipientAgent(upper, (pubkey) => {
    asked.push(pubkey);
    return false;
  });
  assert.deepEqual(asked, [UNATTESTED_AGENT.pubkey]);
});

test("dedup drops normalised identical keys and nothing else", () => {
  const sameKeyDifferentCase = {
    ...HUMAN,
    pubkey: HUMAN.pubkey.toUpperCase(),
    displayName: "Ada (stale page)",
  };
  assert.deepEqual(filter({ users: [HUMAN, sameKeyDifferentCase] }), [
    HUMAN.pubkey,
  ]);

  // Same display name, different keys: both stay. This is the case the row's
  // short key exists to tell apart.
  const twin = { ...UNATTESTED_AGENT, displayName: HUMAN.displayName };
  assert.deepEqual(filter({ users: [HUMAN, twin] }), [
    HUMAN.pubkey,
    twin.pubkey,
  ]);
});

test("dedup keeps the first surviving row, not the first excluded one", () => {
  const duplicate = { ...AGENT, displayName: "Builder (page 2)" };
  assert.deepEqual(
    filter({
      allowAgents: true,
      excludedPubkeys: new Set([HUMAN.pubkey]),
      users: [AGENT, duplicate],
    }),
    [AGENT.pubkey],
  );
});
