import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  PULSE_ACTIVE_WINDOW_SECONDS,
  foldProjectPulseDigest,
  pulseCommitConfirmation,
  pulseDigestBranches,
  pulseSessionActivity,
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

test("the active window matches the corpus that pins it", () => {
  assert.equal(PULSE_ACTIVE_WINDOW_SECONDS, VECTORS.activeWindowSeconds);
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

test("activity needs all three positive signals", () => {
  const base = {
    status: "running",
    statusAt: 1_000,
    closed: false,
    now: 1_000,
  };
  assert.equal(pulseSessionActivity(base), "active");
  assert.equal(
    pulseSessionActivity({ ...base, now: 1_000 + PULSE_ACTIVE_WINDOW_SECONDS }),
    "active",
    "the window's last second is still active",
  );
  assert.equal(
    pulseSessionActivity({
      ...base,
      now: 1_001 + PULSE_ACTIVE_WINDOW_SECONDS,
    }),
    "stale",
  );
  assert.equal(
    pulseSessionActivity({ ...base, status: "disconnected" }),
    "stale",
    "a dead provider is never active work",
  );
  assert.equal(
    pulseSessionActivity({ ...base, closed: true }),
    "stale",
    "a closed umbrella is never active work",
  );
});

test("commit confirmation is tri-state and never says unreachable", () => {
  assert.equal(pulseCommitConfirmation(true), "Commit confirmed on relay");
  assert.equal(pulseCommitConfirmation(false), "Commit not found on relay");
  assert.equal(pulseCommitConfirmation(null), "Commit not checked");
});
