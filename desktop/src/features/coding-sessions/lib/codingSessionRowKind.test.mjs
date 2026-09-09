/**
 * The row-kind vocabulary, in isolation. No JSDOM: every rule here is a
 * statement about evidence, and none of it needs a DOM to be wrong.
 */
import assert from "node:assert/strict";
import { test } from "node:test";

const {
  codingSessionRosterBadgesSettled,
  codingSessionRowKind,
  codingSessionRowKindBadge,
  codingSessionRowKindIsFinal,
  codingSessionRowSentence,
  deriveCodingSessionProviderRuntimeLabels,
} = await import("./codingSessionRowKind.ts");

const NOTHING = {
  isFounder: false,
  isProvider: false,
  seatRole: null,
  isAgent: false,
};

test("no evidence is unidentified, never a person", () => {
  assert.equal(codingSessionRowKind(NOTHING), "unidentified");
});

test("a resolved profile is not evidence: `human` is not a value", () => {
  // There is no input that can produce a "person" answer — the only way to
  // leave `unidentified` is positive, verifiable agent-side evidence.
  const kinds = new Set();
  for (const isFounder of [false, true]) {
    for (const isProvider of [false, true]) {
      for (const seatRole of [null, "lead"]) {
        for (const isAgent of [false, true]) {
          kinds.add(
            codingSessionRowKind({
              isFounder,
              isProvider,
              seatRole,
              isAgent,
            }),
          );
        }
      }
    }
  }
  assert.deepEqual([...kinds].sort(), [
    "agent",
    "owner",
    "provider",
    "seated",
    "unidentified",
  ]);
});

test("the founder outranks every other fact", () => {
  assert.equal(
    codingSessionRowKind({
      isFounder: true,
      isProvider: true,
      seatRole: "lead",
      isAgent: true,
    }),
    "owner",
  );
});

test("a provider that also holds a seat is still first a provider", () => {
  assert.equal(
    codingSessionRowKind({
      ...NOTHING,
      isProvider: true,
      seatRole: "lead",
      isAgent: true,
    }),
    "provider",
  );
});

test("a seat outranks bare agent-ness: it is the more specific evidence", () => {
  assert.equal(
    codingSessionRowKind({ ...NOTHING, seatRole: "builder", isAgent: true }),
    "seated",
  );
  // A blank slug is not a seat.
  assert.equal(
    codingSessionRowKind({ ...NOTHING, seatRole: "   ", isAgent: true }),
    "agent",
  );
});

test("agent evidence alone reads as agent", () => {
  assert.equal(codingSessionRowKind({ ...NOTHING, isAgent: true }), "agent");
});

test("chain-derived kinds are final; profile-derived kinds are not", () => {
  assert.equal(codingSessionRowKindIsFinal("owner"), true);
  assert.equal(codingSessionRowKindIsFinal("provider"), true);
  assert.equal(codingSessionRowKindIsFinal("seated"), true);
  assert.equal(codingSessionRowKindIsFinal("agent"), false);
  assert.equal(codingSessionRowKindIsFinal("unidentified"), false);
});

test("badges wait for a real profile result, not placeholder labels", () => {
  // `useUsersBatchQuery` serves persisted labels as placeholderData, which
  // leaves `dataUpdatedAt` at 0 and carries no `isAgent` at all.
  assert.equal(
    codingSessionRosterBadgesSettled({
      memberCount: 3,
      profilesUpdatedAt: 0,
      profilesFailed: false,
    }),
    false,
  );
  assert.equal(
    codingSessionRosterBadgesSettled({
      memberCount: 3,
      profilesUpdatedAt: 1_759_000_000_000,
      profilesFailed: false,
    }),
    true,
  );
});

test("an empty roster and a failed batch both settle", () => {
  assert.equal(
    codingSessionRosterBadgesSettled({
      memberCount: 0,
      profilesUpdatedAt: 0,
      profilesFailed: false,
    }),
    true,
  );
  assert.equal(
    codingSessionRosterBadgesSettled({
      memberCount: 2,
      profilesUpdatedAt: 0,
      profilesFailed: true,
    }),
    true,
  );
});

test("a seat badge names its role slug", () => {
  assert.equal(
    codingSessionRowKindBadge({ kind: "seated", seatRole: "lead" }),
    "Seat · lead",
  );
  assert.equal(
    codingSessionRowKindBadge({ kind: "provider", seatRole: null }),
    "Provider",
  );
  assert.equal(
    codingSessionRowKindBadge({ kind: "unidentified", seatRole: null }),
    "Unidentified",
  );
});

const PROVIDER = "ab".repeat(32);
const OTHER = "cd".repeat(32);

test("provider labels come from the channel's own signed facts", () => {
  const labels = deriveCodingSessionProviderRuntimeLabels("channel-1", [
    {
      channelId: "channel-1",
      signerPubkey: PROVIDER.toUpperCase(),
      metadata: { provider: "codex-primary", runtime: "codex" },
    },
    {
      channelId: "channel-2",
      signerPubkey: OTHER,
      metadata: { provider: "other", runtime: "other" },
    },
  ]);
  assert.equal(labels.get(PROVIDER), "codex");
  assert.equal(labels.has(OTHER), false, "another channel is not this one");
});

test("a provider with no runtime label is still a provider", () => {
  const labels = deriveCodingSessionProviderRuntimeLabels("channel-1", [
    {
      channelId: "channel-1",
      signerPubkey: PROVIDER,
      metadata: { provider: null, runtime: null },
    },
  ]);
  assert.equal(labels.has(PROVIDER), true);
  assert.equal(labels.get(PROVIDER), null);
});

test("a later blank never erases a label the same key already reached", () => {
  const labels = deriveCodingSessionProviderRuntimeLabels("channel-1", [
    {
      channelId: "channel-1",
      signerPubkey: PROVIDER,
      metadata: { provider: null, runtime: "claude-code" },
    },
    {
      channelId: "channel-1",
      signerPubkey: PROVIDER,
      metadata: { provider: null, runtime: null },
    },
  ]);
  assert.equal(labels.get(PROVIDER), "claude-code");
});

test("a blank label falls back to the instance ref, then to null", () => {
  const labels = deriveCodingSessionProviderRuntimeLabels("channel-1", [
    {
      channelId: "channel-1",
      signerPubkey: PROVIDER,
      metadata: { provider: "codex-primary", runtime: "  " },
    },
  ]);
  assert.equal(labels.get(PROVIDER), "codex-primary");
});

test("every row states its capability in the invite menu's own words", () => {
  const collaborator = codingSessionRowSentence({
    kind: "agent",
    role: "collaborator",
    seatRole: null,
    providerLabel: null,
    hasProfile: true,
  });
  assert.equal(collaborator.capability, "Can edit and interact");
  const viewer = codingSessionRowSentence({
    kind: "unidentified",
    role: "viewer",
    seatRole: null,
    providerLabel: null,
    hasProfile: true,
  });
  assert.equal(viewer.capability, "Read-only access");
  const owner = codingSessionRowSentence({
    kind: "owner",
    role: "owner",
    seatRole: null,
    providerLabel: null,
    hasProfile: true,
  });
  assert.equal(owner.capability, "Full access, including managing members");
});

test("a provider row says it is a computer, and names its runtime", () => {
  const withLabel = codingSessionRowSentence({
    kind: "provider",
    role: "collaborator",
    seatRole: null,
    providerLabel: "codex",
    hasProfile: false,
  });
  assert.match(withLabel.evidence, /\(codex\)/);
  assert.match(withLabel.evidence, /a computer, not a person/);
  const withoutLabel = codingSessionRowSentence({
    kind: "provider",
    role: "collaborator",
    seatRole: null,
    providerLabel: null,
    hasProfile: false,
  });
  assert.match(withoutLabel.evidence, /a computer, not a person/);
});

test("the ungated row says only what it may do, never a kind", () => {
  const gated = codingSessionRowSentence({
    kind: null,
    role: "collaborator",
    seatRole: null,
    providerLabel: null,
    hasProfile: false,
  });
  assert.equal(gated.capability, "Can edit and interact");
  assert.equal(gated.evidence, null);
});

test("an unidentified row distinguishes no-profile from no-evidence", () => {
  const named = codingSessionRowSentence({
    kind: "unidentified",
    role: "collaborator",
    seatRole: null,
    providerLabel: null,
    hasProfile: true,
  });
  assert.equal(named.evidence, "No agent evidence held for this key");
  const anonymous = codingSessionRowSentence({
    kind: "unidentified",
    role: "collaborator",
    seatRole: null,
    providerLabel: null,
    hasProfile: false,
  });
  assert.equal(
    anonymous.evidence,
    "No profile and no agent evidence held for this key",
  );
});

test("a seated row names the seat it holds", () => {
  const seated = codingSessionRowSentence({
    kind: "seated",
    role: "collaborator",
    seatRole: "lead",
    providerLabel: null,
    hasProfile: false,
  });
  assert.equal(seated.evidence, "Holds the lead seat in this session");
});
