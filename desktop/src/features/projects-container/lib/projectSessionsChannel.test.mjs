import assert from "node:assert/strict";
import test from "node:test";

import {
  projectSessionActivityByChannel,
  projectSessionsChannelDescription,
  projectSessionsChannelName,
  resolveProjectSessionsChannel,
  withoutProjectSessionTransportChannels,
} from "./projectSessionsChannel.ts";

test("the canonical name is the project name plus 'sessions'", () => {
  assert.equal(projectSessionsChannelName("Buzz Glue"), "Buzz Glue sessions");
  assert.equal(
    projectSessionsChannelName("  spaced   out  "),
    "spaced out sessions",
  );
  assert.equal(projectSessionsChannelName("   "), "sessions");
});

test("the canonical description identifies a generated session transport", () => {
  assert.equal(
    projectSessionsChannelDescription("  Buzz   Glue "),
    "Coding sessions for Buzz Glue.",
  );
  assert.equal(projectSessionsChannelDescription("   "), "Coding sessions.");
});

test("a dedicated transport disappears once its session row can replace it", () => {
  const channels = [
    {
      id: "sessions",
      name: "Buzz Glue sessions",
      description: "Coding sessions for Buzz Glue.",
    },
    { id: "general", name: "general", description: "Team chat" },
  ];
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Buzz Glue",
      channels,
      codingSessions: [{ channelId: "sessions" }],
    }),
    [channels[1]],
  );
});

test("a generated transport remains visible until a session row exists", () => {
  const channels = [
    {
      id: "sessions",
      name: "Buzz Glue sessions",
      description: "Coding sessions for Buzz Glue.",
    },
  ];
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Buzz Glue",
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
      projectName: "Buzz Glue",
      channels,
      codingSessions: [{ channelId: "general" }],
    }),
    channels,
  );
});

test("the generated description still identifies a renamed transport", () => {
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Buzz Glue",
      channels: [
        {
          id: "sessions",
          name: "agent work",
          description: "Coding sessions for Buzz Glue.",
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
      projectName: "Buzz  Glue",
      projectChannels: [
        { id: "general", name: "general" },
        { id: "sessions", name: "buzz glue SESSIONS" },
      ],
    }),
    { channelId: "sessions", reason: "name" },
  );
});

test("without a name match the channel with the newest session activity wins", () => {
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Buzz Glue",
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
      projectName: "Buzz Glue",
      projectChannels: [
        { id: "busy", name: "workbench" },
        { id: "named", name: "Buzz Glue sessions" },
      ],
      sessionActivityByChannel: new Map([["busy", "2026-08-09T00:00:00.000Z"]]),
    }),
    { channelId: "named", reason: "name" },
  );
});

test("a project with no named and no active channel has no sessions channel yet", () => {
  assert.equal(
    resolveProjectSessionsChannel({
      projectName: "Buzz Glue",
      projectChannels: [{ id: "general", name: "general" }],
    }),
    null,
  );
  assert.equal(
    resolveProjectSessionsChannel({
      projectName: "Buzz Glue",
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
    { id: "b", name: "Buzz Glue sessions" },
    { id: "a", name: "buzz glue sessions" },
  ];
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Buzz Glue",
      projectChannels: channels,
    }),
    { channelId: "a", reason: "name" },
  );
  assert.deepEqual(
    resolveProjectSessionsChannel({
      projectName: "Buzz Glue",
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
      projectName: "Buzz Glue",
      projectChannels: [
        // A user-named decoy that matches rule 1 must not beat the type.
        { id: "decoy", name: "Buzz Glue sessions", channelType: "stream" },
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

test("transport-typed channels are always hidden; the name heuristic stays for legacy", () => {
  const channels = [
    { id: "t", name: "Anything At All", channelType: "transport" },
    // Legacy transport: stream-typed but canonical name + hosting sessions.
    { id: "legacy", name: "Buzz Glue sessions", channelType: "stream" },
    // A user channel hosting a standalone session stays visible.
    { id: "chat", name: "general chat", channelType: "stream" },
    // A canonical name with no sessions is just a channel someone named.
    { id: "named", name: "Buzz Glue sessions", channelType: "stream" },
  ];
  assert.deepEqual(
    withoutProjectSessionTransportChannels({
      projectName: "Buzz Glue",
      channels,
      codingSessions: [{ channelId: "legacy" }, { channelId: "chat" }],
    }).map((channel) => channel.id),
    ["chat", "named"],
  );
});
