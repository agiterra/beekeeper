import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildPulseMissionSessionInput,
  indexPulseMissionGenesis,
  MAX_PULSE_MISSION_SESSIONS,
  pulseMissionGrantInputs,
  pulseMissionOpenSessions,
  pulseMissionSeatInputs,
  readPulseMissionSessions,
  selectPulseMissionSessions,
} from "@/features/project-pulse/lib/pulseMissionSessionRead";

const CHANNEL = "11111111-2222-3333-4444-555555555555";
const OTHER_CHANNEL = "99999999-2222-3333-4444-555555555555";
const SESSION = "aaaaaaaa-1111-2222-3333-444444444444";
const GENESIS = "ab".repeat(32);
const FOUNDER = "cd".repeat(32);
const RELAY_SELF = "ef".repeat(32);
const OPERATOR = "12".repeat(32);
const SEAT = "34".repeat(32);

function session(sessionKey, latestObservationAt, sessionRef = SESSION) {
  return { sessionKey, sessionRef, name: null, latestObservationAt };
}

function genesisEvent(channelRef, sessionRef, id, pubkey = FOUNDER) {
  return {
    id,
    pubkey,
    created_at: 1_756_700_000,
    kind: 44226,
    tags: [
      ["h", channelRef],
      ["csg-v", "csg1-1"],
      ["csg-session", sessionRef],
    ],
    content: JSON.stringify({ sessionRef, v: 1 }),
    sig: "0".repeat(128),
  };
}

function record(kind, id, createdAt) {
  return {
    id,
    pubkey: FOUNDER,
    created_at: createdAt,
    kind,
    tags: [
      ["h", CHANNEL],
      ["d", SESSION],
    ],
    content: "{}",
    sig: "0".repeat(128),
  };
}

// ── Which sessions, in which order ───────────────────────────────────────────

test("only the digest's open sessions are candidates", () => {
  const open = pulseMissionOpenSessions({
    sessions: [
      {
        sessionKey: "a",
        sessionRef: SESSION,
        name: "A",
        lifecycle: "open",
        latestObservationAt: 10,
      },
      {
        sessionKey: "b",
        sessionRef: SESSION,
        name: "B",
        lifecycle: "closed",
        latestObservationAt: 20,
      },
    ],
  });
  assert.deepEqual(
    open.map((candidate) => candidate.sessionKey),
    ["a"],
  );
});

test("sessions are read newest-observation-first and tie by key, capped at eight", () => {
  const candidates = [
    session("z", 100),
    session("a", 100),
    session("older", 50),
    session("never", null),
  ];
  assert.deepEqual(
    selectPulseMissionSessions(candidates).map((entry) => entry.sessionKey),
    ["a", "z", "older", "never"],
  );
  const many = [];
  for (let index = 0; index < 9; index += 1) {
    many.push(session(`s-${index}`, 1_000 - index));
  }
  assert.equal(MAX_PULSE_MISSION_SESSIONS, 8);
  assert.equal(selectPulseMissionSessions(many).length, 8);
  assert.deepEqual(
    selectPulseMissionSessions(many).map((entry) => entry.sessionKey),
    ["s-0", "s-1", "s-2", "s-3", "s-4", "s-5", "s-6", "s-7"],
  );
});

// ── Genesis, the only source of the founder and the channel ──────────────────

test("a genesis event names the session's channel, genesis id and founder", () => {
  const indexed = indexPulseMissionGenesis([
    genesisEvent(CHANNEL, SESSION, GENESIS),
  ]);
  assert.deepEqual(indexed.genesis.get(SESSION), {
    channelRef: CHANNEL,
    genesisRef: GENESIS,
    founderPubkey: FOUNDER,
  });
  assert.deepEqual(indexed.ambiguous, []);
});

test("two genesis records for one session prove nothing and are disclosed", () => {
  const indexed = indexPulseMissionGenesis([
    genesisEvent(CHANNEL, SESSION, GENESIS),
    genesisEvent(OTHER_CHANNEL, SESSION, "ba".repeat(32)),
  ]);
  assert.equal(indexed.genesis.has(SESSION), false);
  assert.deepEqual(indexed.ambiguous, [SESSION]);
});

// ── The verified authority projection, mapped onto the wire ──────────────────

test("an operator grant is granted; a viewer grant and a revoke are not", () => {
  const grants = pulseMissionGrantInputs({
    activeGrants: [
      { actorPubkey: OPERATOR, grantEventRef: "11".repeat(32), maySteer: true },
      { actorPubkey: SEAT, grantEventRef: "22".repeat(32), maySteer: false },
    ],
    policyGrants: [
      { grantee: OPERATOR, acceptedAt: 100, transitionType: "grant-operator" },
      { grantee: SEAT, acceptedAt: 110, transitionType: "grant-viewer" },
      // A later seat transition for the same actor must not be mistaken for
      // the grant that put them in `activeGrants`.
      { grantee: OPERATOR, acceptedAt: 120, transitionType: "grant-seat" },
    ],
  });
  assert.deepEqual(grants, [
    {
      actorPubkey: OPERATOR,
      grantEventRef: "11".repeat(32),
      maySteer: true,
      acceptedAt: 100,
      granted: true,
    },
    {
      actorPubkey: SEAT,
      grantEventRef: "22".repeat(32),
      maySteer: false,
      acceptedAt: 110,
      granted: false,
    },
  ]);
});

test("an active grant with no accepted transition behind it proves nothing", () => {
  assert.equal(
    pulseMissionGrantInputs({
      activeGrants: [
        {
          actorPubkey: OPERATOR,
          grantEventRef: "11".repeat(32),
          maySteer: true,
        },
      ],
      policyGrants: [],
    }),
    null,
  );
});

test("seats reach the wire as actor and role, nothing else", () => {
  assert.deepEqual(
    pulseMissionSeatInputs({
      activeSeats: [
        { actorPubkey: SEAT, role: "lead", grantEventRef: "33".repeat(32) },
      ],
    }),
    [{ actorPubkey: SEAT, role: "lead" }],
  );
});

// ── Shaping one umbrella ─────────────────────────────────────────────────────

test("one session's input carries whole signed events, ascending, and no overlap guess", () => {
  const older = record(44246, "01".repeat(32), 10);
  const newer = record(44246, "02".repeat(32), 20);
  const built = buildPulseMissionSessionInput({
    session: session("s-1", 20),
    genesis: {
      channelRef: CHANNEL,
      genesisRef: GENESIS,
      founderPubkey: FOUNDER,
    },
    relayPubkey: RELAY_SELF,
    transitions: [],
    receipts: [],
    teamEvents: [],
    policyEvents: [],
    observationEvents: [newer, older],
  });
  assert.equal(built.ok, true);
  const value = built.value;
  assert.equal(value.sessionKey, "s-1");
  assert.equal(value.channelRef, CHANNEL);
  assert.equal(value.sessionRef, SESSION);
  assert.equal(value.genesisRef, GENESIS);
  assert.equal(value.founderPubkey, FOUNDER);
  assert.equal(value.latestObservationAt, 20);
  assert.deepEqual(value.activeSeats, []);
  assert.deepEqual(value.activeGrants, []);
  assert.deepEqual(value.claimedSeats, []);
  assert.deepEqual(value.refState, []);
  // `checkpoint.files` has not landed, so there is nothing to overlap on and
  // no row is guessed from a commit alone.
  assert.deepEqual(value.overlapFiles, []);
  assert.equal(value.overlapSha, null);
  assert.equal(value.overlapAsOf, null);
  assert.equal(value.overlapAuthor, null);
  // Ascending, and the very objects the relay handed back — the Rust side
  // verifies these signatures, so a reshaped event is an unverifiable one.
  assert.equal(value.observationEvents[0], older);
  assert.equal(value.observationEvents[1], newer);
});

// ── The read itself ──────────────────────────────────────────────────────────

function reader(overrides = {}) {
  const asked = [];
  const fetchEvents = async (filter) => {
    asked.push(filter);
    if (overrides.fetchEvents) return overrides.fetchEvents(filter);
    if (filter.kinds[0] === 44226) {
      return [genesisEvent(CHANNEL, SESSION, GENESIS)];
    }
    return [];
  };
  return { asked, fetchEvents };
}

test("a read with no trusted relay key verifies no authority and says so", async () => {
  const { fetchEvents } = reader();
  const result = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents, relaySelf: async () => null },
  );
  assert.deepEqual(result.sessions, []);
  assert.equal(result.readErrors.length, 1);
  assert.equal(result.readErrors[0].scope, "authority");
  assert.match(result.readErrors[0].message, /self signing key/i);
});

test("a failed team-record read is disclosed by scope, and its session is not sent", async () => {
  const result = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    {
      fetchEvents: async (filter) => {
        if (filter.kinds[0] === 44226) {
          return [genesisEvent(CHANNEL, SESSION, GENESIS)];
        }
        if (filter.kinds[0] === 44244) throw new Error("relay unreachable");
        return [];
      },
      relaySelf: async () => RELAY_SELF,
    },
  );
  assert.deepEqual(result.sessions, []);
  const disclosed = result.readErrors.find((error) =>
    error.message.includes("relay unreachable"),
  );
  assert.notEqual(disclosed, undefined);
  assert.equal(disclosed.scope, `missions:${CHANNEL}`);
});

test("a truncated record read is a partial read, disclosed rather than folded", async () => {
  const result = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    {
      fetchEvents: async (filter) => {
        if (filter.kinds[0] === 44226) {
          return [genesisEvent(CHANNEL, SESSION, GENESIS)];
        }
        if (filter.kinds[0] === 44246) {
          return Array.from({ length: filter.limit }, (_value, index) =>
            record(44246, `${index}`.padStart(64, "0"), 100 + index),
          );
        }
        return [];
      },
      relaySelf: async () => RELAY_SELF,
    },
  );
  assert.deepEqual(result.sessions, []);
  assert.equal(
    result.readErrors.some((error) => /truncated/.test(error.message)),
    true,
  );
});

test("a session with no genesis record is named, never quietly absent", async () => {
  const result = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents: async () => [], relaySelf: async () => RELAY_SELF },
  );
  assert.deepEqual(result.sessions, []);
  assert.equal(result.readErrors.length, 1);
  assert.equal(result.readErrors[0].scope, "missions:s-1");
  assert.match(result.readErrors[0].message, /genesis/i);
});

test("a complete read sends the session and records nothing", async () => {
  const { fetchEvents, asked } = reader();
  const result = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents, relaySelf: async () => RELAY_SELF },
  );
  assert.deepEqual(result.readErrors, []);
  assert.equal(result.sessions.length, 1);
  assert.equal(result.sessions[0].channelRef, CHANNEL);
  // The team, policy and observation reads each carry their own budget: one
  // filter naming all three would let observation volume evict the records a
  // seat's authority is proven from.
  const kinds = asked.map((filter) => filter.kinds.join(","));
  assert.equal(kinds.includes("44244"), true);
  assert.equal(kinds.includes("44245"), true);
  assert.equal(kinds.includes("44246"), true);
});
