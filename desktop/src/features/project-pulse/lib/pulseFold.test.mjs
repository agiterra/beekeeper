import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  PULSE_LEASE_TTL_SECONDS,
  foldProjectPulseDigest,
  pulseCommitConfirmation,
  pulseDigestBranches,
} from "@/features/project-pulse/lib/pulseFold";

const HERE = path.dirname(fileURLToPath(import.meta.url));
const VECTORS = JSON.parse(
  readFileSync(
    path.resolve(
      HERE,
      "../../../../../conformance/project-pulse-fold/fixtures/fold-vectors.json",
    ),
    "utf8",
  ),
);

const PROJECT =
  "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo";
const ALICE = "a1".repeat(32);
const BOB = "b2".repeat(32);

function entryEvent({
  id,
  pubkey = ALICE,
  createdAt,
  supersedes = null,
  branch = null,
  type = "plan",
  text = "A claim.",
  areas = [],
  sessionRef = null,
}) {
  const tags = [
    ["a", PROJECT],
    ["pu-v", "pu1-1"],
    ["pu-type", type],
  ];
  if (branch !== null) tags.push(["branch", branch]);
  if (sessionRef !== null) tags.push(["pu-session", sessionRef]);
  return {
    id,
    pubkey,
    created_at: createdAt,
    kind: 44240,
    tags,
    content: JSON.stringify({
      schema: "buzz-pulse-entry/v1",
      type,
      text,
      codeAreas: areas,
      branch,
      supersedes,
    }),
  };
}

const id = (suffix) => suffix.padStart(64, "0");

test("every banked conformance vector folds byte-identically", () => {
  for (const vector of VECTORS.vectors) {
    const digest = foldProjectPulseDigest({
      project: vector.input.project,
      now: vector.input.now,
      events: vector.input.events,
      sourceErrors: vector.input.sourceErrors,
    });
    assert.deepEqual(digest, vector.expected, vector.name);
  }
});

test("the lease TTL is the conservative client-composed lifetime", () => {
  assert.equal(PULSE_LEASE_TTL_SECONDS, 150);
});

test("a same-author revision retires its target and stays active itself", () => {
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 2_000,
    events: [
      entryEvent({ id: id("a1"), createdAt: 1_000 }),
      entryEvent({ id: id("a2"), createdAt: 1_500, supersedes: id("a1") }),
    ],
  });
  const [newest, oldest] = digest.entries;
  assert.equal(newest.eventId, id("a2"));
  assert.equal(newest.active, true);
  assert.equal(oldest.active, false);
  assert.deepEqual(oldest.supersededBy, [
    { eventId: id("a2"), pubkey: ALICE, honored: true, reason: null },
  ]);
});

test("a peer cannot retire another author's blocker", () => {
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 2_000,
    events: [
      entryEvent({ id: id("b1"), createdAt: 1_000, type: "blocker" }),
      entryEvent({
        id: id("b2"),
        pubkey: BOB,
        createdAt: 1_500,
        supersedes: id("b1"),
      }),
    ],
  });
  const target = digest.entries.find((entry) => entry.eventId === id("b1"));
  const claimant = digest.entries.find((entry) => entry.eventId === id("b2"));
  assert.equal(target.active, true, "the blocker stays in the active set");
  assert.deepEqual(claimant.supersededBy, [
    {
      eventId: id("b1"),
      pubkey: ALICE,
      honored: false,
      reason: "cross-author",
    },
  ]);
  assert.equal(digest.complete, true);
});

test("a supersession cycle cannot blank the active set", () => {
  // Two entries naming each other: single-pass marking means the total
  // (created_at, id) order decides once, and never recurses.
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 2_000,
    events: [
      entryEvent({ id: id("c1"), createdAt: 1_000, supersedes: id("c2") }),
      entryEvent({ id: id("c2"), createdAt: 1_000, supersedes: id("c1") }),
    ],
  });
  const active = digest.entries.filter((entry) => entry.active);
  assert.equal(active.length, 1);
  assert.equal(active[0].eventId, id("c2"), "the greater id wins the tie");
});

test("an invalid entry is excluded and reported, and is not a failed read", () => {
  const smuggled = entryEvent({ id: id("d1"), createdAt: 1_000 });
  smuggled.tags.push(["not-a-pulse-key", "x"]);
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 2_000,
    events: [smuggled],
  });
  assert.deepEqual(digest.entries, []);
  assert.deepEqual(digest.errors, [
    {
      scope: "invalid-entry",
      message: `entry ${id("d1")} failed validation and was excluded`,
    },
  ]);
  assert.equal(digest.complete, true);
});

test("a source error is the only thing that makes a read incomplete", () => {
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 2_000,
    events: [entryEvent({ id: id("e1"), createdAt: 1_000 })],
    sourceErrors: [{ scope: "sessions", message: "relay unavailable" }],
  });
  assert.equal(digest.complete, false);
  assert.equal(digest.entries.length, 1, "a partial read is never empty");
  assert.equal(digest.errors.length, 1);
});

test("an entry for another project never enters this digest", () => {
  const foreign = entryEvent({ id: id("f1"), createdAt: 1_000 });
  foreign.tags[0] = ["a", `30621:${"c3".repeat(32)}:other`];
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 2_000,
    events: [foreign],
  });
  assert.deepEqual(digest.entries, []);
  assert.deepEqual(digest.errors, []);
});

test("branch groups keep null as its own group, last", () => {
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 2_000,
    events: [
      entryEvent({ id: id("g1"), createdAt: 1_000, branch: "wip/two" }),
      entryEvent({ id: id("g2"), createdAt: 1_001, branch: "wip/one" }),
      entryEvent({ id: id("g3"), createdAt: 1_002 }),
    ],
  });
  assert.deepEqual(pulseDigestBranches(digest), ["wip/one", "wip/two", null]);
});

function foldedVector(name) {
  const vector = VECTORS.vectors.find((candidate) => candidate.name === name);
  assert.ok(vector, `missing vector ${name}`);
  return foldProjectPulseDigest({
    project: vector.input.project,
    now: vector.input.now,
    events: vector.input.events,
    sourceErrors: vector.input.sourceErrors,
  });
}

test("old idle metadata remains reachable only through authorized lease evidence", () => {
  const digest = foldedVector("idle-hours-old-with-live-authorized-lease");
  assert.deepEqual(digest.providerReachableSessions, [
    "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
  ]);
  const generation = digest.sessions[0].generations[0];
  assert.equal(digest.sessions[0].observedAgeSeconds, 10_000);
  assert.equal(generation.reachability, "provider_reachable");
  assert.equal(generation.leaseAcceptedAt, null);
  assert.equal(generation.leaseExpiresAt, 1_785_600_090);
});

test("reordered lifecycle and lease envelopes are rejected", () => {
  const source = VECTORS.vectors.find(
    (vector) => vector.name === "idle-hours-old-with-live-authorized-lease",
  );
  assert.ok(source);
  for (const kind of [44221, 44224, 24223]) {
    const vector = structuredClone(source);
    const events = vector.input.events.filter(
      (candidate) => candidate.kind === kind,
    );
    assert.notEqual(events.length, 0, `missing kind ${kind}`);
    for (const event of events) {
      [event.tags[0], event.tags[1]] = [event.tags[1], event.tags[0]];
    }
    const digest = foldProjectPulseDigest({
      project: vector.input.project,
      now: vector.input.now,
      events: vector.input.events,
      sourceErrors: vector.input.sourceErrors,
    });
    if (kind === 24223) {
      assert.deepEqual(digest.providerReachableSessions, []);
      assert.deepEqual(digest.openUnverifiedSessions, [
        "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
      ]);
    } else {
      assert.deepEqual(digest.sessions, []);
    }
  }
});

test("malformed lifecycle JSON fails closed instead of throwing", () => {
  const source = VECTORS.vectors.find(
    (vector) => vector.name === "idle-hours-old-with-live-authorized-lease",
  );
  assert.ok(source);
  const vector = structuredClone(source);
  const command = vector.input.events.find((event) => event.kind === 44221);
  assert.ok(command);
  command.content = '{"unterminated';
  assert.doesNotThrow(() =>
    foldProjectPulseDigest({
      project: vector.input.project,
      now: vector.input.now,
      events: vector.input.events,
      sourceErrors: vector.input.sourceErrors,
    }),
  );
});

test("all non-live lease outcomes fail closed without erasing evidence", () => {
  const digest = foldedVector("unverified-lease-outcomes");
  assert.deepEqual(digest.providerReachableSessions, []);
  assert.equal(digest.openUnverifiedSessions.length, 1);
  const generations = Object.fromEntries(
    digest.sessions[0].generations.map((generation) => [
      generation.executionKey.split(":").at(-1),
      generation,
    ]),
  );
  assert.equal(Object.keys(generations).length, 6);
  assert.equal(generations.none.leaseState, null);
  assert.equal(generations.released.leaseState, "released");
  assert.equal(generations.expired.leaseExpiresAt, 1_785_599_970);
  assert.equal(generations.expired.reachability, "unverified");
  assert.equal(generations.conflict.leaseSequence, 7);
  assert.equal(generations.conflict.leaseSourceEventId, null);
  assert.equal(generations.wrong.leaseSequence, null);
  assert.equal(generations.terminal.reachability, "terminal");
});

test("resume advances exactly one generation and never inherits reachability", () => {
  const digest = foldedVector("resume-generation-isolation-and-continuity");
  const [current, predecessor] = digest.sessions[0].generations;
  assert.equal(digest.sessions.length, 1, "resume stays in one umbrella");
  assert.equal(
    digest.sessions[0].generations.length,
    2,
    "generation 4 is rejected",
  );
  assert.equal(current.targetKey.endsWith("1:2"), true);
  assert.equal(current.current, true);
  assert.equal(current.reachability, "unverified");
  assert.equal(predecessor.current, false);
  assert.equal(predecessor.leaseState, "live");
  assert.equal(predecessor.reachability, "unverified");
});

test("durable closure outranks a live current generation", () => {
  const digest = foldedVector("closure-outranks-live-generation");
  assert.equal(
    digest.sessions[0].generations[0].reachability,
    "provider_reachable",
  );
  assert.equal(digest.sessions[0].coordinationState, "closed");
  assert.deepEqual(digest.providerReachableSessions, []);
  assert.deepEqual(digest.closedSessions, [
    "8d3f6b41-2c07-4e6a-9f52-31ab7c9e0d64",
  ]);
});

test("commit confirmation is tri-state and never says unreachable", () => {
  assert.equal(pulseCommitConfirmation(true), "Commit confirmed on relay");
  assert.equal(pulseCommitConfirmation(false), "Commit not found on relay");
  assert.equal(pulseCommitConfirmation(null), "Commit not checked");
});
