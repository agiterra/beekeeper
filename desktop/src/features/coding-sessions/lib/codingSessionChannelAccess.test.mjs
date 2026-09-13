import assert from "node:assert/strict";
import test from "node:test";

import {
  PROJECT_LIST_FETCH_LIMIT,
  codingSessionChannelAccessAllowsSend,
  describeCodingSessionChannelAccess,
  projectCreatorFromAddress,
  resolveCodingSessionChannelAccess,
} from "./codingSessionChannelAccess.ts";

const ANDY = "a".repeat(64); // project creator
const BRIAN = "b".repeat(64); // roster owner, session founder
const COLLAB = "c".repeat(64);
const VIEWER = "d".repeat(64);
const INVITEE = "e".repeat(64); // per-session viewer grant, not on the roster
const PROJECT = `30621:${ANDY}:tank-loop`;

const ROSTER = [
  { pubkey: BRIAN, role: "owner" },
  { pubkey: COLLAB, role: "collaborator" },
  { pubkey: VIEWER, role: "viewer" },
];

function transport(overrides = {}) {
  return {
    channelType: "transport",
    isMember: false,
    projectRef: PROJECT,
    ...overrides,
  };
}

function resolve({
  channel = transport(),
  channelsStatus = "ready",
  currentUserPubkey,
  project = { status: "ready", found: true, truncated: false },
  roster = { status: "ready", members: ROSTER },
}) {
  return resolveCodingSessionChannelAccess({
    channel,
    channelsStatus,
    currentUserPubkey,
    project,
    roster,
  });
}

/**
 * The relay's transport write gate, restated independently of the client
 * code: an explicit member, or — when a project ACL row's coordinate equals
 * the channel's `project_ref` exactly — the creator in that coordinate or a
 * roster owner/collaborator (`get_channel_transport_gate`,
 * `ProjectGate::admits_write`). Every case below is checked against this,
 * not only against a string.
 */
function relayAdmitsWrite({ channel, pubkey, roster, aclCoordinates }) {
  if (channel.isMember) return true;
  if (channel.channelType !== "transport" || !channel.projectRef) return false;
  if (!aclCoordinates.includes(channel.projectRef)) return false;
  if (channel.projectRef.split(":")[1] === pubkey) return true;
  const role = roster.find((row) => row.pubkey === pubkey)?.role;
  return role === "owner" || role === "collaborator";
}

for (const [name, pubkey, channel, expected] of [
  ["explicit member", VIEWER, transport({ isMember: true }), "member"],
  [
    "explicit member of an ordinary channel",
    INVITEE,
    { channelType: "stream", isMember: true, projectRef: null },
    "member",
  ],
  ["project creator without membership", ANDY, transport(), "project-writer"],
  [
    "roster-owner founder on a transport without membership",
    BRIAN,
    transport(),
    "project-writer",
  ],
  ["project collaborator", COLLAB, transport(), "project-writer"],
  ["project viewer", VIEWER, transport(), "project-reader"],
  [
    "per-session viewer invitee not on the project roster",
    INVITEE,
    transport(),
    "project-reader",
  ],
  [
    "ordinary channel non-member, even a project owner",
    BRIAN,
    { channelType: "stream", isMember: false, projectRef: PROJECT },
    "not-member",
  ],
  [
    "transport with no project has no gate",
    BRIAN,
    transport({ projectRef: null }),
    "not-member",
  ],
]) {
  test(`relay rule: ${name}`, () => {
    const access = resolve({ channel, currentUserPubkey: pubkey });
    assert.equal(access.kind, expected);
    assert.equal(
      codingSessionChannelAccessAllowsSend(access),
      relayAdmitsWrite({
        channel,
        pubkey,
        roster: ROSTER,
        aclCoordinates: [PROJECT],
      }),
      "allow-send must match the relay's write gate",
    );
  });
}

test("a roster owner or collaborator carries the role that admitted them", () => {
  assert.deepEqual(resolve({ currentUserPubkey: BRIAN }), {
    kind: "project-writer",
    role: "owner",
  });
  assert.deepEqual(resolve({ currentUserPubkey: COLLAB }), {
    kind: "project-writer",
    role: "collaborator",
  });
  assert.deepEqual(resolve({ currentUserPubkey: VIEWER }), {
    kind: "project-reader",
    projectRole: "viewer",
  });
  assert.deepEqual(resolve({ currentUserPubkey: INVITEE }), {
    kind: "project-reader",
    projectRole: null,
  });
});

test("pubkeys compare case-insensitively", () => {
  assert.equal(
    resolve({ currentUserPubkey: BRIAN.toUpperCase() }).kind,
    "project-writer",
  );
});

test("a loading or failed roster is unresolved and refuses, never a verdict", () => {
  for (const status of ["loading", "error"]) {
    const access = resolve({
      currentUserPubkey: BRIAN,
      roster: { status, members: [] },
    });
    assert.deepEqual(access, {
      kind: "unresolved",
      reason: status === "error" ? "error" : "loading",
    });
    assert.equal(codingSessionChannelAccessAllowsSend(access), false);
  }
});

test("a creator whose project is absent from a complete list is not a writer", () => {
  // Deleted or never published: the relay's ACL join finds no row, so there
  // is no transport gate and only membership would admit.
  const access = resolve({
    currentUserPubkey: ANDY,
    project: { status: "ready", found: false, truncated: false },
  });
  assert.deepEqual(access, { kind: "not-member" });
  assert.equal(
    codingSessionChannelAccessAllowsSend(access),
    relayAdmitsWrite({
      channel: transport(),
      pubkey: ANDY,
      roster: ROSTER,
      aclCoordinates: [],
    }),
  );
  // A roster owner of a project that is gone is no writer either.
  assert.equal(
    codingSessionChannelAccessAllowsSend(
      resolve({
        currentUserPubkey: BRIAN,
        project: { status: "ready", found: false, truncated: false },
      }),
    ),
    false,
  );
});

test("an address with uppercase owner hex has no gate: not-member, even for its creator", () => {
  const upper = transport({
    projectRef: `30621:${ANDY.toUpperCase()}:tank-loop`,
  });
  for (const pubkey of [ANDY, BRIAN]) {
    const access = resolve({ channel: upper, currentUserPubkey: pubkey });
    assert.deepEqual(access, { kind: "not-member" });
    assert.equal(
      codingSessionChannelAccessAllowsSend(access),
      relayAdmitsWrite({
        channel: upper,
        pubkey,
        roster: ROSTER,
        aclCoordinates: [PROJECT],
      }),
    );
  }
});

test("a project missing from a list at its fetch limit is unresolved, not a verdict", () => {
  assert.equal(PROJECT_LIST_FETCH_LIMIT, 200);
  for (const pubkey of [ANDY, BRIAN, VIEWER]) {
    const access = resolve({
      currentUserPubkey: pubkey,
      project: { status: "ready", found: false, truncated: true },
    });
    assert.deepEqual(access, { kind: "unresolved", reason: "list-truncated" });
    assert.equal(codingSessionChannelAccessAllowsSend(access), false);
  }
  assert.equal(
    describeCodingSessionChannelAccess({
      kind: "unresolved",
      reason: "list-truncated",
    }).placeholder,
    "Couldn't confirm your project role from the loaded project list.",
  );
  // Found in a long list is still found.
  assert.equal(
    resolve({
      currentUserPubkey: BRIAN,
      project: { status: "ready", found: true, truncated: true },
    }).kind,
    "project-writer",
  );
});

test("a loading or failed project list is unresolved, even for the creator", () => {
  for (const status of ["loading", "error"]) {
    assert.deepEqual(
      resolve({
        currentUserPubkey: ANDY,
        project: { status, found: false, truncated: false },
      }),
      { kind: "unresolved", reason: status },
    );
  }
});

test("a non-creator missing from a complete list is at most a reader", () => {
  assert.deepEqual(
    resolve({
      currentUserPubkey: INVITEE,
      project: { status: "ready", found: false, truncated: false },
    }),
    { kind: "project-reader", projectRole: null },
  );
});

test("the creator needs no roster: the address already names them", () => {
  const access = resolve({
    currentUserPubkey: ANDY,
    roster: { status: "loading", members: [] },
  });
  assert.equal(access.kind, "project-writer");
});

test("an unknown identity on a transport is unresolved", () => {
  assert.equal(resolve({ currentUserPubkey: null }).kind, "unresolved");
});

test("the channel list decides whether the channel is absent", () => {
  assert.deepEqual(
    resolve({
      channel: null,
      channelsStatus: "loading",
      currentUserPubkey: BRIAN,
    }),
    { kind: "unresolved", reason: "loading" },
  );
  assert.deepEqual(
    resolve({
      channel: null,
      channelsStatus: "error",
      currentUserPubkey: BRIAN,
    }),
    { kind: "unresolved", reason: "error" },
  );
  assert.deepEqual(
    resolve({
      channel: null,
      channelsStatus: "ready",
      currentUserPubkey: BRIAN,
    }),
    { kind: "not-member" },
  );
});

test("project addresses parse only as 30621 with a hex owner", () => {
  assert.equal(projectCreatorFromAddress(PROJECT), ANDY);
  assert.equal(
    projectCreatorFromAddress(`30621:${ANDY.toUpperCase()}:x`),
    null,
  );
  assert.equal(projectCreatorFromAddress(`30617:${ANDY}:x`), null);
  assert.equal(projectCreatorFromAddress("30621:nothex:x"), null);
  assert.equal(projectCreatorFromAddress(`30621:${ANDY}`), null);
  assert.equal(projectCreatorFromAddress(null), null);
});

test("copy: allowed states say nothing; each refusal names its own state", () => {
  assert.equal(describeCodingSessionChannelAccess({ kind: "member" }), null);
  assert.equal(
    describeCodingSessionChannelAccess({
      kind: "project-writer",
      role: "owner",
    }),
    null,
  );

  const notMember = describeCodingSessionChannelAccess({ kind: "not-member" });
  assert.equal(notMember.placeholder, "Join this channel to send a message.");
  assert.equal(notMember.label, "View only");

  const viewer = describeCodingSessionChannelAccess({
    kind: "project-reader",
    projectRole: "viewer",
  });
  assert.equal(
    viewer.placeholder,
    "You can read this project session, but your project role (Viewer) does not allow steering.",
  );
  const invitee = describeCodingSessionChannelAccess({
    kind: "project-reader",
    projectRole: null,
  });
  assert.match(invitee.placeholder, /^You can read this project session, but/);
  for (const copy of [viewer, invitee]) {
    assert.doesNotMatch(Object.values(copy).join(" "), /Join this channel/);
    assert.equal(copy.label, "Read only");
  }

  const loading = describeCodingSessionChannelAccess({
    kind: "unresolved",
    reason: "loading",
  });
  const failed = describeCodingSessionChannelAccess({
    kind: "unresolved",
    reason: "error",
  });
  assert.equal(loading.placeholder, "Checking your access to this session…");
  assert.equal(loading.label, "Checking access");
  assert.equal(failed.label, "Access unknown");
  const truncated = describeCodingSessionChannelAccess({
    kind: "unresolved",
    reason: "list-truncated",
  });
  for (const copy of [loading, failed, truncated]) {
    const all = Object.values(copy).join(" ");
    assert.doesNotMatch(all, /Join this channel/);
    assert.doesNotMatch(all, /View only/);
  }
});
