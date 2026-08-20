import assert from "node:assert/strict";
import test from "node:test";

import {
  channelRoleModerates,
  decideMessageModeration,
  viewerChannelRole,
} from "./messageModerationAuthority.ts";

const VIEWER = "a".repeat(64);
const OTHER = "b".repeat(64);

function member(pubkey, role) {
  return {
    pubkey,
    role,
    isAgent: false,
    joinedAt: "2026-01-01T00:00:00Z",
    displayName: null,
  };
}

/** Base input: a real stream channel, viewer holds no community role. */
function input(overrides) {
  return {
    channelId: "channel-1",
    channelType: "stream",
    isCommunityManager: false,
    viewerPubkey: VIEWER,
    channelMembers: undefined,
    ...overrides,
  };
}

test("a channel owner with no community role is a moderator", () => {
  // The live defect: the user owned #general in `channel_members` but had no
  // `relay_members` row, so the app offered no delete control on a message the
  // relay would have accepted a kind:9005 for.
  assert.equal(
    decideMessageModeration(
      input({ channelMembers: [member(VIEWER, "owner")] }),
    ),
    true,
  );
});

test("a channel admin with no community role is a moderator", () => {
  assert.equal(
    decideMessageModeration(
      input({ channelMembers: [member(VIEWER, "admin")] }),
    ),
    true,
  );
});

test("channel member, guest, and bot roles grant nothing", () => {
  for (const role of ["member", "guest", "bot"]) {
    assert.equal(
      decideMessageModeration(
        input({ channelMembers: [member(VIEWER, role)] }),
      ),
      false,
      `channel role ${role} must not grant moderation`,
    );
  }
});

test("someone else's owner row grants the viewer nothing", () => {
  assert.equal(
    decideMessageModeration(
      input({
        channelMembers: [member(OTHER, "owner"), member(VIEWER, "member")],
      }),
    ),
    false,
  );
});

test("a viewer absent from the members list grants nothing", () => {
  assert.equal(
    decideMessageModeration(
      input({ channelMembers: [member(OTHER, "owner")] }),
    ),
    false,
  );
});

test("a community admin who is not a channel member stays a moderator", () => {
  // No regression on the community-wide path: the relay grants
  // CommunityAdmin without ever reading `channel_members`.
  assert.equal(
    decideMessageModeration(
      input({
        isCommunityManager: true,
        channelMembers: [member(OTHER, "owner")],
      }),
    ),
    true,
  );
  // ...and with the members query still unresolved, which is what actually
  // happens: the query is disabled for a community manager.
  assert.equal(
    decideMessageModeration(
      input({ isCommunityManager: true, channelMembers: undefined }),
    ),
    true,
  );
});

test("a channel owner in a DM is not a moderator", () => {
  // The carve-out precedes every role. The relay refuses DeleteMessage in a DM
  // for community owner and channel owner alike, so offering the control here
  // would be a lie about what it enforces.
  assert.equal(
    decideMessageModeration(
      input({
        channelType: "dm",
        channelMembers: [member(VIEWER, "owner")],
      }),
    ),
    false,
  );
  // Community authority does not reopen it either.
  assert.equal(
    decideMessageModeration(
      input({
        channelType: "dm",
        isCommunityManager: true,
        channelMembers: [member(VIEWER, "owner")],
      }),
    ),
    false,
  );
});

test("a loading or errored members query grants nothing", () => {
  // The hook passes `undefined` for pending, disabled, and errored alike —
  // an unconfirmed role must never grant the control.
  assert.equal(
    decideMessageModeration(input({ channelMembers: undefined })),
    false,
  );
  // An empty successful list is a confirmed "not a member".
  assert.equal(decideMessageModeration(input({ channelMembers: [] })), false);
});

test("an unresolved viewer pubkey grants nothing", () => {
  for (const viewerPubkey of [undefined, null, "", "   "]) {
    assert.equal(
      decideMessageModeration(
        input({ viewerPubkey, channelMembers: [member(VIEWER, "owner")] }),
      ),
      false,
      `viewerPubkey ${JSON.stringify(viewerPubkey)} must grant nothing`,
    );
  }
});

test("an unresolved channel id or channel type grants nothing", () => {
  const members = [member(VIEWER, "owner")];
  assert.equal(
    decideMessageModeration(
      input({ channelId: null, channelMembers: members }),
    ),
    false,
  );
  assert.equal(
    decideMessageModeration(
      input({ channelType: undefined, channelMembers: members }),
    ),
    false,
  );
  assert.equal(
    decideMessageModeration(
      input({ channelType: null, isCommunityManager: true }),
    ),
    false,
  );
});

test("forum channels honour channel roles too", () => {
  for (const channelType of ["forum"]) {
    assert.equal(
      decideMessageModeration(
        input({ channelType, channelMembers: [member(VIEWER, "admin")] }),
      ),
      true,
      `${channelType} must honour a channel admin`,
    );
  }
});

test("channelRoleModerates accepts only owner and admin", () => {
  assert.equal(channelRoleModerates("owner"), true);
  assert.equal(channelRoleModerates("admin"), true);
  for (const role of ["member", "guest", "bot", null, undefined]) {
    assert.equal(
      channelRoleModerates(role),
      false,
      `${JSON.stringify(role)} must not moderate`,
    );
  }
  // Not narrowed at the IPC boundary: anything outside the union fails closed.
  for (const role of ["OWNER", "moderator", "", "owner "]) {
    assert.equal(
      channelRoleModerates(role),
      false,
      `${JSON.stringify(role)} must not moderate`,
    );
  }
});

test("viewerChannelRole matches case-insensitively and fails closed", () => {
  const members = [member(VIEWER, "owner")];
  assert.equal(viewerChannelRole(members, VIEWER.toUpperCase()), "owner");
  assert.equal(viewerChannelRole(members, ` ${VIEWER} `), "owner");
  assert.equal(viewerChannelRole(members, OTHER), null);
  assert.equal(viewerChannelRole(undefined, VIEWER), null);
  assert.equal(viewerChannelRole([], VIEWER), null);
});
