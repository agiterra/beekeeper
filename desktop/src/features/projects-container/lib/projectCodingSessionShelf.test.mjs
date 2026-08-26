import assert from "node:assert/strict";
import test from "node:test";

import {
  bucketProjectCodingSessions,
  parseActiveProjectCodingSessionPath,
  resolveProjectCodingSessionPlacement,
  resolveProjectCodingSessionShelf,
  withProjectCodingSessionProjectionError,
} from "./projectCodingSessionShelf.ts";

function session(overrides = {}) {
  return {
    generationId: "opaque-generation-id",
    label: "Session actor-secret / generation 7",
    title: "Coding session",
    providerAuthorityPubkey: null,
    metadataAuthorityPubkey: null,
    lastEventAt: "2026-07-30T12:00:00.000Z",
    status: "unknown",
    transcript: [],
    conflictCount: 0,
    projectRef: null,
    repoRef: null,
    sessionRef: null,
    provider: null,
    runtime: null,
    model: null,
    capabilities: null,
    commandTarget: {
      driver: "hive-seat",
      instanceId: "private-instance",
      sessionId: "private-seat",
      generation: 7,
    },
    ...overrides,
  };
}

function catalog(entries, overrides = {}) {
  return {
    entries,
    isLoading: false,
    errorMessage: null,
    authorityErrorMessage: null,
    ...overrides,
  };
}

function index({ byRef = [], byChannel = [] } = {}) {
  return {
    projectIdByRef: new Map(byRef),
    projectIdByChannel: new Map(byChannel),
  };
}

test("trusted global entries stay unassigned and preserve exact route coordinates without id leakage", () => {
  const shelf = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "sessions-channel",
        session: session({ title: "buzz-glue" }),
      },
    ]),
    index(),
    new Map([["sessions-channel", "Project Sessions"]]),
  );
  const { entries } = shelf;

  assert.equal(entries.length, 1);
  assert.equal(shelf.state.kind, "ready");
  assert.equal(entries[0].placement, "unassigned");
  assert.equal(entries[0].projectId, null);
  assert.equal(entries[0].channelId, "sessions-channel");
  assert.equal(entries[0].generationId, "opaque-generation-id");
  assert.equal(entries[0].label, "buzz-glue");
  assert.equal(entries[0].sourceChannelLabel, "Project Sessions");
  assert.equal(entries[0].runtimeLabel, "Hive Seat");
  assert.doesNotMatch(
    entries[0].label,
    /actor-secret|private-instance|private-seat/,
  );
});

test("provider executions sharing an umbrella render as one durable session row", () => {
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "claude-generation",
          sessionRef: "shared-session",
          runtime: "claude-code",
          title: "Improve coding-session transcripts",
        }),
      },
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "codex-generation",
          sessionRef: "shared-session",
          runtime: "codex-acp",
          title: "Improve coding-session transcripts",
          lastEventAt: "2026-07-30T12:05:00.000Z",
        }),
      },
    ]),
  );

  assert.equal(entries.length, 1);
  assert.equal(entries[0].label, "Improve coding-session transcripts");
  assert.equal(entries[0].executionCount, 2);
  assert.deepEqual(entries[0].runtimeLabels, ["Claude Code", "Codex"]);
  assert.equal(entries[0].runtimeLabel, "Claude Code + Codex");
  assert.equal(entries[0].generationId, "codex-generation");
});

test("founder-authored session name overrides provider titles on the shelf", () => {
  const channelId = "sessions-channel";
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const founderPubkey = "f".repeat(64);
  const target = {
    driver: "hive-seat",
    instanceId: "private-instance",
    sessionId: "private-seat",
    generation: 7,
  };
  const record = session({
    commandTarget: target,
    sessionRef,
    title: "Provider execution title",
  });
  const names = new Map([
    [
      `${channelId}\u0000${sessionRef}\u0000${founderPubkey}`,
      {
        channelId,
        content: "Durable session name",
        createdAt: 1,
        eventId: "a".repeat(64),
        founderPubkey,
        sessionRef,
      },
    ],
  ]);
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([{ channelId, session: record }], {
      creates: [
        {
          channelId,
          sessionRef,
          signerPubkey: founderPubkey,
          createdAt: 1,
          eventId: "create-a",
          target,
        },
      ],
    }),
    index(),
    new Map(),
    names,
  );

  assert.equal(entries[0].label, "Durable session name");
});

test("active sessions sort before compact idle and unknown rows", () => {
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "idle-channel",
        session: session({
          generationId: "idle",
          transcript: [
            {
              id: "done",
              type: "lifecycle",
              title: "Turn result",
              text: "completed",
              timestamp: "2026-07-30T12:05:00.000Z",
            },
          ],
        }),
      },
      {
        channelId: "unknown-channel",
        session: session({ generationId: "unknown" }),
      },
      {
        channelId: "working-channel",
        session: session({
          generationId: "working",
          lastEventAt: "2026-07-30T11:00:00.000Z",
          transcript: [
            {
              id: "stream",
              type: "lifecycle",
              title: "Status",
              text: "streaming",
              timestamp: "2026-07-30T11:00:00.000Z",
            },
          ],
        }),
      },
    ]),
  );

  assert.deepEqual(
    entries.map((entry) => entry.status.kind),
    ["working", "unknown", "idle"],
  );
});

test("invalid authority and empty catalogs expose honest shelf states", () => {
  const invalid = resolveProjectCodingSessionShelf(
    catalog([], { authorityErrorMessage: "Invalid bridge authority." }),
  );
  assert.deepEqual(invalid.entries, []);
  assert.deepEqual(invalid.state, {
    kind: "unavailable",
    message: "Sessions unavailable",
    detail: "Invalid bridge authority.",
  });

  const empty = resolveProjectCodingSessionShelf(catalog([]));
  assert.deepEqual(empty.entries, []);
  assert.deepEqual(empty.state, {
    kind: "ready",
    message: "No trusted sessions yet",
    detail: null,
  });
});

test("loading, unavailable, and partial history remain explicit", () => {
  const loading = resolveProjectCodingSessionShelf(
    catalog([], { isLoading: true }),
  );
  assert.equal(loading.state.kind, "loading");
  assert.match(loading.state.message, /Loading/);

  const unavailable = resolveProjectCodingSessionShelf(
    catalog([], { errorMessage: "relay offline" }),
  );
  assert.equal(unavailable.state.kind, "unavailable");
  assert.equal(unavailable.state.detail, "relay offline");

  const partial = resolveProjectCodingSessionShelf(
    catalog([{ channelId: "channel-a", session: session() }], {
      errorMessage: "history cap reached",
    }),
  );
  assert.equal(partial.entries.length, 1);
  assert.equal(partial.state.kind, "partial");
  assert.equal(partial.state.detail, "history cap reached");
});

test("source channel labels are presentation only and never alter placement", () => {
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([{ channelId: "source", session: session() }]),
    index(),
    new Map([["source", "Pretends To Be A Project"]]),
  );
  assert.equal(entries[0].placement, "unassigned");
  assert.equal(entries[0].projectId, null);
  assert.equal(entries[0].sourceChannelLabel, "Pretends To Be A Project");
});

test("exact signed projectRef places a session under only the matching project", () => {
  const owner =
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
  const projectRef = `30621:${owner}:buzz-glue`;
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "source",
        session: session({ projectRef, runtime: "claude-code" }),
      },
    ]),
    index({
      byRef: [
        [projectRef, `${owner}:buzz-glue`],
        [`30621:${owner}:buzz`, `${owner}:buzz`],
      ],
    }),
  );
  assert.equal(entries[0].placement, "project");
  assert.equal(entries[0].projectId, `${owner}:buzz-glue`);
  assert.equal(entries[0].placedBy, "project-ref");
  assert.equal(entries[0].runtimeLabel, "Claude Code");
});

test("null and unmatched signed projectRef fall through to the channel-owning project", () => {
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "owned",
        session: session({ generationId: "null", projectRef: null }),
      },
      {
        channelId: "owned",
        session: session({
          generationId: "unmatched",
          projectRef: `30621:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb:not-visible`,
        }),
      },
    ]),
    index({
      byRef: [
        [
          `30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:buzz-glue`,
          "project:buzz-glue",
        ],
      ],
      byChannel: [["owned", "project:host"]],
    }),
  );
  assert.deepEqual(
    entries.map((entry) => ({
      projectId: entry.projectId,
      placedBy: entry.placedBy,
    })),
    [
      { projectId: "project:host", placedBy: "channel" },
      { projectId: "project:host", placedBy: "channel" },
    ],
  );
});

test("a signed projectRef overrides the project that owns the session's channel", () => {
  const signedRef = `30621:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa:signed`;
  const placement = resolveProjectCodingSessionPlacement(
    signedRef,
    "channel-of-other-project",
    index({
      byRef: [[signedRef, "project:signed"]],
      byChannel: [["channel-of-other-project", "project:other"]],
    }),
  );
  assert.deepEqual(placement, {
    projectId: "project:signed",
    placedBy: "project-ref",
  });
});

test("a session in no project's channel stays unclaimed for General", () => {
  const placement = resolveProjectCodingSessionPlacement(
    null,
    "loose-channel",
    index({ byChannel: [["owned", "project:host"]] }),
  );
  assert.deepEqual(placement, { projectId: null, placedBy: null });
});

test("bucketing splits placed sessions per project and keeps the rest unclaimed", () => {
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      { channelId: "a", session: session({ generationId: "one" }) },
      { channelId: "b", session: session({ generationId: "two" }) },
      { channelId: "loose", session: session({ generationId: "three" }) },
    ]),
    index({
      byChannel: [
        ["a", "project:alpha"],
        ["b", "project:alpha"],
      ],
    }),
  );
  const buckets = bucketProjectCodingSessions(entries);
  assert.deepEqual(
    buckets.byProject.get("project:alpha")?.map((entry) => entry.generationId),
    ["one", "two"],
  );
  assert.deepEqual(
    buckets.unclaimed.map((entry) => entry.generationId),
    ["three"],
  );
});

test("active coding-session paths preserve exact encoded coordinates", () => {
  assert.deepEqual(
    parseActiveProjectCodingSessionPath(
      "/coding-sessions/channel%2Fa/generation%7C1",
    ),
    { channelId: "channel/a", generationId: "generation|1" },
  );
  assert.equal(
    parseActiveProjectCodingSessionPath("/channels/channel%2Fa"),
    null,
  );
  assert.equal(
    parseActiveProjectCodingSessionPath("/coding-sessions/channel/%E0%A4%A"),
    null,
  );
});

test("executions sharing an umbrella sessionRef collapse to the one with a transcript", () => {
  // The prod repro: one create left the provider's abandoned handshake
  // session (metadata only, no transcript) next to the real one; both echo
  // the same umbrella sessionRef and rendered as two identical-looking rows.
  const shelf = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({
          generationId: "phantom",
          sessionRef: "8e0e9f45-92b3-4fc7-84e6-affab615126a",
          transcript: [],
          lastEventAt: "2026-08-15T13:28:16.000Z",
        }),
      },
      {
        channelId: "transport",
        session: session({
          generationId: "real",
          sessionRef: "8e0e9f45-92b3-4fc7-84e6-affab615126a",
          transcript: [{ type: "lifecycle", title: "Turn result", text: "" }],
          lastEventAt: "2026-08-15T13:00:17.000Z",
        }),
      },
    ]),
  );
  assert.deepEqual(
    shelf.entries.map((entry) => entry.generationId),
    ["real"],
  );
});

test("distinct umbrellas and pre-umbrella records keep their own rows", () => {
  const shelf = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({ generationId: "a", sessionRef: "umbrella-a" }),
      },
      {
        channelId: "transport",
        session: session({ generationId: "b", sessionRef: "umbrella-b" }),
      },
      {
        channelId: "transport",
        session: session({ generationId: "legacy", sessionRef: null }),
      },
    ]),
  );
  assert.deepEqual(shelf.entries.map((entry) => entry.generationId).sort(), [
    "a",
    "b",
    "legacy",
  ]);
});

test("within an umbrella, an active transcripted execution beats a finished one", () => {
  const shelf = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({
          generationId: "finished",
          sessionRef: "shared",
          transcript: [{ type: "lifecycle", title: "Turn result", text: "" }],
          lastEventAt: "2026-08-15T14:00:00.000Z",
        }),
      },
      {
        channelId: "transport",
        session: session({
          generationId: "active",
          sessionRef: "shared",
          transcript: [{ type: "lifecycle", title: "Status", text: "running" }],
          lastEventAt: "2026-08-15T13:00:00.000Z",
        }),
      },
    ]),
  );
  assert.equal(shelf.entries.length, 1);
  assert.equal(shelf.entries[0].generationId, "active");
  assert.equal(shelf.entries[0].status.kind, "working");
});

test("a session stopped via the durable command reads ended, not idle", () => {
  // The provider's final 44223 says `stopped` while the transcript still ends
  // in an ordinary turn result — the lifecycle status must win.
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({
          status: "stopped",
          transcript: [{ type: "lifecycle", title: "Turn result", text: "" }],
        }),
      },
    ]),
  );
  assert.equal(entries[0].status.kind, "ended");
  assert.equal(
    entries[0].isClosed,
    false,
    "stopping an execution must not close its durable session",
  );
});

test("ended executions remain in the open-session shelf with nothing to stop", () => {
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({
          generationId: "done",
          sessionRef: "u-done",
          status: "stopped",
          transcript: [{ type: "lifecycle", title: "Turn result", text: "" }],
        }),
      },
      {
        channelId: "transport",
        session: session({
          generationId: "live",
          sessionRef: "u-live",
          providerAuthorityPubkey: "a".repeat(64),
          transcript: [{ type: "lifecycle", title: "Status", text: "running" }],
        }),
      },
    ]),
  );
  // Ended executions persist inside their still-open sessions and sort behind
  // live work so they never crowd it out of the row limit.
  assert.deepEqual(
    entries.map((e) => e.generationId),
    ["live", "done"],
  );
  const done = entries.find((e) => e.generationId === "done");
  const live = entries.find((e) => e.generationId === "live");
  assert.deepEqual(
    done.stopTargets,
    [],
    "an ended execution has nothing to stop",
  );
  assert.equal(done.isClosed, false);
  assert.equal(live.stopTargets.length, 1);
  assert.equal(live.stopTargets[0].target.sessionId, "private-seat");
});

test("an umbrella unions stop targets while runtime end stays separate from closure", () => {
  const twoLive = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({
          generationId: "claude-execution",
          sessionRef: "u-pair",
          providerAuthorityPubkey: "a".repeat(64),
          transcript: [{ type: "lifecycle", title: "Status", text: "running" }],
        }),
      },
      {
        channelId: "transport",
        session: session({
          generationId: "codex-execution",
          sessionRef: "u-pair",
          providerAuthorityPubkey: "b".repeat(64),
          commandTarget: {
            driver: "codex-acp",
            instanceId: "other-instance",
            sessionId: "other-seat",
            generation: 1,
          },
        }),
      },
    ]),
  );
  assert.equal(twoLive.entries.length, 1);
  assert.equal(
    twoLive.entries[0].stopTargets.length,
    2,
    "stopping an umbrella's executions must address every live execution",
  );

  // An ended execution is the runtime face when a stale idle prior generation
  // shares the umbrella, but it still does not close the durable session.
  const endedFace = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({
          generationId: "stopped-face",
          sessionRef: "u-ended",
          status: "stopped",
          // The stop is the group's latest activity — the user's last word.
          lastEventAt: "2026-07-30T13:00:00.000Z",
          transcript: [{ type: "lifecycle", title: "Turn result", text: "" }],
        }),
      },
      {
        channelId: "transport",
        session: session({
          generationId: "stale-idle",
          sessionRef: "u-ended",
          transcript: [{ type: "lifecycle", title: "Turn result", text: "" }],
        }),
      },
    ]),
  );
  assert.equal(endedFace.entries.length, 1);
  assert.equal(endedFace.entries[0].status.kind, "ended");
  assert.equal(endedFace.entries[0].isClosed, false);

  // …but live work always outranks the ended face.
  const stillWorking = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "transport",
        session: session({
          generationId: "stopped-face",
          sessionRef: "u-work",
          status: "stopped",
          lastEventAt: "2026-07-30T13:00:00.000Z",
          transcript: [{ type: "lifecycle", title: "Turn result", text: "" }],
        }),
      },
      {
        channelId: "transport",
        session: session({
          generationId: "still-working",
          sessionRef: "u-work",
          providerAuthorityPubkey: "a".repeat(64),
          transcript: [{ type: "lifecycle", title: "Status", text: "running" }],
        }),
      },
    ]),
  );
  assert.equal(stillWorking.entries.length, 1);
  assert.equal(stillWorking.entries[0].status.kind, "working");
  assert.equal(stillWorking.entries[0].isClosed, false);
});

test("an exact closure fact alone moves an umbrella between Sessions and Settled", () => {
  const channelId = "sessions-channel";
  const sessionRef = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
  const genesisRef = "a".repeat(64);
  const founderPubkey = "f".repeat(64);
  const target = {
    driver: "hive-seat",
    instanceId: "private-instance",
    sessionId: "private-seat",
    generation: 7,
  };
  const snapshot = catalog(
    [
      {
        channelId,
        session: session({
          commandTarget: target,
          providerAuthorityPubkey: "b".repeat(64),
          sessionRef,
          status: "stopped",
        }),
      },
    ],
    {
      creates: [
        {
          channelId,
          sessionRef,
          signerPubkey: founderPubkey,
          createdAt: 1,
          eventId: genesisRef,
          genesisRef,
          target,
        },
      ],
    },
  );
  const key = `${channelId}\u0000${sessionRef}\u0000${genesisRef}`;
  const closure = {
    action: "closed",
    channelId,
    createdAt: 2,
    eventId: "c".repeat(64),
    founderPubkey,
    genesisRef,
    sessionRef,
    signerPubkey: founderPubkey,
  };

  const closed = resolveProjectCodingSessionShelf(
    snapshot,
    index(),
    new Map(),
    new Map(),
    new Map([[key, closure]]),
  ).entries[0];
  assert.equal(closed.status.kind, "ended");
  assert.equal(closed.isClosed, true);
  assert.equal(closed.closure?.action, "closed");

  const reopened = resolveProjectCodingSessionShelf(
    snapshot,
    index(),
    new Map(),
    new Map(),
    new Map([[key, { ...closure, action: "open", eventId: "d".repeat(64) }]]),
  ).entries[0];
  assert.equal(reopened.status.kind, "ended");
  assert.equal(reopened.isClosed, false);
  assert.equal(reopened.closure?.action, "open");
});

test("an untitled transcript-bearing representative borrows a titled member's label", () => {
  // The transcript-bearing execution wins the representative pick (it is the
  // session's face for routing), but the person named the session on the
  // metadata-only member — the name must survive the collapse.
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "transcript-generation",
          sessionRef: "shared-session",
          title: "",
          transcript: [
            {
              eventSeq: 1,
              timestamp: 1_753_876_800_000,
              turnId: "turn-1",
              item: { kind: "assistant_text", text: "hello" },
            },
          ],
        }),
      },
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "titled-generation",
          sessionRef: "shared-session",
          title: "Testing 2",
        }),
      },
    ]),
  );

  assert.equal(entries.length, 1);
  assert.equal(
    entries[0].generationId,
    "transcript-generation",
    "coordinates stay with the transcript-bearing representative",
  );
  assert.equal(entries[0].label, "Testing 2");
});

test("among several titled members the newest label wins, and a titled representative keeps its own", () => {
  const { entries } = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "untitled-face",
          sessionRef: "shared-session",
          title: "",
          transcript: [
            {
              eventSeq: 1,
              timestamp: 1_753_876_800_000,
              turnId: "turn-1",
              item: { kind: "assistant_text", text: "hello" },
            },
          ],
        }),
      },
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "older-title",
          sessionRef: "shared-session",
          title: "Older name",
          lastEventAt: "2026-07-30T11:00:00.000Z",
        }),
      },
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "newer-title",
          sessionRef: "shared-session",
          title: "Newer name",
          lastEventAt: "2026-07-30T12:30:00.000Z",
        }),
      },
    ]),
  );
  assert.equal(entries.length, 1);
  assert.equal(entries[0].label, "Newer name");

  const titledFace = resolveProjectCodingSessionShelf(
    catalog([
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "titled-face",
          sessionRef: "shared-session",
          title: "Representative name",
          transcript: [
            {
              eventSeq: 1,
              timestamp: 1_753_876_800_000,
              turnId: "turn-1",
              item: { kind: "assistant_text", text: "hello" },
            },
          ],
        }),
      },
      {
        channelId: "sessions-channel",
        session: session({
          generationId: "other-title",
          sessionRef: "shared-session",
          title: "Sibling name",
          lastEventAt: "2026-07-30T12:30:00.000Z",
        }),
      },
    ]),
  );
  assert.equal(titledFace.entries[0].label, "Representative name");
});

test("a failed closure or name read stops the shelf reading as settled", () => {
  const ready = resolveProjectCodingSessionShelf(
    catalog([{ channelId: "channel-a", session: session() }]),
  ).state;
  assert.equal(ready.kind, "ready");

  const degraded = withProjectCodingSessionProjectionError(
    ready,
    "Failed to load session closures.",
  );
  assert.equal(degraded.kind, "partial");
  assert.equal(degraded.detail, "Failed to load session closures.");

  // No error changes nothing, and a louder failure keeps its own message.
  assert.equal(withProjectCodingSessionProjectionError(ready, null), ready);
  const unavailable = resolveProjectCodingSessionShelf(
    catalog([], { authorityErrorMessage: "Invalid bridge authority." }),
  ).state;
  assert.deepEqual(
    withProjectCodingSessionProjectionError(unavailable, "closures failed"),
    unavailable,
  );
  const loading = resolveProjectCodingSessionShelf(
    catalog([], { isLoading: true }),
  ).state;
  assert.deepEqual(
    withProjectCodingSessionProjectionError(loading, "closures failed"),
    loading,
  );
});
