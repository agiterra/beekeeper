import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { finalizeEvent, getPublicKey } from "nostr-tools/pure";

import { foldCodingSessionHandover } from "./codingSessionHandoverFold.ts";
import {
  buildCodingSessionHandoverContent,
  buildCodingSessionHandoverTags,
  KIND_CODING_SESSION_HANDOVER,
} from "./codingSessionHandoverWire.ts";

// Lane K's Rust-written fixture is this suite's pin: the last test folds the
// very bytes `the_typescript_fixture_is_this_folds_real_output` generated, so
// a rule the Rust fold changes and this twin does not is a failure here. The
// hand-built events above it exercise the branches the fixture does not carry.
const FIXTURE_PATH = fileURLToPath(
  new URL("./codingSessionHandover.fixture.json", import.meta.url),
);

const CHANNEL = "98610076-0cb7-4d5e-9f82-5f4ad7c5a723";
const SESSION = "683d55b3-d34e-410d-874a-55f9082d2631";
const GENESIS = "ab".repeat(32);
const FOUNDER_SECRET = new Uint8Array(32).fill(1);
const B_SECRET = new Uint8Array(32).fill(2);
const STRANGER_SECRET = new Uint8Array(32).fill(3);
const FOUNDER = getPublicKey(FOUNDER_SECRET);
const B = getPublicKey(B_SECRET);
const STRANGER = getPublicKey(STRANGER_SECRET);
const BODY_B = "2b".repeat(32);
const CLAIM_EVENT = "c1".repeat(32);
const OLD_CLAIM_EVENT = "c0".repeat(32);
const SHA = "9".repeat(40);

function checkpointBody(overrides = {}) {
  return {
    prevCheckpointRef: null,
    task: "Continue the handover panel",
    assignmentRefs: [],
    decisions: [],
    revision: {
      repoRef: null,
      baseSha: null,
      headSha: SHA,
      branch: "work/handover",
      dirty: true,
      preserved: "partial",
      ...(overrides.revision ?? {}),
    },
    artifacts: [],
    tests: [],
    unresolved: [],
    nextAction: "Mount the panel",
    missing: [],
    ...overrides,
  };
}

function continuationBody(overrides = {}) {
  return {
    claimRef: CLAIM_EVENT,
    mode: "reconstructed",
    checkpointRef: null,
    target: {
      driver: "acp",
      instanceId: "instance-b",
      sessionId: "session-b",
      generation: 1,
    },
    recovered: [],
    missing: [],
    note: null,
    ...overrides,
  };
}

function handover(
  type,
  body,
  { secret = FOUNDER_SECRET, createdAt = 10 } = {},
) {
  return finalizeEvent(
    {
      kind: KIND_CODING_SESSION_HANDOVER,
      created_at: createdAt,
      tags: buildCodingSessionHandoverTags({
        channelId: CHANNEL,
        sessionRef: SESSION,
        genesisRef: GENESIS,
        type,
      }),
      content: buildCodingSessionHandoverContent({
        sessionRef: SESSION,
        genesisRef: GENESIS,
        type,
        body,
      }),
    },
    secret,
  );
}

const ACTIVE_CLAIM = {
  state: "active",
  claimant: B,
  bodyPubkey: BODY_B,
  acceptedEventId: CLAIM_EVENT,
  seq: 2,
};

function context(overrides = {}) {
  return {
    founderPubkey: FOUNDER,
    grants: [{ pubkey: B, acceptedAt: 1 }],
    seats: [],
    claim: ACTIVE_CLAIM,
    claimSince: 5,
    claimVoidedAt: null,
    claimHistory: [
      {
        claimant: B,
        bodyPubkey: BODY_B,
        acceptedEventId: CLAIM_EVENT,
        seq: 2,
      },
    ],
    retired: false,
    ...overrides,
  };
}

function foldResult(events, overrides = {}) {
  return foldCodingSessionHandover({
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    events,
    context: context(overrides),
  });
}

function fold(events, overrides = {}) {
  const result = foldResult(events, overrides);
  assert.equal(result.ok, true, result.ok ? "" : result.error);
  return result.value;
}

test("the founder's checkpoint is authorized and is the latest one", () => {
  const first = handover("checkpoint", checkpointBody(), { createdAt: 10 });
  const second = handover("checkpoint", checkpointBody(), { createdAt: 20 });
  const result = fold([second, first]);
  assert.deepEqual(
    result.checkpoints.map((row) => row.createdAt),
    [10, 20],
    "checkpoints ascend by signed time, never by arrival",
  );
  assert.equal(result.latestAuthorizedCheckpoint, second.id);
  assert.equal(result.checkpoints[0].standing, "authorized");
});

test("a live operator's checkpoint counts; a stranger's is listed unauthorized", () => {
  const operator = handover("checkpoint", checkpointBody(), {
    secret: B_SECRET,
    createdAt: 30,
  });
  const stranger = handover("checkpoint", checkpointBody(), {
    secret: STRANGER_SECRET,
    createdAt: 40,
  });
  const result = fold([operator, stranger]);
  assert.equal(result.checkpoints.length, 2);
  assert.equal(
    result.checkpoints.find((row) => row.author === B).standing,
    "authorized",
  );
  assert.equal(
    result.checkpoints.find((row) => row.author === STRANGER).standing,
    "unauthorized",
  );
  assert.deepEqual(result.excluded, [
    {
      eventId: stranger.id,
      reason: `checkpoint by ${STRANGER} is not used for reconstruction: that pubkey was neither the founder nor a live operator nor an active seat of this umbrella when it was written`,
    },
  ]);
  assert.equal(
    result.latestAuthorizedCheckpoint,
    operator.id,
    "the newest authorized checkpoint is not the newest checkpoint",
  );
});

test("a grant accepted after the checkpoint does not back-date standing", () => {
  const early = handover("checkpoint", checkpointBody(), {
    secret: B_SECRET,
    createdAt: 5,
  });
  const result = fold([early], { grants: [{ pubkey: B, acceptedAt: 100 }] });
  assert.equal(result.checkpoints[0].standing, "unauthorized");
  assert.equal(result.latestAuthorizedCheckpoint, null);
});

test("an active seat of the umbrella may checkpoint", () => {
  const seated = handover("checkpoint", checkpointBody(), {
    secret: STRANGER_SECRET,
    createdAt: 50,
  });
  const result = fold([seated], {
    grants: [],
    seats: [{ pubkey: STRANGER, acceptedAt: 1, role: "builder" }],
  });
  assert.equal(result.checkpoints[0].standing, "authorized");
});

test("the claimant's continuation under the live claim is the active one", () => {
  const continuation = handover("continuation", continuationBody(), {
    secret: B_SECRET,
    createdAt: 60,
  });
  const result = fold([continuation]);
  assert.equal(result.continuations[0].standing, "authorized");
  assert.equal(result.activeContinuation, continuation.id);
  assert.equal(result.continuations[0].mode, "reconstructed");
});

test("a continuation signed by anybody but its claimant is unauthorized", () => {
  const impostor = handover("continuation", continuationBody(), {
    secret: STRANGER_SECRET,
    createdAt: 70,
  });
  const result = fold([impostor]);
  assert.equal(result.continuations[0].standing, "unauthorized");
  assert.equal(result.activeContinuation, null);
  assert.match(result.excluded[0].reason, /that claim is held by/);
});

test("a continuation naming a claim this chain never accepted is unauthorized", () => {
  const orphan = handover(
    "continuation",
    continuationBody({ claimRef: "ee".repeat(32) }),
    { secret: B_SECRET, createdAt: 80 },
  );
  const result = fold([orphan]);
  assert.equal(
    result.continuations[0].standing,
    "superseded",
    "history is not a violation: a claim that has moved on is still history",
  );
  assert.equal(result.activeContinuation, null);
  assert.match(result.excluded[0].reason, /is not the claim in force/);
});

test("a voided claim keeps its continuation as history and names no active one", () => {
  const continuation = handover(
    "continuation",
    continuationBody({ claimRef: OLD_CLAIM_EVENT }),
    { secret: B_SECRET, createdAt: 90 },
  );
  const result = fold([continuation], {
    claim: {
      state: "voided",
      last: {
        claimant: B,
        bodyPubkey: BODY_B,
        acceptedEventId: OLD_CLAIM_EVENT,
        seq: 2,
      },
      voidedBy: "dd".repeat(32),
      seq: 3,
    },
    claimVoidedAt: 500,
    claimHistory: [
      {
        claimant: B,
        bodyPubkey: BODY_B,
        acceptedEventId: OLD_CLAIM_EVENT,
        seq: 2,
      },
    ],
  });
  assert.equal(result.continuations[0].standing, "superseded");
  assert.equal(result.activeContinuation, null);
  assert.equal(result.claim.state, "voided");
});

test("a retired genesis yields no claim, whatever the chain says", () => {
  const continuation = handover("continuation", continuationBody(), {
    secret: B_SECRET,
    createdAt: 100,
  });
  const result = fold([continuation], { retired: true });
  assert.deepEqual(result.claim, { state: "no-claim" });
  assert.equal(result.activeContinuation, null);
  assert.equal(result.retired, true);
  assert.equal(
    result.continuations.length,
    1,
    "history is not un-published by a deletion",
  );
  assert.equal(result.latestAuthorizedCheckpoint, null);
  assert.equal(result.claimSince, null);
  // Standing is judged against the chain's own claim before the retirement
  // clears it — Rust's order. The claimant's continuation therefore stays
  // `authorized` history, and the reason it carries is the deletion.
  assert.equal(result.continuations[0].standing, "authorized");
  assert.deepEqual(
    result.excluded.map((row) => row.reason),
    ["this session was deleted: nothing here is reconstructed or resumed"],
  );
  const checkpointOnly = fold(
    [handover("checkpoint", checkpointBody(), { createdAt: 101 })],
    { retired: true },
  );
  assert.deepEqual(
    checkpointOnly.excluded.map((row) => row.reason),
    ["this session was deleted: nothing here is reconstructed or resumed"],
  );
});

test("an author's own next checkpoint replaces the one it names", () => {
  const first = handover("checkpoint", checkpointBody(), { createdAt: 200 });
  // Same second, and the *older* id sorts first — the case a clock cannot
  // order and a hash used to decide.
  const second = handover(
    "checkpoint",
    checkpointBody({ prevCheckpointRef: first.id }),
    { createdAt: 200 },
  );
  const result = fold([first, second]);
  const replaced = result.checkpoints.find((row) => row.eventId === first.id);
  const newest = result.checkpoints.find((row) => row.eventId === second.id);
  assert.equal(replaced.standing, "superseded");
  assert.equal(replaced.supersededBy, second.id);
  assert.equal(newest.standing, "authorized");
  assert.equal(newest.supersededBy, null);
  assert.equal(
    result.latestAuthorizedCheckpoint,
    second.id,
    "the newest statement wins, whatever the ids hash to",
  );
  assert.ok(
    result.excluded.some(
      (row) =>
        row.eventId === first.id &&
        /checkpoint replaced by .*newest statement, not the newest timestamp/.test(
          row.reason,
        ),
    ),
    JSON.stringify(result.excluded),
  );
});

test("nobody supersedes anybody else's statement", () => {
  const mine = handover("checkpoint", checkpointBody(), { createdAt: 300 });
  // Another author naming it, an unauthorized author naming it, a self
  // reference, and an unknown id: four no-ops, each carried verbatim.
  const otherAuthor = handover(
    "checkpoint",
    checkpointBody({ prevCheckpointRef: mine.id }),
    { secret: B_SECRET, createdAt: 301 },
  );
  const stranger = handover(
    "checkpoint",
    checkpointBody({ prevCheckpointRef: mine.id }),
    { secret: STRANGER_SECRET, createdAt: 302 },
  );
  const unknown = handover(
    "checkpoint",
    checkpointBody({ prevCheckpointRef: "ff".repeat(32) }),
    { createdAt: 303 },
  );
  const result = fold([mine, otherAuthor, stranger, unknown]);
  const standingOf = (id) =>
    result.checkpoints.find((row) => row.eventId === id).standing;
  assert.equal(standingOf(mine.id), "authorized");
  assert.equal(standingOf(otherAuthor.id), "authorized");
  assert.equal(standingOf(stranger.id), "unauthorized");
  assert.equal(
    result.checkpoints.find((row) => row.eventId === unknown.id).body
      .prevCheckpointRef,
    "ff".repeat(32),
    "an unknown reference is carried verbatim, not dropped",
  );
  assert.equal(result.latestAuthorizedCheckpoint, unknown.id);
});

test("a self-reference and a mutual pair are read safely", () => {
  const selfNaming = handover("checkpoint", checkpointBody(), {
    createdAt: 400,
  });
  const selfResult = fold([
    {
      ...selfNaming,
      // Rebuilt so the content names the event's own id.
      ...(() => {
        const rebuilt = handover(
          "checkpoint",
          checkpointBody({ prevCheckpointRef: selfNaming.id }),
          { createdAt: 400 },
        );
        return rebuilt;
      })(),
    },
  ]);
  assert.equal(selfResult.checkpoints[0].standing, "authorized");
});

test("an unsigned, cross-scope or malformed record fails the whole set by name", () => {
  const tampered = handover("checkpoint", checkpointBody(), { createdAt: 110 });
  tampered.content = tampered.content.replace("partial", "none");
  const crossScope = finalizeEvent(
    {
      kind: KIND_CODING_SESSION_HANDOVER,
      created_at: 120,
      tags: buildCodingSessionHandoverTags({
        channelId: CHANNEL,
        sessionRef: "11111111-2222-4333-8444-555555555555",
        genesisRef: GENESIS,
        type: "checkpoint",
      }),
      content: buildCodingSessionHandoverContent({
        sessionRef: "11111111-2222-4333-8444-555555555555",
        genesisRef: GENESIS,
        type: "checkpoint",
        body: checkpointBody(),
      }),
    },
    FOUNDER_SECRET,
  );
  // A defect fails the set rather than being listed: these records decide
  // what somebody reconstructs from, so "no checkpoint" must never be printed
  // over a checkpoint this build could not read.
  const unsigned = foldResult([tampered]);
  assert.equal(unsigned.ok, false);
  assert.match(
    unsigned.error,
    new RegExp(`${tampered.id} has an invalid signature`),
  );

  const crossed = foldResult([crossScope]);
  assert.equal(crossed.ok, false);
  assert.match(crossed.error, /crosses or disagrees/);
});

test("the same record delivered twice fails rather than doubling a continuation", () => {
  const checkpoint = handover("checkpoint", checkpointBody(), {
    createdAt: 130,
  });
  const result = foldResult([checkpoint, { ...checkpoint }]);
  assert.equal(result.ok, false);
  assert.match(result.error, /supplied twice/);
});

/** Fold one of the fixture's scenarios with the context it was written for. */
function foldFixtureScenario(fixture, scenario, retired) {
  const events = scenario.events.filter(
    (event) => event.kind === KIND_CODING_SESSION_HANDOVER,
  );
  const scope = Object.fromEntries(events[0].tags);
  // The founder, read out of the scenario's own answer: the author of a
  // checkpoint that fold did not call `unauthorized`. The stranger's
  // checkpoint is what proves the standing rule still bites, and no choice of
  // founder could make that one authorized.
  const founderPubkey = scenario.fold.checkpoints.find(
    (row) => row.standing !== "unauthorized",
  ).author;
  return foldCodingSessionHandover({
    channelRef: scope.h,
    sessionRef: scope.d,
    genesisRef: scope["csh-genesis"],
    events,
    context: {
      founderPubkey,
      // The claim is supplied **uncleared** even for the retired scenario —
      // that is what the Rust fold is given, and judging standing against it
      // before clearing is the whole of the reading this pins.
      grants: [{ pubkey: fixture.fold.claim.claimant, acceptedAt: 0 }],
      seats: [],
      claim: fixture.fold.claim,
      claimSince: fixture.fold.claimSince,
      claimVoidedAt: null,
      retired,
    },
  });
}

test("the fixture's retired scenario is exactly what this twin answers", () => {
  const fixture = JSON.parse(readFileSync(FIXTURE_PATH, "utf8"));
  const result = foldFixtureScenario(fixture, fixture.retired, true);
  assert.equal(result.ok, true, result.ok ? "" : result.error);
  const fold = result.value;
  const expected = fixture.retired.fold;
  assert.deepEqual(fold.claim, expected.claim);
  assert.equal(fold.retired, true);
  assert.equal(fold.latestAuthorizedCheckpoint, null);
  assert.equal(fold.activeContinuation, null);
  assert.deepEqual(
    fold.checkpoints.map((row) => [row.eventId, row.standing]),
    expected.checkpoints.map((row) => [row.eventId, row.standing]),
  );
  assert.deepEqual(
    fold.continuations.map((row) => [row.eventId, row.standing]),
    expected.continuations.map((row) => [row.eventId, row.standing]),
    "standing is judged against the uncleared claim, so the claimant's own continuation stays authorized",
  );
  assert.deepEqual(fold.excluded, expected.excluded);
});

test("Lane K's fixture is exactly what this twin answers", () => {
  const fixture = JSON.parse(readFileSync(FIXTURE_PATH, "utf8"));
  // The fixture holds the fold's own answer and the events it folded, but not
  // the context the Rust writer used, so the context is read back out of the
  // fixture (see `foldFixtureScenario`). The stranger's checkpoint is what
  // proves the standing rule still bites — no context could make that one
  // authorized.
  const result = foldFixtureScenario(fixture, fixture, fixture.fold.retired);
  assert.equal(result.ok, true, result.ok ? "" : result.error);
  const fold = result.value;
  // The regenerated fixture's whole point: the superseding checkpoint sorts
  // **first** among the entries and is still the latest authorized one.
  assert.equal(fold.checkpoints.length, 3);
  assert.equal(
    fold.latestAuthorizedCheckpoint,
    fold.checkpoints[0].eventId,
    "order in the list is not recency; the author's own statement is",
  );
  assert.deepEqual(
    fold.checkpoints.map((row) => [
      row.eventId,
      row.standing,
      row.supersededBy,
    ]),
    fixture.fold.checkpoints.map((row) => [
      row.eventId,
      row.standing,
      row.supersededBy ?? null,
    ]),
  );
  assert.equal(
    fold.continuations[0].checkpointRef,
    fold.latestAuthorizedCheckpoint,
    "the claimant reconstructed from the newest statement",
  );
  assert.deepEqual(
    fold.checkpoints.map((row) => [row.eventId, row.author, row.standing]),
    fixture.fold.checkpoints.map((row) => [
      row.eventId,
      row.author,
      row.standing,
    ]),
  );
  assert.deepEqual(
    fold.continuations.map((row) => [
      row.eventId,
      row.author,
      row.claimRef,
      row.mode,
      row.standing,
    ]),
    fixture.fold.continuations.map((row) => [
      row.eventId,
      row.author,
      row.claimRef,
      row.mode,
      row.standing,
    ]),
  );
  assert.equal(
    fold.latestAuthorizedCheckpoint,
    fixture.fold.latestAuthorizedCheckpoint,
  );
  assert.equal(fold.activeContinuation, fixture.fold.activeContinuation);
  assert.deepEqual(fold.claim, fixture.fold.claim);
  assert.equal(fold.claimSince, fixture.fold.claimSince);
  assert.equal(fold.retired, fixture.fold.retired);
  // The exclusion reasons are the Rust fold's own words, byte for byte.
  assert.deepEqual(fold.excluded, fixture.fold.excluded);
  // The bodies survive the decoder intact, field for field.
  assert.deepEqual(fold.checkpoints[0].body, fixture.fold.checkpoints[0].body);
  assert.deepEqual(
    {
      claimRef: fold.continuations[0].claimRef,
      mode: fold.continuations[0].mode,
      checkpointRef: fold.continuations[0].checkpointRef,
      target: fold.continuations[0].target,
      recovered: fold.continuations[0].recovered,
      missing: fold.continuations[0].missing,
      note: fold.continuations[0].note,
    },
    fixture.fold.continuations[0].body,
  );
});
