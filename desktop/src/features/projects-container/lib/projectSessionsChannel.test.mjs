import assert from "node:assert/strict";
import test from "node:test";

import {
  projectSessionActivityByChannel,
  projectSessionsChannelCandidates,
  projectSessionsChannelDescription,
  projectSessionsChannelName,
  resolveProjectSessionsChannel,
  withoutProjectSessionTransportChannels,
} from "./projectSessionsChannel.ts";

test("the canonical name is the project name plus 'sessions'", () => {
  assert.equal(
    projectSessionsChannelName("Beekeeper Glue"),
    "Beekeeper Glue sessions",
  );
  assert.equal(
    projectSessionsChannelName("  spaced   out  "),
    "spaced out sessions",
  );
  assert.equal(projectSessionsChannelName("   "), "sessions");
});

test("the canonical description identifies a generated session transport", () => {
  assert.equal(
    projectSessionsChannelDescription("  Beekeeper   Glue "),
    "Coding sessions for Beekeeper Glue.",
  );
  assert.equal(projectSessionsChannelDescription("   "), "Coding sessions.");
});

test("a dedicated transport disappears once its session row can replace it", () => {
  const channels = [
    {
      id: "sessions",
      name: "Beekeeper Glue sessions",
      description: "Coding sessions for Beekeeper Glue.",
    },
    { id: "general", name: "general", description: "Team chat" },
  ];
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Beekeeper Glue",
      channels,
      codingSessions: [{ channelId: "sessions" }],
    }),
    [channels[1]],
  );
});

test("a stamped transport is hidden even before any session row exists", () => {
  // The prod repro: the old-relay fallback creates the channel seconds before
  // the first session's events land, and waiting for catalog ingestion left
  // the freshly created channel sitting in the sidebar. The canonical
  // description is this app's own creation stamp — hide on sight.
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Beekeeper Glue",
      channels: [
        {
          id: "sessions",
          name: "Beekeeper Glue sessions",
          description: "Coding sessions for Beekeeper Glue.",
        },
      ],
      codingSessions: [],
    }),
    [],
  );
});

test("a person's own '<project> sessions' channel stays visible while it hosts none", () => {
  // Only the name matches — no creation stamp, no hosted sessions. A human
  // could legitimately name a chat channel this way; keep it until sessions
  // actually live there.
  const channels = [
    {
      id: "sessions",
      name: "Beekeeper Glue sessions",
      description: "Planning",
    },
  ];
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Beekeeper Glue",
      channels,
      codingSessions: [],
    }),
    channels,
  );
});

test("an ordinary chat channel stays visible when it also hosts a session", () => {
  const channels = [
    { id: "general", name: "general", description: "Team chat" },
  ];
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Beekeeper Glue",
      channels,
      codingSessions: [{ channelId: "general" }],
    }),
    channels,
  );
});

test("the generated description still identifies a renamed transport", () => {
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Beekeeper Glue",
      channels: [
        {
          id: "sessions",
          name: "agent work",
          description: "Coding sessions for Beekeeper Glue.",
        },
      ],
      codingSessions: [{ channelId: "sessions" }],
    }),
    [],
  );
});

test("the named channel wins regardless of case and spacing", () => {
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper  Glue",
      projectChannels: [
        { id: "general", name: "general" },
        { id: "sessions", name: "beekeeper glue SESSIONS" },
      ],
    }),
    { channelId: "sessions", reason: "name" },
  );
});

test("without a name match the channel with the newest session activity wins", () => {
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper Glue",
      projectChannels: [
        { id: "stale", name: "old-agents" },
        { id: "recent", name: "workbench" },
        { id: "quiet", name: "general" },
      ],
      sessionActivityByChannel: new Map([
        ["stale", "2026-07-01T00:00:00.000Z"],
        ["recent", "2026-08-01T00:00:00.000Z"],
      ]),
    }),
    { channelId: "recent", reason: "activity" },
  );
});

test("a name match beats a busier channel — the published name is the mapping", () => {
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper Glue",
      projectChannels: [
        { id: "busy", name: "workbench" },
        { id: "named", name: "Beekeeper Glue sessions" },
      ],
      sessionActivityByChannel: new Map([["busy", "2026-08-09T00:00:00.000Z"]]),
    }),
    { channelId: "named", reason: "name" },
  );
});

test("a project with no named and no active channel has no sessions channel yet", () => {
  assert.equal(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper Glue",
      projectChannels: [{ id: "general", name: "general" }],
    }),
    null,
  );
  assert.equal(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper Glue",
      projectChannels: [],
      sessionActivityByChannel: new Map([
        ["not-in-project", "2026-08-01T00:00:00.000Z"],
      ]),
    }),
    null,
  );
});

test("duplicate name matches resolve to one stable channel", () => {
  const channels = [
    { id: "b", name: "Beekeeper Glue sessions" },
    { id: "a", name: "beekeeper glue sessions" },
  ];
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper Glue",
      projectChannels: channels,
    }),
    { channelId: "a", reason: "name" },
  );
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper Glue",
      projectChannels: [...channels].reverse(),
    }),
    { channelId: "a", reason: "name" },
  );
});

test("activity is the newest event per channel", () => {
  assert.deepEqual(
    [
      ...projectSessionActivityByChannel([
        {
          channelId: "a",
          session: { lastEventAt: "2026-08-01T00:00:00.000Z" },
        },
        {
          channelId: "a",
          session: { lastEventAt: "2026-08-05T00:00:00.000Z" },
        },
        {
          channelId: "b",
          session: { lastEventAt: "2026-07-01T00:00:00.000Z" },
        },
      ]),
    ],
    [
      ["a", "2026-08-05T00:00:00.000Z"],
      ["b", "2026-07-01T00:00:00.000Z"],
    ],
  );
});

test("a transport-typed channel wins the resolution outright", () => {
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Beekeeper Glue",
      projectChannels: [
        // A user-named decoy that matches rule 1 must not beat the type.
        { id: "decoy", name: "Beekeeper Glue sessions", channelType: "stream" },
        { id: "t2", name: "whatever", channelType: "transport" },
        { id: "t1", name: "anything", channelType: "transport" },
      ],
      sessionActivityByChannel: new Map([
        ["decoy", "2026-08-14T00:00:00.000Z"],
      ]),
    }),
    // Deterministic across members: lowest id among transports.
    { channelId: "t1", reason: "transport" },
  );
});

test("among several transports the one with the newest session activity wins", () => {
  // The dev relay grew seven transports for one project (one per create,
  // 2026-09-08); the sessions live in three of them. Every device must pick
  // the same one, and it must be one the provider already advertises in.
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Mobile Test",
      projectChannels: [
        {
          id: "t-empty-a",
          name: "Mobile Test sessions",
          channelType: "transport",
        },
        { id: "t-old", name: "Mobile Test sessions", channelType: "transport" },
        { id: "t-new", name: "Mobile Test sessions", channelType: "transport" },
        {
          id: "t-empty-b",
          name: "Mobile Test sessions",
          channelType: "transport",
        },
      ],
      sessionActivityByChannel: new Map([
        ["t-old", "2026-09-07T23:42:58.000Z"],
        ["t-new", "2026-09-08T08:44:29.000Z"],
      ]),
    }),
    { channelId: "t-new", reason: "transport" },
  );
});

test("the candidate list keeps a project's transports (the sidebar partition does not)", () => {
  const project = {
    address: "30621:owner:mobile-test",
    channelIds: ["listed-stream", "listed-transport"],
  };
  const channels = [
    { id: "listed-stream", channelType: "stream" },
    { id: "listed-transport", channelType: "transport" },
    // Claimed only by its own back-reference, in the raw `kind:owner:dtag` form.
    {
      id: "backref-transport",
      channelType: "transport",
      projectRef: "30621:owner:mobile-test",
    },
    {
      id: "other-project",
      channelType: "transport",
      projectRef: "30621:owner:other",
    },
    { id: "unclaimed", channelType: "stream", projectRef: null },
  ];
  assert.deepEqual(
    projectSessionsChannelCandidates(project, channels).map(
      (channel) => channel.id,
    ),
    ["listed-stream", "listed-transport", "backref-transport"],
  );
  // And the resolution over that list reaches rule 0.
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Mobile Test",
      projectChannels: projectSessionsChannelCandidates(project, channels),
    }),
    { channelId: "backref-transport", reason: "transport" },
  );
});

test("transport-typed channels are always hidden; the name heuristic stays for legacy", () => {
  const channels = [
    { id: "t", name: "Anything At All", channelType: "transport" },
    // Legacy transport: stream-typed but canonical name + hosting sessions.
    { id: "legacy", name: "Beekeeper Glue sessions", channelType: "stream" },
    // A user channel hosting a standalone session stays visible.
    { id: "chat", name: "general chat", channelType: "stream" },
    // A canonical name with no sessions is just a channel someone named.
    { id: "named", name: "Beekeeper Glue sessions", channelType: "stream" },
  ];
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Beekeeper Glue",
      channels,
      codingSessions: [{ channelId: "legacy" }, { channelId: "chat" }],
    }).map((channel) => channel.id),
    ["chat", "named"],
  );
});
