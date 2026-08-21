import assert from "node:assert/strict";
import test from "node:test";

import {
  canManageMessageForCurrentUser,
  messageManageAuthority,
  requiresModeratorDelete,
} from "./canManageMessage.ts";

const CURRENT = "a".repeat(64);
const OWNED_AGENT = "b".repeat(64);
const OTHER_PERSON = "c".repeat(64);

const KIND_STREAM_MESSAGE = 9;
const KIND_HUDDLE_STARTED = 48100;

const message = (pubkey, overrides = {}) => ({
  kind: KIND_STREAM_MESSAGE,
  pubkey,
  ...overrides,
});
const profiles = {
  [OWNED_AGENT]: { isAgent: true, ownerPubkey: CURRENT },
};
const asModerator = { isModerator: true };

test("self-authored messages resolve to self authority", () => {
  assert.equal(
    messageManageAuthority(message(CURRENT), CURRENT, profiles),
    "self",
  );
});

test("messages from an agent the viewer owns resolve to self authority", () => {
  assert.equal(
    messageManageAuthority(message(OWNED_AGENT), CURRENT, profiles),
    "self",
  );
});

test("another user's message grants nothing without moderator standing", () => {
  assert.equal(
    messageManageAuthority(message(OTHER_PERSON), CURRENT, profiles),
    null,
  );
  assert.equal(
    canManageMessageForCurrentUser(message(OTHER_PERSON), CURRENT, profiles),
    false,
  );
});

test("a moderator gets moderator authority over another user's message", () => {
  assert.equal(
    messageManageAuthority(
      message(OTHER_PERSON),
      CURRENT,
      profiles,
      asModerator,
    ),
    "moderator",
  );
  assert.equal(
    canManageMessageForCurrentUser(
      message(OTHER_PERSON),
      CURRENT,
      profiles,
      asModerator,
    ),
    true,
  );
});

test("a moderator's own message stays on self authority", () => {
  // Load-bearing: the caller picks the delete kind from this value, and a
  // "moderator" answer here would publish kind:9005 for an ordinary
  // self-delete — painting a public `message_deleted` tombstone every time
  // someone removed their own message.
  assert.equal(
    messageManageAuthority(message(CURRENT), CURRENT, profiles, asModerator),
    "self",
  );
  assert.equal(
    messageManageAuthority(
      message(OWNED_AGENT),
      CURRENT,
      profiles,
      asModerator,
    ),
    "self",
  );
});

test("huddle-started cards are immutable even for a moderator", () => {
  assert.equal(
    messageManageAuthority(
      message(OTHER_PERSON, { kind: KIND_HUDDLE_STARTED }),
      CURRENT,
      profiles,
      asModerator,
    ),
    null,
  );
  assert.equal(
    messageManageAuthority(
      message(CURRENT, { kind: KIND_HUDDLE_STARTED }),
      CURRENT,
      profiles,
    ),
    null,
  );
});

test("a message with no author grants nothing, moderator or not", () => {
  assert.equal(
    messageManageAuthority(message(undefined), CURRENT, profiles, asModerator),
    null,
  );
});

test("a self-authored target takes the self delete path", () => {
  // The moderation queue's "Delete content" used to pass `moderator: true`
  // unconditionally, so a moderator resolving a report against their OWN
  // message published a kind:9005 — and the relay answers that with a public
  // `message_deleted` tombstone in the channel naming them as the moderator
  // who deleted themselves.
  assert.equal(requiresModeratorDelete(CURRENT, CURRENT), false);
  // Case-insensitive: pubkeys arrive hex-cased differently per surface.
  assert.equal(requiresModeratorDelete(CURRENT.toUpperCase(), CURRENT), false);
});

test("another user's message needs the moderator delete path", () => {
  assert.equal(requiresModeratorDelete(OTHER_PERSON, CURRENT), true);
});

test("an unresolvable author or viewer stays on the moderator path", () => {
  // No identity means no self claim to make; the relay decides from there.
  assert.equal(requiresModeratorDelete(null, CURRENT), true);
  assert.equal(requiresModeratorDelete(undefined, CURRENT), true);
  assert.equal(requiresModeratorDelete(OTHER_PERSON, undefined), true);
});

test("a signed-out viewer may still moderate but never self-manage", () => {
  assert.equal(
    messageManageAuthority(message(OTHER_PERSON), undefined, profiles),
    null,
  );
  assert.equal(
    messageManageAuthority(
      message(OTHER_PERSON),
      undefined,
      profiles,
      asModerator,
    ),
    "moderator",
  );
});
