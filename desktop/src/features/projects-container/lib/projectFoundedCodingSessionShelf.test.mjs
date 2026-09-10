/**
 * Founded rows on the project shelf: how a genesis with nothing running is
 * labelled, placed, closed, hidden while its Start is pending, and sorted —
 * and why it must join the shelf *after* the pending overlay.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  applyPendingCodingSessionLifecycle,
  PENDING_CODING_SESSION_LIFECYCLE_TTL_MS,
  pendingCodingSessionLifecycleKey,
} from "@/features/coding-sessions/lib/codingSessionPendingLifecycle.ts";
import { codingSessionClosureKey } from "@/features/coding-sessions/lib/codingSessionClosure.ts";
import { codingSessionNameKey } from "@/features/coding-sessions/lib/codingSessionName.ts";
import { compareProjectCodingSessionEntries } from "./projectCodingSessionShelf.ts";
import {
  mergeFoundedCodingSessionShelfEntries,
  resolveFoundedProjectCodingSessionEntries,
  resolveGlobalFoundedCodingSessions,
  resolveProjectCodingSessionOpenTarget,
  UNTITLED_FOUNDED_CODING_SESSION_LABEL,
} from "./projectFoundedCodingSessionShelf.ts";

const CHANNEL_ID = "sessions-channel";
const OTHER_CHANNEL_ID = "other-channel";
const REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const OTHER_REF = "6c8f2d3b-01e5-4c1f-b2a4-8d3e9f7a5b21";
const GENESIS_REF = "c".repeat(64);
const FOUNDER = "f".repeat(64);
const PROVIDER = "a".repeat(64);
const FOUNDED_AT = 1_800_000_000;
const NOW = 1_800_000_100_000;

function founded(overrides = {}) {
  return {
    channelId: CHANNEL_ID,
    sessionRef: REF,
    genesisRef: GENESIS_REF,
    founderPubkey: FOUNDER,
    foundedAt: FOUNDED_AT,
    ...overrides,
  };
}

function index({ byChannel = [] } = {}) {
  return {
    projectIdByRef: new Map(),
    projectIdByChannel: new Map(byChannel),
  };
}

function pendingCreate(overrides = {}) {
  return {
    kind: "create",
    channelId: CHANNEL_ID,
    commandId: "csl-1",
    sessionRef: REF,
    title: "Team topic",
    projectRef: null,
    providerAuthorityPubkey: PROVIDER,
    hasInitialTurn: true,
    recordedAt: NOW - 1_000,
    ...overrides,
  };
}

function startedEntry(overrides = {}) {
  return {
    placement: "unassigned",
    projectId: null,
    placedBy: null,
    channelId: CHANNEL_ID,
    generationId: "gen-1",
    label: "Started session",
    sourceChannelLabel: null,
    runtimeLabel: "Claude Code",
    runtimeLabels: ["Claude Code"],
    executionCount: 1,
    closure: null,
    isClosed: false,
    isArchived: false,
    sessionRef: OTHER_REF,
    genesisRef: null,
    founderPubkey: FOUNDER,
    status: { kind: "idle", label: "Idle" },
    stopTargets: [],
    session: {
      generationId: "gen-1",
      sessionRef: OTHER_REF,
      commandTarget: null,
      transcript: [],
      title: "",
      lastEventAt: "2026-09-08T09:00:00.000Z",
    },
    ...overrides,
  };
}

test("founded rows come from every channel's geneses, minus what creates and entries claim", () => {
  const rows = resolveGlobalFoundedCodingSessions({
    entries: [
      {
        channelId: OTHER_CHANNEL_ID,
        session: { generationId: "gen-9", sessionRef: OTHER_REF },
      },
    ],
    creates: [],
    geneses: [
      founded(),
      founded({ channelId: OTHER_CHANNEL_ID, sessionRef: OTHER_REF }),
    ],
    authorityErrorMessage: null,
  });
  assert.deepEqual(
    rows.map((row) => [row.channelId, row.sessionRef]),
    [[CHANNEL_ID, REF]],
  );
  // No geneses collected (an older snapshot, a test fixture) means no rows,
  // and an authority error means none too.
  assert.deepEqual(
    resolveGlobalFoundedCodingSessions({
      entries: [],
      authorityErrorMessage: null,
    }),
    [],
  );
  assert.deepEqual(
    resolveGlobalFoundedCodingSessions({
      entries: [],
      geneses: [founded()],
      authorityErrorMessage: "refused",
    }),
    [],
  );
});

test("a founded row is labelled by its founder-keyed name, placed by channel, and reads Not started", () => {
  const [row] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded()],
    index: index({ byChannel: [[CHANNEL_ID, "project-1"]] }),
    channelLabels: new Map([[CHANNEL_ID, " Project Sessions "]]),
    names: new Map([
      [
        codingSessionNameKey(CHANNEL_ID, REF, FOUNDER),
        {
          channelId: CHANNEL_ID,
          content: "Ship the founded flow",
          createdAt: FOUNDED_AT + 1,
          eventId: "d".repeat(64),
          founderPubkey: FOUNDER,
          sessionRef: REF,
        },
      ],
    ]),
  });
  assert.equal(row.label, "Ship the founded flow");
  assert.equal(row.placement, "project");
  assert.equal(row.projectId, "project-1");
  assert.equal(row.placedBy, "channel");
  assert.equal(row.sourceChannelLabel, "Project Sessions");
  assert.equal(row.generationId, `founded:${REF}`);
  assert.equal(row.executionCount, 0);
  assert.equal(row.runtimeLabel, null);
  assert.deepEqual(row.runtimeLabels, []);
  assert.deepEqual(row.stopTargets, []);
  assert.equal(row.founded, true);
  assert.deepEqual(row.status, { kind: "founded", label: "Not started" });
  assert.equal(row.isClosed, false);
  assert.equal(row.sessionRef, REF);
  assert.equal(row.genesisRef, GENESIS_REF);
  assert.equal(row.founderPubkey, FOUNDER);
  // The synthesized record claims nothing a provider has not reported.
  assert.equal(row.session.generationId, `founded:${REF}`);
  assert.equal(row.session.lastEventAt, "2027-01-15T08:00:00.000Z");
  assert.equal(row.session.status, "unknown");
  assert.equal(row.session.providerAuthorityPubkey, null);
  assert.equal(row.session.commandTarget, null);
  assert.equal(row.session.provider, null);
  assert.equal(row.session.sessionRef, REF);
});

test("an unnamed founded row is Untitled session and lands under General when no project owns its channel", () => {
  const [row] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded()],
    index: index(),
  });
  assert.equal(row.label, UNTITLED_FOUNDED_CODING_SESSION_LABEL);
  assert.equal(row.placement, "unassigned");
  assert.equal(row.projectId, null);
  assert.equal(row.placedBy, null);
  assert.equal(row.sourceChannelLabel, null);
});

test("a closure keyed by (channel, ref, genesis) settles a founded row like any other", () => {
  const closure = {
    action: "archived",
    channelId: CHANNEL_ID,
    sessionRef: REF,
    genesisRef: GENESIS_REF,
    founderPubkey: FOUNDER,
    signerPubkey: FOUNDER,
    createdAt: FOUNDED_AT + 5,
    eventId: "b".repeat(64),
  };
  const [row] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded()],
    index: index(),
    closures: new Map([
      [codingSessionClosureKey(CHANNEL_ID, REF, GENESIS_REF), closure],
    ]),
  });
  assert.equal(row.closure, closure);
  assert.equal(row.isClosed, true);
  assert.equal(row.isArchived, true);
  // A closure for a different genesis under the same ref is not this row's.
  const [open] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded()],
    index: index(),
    closures: new Map([
      [codingSessionClosureKey(CHANNEL_ID, REF, "9".repeat(64)), closure],
    ]),
  });
  assert.equal(open.isClosed, false);
});

test("a live pending create for the same channel and ref hides the founded row; an expired one does not", () => {
  const [row] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded()],
    index: index(),
  });
  assert.deepEqual(
    mergeFoundedCodingSessionShelfEntries([], [row], [pendingCreate()], NOW),
    [],
  );
  assert.equal(
    mergeFoundedCodingSessionShelfEntries(
      [],
      [row],
      [
        pendingCreate({
          recordedAt: NOW - PENDING_CODING_SESSION_LIFECYCLE_TTL_MS - 1,
        }),
      ],
      NOW,
    ).length,
    1,
    "a provider that never answered leaves the founded row as the truth",
  );
  // Neither a pending for another ref, nor one in another channel, nor a
  // pending stop, hides it.
  for (const pending of [
    pendingCreate({ sessionRef: OTHER_REF }),
    pendingCreate({ channelId: OTHER_CHANNEL_ID }),
    pendingCreate({ sessionRef: null }),
    {
      kind: "stop",
      channelId: CHANNEL_ID,
      targetKey: "target",
      providerAuthorityPubkey: PROVIDER,
      recordedAt: NOW,
    },
  ]) {
    assert.equal(
      mergeFoundedCodingSessionShelfEntries([], [row], [pending], NOW).length,
      1,
    );
  }
  // A real row already carrying the ref hides it too; founded rows already
  // on the shelf do not hide each other.
  assert.equal(
    mergeFoundedCodingSessionShelfEntries(
      [startedEntry({ sessionRef: REF })],
      [row],
      [],
      NOW,
    ).length,
    1,
  );
  assert.equal(
    mergeFoundedCodingSessionShelfEntries([row], [row], [], NOW).length,
    2,
  );
});

test("founded rows join after the pending overlay: fed into it, they would eat their own Starting row", () => {
  const [row] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded()],
    index: index(),
  });
  const pending = pendingCreate();
  const placement = () => ({ projectId: null, placedBy: null });

  // The hazard: the overlay acknowledges a create by its sessionRef echo, and
  // a founded row echoes exactly that ref.
  const wrong = applyPendingCodingSessionLifecycle(
    [row],
    [pending],
    placement,
    new Map(),
    NOW,
  );
  assert.deepEqual(wrong.consumedKeys, [
    pendingCodingSessionLifecycleKey(pending),
  ]);
  assert.equal(
    wrong.entries.some((entry) => entry.pending === true),
    false,
    "the Starting row was never synthesized",
  );

  // The wiring order: shelf → overlay → merge founded.
  const applied = applyPendingCodingSessionLifecycle(
    [],
    [pending],
    placement,
    new Map(),
    NOW,
  );
  assert.deepEqual(applied.consumedKeys, []);
  const merged = mergeFoundedCodingSessionShelfEntries(
    applied.entries,
    [row],
    [pending],
    NOW,
  );
  assert.equal(merged.length, 1);
  assert.equal(merged[0].pending, true);
  assert.equal(merged[0].generationId, `pending:${pending.commandId}`);
});

test("founded sorts after working and waiting, before idle; a closed founded row files under Settled", () => {
  const [foundedRow] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded()],
    index: index(),
  });
  const [closedFounded] = resolveFoundedProjectCodingSessionEntries({
    founded: [founded({ sessionRef: OTHER_REF, genesisRef: "9".repeat(64) })],
    index: index(),
    closures: new Map([
      [
        codingSessionClosureKey(CHANNEL_ID, OTHER_REF, "9".repeat(64)),
        {
          action: "closed",
          channelId: CHANNEL_ID,
          sessionRef: OTHER_REF,
          genesisRef: "9".repeat(64),
          founderPubkey: FOUNDER,
          signerPubkey: FOUNDER,
          createdAt: FOUNDED_AT + 5,
          eventId: "b".repeat(64),
        },
      ],
    ]),
  });
  const working = startedEntry({
    generationId: "gen-working",
    status: { kind: "working", label: "Working" },
    session: {
      ...startedEntry().session,
      lastEventAt: "2020-01-01T00:00:00.000Z",
    },
  });
  const waiting = startedEntry({
    generationId: "gen-waiting",
    status: { kind: "waiting", label: "Waiting for you" },
    session: {
      ...startedEntry().session,
      lastEventAt: "2020-01-01T00:00:00.000Z",
    },
  });
  const idle = startedEntry({
    generationId: "gen-idle",
    status: { kind: "idle", label: "Idle" },
    session: {
      ...startedEntry().session,
      lastEventAt: "2030-01-01T00:00:00.000Z",
    },
  });
  const sorted = [closedFounded, idle, foundedRow, waiting, working].sort(
    compareProjectCodingSessionEntries,
  );
  assert.deepEqual(
    sorted.map((entry) => entry.generationId),
    [
      "gen-working",
      "gen-waiting",
      `founded:${REF}`,
      "gen-idle",
      `founded:${OTHER_REF}`,
    ],
  );
  assert.equal(sorted.at(-1).isClosed, true);
});

test("opening a row tells a founded id apart from a generation", () => {
  assert.deepEqual(
    resolveProjectCodingSessionOpenTarget({
      channelId: CHANNEL_ID,
      generationId: `founded:${REF}`,
    }),
    { kind: "founded", channelId: CHANNEL_ID, sessionRef: REF },
  );
  assert.deepEqual(
    resolveProjectCodingSessionOpenTarget({
      channelId: CHANNEL_ID,
      generationId: "gen-1",
    }),
    { kind: "generation", channelId: CHANNEL_ID, generationId: "gen-1" },
  );
  // A bare prefix names no session; it is not a founded open.
  assert.equal(
    resolveProjectCodingSessionOpenTarget({
      channelId: CHANNEL_ID,
      generationId: "founded:",
    }).kind,
    "generation",
  );
});
