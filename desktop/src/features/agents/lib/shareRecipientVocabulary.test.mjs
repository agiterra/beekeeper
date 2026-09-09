import assert from "node:assert/strict";
import test from "node:test";

import {
  describeShareRecipientEmptyState,
  describeShareRecipientRow,
  hasResolvedShareRecipientProfile,
  listShareRecipientVocabulary,
  resolveShareRecipientKind,
  SHARE_RECIPIENT_KIND_NOTES,
  SHARE_RECIPIENT_KIND_TABS,
} from "./shareRecipientVocabulary.ts";

/**
 * The app holds no positive evidence that a key is a person, so no rendered
 * string may claim one. "People" is a name for an absence of agent evidence
 * and is spelled that way on purpose; the singular claims are what is banned.
 */
const FORBIDDEN = [
  /\bhumans?\b/i,
  /\bpersons?\b/i,
  /\bverified\b/i,
  /\bconfirmed\b/i,
  /\breal\s+(?:user|account)\b/i,
];

test("no string in the vocabulary claims a key belongs to a human being", () => {
  const strings = listShareRecipientVocabulary();
  assert.ok(strings.length > 0);
  for (const value of strings) {
    for (const pattern of FORBIDDEN) {
      assert.equal(
        pattern.test(value),
        false,
        `"${value}" matches banned claim ${pattern}`,
      );
    }
  }
});

test("the tabs are People / Agents / All, in that order", () => {
  assert.deepEqual(
    SHARE_RECIPIENT_KIND_TABS.map((tab) => tab.kind),
    ["people", "agents", "all"],
  );
  assert.deepEqual(
    SHARE_RECIPIENT_KIND_TABS.map((tab) => tab.label),
    ["People", "Agents", "All"],
  );
});

test("the People note says what People actually selected on", () => {
  assert.match(SHARE_RECIPIENT_KIND_NOTES.people, /no agent evidence/i);
  assert.match(SHARE_RECIPIENT_KIND_NOTES.people, /absence of evidence/i);
  assert.match(SHARE_RECIPIENT_KIND_NOTES.agents, /attestation/i);
  assert.match(SHARE_RECIPIENT_KIND_NOTES.all, /agents included/i);
});

test("an omitted kind falls back to allowAgents", () => {
  assert.equal(resolveShareRecipientKind({ allowAgents: false }), "people");
  assert.equal(resolveShareRecipientKind({ allowAgents: true }), "all");
  for (const kind of ["all", "people", "agents"]) {
    assert.equal(
      resolveShareRecipientKind({ allowAgents: false, kind }),
      kind,
      "an explicit kind wins",
    );
    assert.equal(resolveShareRecipientKind({ allowAgents: true, kind }), kind);
  }
});

test("a profile is resolved only when it carries a usable name", () => {
  assert.equal(
    hasResolvedShareRecipientProfile({ displayName: "Ada", nip05Handle: null }),
    true,
  );
  assert.equal(
    hasResolvedShareRecipientProfile({
      displayName: null,
      nip05Handle: "ada@example.com",
    }),
    true,
  );
  assert.equal(
    hasResolvedShareRecipientProfile({ displayName: "  ", nip05Handle: null }),
    false,
  );
  assert.equal(
    hasResolvedShareRecipientProfile({ displayName: null, nip05Handle: null }),
    false,
  );
});

test("a key with no profile is unidentified, never a confirmed anything", () => {
  const row = describeShareRecipientRow({
    hasProfile: false,
    isAgent: false,
    ownerLabel: null,
  });
  assert.equal(row.rowKind, "unidentified");
  assert.equal(row.label, "Unidentified");
  assert.equal(row.detail, "No profile on this relay");
  // The row's name is already the truncated key; do not print it twice.
  assert.equal(row.showShortKey, false);
});

test("a named key with no agent evidence claims nothing", () => {
  const row = describeShareRecipientRow({
    hasProfile: true,
    isAgent: false,
    ownerLabel: null,
  });
  assert.equal(row.rowKind, null);
  assert.equal(row.label, null);
  assert.equal(row.detail, null);
  assert.equal(
    row.showShortKey,
    true,
    "the short key tells same-name keys apart",
  );
});

test("an agent row names its owner, or says the owner is unknown", () => {
  const known = describeShareRecipientRow({
    hasProfile: true,
    isAgent: true,
    ownerLabel: "Ada",
  });
  assert.equal(known.rowKind, "agent");
  assert.equal(known.label, "Agent");
  assert.equal(known.detail, "Owner Ada");
  assert.equal(known.showShortKey, true);

  const unknown = describeShareRecipientRow({
    hasProfile: true,
    isAgent: true,
    ownerLabel: null,
  });
  assert.equal(unknown.detail, "Owner unknown");
});

test("an agent evidenced only locally still reads as an agent, not unidentified", () => {
  const row = describeShareRecipientRow({
    hasProfile: false,
    isAgent: true,
    ownerLabel: null,
  });
  assert.equal(row.rowKind, "agent");
  assert.equal(row.detail, "Owner unknown");
});

test("an exhausted view names the population it actually searched", () => {
  // All three are pinned: "No people found." is reserved for the People view,
  // and the All view says "No matches found." because it searched both
  // populations and "no people" would understate that.
  const messages = {
    all: "No matches found.",
    agents: "No agents found.",
    people: "No people found.",
  };
  for (const [kind, message] of Object.entries(messages)) {
    const state = describeShareRecipientEmptyState({
      hasMoreResults: false,
      isLoadingMore: false,
      kind,
    });
    assert.equal(state.message, message);
    assert.equal(state.loadMoreLabel, null);
    assert.equal(state.hint, null);
  }

  // No other view may borrow the People string.
  for (const kind of ["all", "agents"]) {
    for (const hasMoreResults of [true, false]) {
      const state = describeShareRecipientEmptyState({
        hasMoreResults,
        isLoadingMore: false,
        kind,
      });
      assert.notEqual(state.message, "No people found.");
    }
  }
});

test("an unexhausted view says only what it can see, and offers one press", () => {
  const messages = {
    all: "No matches in the results loaded so far.",
    agents: "No agents in the results loaded so far.",
    people: "No people in the results loaded so far.",
  };
  for (const [kind, message] of Object.entries(messages)) {
    const state = describeShareRecipientEmptyState({
      hasMoreResults: true,
      isLoadingMore: false,
      kind,
    });
    assert.equal(state.message, message);
    assert.notEqual(state.message, "No people found.");
    assert.equal(state.loadMoreLabel, "Load more results");
    assert.match(state.hint, /search by name or public key/i);
  }
});

test("the bounded load reports itself while a page is in flight", () => {
  const state = describeShareRecipientEmptyState({
    hasMoreResults: true,
    isLoadingMore: true,
    kind: "people",
  });
  assert.equal(state.loadMoreLabel, "Loading more results…");
});
