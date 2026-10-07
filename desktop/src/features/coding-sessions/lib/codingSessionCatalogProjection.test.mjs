/**
 * SV-118: the retained catalog projection against the frozen full rebuild.
 *
 * Every stream here goes through the real verified store and the real publish
 * reuse, exactly as `useTrustedCodingSessionIngress` feeds the catalog, and
 * after EVERY delivered event the complete catalog records are compared with
 * `oracleMergeTrustedCodingSessionIngress` — a verbatim copy of the merge and
 * fold as they stood before SV-118, fed the store's fresh (un-reused)
 * snapshot. Parity alone can preserve an old bug, so the deterministic tests
 * below also pin the behaviours themselves.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";
import { finalizeEvent, getPublicKey } from "nostr-tools/pure";
import React, { act } from "react";
import { createRoot } from "react-dom/client";

import {
  KIND_CODING_SESSION_METADATA,
  KIND_CODING_SESSION_TRANSCRIPT,
} from "@/shared/constants/kinds.ts";
import {
  createCodingSessionCatalogProjection,
  useRetainedCodingSessionCatalogProjection,
} from "./codingSessionCatalogProjection.ts";
import { buildCodingSessionTargetKey } from "./codingSessionCommand.ts";
import { OPEN_CODING_SESSION_INGRESS_AUTHORITY } from "./codingSessionIngressAuthority.ts";
import { oracleMergeTrustedCodingSessionIngress } from "./codingSessionProjectionOracle.testFixtures.ts";
import {
  BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
  BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
  CODING_SESSION_METADATA_TAG_VERSION,
  CODING_SESSION_TRANSCRIPT_TAG_VERSION,
  codingSessionMetadataSemanticKey,
  codingSessionTranscriptSemanticKey,
  TrustedCodingSessionIngressStore,
} from "./codingSessionTrustedIngress.ts";
import { reuseCodingSessionIngressSnapshotArrays } from "./useTrustedCodingSessionIngressPublish.ts";
import { composeCodingSessionCatalogSnapshot } from "../useCodingSessionCatalog.ts";

const CHANNEL = "channel-sv118";
const OTHER_CHANNEL = "channel-sv118-other";
const CHANNELS = [CHANNEL, OTHER_CHANNEL];
const OPEN = OPEN_CODING_SESSION_INGRESS_AUTHORITY;
const SECRETS = [new Uint8Array(32).fill(7), new Uint8Array(32).fill(9)];
const PUBKEYS = SECRETS.map((secret) => getPublicKey(secret));

const SESSION_A = "11111111-2222-3333-4444-555555555555";
const SESSION_B = "66666666-7777-8888-9999-000000000000";
const UMBRELLA = "abcdefab-cdef-4abc-8def-abcdefabcdef";
const TARGETS = [
  { signer: 0, target: target(SESSION_A, 1) },
  { signer: 0, target: target(SESSION_A, 2) },
  { signer: 1, target: target(SESSION_B, 1) },
];

function target(sessionId, generation) {
  return {
    driver: "claude-agent-acp",
    instanceId: "0123456789abcdef",
    sessionId,
    generation,
  };
}

/** Deterministic PRNG (mulberry32) so a failing seed replays exactly. */
function prng(seed) {
  let state = seed >>> 0;
  return () => {
    state = (state + 0x6d2b79f5) >>> 0;
    let t = state;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

function transcriptEvent(
  { signer, target: session },
  eventSeq,
  turnId,
  item,
  channelId = CHANNEL,
  createdAt = 1_800_000_000 + Math.floor(eventSeq / 3),
) {
  const value = {
    schema: BEEKEEPER_CODING_SESSION_TRANSCRIPT_SCHEMA,
    session,
    eventSeq,
    // Provider clocks are not monotonic in eventSeq; the catalog's
    // timestamps are maxima, never "the last entry's".
    timestamp:
      1_800_000_000_000 + eventSeq * 250 + ((eventSeq * 7919) % 5) * 400,
    turnId,
    item,
  };
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_TRANSCRIPT,
      created_at: createdAt,
      tags: [
        ["h", channelId],
        ["cst-v", CODING_SESSION_TRANSCRIPT_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(session)],
        ["cst-seq", String(eventSeq)],
        ["cst-key", codingSessionTranscriptSemanticKey(session, eventSeq)],
      ],
      content: JSON.stringify(value),
    },
    SECRETS[signer],
  );
}

function metadataEvent(
  { signer, target: session },
  createdAt,
  overrides = {},
  channelId = CHANNEL,
) {
  const value = {
    schema: BEEKEEPER_CODING_SESSION_METADATA_SCHEMA,
    session,
    projectRef: null,
    repoRef: null,
    title: `Session ${session.sessionId.slice(0, 4)} g${session.generation}`,
    agentRef: null,
    provider: "claude-agent-acp",
    runtime: "claude-agent-acp",
    model: "sonnet",
    status: "running",
    branch: null,
    capabilities: {
      threadTurnStart: true,
      threadTurnInterrupt: true,
      threadSteer: true,
      context: false,
      diff: false,
      plan: false,
      promptImage: false,
    },
    ...overrides,
  };
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_METADATA,
      created_at: createdAt,
      tags: [
        ["h", channelId],
        ["csm-v", CODING_SESSION_METADATA_TAG_VERSION],
        ["cs-target", buildCodingSessionTargetKey(session)],
        ["csm-key", codingSessionMetadataSemanticKey(session)],
      ],
      content: JSON.stringify(value),
    },
    SECRETS[signer],
  );
}

/**
 * One generation's plausible transcript: prompted turns (some steered, some
 * with a null turn id), tool calls whose results arrive later — sometimes
 * before the call — subagent calls, background tasks, answers, and a terminal
 * result or interrupt.
 */
function generationScript(random, spec, length) {
  const items = [];
  let turn = 0;
  let tool = 0;
  const pending = [];
  let turnId = null;
  while (items.length < length) {
    const roll = random();
    if (items.length === 0 || roll < 0.12) {
      turn += 1;
      turnId =
        random() < 0.25 ? null : `turn-${spec.target.generation}-${turn}`;
      items.push({
        turnId,
        item: {
          kind: "user_prompt",
          content: `prompt ${turn}`,
          ...(random() < 0.15 ? { steered: true } : {}),
        },
      });
    } else if (roll < 0.4) {
      tool += 1;
      const toolId = `tool-${spec.target.generation}-${tool}`;
      const subagent = random() < 0.15;
      items.push({
        turnId,
        item: {
          kind: "tool_call",
          tool: subagent
            ? {
                toolName: "Task",
                toolId,
                input: { description: `sub ${tool}`, subagent_type: "general" },
              }
            : {
                toolName: "Bash",
                toolId,
                input: {
                  command: `echo ${tool}`,
                  ...(random() < 0.2 ? { run_in_background: true } : {}),
                },
              },
        },
      });
      pending.push(toolId);
    } else if (roll < 0.62 && pending.length > 0) {
      const toolId = pending.splice(
        Math.floor(random() * pending.length),
        1,
      )[0];
      items.push({
        turnId,
        item: {
          kind: "tool_result",
          toolId,
          toolName: "Bash",
          content:
            random() < 0.2
              ? `Command running in background with ID: ${toolId}. You will be notified when it completes.`
              : `output ${toolId}`,
          isError: random() < 0.1,
        },
      });
    } else if (roll < 0.66) {
      // A result for a call that has not been published yet.
      items.push({
        turnId,
        item: {
          kind: "tool_result",
          toolId: `tool-${spec.target.generation}-${tool + 1}`,
          toolName: "Bash",
          content: "early",
          isError: false,
        },
      });
    } else if (roll < 0.86) {
      items.push({
        turnId,
        item: { kind: "assistant_text", text: `answer ${items.length}` },
      });
    } else if (roll < 0.94) {
      items.push({
        turnId,
        item: {
          kind: "result",
          subtype: random() < 0.8 ? "success" : "error",
          isError: false,
          durationMs: 1000,
          result: "",
          costUsd: 0.01,
        },
      });
    } else {
      items.push({ turnId, item: { kind: "interrupted" } });
    }
  }
  return items;
}

/**
 * A delivery schedule: per-generation in-order streams interleaved at random,
 * then perturbed with late arrivals, duplicates, conflicting re-publications,
 * metadata restatements and events for another channel.
 */
function deliverySchedule(seed, perGeneration) {
  const random = prng(seed);
  const streams = TARGETS.map((spec) =>
    generationScript(random, spec, perGeneration).map(({ turnId, item }, i) =>
      transcriptEvent(spec, i + 1, turnId, item),
    ),
  );
  const cursors = streams.map(() => 0);
  const schedule = [];
  const held = [];
  let metadataClock = 1_800_000_000;
  while (cursors.some((cursor, i) => cursor < streams[i].length)) {
    const open = streams
      .map((_, i) => i)
      .filter((i) => cursors[i] < streams[i].length);
    const pick = open[Math.floor(random() * open.length)];
    const event = streams[pick][cursors[pick]];
    cursors[pick] += 1;
    const roll = random();
    if (roll < 0.08) {
      held.push(event);
    } else {
      schedule.push(event);
    }
    if (roll > 0.97 && held.length > 0) schedule.push(held.shift());
    if (random() < 0.05 && schedule.length > 0) {
      schedule.push(schedule[Math.floor(random() * schedule.length)]);
    }
    if (random() < 0.03) {
      const spec = TARGETS[pick];
      const seq = 1 + Math.floor(random() * cursors[pick]);
      schedule.push(
        transcriptEvent(spec, seq, "turn-conflict", {
          kind: "assistant_text",
          text: `conflicting restatement of ${seq}`,
        }),
      );
    }
    if (random() < 0.06) {
      metadataClock += 1;
      const umbrella = random() < 0.6;
      schedule.push(
        metadataEvent(TARGETS[pick], metadataClock, {
          status: random() < 0.5 ? "running" : "idle",
          title: `retitled ${metadataClock}`,
          ...(umbrella ? { sessionRef: UMBRELLA } : {}),
          ...(umbrella && random() < 0.6
            ? { turnBudget: { used: Math.floor(random() * 5), limit: 10 } }
            : {}),
        }),
      );
    }
    if (random() < 0.03) {
      schedule.push(
        transcriptEvent(
          TARGETS[pick],
          900 + schedule.length,
          null,
          {
            kind: "assistant_text",
            text: "another channel",
          },
          OTHER_CHANNEL,
        ),
      );
    }
  }
  return [...schedule, ...held.reverse()];
}

function exactEntries(snapshot, record) {
  return snapshot.transcripts.filter(
    (entry) =>
      entry.channelId === CHANNEL &&
      entry.signerPubkey === record.providerAuthorityPubkey &&
      entry.targetKey === buildCodingSessionTargetKey(record.commandTarget) &&
      entry.conflictCount === 0,
  );
}

function sameList(left, right) {
  return (
    left.length === right.length && left.every((entry, i) => entry === right[i])
  );
}

function isPrefix(prefix, list) {
  return (
    prefix.length <= list.length &&
    prefix.every((entry, i) => entry === list[i])
  );
}

function assertDeepFrozen(value, path, seen = new Set()) {
  if (value === null || typeof value !== "object" || seen.has(value)) return;
  seen.add(value);
  assert.ok(Object.isFrozen(value), `${path} is frozen`);
  for (const [key, child] of Object.entries(value)) {
    assertDeepFrozen(child, `${path}.${key}`, seen);
  }
}

/**
 * Drive one schedule through store → publish reuse → retained projection,
 * asserting parity, identity, immutability and work after every event.
 */
function replay(schedule) {
  const store = new TrustedCodingSessionIngressStore();
  const projection = createCodingSessionCatalogProjection();
  let published = null;
  let previousExact = new Map();
  let previousTranscripts = new Map();
  const history = [];
  let step = 0;
  for (const event of schedule) {
    step += 1;
    store.ingestRelayEvents([event], CHANNELS, OPEN);
    const fresh = store.snapshot([CHANNEL]);
    published = reuseCodingSessionIngressSnapshotArrays(published, fresh);
    const before = projection.stats();
    const records = projection.merge(
      CHANNEL,
      published.metadata,
      published.transcripts,
    );
    const after = projection.stats();

    // Parity: the complete records, every field, against the frozen rebuild
    // of the store's own un-reused snapshot.
    assert.deepStrictEqual(
      records,
      oracleMergeTrustedCodingSessionIngress(
        CHANNEL,
        fresh.metadata,
        fresh.transcripts,
      ),
      `step ${step}: records equal the full rebuild`,
    );

    // Work: an append presents only its new entries, a non-prefix change
    // re-presents its own generation and nothing else.
    let expectedPresented = 0;
    let nonPrefix = 0;
    let fresh_ = 0;
    const nextExact = new Map();
    const nextTranscripts = new Map();
    for (const record of records) {
      const exact = exactEntries(published, record);
      const prior = previousExact.get(record.generationId) ?? [];
      nextExact.set(record.generationId, exact);
      nextTranscripts.set(record.generationId, record.transcript);
      if (sameList(prior, exact)) {
        if (exact.length > 0 && previousTranscripts.has(record.generationId)) {
          assert.equal(
            record.transcript,
            previousTranscripts.get(record.generationId),
            `step ${step}: an untouched generation keeps its transcript array`,
          );
        }
        continue;
      }
      if (prior.length === 0) {
        fresh_ += 1;
        expectedPresented += exact.length;
      } else if (isPrefix(prior, exact)) {
        expectedPresented += exact.length - prior.length;
        const previous = previousTranscripts.get(record.generationId);
        // Items before the change keep their identity unless a result
        // patched them into a new object.
        for (let i = 0; i < previous.length - 1; i += 1) {
          if (record.transcript[i] !== previous[i]) {
            assert.notDeepStrictEqual(
              record.transcript[i],
              previous[i],
              `step ${step}: only a patched item is replaced`,
            );
          }
        }
      } else if (exact.length > 0) {
        nonPrefix += 1;
        expectedPresented += exact.length;
      }
    }
    assert.equal(
      after.presentedEntries - before.presentedEntries,
      expectedPresented,
      `step ${step}: presentation work is exactly the changed entries`,
    );
    const rebuilt = after.rebuilt - before.rebuilt;
    assert.ok(
      rebuilt >= nonPrefix && rebuilt <= nonPrefix + fresh_,
      `step ${step}: ${rebuilt} rebuilds for ${nonPrefix} out-of-order changes`,
    );

    for (const record of records) {
      history.push({
        array: record.transcript,
        json: JSON.stringify(record.transcript),
      });
    }
    previousExact = nextExact;
    previousTranscripts = nextTranscripts;
  }
  // Every fixture event was admitted; a malformed one would silently test
  // less than it claims.
  assert.equal(
    store.snapshot([CHANNEL]).malformedCount,
    0,
    "no malformed fixtures",
  );
  // Nothing published earlier moved, and all of it is frozen.
  for (const [i, { array, json }] of history.entries()) {
    assert.equal(JSON.stringify(array), json, `published array ${i} unchanged`);
  }
  for (const { array } of history.slice(-30)) {
    assertDeepFrozen(array, "transcript");
  }
  return projection.stats();
}

for (const seed of [1, 2, 3, 118, 2026]) {
  test(`seed ${seed}: retained catalog equals the full rebuild after every event`, () => {
    const stats = replay(deliverySchedule(seed, 70));
    assert.ok(stats.appended > 0, "the stream exercised the append path");
    assert.ok(stats.rebuilt > 0, "the stream exercised the rebuild path");
  });
}

/** Build a projection, merge a store's snapshot, return both. */
function harness(events) {
  const store = new TrustedCodingSessionIngressStore();
  const projection = createCodingSessionCatalogProjection();
  let published = null;
  const merge = (more = []) => {
    store.ingestRelayEvents(more, CHANNELS, OPEN);
    published = reuseCodingSessionIngressSnapshotArrays(
      published,
      store.snapshot([CHANNEL]),
    );
    return projection.merge(CHANNEL, published.metadata, published.transcripts);
  };
  merge(events);
  return { store, projection, merge, published: () => published };
}

const SPEC = TARGETS[0];

test("a tool result patches a middle item though length and endpoints hold", () => {
  const { merge } = harness([
    metadataEvent(SPEC, 1_800_000_000),
    transcriptEvent(SPEC, 1, "t1", { kind: "user_prompt", content: "go" }),
    transcriptEvent(SPEC, 2, "t1", {
      kind: "tool_call",
      tool: { toolName: "Bash", toolId: "x", input: { command: "ls" } },
    }),
    transcriptEvent(SPEC, 3, "t1", { kind: "assistant_text", text: "waiting" }),
  ]);
  const [before] = merge();
  const middleBefore = before.transcript[1];
  const frozenView = JSON.stringify(middleBefore);
  const [afterRecord] = merge([
    transcriptEvent(SPEC, 4, "t1", {
      kind: "tool_result",
      toolId: "x",
      toolName: "Bash",
      content: "a b c",
      isError: false,
    }),
  ]);
  assert.equal(afterRecord.transcript.length, before.transcript.length);
  assert.equal(afterRecord.transcript[0], before.transcript[0]);
  assert.equal(afterRecord.transcript[2], before.transcript[2]);
  assert.notEqual(afterRecord.transcript, before.transcript);
  assert.notEqual(afterRecord.transcript[1], middleBefore);
  assert.notDeepStrictEqual(afterRecord.transcript[1], middleBefore);
  assert.equal(JSON.stringify(middleBefore), frozenView, "the old item held");
  assert.throws(() => {
    middleBefore.title = "mutated";
  }, TypeError);
});

test("metadata-only changes flow while the transcript array is retained", () => {
  const { merge } = harness([
    metadataEvent(SPEC, 1_800_000_000, { status: "running" }),
    transcriptEvent(SPEC, 1, "t1", { kind: "user_prompt", content: "go" }),
  ]);
  const [running] = merge();
  const [done] = merge([
    metadataEvent(SPEC, 1_800_000_050, { status: "idle", title: "Renamed" }),
  ]);
  assert.equal(running.status, "running");
  assert.equal(done.status, "idle");
  assert.equal(done.title, "Renamed");
  assert.equal(done.statusAt, 1_800_000_050_000);
  assert.equal(done.transcript, running.transcript);
});

test("metadata-only and transcript-only generations are both listed", () => {
  const { merge } = harness([
    metadataEvent(TARGETS[1], 1_800_000_000),
    transcriptEvent(TARGETS[2], 1, null, {
      kind: "result",
      subtype: "success",
      isError: false,
      durationMs: 1,
      result: "",
      costUsd: 0,
    }),
  ]);
  const records = merge();
  const byTarget = new Map(
    records.map((record) => [
      record.commandTarget.sessionId + record.commandTarget.generation,
      record,
    ]),
  );
  const metadataOnly = byTarget.get(`${SESSION_A}2`);
  const transcriptOnly = byTarget.get(`${SESSION_B}1`);
  assert.deepEqual(metadataOnly.transcript, []);
  assert.equal(metadataOnly.lastTranscriptAt, null);
  assert.equal(transcriptOnly.status, "completed", "inferred, not invented");
  assert.equal(transcriptOnly.metadataAuthorityPubkey, null);
});

test("a conflicting restatement withdraws the entry and rebuilds only its generation", () => {
  const other = TARGETS[2];
  const { merge, projection } = harness([
    transcriptEvent(SPEC, 1, "t1", { kind: "user_prompt", content: "go" }),
    transcriptEvent(SPEC, 2, "t1", { kind: "assistant_text", text: "one" }),
    transcriptEvent(other, 1, "o1", { kind: "user_prompt", content: "hi" }),
  ]);
  const before = merge();
  const otherBefore = before.find(
    (r) => r.providerAuthorityPubkey === PUBKEYS[1],
  );
  const statsBefore = projection.stats();
  const after = merge([
    transcriptEvent(SPEC, 2, "t1", { kind: "assistant_text", text: "two" }),
  ]);
  const mine = after.find((r) => r.providerAuthorityPubkey === PUBKEYS[0]);
  const otherAfter = after.find(
    (r) => r.providerAuthorityPubkey === PUBKEYS[1],
  );
  assert.equal(mine.transcript.length, 1, "the conflicted item is withheld");
  assert.equal(mine.conflictCount, 1);
  assert.equal(otherAfter.transcript, otherBefore.transcript);
  assert.equal(projection.stats().rebuilt - statsBefore.rebuilt, 1);
});

test("an invalid signature changes nothing and is counted, not shown", () => {
  const store = new TrustedCodingSessionIngressStore();
  const good = transcriptEvent(SPEC, 1, "t1", {
    kind: "user_prompt",
    content: "go",
  });
  const forged = {
    ...transcriptEvent(SPEC, 2, "t1", {
      kind: "assistant_text",
      text: "forged",
    }),
  };
  forged.sig = "0".repeat(128);
  store.ingestRelayEvents([good, forged], CHANNELS, OPEN);
  const snapshot = store.snapshot([CHANNEL]);
  const projection = createCodingSessionCatalogProjection();
  const composed = composeCodingSessionCatalogSnapshot(
    CHANNEL,
    {
      ...snapshot,
      isLoading: false,
      historyCompleteness: { state: "complete" },
      errorMessage: null,
      authorityErrorMessage: null,
      turnStartedAtFor: () => null,
    },
    { observations: [], geneses: [], isLoading: false },
    projection,
  );
  assert.equal(composed.invalidSignatureCount, 1);
  assert.equal(composed.entries[0].transcript.length, 1);
});

test("loading, completeness and errors flow through with no new transcript text", () => {
  const { published, projection } = harness([
    transcriptEvent(SPEC, 1, "t1", { kind: "user_prompt", content: "go" }),
  ]);
  const slice = (overrides) => ({
    ...published(),
    isLoading: true,
    historyCompleteness: { state: "pending" },
    errorMessage: null,
    authorityErrorMessage: null,
    turnStartedAtFor: () => null,
    ...overrides,
  });
  const creates = { observations: [], geneses: [], isLoading: false };
  const loading = composeCodingSessionCatalogSnapshot(
    CHANNEL,
    slice({}),
    creates,
    projection,
  );
  const settled = composeCodingSessionCatalogSnapshot(
    CHANNEL,
    slice({
      isLoading: false,
      historyCompleteness: { state: "complete" },
      errorMessage: "relay disconnected",
    }),
    creates,
    projection,
  );
  assert.equal(loading.isLoading, true);
  assert.equal(settled.isLoading, false);
  assert.deepEqual(settled.historyCompleteness, { state: "complete" });
  assert.equal(settled.errorMessage, "relay disconnected");
  assert.equal(settled.entries[0].status, "running", "running is not finished");
  assert.equal(settled.entries[0].transcript, loading.entries[0].transcript);
});

test("empty input, reset and repopulation never serve a stale prefix", () => {
  const projection = createCodingSessionCatalogProjection();
  const storeA = new TrustedCodingSessionIngressStore();
  storeA.ingestRelayEvents(
    [1, 2, 3].map((seq) =>
      transcriptEvent(SPEC, seq, "a", {
        kind: "assistant_text",
        text: `a${seq}`,
      }),
    ),
    CHANNELS,
    OPEN,
  );
  const a = storeA.snapshot([CHANNEL]);
  projection.merge(CHANNEL, a.metadata, a.transcripts);
  assert.deepEqual(projection.merge(CHANNEL, [], []), []);
  assert.equal(projection.stats().generations, 0, "evicted with its input");

  const storeB = new TrustedCodingSessionIngressStore();
  storeB.ingestRelayEvents(
    [1, 2].map((seq) =>
      transcriptEvent(SPEC, seq, "b", {
        kind: "assistant_text",
        text: `b${seq}`,
      }),
    ),
    CHANNELS,
    OPEN,
  );
  const b = storeB.snapshot([CHANNEL]);
  const repopulated = projection.merge(CHANNEL, b.metadata, b.transcripts);
  assert.deepStrictEqual(
    repopulated,
    oracleMergeTrustedCodingSessionIngress(CHANNEL, b.metadata, b.transcripts),
  );
  projection.reset();
  assert.deepStrictEqual(
    projection.merge(CHANNEL, a.metadata, a.transcripts),
    oracleMergeTrustedCodingSessionIngress(CHANNEL, a.metadata, a.transcripts),
  );
});

test("channels share a projection without evicting or leaking into each other", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents(
    [
      transcriptEvent(SPEC, 1, "t", { kind: "user_prompt", content: "here" }),
      transcriptEvent(
        SPEC,
        1,
        "t",
        { kind: "user_prompt", content: "there" },
        OTHER_CHANNEL,
      ),
    ],
    CHANNELS,
    OPEN,
  );
  const both = store.snapshot(CHANNELS);
  const projection = createCodingSessionCatalogProjection();
  const [here] = projection.merge(CHANNEL, both.metadata, both.transcripts);
  const [there] = projection.merge(
    OTHER_CHANNEL,
    both.metadata,
    both.transcripts,
  );
  const [hereAgain] = projection.merge(
    CHANNEL,
    both.metadata,
    both.transcripts,
  );
  assert.equal(projection.stats().generations, 2);
  assert.equal(
    hereAgain.transcript,
    here.transcript,
    "not evicted by the other channel",
  );
  assert.notEqual(here.generationId, there.generationId);
  assert.notDeepStrictEqual(here.transcript, there.transcript);
});

let dom;
before(() => {
  dom = new JSDOM(
    "<!doctype html><html><body><div id=root></div></body></html>",
  );
  Object.assign(globalThis, {
    document: dom.window.document,
    window: dom.window,
    IS_REACT_ACT_ENVIRONMENT: true,
  });
});
after(() => {
  dom.window.close();
});

test("the hook keeps one projection per scope under Strict Mode and swaps it on a scope change", async () => {
  const seen = [];
  function Probe({ scopeKey }) {
    seen.push({
      scopeKey,
      projection: useRetainedCodingSessionCatalogProjection(scopeKey),
    });
    return null;
  }
  let root = createRoot(document.getElementById("root"));
  const render = (scopeKey) =>
    act(async () => {
      root.render(
        React.createElement(
          React.StrictMode,
          null,
          React.createElement(Probe, { scopeKey }),
        ),
      );
    });
  await render("authority-a|channel-1");
  await render("authority-a|channel-1");
  const first = seen.filter((s) => s.scopeKey === "authority-a|channel-1");
  assert.ok(first.length >= 2);
  assert.ok(
    first.every((s) => s.projection === first[0].projection),
    "stable across renders",
  );

  await render("authority-b|channel-1");
  const replaced = seen.filter((s) => s.scopeKey === "authority-b|channel-1");
  assert.ok(replaced.every((s) => s.projection === replaced[0].projection));
  assert.notEqual(
    replaced[0].projection,
    first[0].projection,
    "authority change starts cold",
  );

  await act(async () => root.unmount());
  root = createRoot(document.getElementById("root"));
  await render("authority-b|channel-1");
  const remounted = seen.at(-1).projection;
  assert.notEqual(remounted, replaced[0].projection, "a remount starts cold");
  await act(async () => root.unmount());
});

test("a pre-epoch metadata time keeps the full rebuild's lastEventAt", () => {
  const store = new TrustedCodingSessionIngressStore();
  store.ingestRelayEvents([metadataEvent(SPEC, -5)], CHANNELS, OPEN);
  const snapshot = store.snapshot([CHANNEL]);
  const records = createCodingSessionCatalogProjection().merge(
    CHANNEL,
    snapshot.metadata,
    snapshot.transcripts,
  );
  assert.equal(records.length, 1);
  assert.deepStrictEqual(
    records,
    oracleMergeTrustedCodingSessionIngress(
      CHANNEL,
      snapshot.metadata,
      snapshot.transcripts,
    ),
  );
});
