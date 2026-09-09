import assert from "node:assert/strict";
import { test } from "node:test";

import {
  buildPulseDeclaredWorkSessionInput,
  buildPulseMissionSessionInput,
  indexPulseMissionGenesis,
  MAX_PULSE_MISSION_SESSIONS,
  PULSE_DECLARED_WORK_MAX_PAGES,
  PULSE_DECLARED_WORK_PAGE_SIZE,
  pulseDeclaredWorkPage,
  pulseDeclaredWorkSessions,
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
    lifecycleCommands: [],
    lifecycleReceipts: [],
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
  // S4: the provider proof travels too. Empty here means this read proved no
  // provider, which is what makes the row's gate lines say `unverified`.
  assert.deepEqual(value.lifecycleCommands, []);
  assert.deepEqual(value.lifecycleReceipts, []);
});

test("the read asks for the lifecycle pages that prove who provides a mission", async () => {
  const { asked, fetchEvents } = reader();
  await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents, relaySelf: async () => RELAY_SELF },
  );
  const lifecycle = asked.filter(
    (filter) => filter.kinds[0] === 44221 || filter.kinds[0] === 44224,
  );
  assert.equal(lifecycle.length, 2, JSON.stringify(asked));
  for (const filter of lifecycle) {
    assert.deepEqual(filter["#h"], [CHANNEL]);
    // A lifecycle command is tagged `csl-command`, never the umbrella's `d`:
    // a `#d` filter here would match nothing while looking as though it had
    // asked, and every mission would silently resolve no provider.
    assert.equal(filter["#d"], undefined);
  }
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

// ── Declared work: which sessions, in which page, read how ──────────────────
//
// The declared-work read differs from the mission read on three axes, and each
// one is a separate way to publish a falsehood: it includes **closed**
// sessions (closing settles nothing), it **pages** rather than truncating at
// eight (a trimmed page would be counted as scanned), and it reads **only**
// kind 44244 (a page fetched to be discarded is relay work nobody sees).

function visible(sessionKey, latestObservationAt, lifecycle = "open") {
  return {
    sessionKey,
    sessionRef: SESSION,
    name: null,
    lifecycle,
    latestObservationAt,
  };
}

test("every session is visible to a declared-work read, closed ones included", () => {
  const sessions = pulseDeclaredWorkSessions({
    sessions: [visible("open-one", 10), visible("closed-one", 20, "closed")],
  });
  // Closing an execution settles nothing: an unsettled assignment inside a
  // closed session is still unresolved, and a read that started from open
  // sessions alone would render it as work that does not exist.
  assert.deepEqual(
    sessions.map((session) => [session.sessionKey, session.lifecycle]),
    [
      ["closed-one", "closed"],
      ["open-one", "open"],
    ],
  );
});

test("visible sessions order newest first, unobserved last, ties by key", () => {
  const sessions = pulseDeclaredWorkSessions({
    sessions: [
      visible("b", null),
      visible("c", 30),
      visible("a", null),
      visible("z", 30),
      visible("m", 40),
    ],
  });
  assert.deepEqual(
    sessions.map((session) => session.sessionKey),
    ["m", "c", "z", "a", "b"],
  );
});

test("a null digest is no sessions, never an assumed one", () => {
  assert.deepEqual(pulseDeclaredWorkSessions(null), []);
});

test("pages are disjoint eight-session slices of the ordered list", () => {
  assert.equal(PULSE_DECLARED_WORK_PAGE_SIZE, 8);
  assert.equal(PULSE_DECLARED_WORK_MAX_PAGES, 4);
  const sessions = Array.from({ length: 19 }, (_value, index) => index);
  assert.deepEqual(
    pulseDeclaredWorkPage(sessions, 0),
    [0, 1, 2, 3, 4, 5, 6, 7],
  );
  assert.deepEqual(
    pulseDeclaredWorkPage(sessions, 1),
    [8, 9, 10, 11, 12, 13, 14, 15],
  );
  assert.deepEqual(pulseDeclaredWorkPage(sessions, 2), [16, 17, 18]);
  assert.deepEqual(pulseDeclaredWorkPage(sessions, 3), []);
  assert.deepEqual(pulseDeclaredWorkPage(sessions, -1), []);
});

test("a team-only read asks for kind 44244 and for nothing else per session", async () => {
  const { asked, fetchEvents } = reader();
  const result = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents, relaySelf: async () => RELAY_SELF },
    { records: "team-only" },
  );
  assert.deepEqual(result.readErrors, []);
  assert.equal(result.sessions.length, 1);
  const kinds = asked.map((filter) => filter.kinds.join(","));
  // Genesis, the authority chain and its relay receipts are read exactly as
  // the mission read reads them — the seats and grants this session is sent
  // with are still proven, not assumed.
  assert.equal(kinds.includes("44226"), true);
  assert.equal(kinds.includes("44228"), true);
  assert.equal(kinds.includes("40099"), true);
  assert.equal(kinds.includes("44244"), true);
  // And nothing the declared-work fold does not read.
  for (const unread of ["44245", "44246", "44221", "44224", "30618"]) {
    assert.equal(
      kinds.includes(unread),
      false,
      `a team-only read must not ask for kind ${unread}: ${JSON.stringify(kinds)}`,
    );
  }
  // The lists it did not read come back empty, and empty here means "not
  // asked for" — the lifecycle the request carries comes from the digest.
  const sent = result.sessions[0];
  assert.deepEqual(sent.policyEvents, []);
  assert.deepEqual(sent.observationEvents, []);
  assert.deepEqual(sent.lifecycleCommands, []);
  assert.deepEqual(sent.lifecycleReceipts, []);
  assert.deepEqual(sent.refState, []);
});

test("the default read is unchanged: every mission page is still asked for", async () => {
  const { asked, fetchEvents } = reader();
  await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents, relaySelf: async () => RELAY_SELF },
  );
  const kinds = asked.map((filter) => filter.kinds.join(","));
  for (const asked_kind of ["44244", "44245", "44246", "44221", "44224"]) {
    assert.equal(
      kinds.includes(asked_kind),
      true,
      `the default read must still ask for kind ${asked_kind}`,
    );
  }
});

test("a team-only read refuses more than one page rather than trimming it", async () => {
  const { fetchEvents } = reader();
  const sessions = Array.from({ length: 9 }, (_value, index) =>
    session(`s-${index}`, 100 - index),
  );
  // `selectPulseMissionSessions` would silently keep eight. That is right for
  // the mission read, whose scope sentence says so, and wrong here: the scan
  // sentence has already counted these sessions as scanned.
  await assert.rejects(
    () =>
      readPulseMissionSessions(
        { channelIds: [CHANNEL], openSessions: sessions },
        { fetchEvents, relaySelf: async () => RELAY_SELF },
        { records: "team-only" },
      ),
    /at most 8/,
  );
});

test("the declared-work request carries the digest's lifecycle and the read's records", () => {
  const older = record(44244, "aa".repeat(32), 10);
  const newer = record(44244, "bb".repeat(32), 20);
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
    teamEvents: [newer, older],
    policyEvents: [],
    observationEvents: [],
    lifecycleCommands: [],
    lifecycleReceipts: [],
  });
  assert.equal(built.ok, true);
  const request = buildPulseDeclaredWorkSessionInput(built.value, "closed");
  assert.equal(request.sessionKey, "s-1");
  assert.equal(request.channelRef, CHANNEL);
  assert.equal(request.sessionRef, SESSION);
  assert.equal(request.genesisRef, GENESIS);
  assert.equal(request.founderPubkey, FOUNDER);
  assert.equal(request.name, null);
  assert.equal(request.latestObservationAt, 20);
  // The lifecycle is the digest's, never inferred from an unread page.
  assert.equal(request.lifecycle, "closed");
  assert.deepEqual(request.activeSeats, []);
  assert.deepEqual(request.activeGrants, []);
  // Ascending, and byte-identical to the signed events — the Rust side
  // verifies these signatures itself.
  assert.deepEqual(
    request.teamEvents.map((event) => event.id),
    [older.id, newer.id],
  );
  assert.deepEqual(Object.keys(request.teamEvents[0]).sort(), [
    "content",
    "created_at",
    "id",
    "kind",
    "pubkey",
    "sig",
    "tags",
  ]);
});

test("a team-only record failure is scoped to the session, not to missions", async () => {
  const failing = async (filter) => {
    if (filter.kinds[0] === 44226) {
      return [genesisEvent(CHANNEL, SESSION, GENESIS)];
    }
    if (filter.kinds[0] === 44244) throw new Error("relay unreachable");
    return [];
  };
  const declared = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents: failing, relaySelf: async () => RELAY_SELF },
    { records: "team-only" },
  );
  assert.deepEqual(declared.sessions, []);
  const disclosed = declared.readErrors.find((error) =>
    error.message.includes("relay unreachable"),
  );
  // `missions:<channel>` would name the wrong feature, and two sessions in one
  // channel would report their failures under one indistinguishable scope.
  assert.equal(disclosed.scope, "declared:s-1");

  // The mission read's own scope is unchanged.
  const mission = await readPulseMissionSessions(
    { channelIds: [CHANNEL], openSessions: [session("s-1", 20)] },
    { fetchEvents: failing, relaySelf: async () => RELAY_SELF },
  );
  assert.equal(
    mission.readErrors.find((error) =>
      error.message.includes("relay unreachable"),
    ).scope,
    `missions:${CHANNEL}`,
  );
});
