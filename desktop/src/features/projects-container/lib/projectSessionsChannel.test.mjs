import assert from "node:assert/strict";
import test from "node:test";

import {
  projectSessionActivityByChannel,
  projectSessionsChannelName,
  resolveProjectSessionsChannel,
} from "./projectSessionsChannel.ts";

test("the canonical name is the project name plus 'sessions'", () => {
  assert.equal(projectSessionsChannelName("Buzz Glue"), "Buzz Glue sessions");
  assert.equal(
    projectSessionsChannelName("  spaced   out  "),
    "spaced out sessions",
  );
  assert.equal(projectSessionsChannelName("   "), "sessions");
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
