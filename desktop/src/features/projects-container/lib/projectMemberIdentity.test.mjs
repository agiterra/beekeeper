import assert from "node:assert/strict";
import test from "node:test";

import { projectMemberIdentity } from "./projectMemberIdentity.ts";

const KEY = `5f6c1173${"0".repeat(52)}4913`;
const truncate = (pubkey) => `${pubkey.slice(0, 8)}…${pubkey.slice(-4)}`;

// Ledger 207(4), the live defect: eight managed agents rendered as hex.
test("a managed agent on this computer shows its name and role", () => {
  const row = projectMemberIdentity({
    pubkey: KEY,
    managedAgent: { pubkey: KEY, name: "Verifier 2", homeRole: "verifier" },
    truncate,
  });
  assert.equal(row.name, "Verifier 2");
  assert.equal(row.role, "verifier");
  assert.equal(row.kind, "agent");
  assert.equal(row.showKey, true, "the hex stays as secondary text");
});

test("a published profile name wins the line; the local record still gives the role", () => {
  const row = projectMemberIdentity({
    pubkey: KEY,
    profileName: "  Verifier Two  ",
    managedAgent: { pubkey: KEY, name: "Verifier 2", homeRole: "verifier" },
    truncate,
  });
  assert.equal(row.name, "Verifier Two");
  assert.equal(row.role, "verifier");
  assert.equal(row.kind, "agent");
});

test("a profile that declares itself an agent is still an agent", () => {
  const row = projectMemberIdentity({
    pubkey: KEY,
    profileName: "Scout",
    profileIsAgent: true,
    truncate,
  });
  assert.equal(row.kind, "agent");
  assert.equal(row.name, "Scout");
});

// The memory rule: never call a missing profile a human.
test("a key nothing knows is unknown, not a person", () => {
  const row = projectMemberIdentity({ pubkey: KEY, truncate });
  assert.equal(row.name, truncate(KEY));
  assert.equal(row.kind, "unknown");
  assert.equal(row.role, null);
});

test("an ordinary published profile is a profile, not an agent", () => {
  const row = projectMemberIdentity({
    pubkey: KEY,
    profileName: "Brian",
    profileIsAgent: false,
    truncate,
  });
  assert.equal(row.kind, "profile");
});

test("a blank managed-agent name falls back to the key, never to a guess", () => {
  const row = projectMemberIdentity({
    pubkey: KEY,
    managedAgent: { pubkey: KEY, name: "   ", homeRole: "  " },
    truncate,
  });
  assert.equal(row.name, truncate(KEY));
  assert.equal(row.role, null);
  assert.equal(row.kind, "agent");
});

// Ledger 266: a private project's roster names this computer's
// session-provider key so the host can read the project; the row says so.
test("this computer's provider key reads as This computer, with its key", () => {
  const row = projectMemberIdentity({
    pubkey: KEY,
    hostPubkey: KEY.toUpperCase(),
    managedAgent: { pubkey: KEY, name: "Verifier 2", homeRole: "verifier" },
    truncate,
  });
  assert.equal(row.name, "This computer");
  assert.equal(row.kind, "host");
  assert.equal(row.role, null);
  assert.equal(row.showKey, true);
});

test("another computer's provider key is not this computer", () => {
  const row = projectMemberIdentity({
    pubkey: KEY,
    hostPubkey: "f".repeat(64),
    truncate,
  });
  assert.equal(row.kind, "unknown");
  assert.notEqual(row.name, "This computer");
});
