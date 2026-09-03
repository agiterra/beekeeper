import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import {
  invokePulseMissionRows,
  PULSE_MISSION_ROWS_COMMAND,
} from "@/features/project-pulse/lib/invokePulseMissionRows";

const here = dirname(fileURLToPath(import.meta.url));
const PROJECT =
  "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo";

function fixture() {
  return JSON.parse(
    readFileSync(resolve(here, "pulseMissionResponse.fixture.json"), "utf8"),
  );
}

function channelIds(payload = fixture()) {
  return [...new Set(payload.missions.map((mission) => mission.channelId))];
}

function stubInvoke(response, calls = []) {
  return async (command, args) => {
    calls.push({ command, args });
    return response;
  };
}

async function failure(promise) {
  try {
    await promise;
    return null;
  } catch (error) {
    return error instanceof Error ? error.message : String(error);
  }
}

test("the request names the command and echoes the coordinate it asked about", async () => {
  const calls = [];
  await invokePulseMissionRows(
    { project: PROJECT, channelIds: channelIds() },
    { invoke: stubInvoke(fixture(), calls) },
  );
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, PULSE_MISSION_ROWS_COMMAND);
  assert.equal(calls[0].args.request.project, PROJECT);
  assert.deepEqual(calls[0].args.request.channelIds, channelIds());
});

test("a response is decoded, not passed through", async () => {
  const rows = await invokePulseMissionRows(
    { project: PROJECT, channelIds: channelIds() },
    { invoke: stubInvoke(fixture()) },
  );
  assert.equal(Object.isFrozen(rows), true);
  assert.equal(rows.missions.length, 3);
});

test("a malformed response fails loudly rather than returning an empty Pulse", async () => {
  const broken = fixture();
  broken.missions[0].state = "vibing";
  const message = await failure(
    invokePulseMissionRows(
      { project: PROJECT, channelIds: channelIds() },
      { invoke: stubInvoke(broken) },
    ),
  );
  assert.match(String(message), /vibing/);
  // The distinction that matters: this is not "no missions", it is "no answer".
  assert.notEqual(message, null);
});

test("a mission for a channel this read never asked about is refused", async () => {
  // A response naming a channel outside the request is not this project's
  // answer. Rendering it would attribute another project's work to this one.
  const message = await failure(
    invokePulseMissionRows(
      { project: PROJECT, channelIds: [channelIds()[0]] },
      { invoke: stubInvoke(fixture()) },
    ),
  );
  assert.match(String(message), /channel/i);
});

test("an empty channel set asks for nothing and binds nothing", async () => {
  // A project whose channel set has not resolved must not silently accept an
  // answer scoped to channels it never named.
  const rows = await invokePulseMissionRows(
    { project: PROJECT, channelIds: [] },
    { invoke: stubInvoke({ ...fixture(), missions: [] }) },
  );
  assert.deepEqual(rows.missions, []);
});

test("a transport failure propagates instead of becoming a quiet project", async () => {
  const message = await failure(
    invokePulseMissionRows(
      { project: PROJECT, channelIds: channelIds() },
      {
        invoke: async () => {
          throw new Error("relay unreachable");
        },
      },
    ),
  );
  assert.equal(message, "relay unreachable");
});

test("the request carries every key the native command declares", () => {
  // The native `PulseMissionRowsRequest` refuses unknown fields and defaults
  // nothing a reader would notice, so a request missing `schema` fails at the
  // boundary rather than in the fold. This pins the shape both sides agreed on.
  let sent = null;
  const invoke = async (_command, args) => {
    sent = args;
    return fixture();
  };
  const session = {
    sessionKey: "s-1",
    channelRef: "11111111-2222-3333-4444-555555555555",
    sessionRef: "aaaaaaaa-1111-2222-3333-444444444444",
    genesisRef: "ab".repeat(32),
    founderPubkey: "cd".repeat(32),
    name: null,
    latestObservationAt: 1_756_800_000,
    activeSeats: [],
    activeGrants: [],
    claimedSeats: [],
    teamEvents: [],
    policyEvents: [],
    observationEvents: [],
    refState: [],
    overlapFiles: [],
    overlapSha: null,
    overlapAsOf: null,
    overlapAuthor: null,
  };
  const readError = { scope: "missions:s-2", message: "relay unreachable" };
  return invokePulseMissionRows(
    {
      project: PROJECT,
      channelIds: [],
      openSessionCount: 3,
      sessions: [session],
      readErrors: [readError],
    },
    { invoke },
  ).then(() => {
    assert.deepEqual(Object.keys(sent.request).sort(), [
      "channelIds",
      "displayNames",
      "nowUnix",
      "openSessionCount",
      "project",
      "readErrors",
      "schema",
      "sessions",
      "viewerPubkey",
    ]);
    assert.equal(sent.request.schema, "buzz-pulse-mission-rows-request/v1");
    // What the caller gathered reaches the command verbatim: this module
    // composes nothing, and a session it reshaped is one the Rust side could
    // no longer verify.
    assert.deepEqual(sent.request.sessions, [session]);
    assert.equal(sent.request.sessions[0], session);
    assert.deepEqual(sent.request.readErrors, [readError]);
    // Three open, one read — the difference is the command's own disclosure.
    assert.equal(sent.request.openSessionCount, 3);
    assert.equal(Number.isInteger(sent.request.nowUnix), true);
  });
});
