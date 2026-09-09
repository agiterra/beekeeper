import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

import { invokePulseDeclaredWork } from "@/features/project-pulse/lib/invokePulseDeclaredWork";
import {
  PULSE_DECLARED_WORK_COMMAND,
  PULSE_DECLARED_WORK_REQUEST_SCHEMA,
} from "@/features/project-pulse/lib/pulseDeclaredWorkWire";

const here = dirname(fileURLToPath(import.meta.url));
const PROJECT =
  "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo";

function fixture() {
  return JSON.parse(
    readFileSync(resolve(here, "pulseDeclaredWork.fixture.json"), "utf8"),
  );
}

function channelIds(payload = fixture()) {
  return [...new Set(payload.sessions.map((session) => session.channelId))];
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

test("the request names the command and every key the native side declares", async () => {
  const calls = [];
  await invokePulseDeclaredWork(
    { project: PROJECT, channelIds: channelIds() },
    { invoke: stubInvoke(fixture(), calls) },
  );
  assert.equal(calls.length, 1);
  assert.equal(calls[0].command, PULSE_DECLARED_WORK_COMMAND);
  const request = calls[0].args.request;
  assert.deepEqual(Object.keys(request).sort(), [
    "channelIds",
    "nowUnix",
    "project",
    "readErrors",
    "schema",
    "sessions",
    "viewerPubkey",
  ]);
  assert.equal(request.schema, PULSE_DECLARED_WORK_REQUEST_SCHEMA);
  assert.equal(request.project, PROJECT);
  assert.deepEqual(request.channelIds, channelIds());
  assert.equal(request.viewerPubkey, null);
  assert.deepEqual(request.sessions, []);
  assert.deepEqual(request.readErrors, []);
  assert.equal(Number.isSafeInteger(request.nowUnix), true);
});

test("the caller's sessions and read errors are copied, not aliased", async () => {
  const calls = [];
  const sessions = [];
  const readErrors = [{ scope: "declared", message: "relay closed" }];
  await invokePulseDeclaredWork(
    {
      project: PROJECT,
      channelIds: channelIds(),
      sessions,
      readErrors,
      viewerPubkey: "33".repeat(32),
    },
    { invoke: stubInvoke(fixture(), calls) },
  );
  const request = calls[0].args.request;
  assert.notEqual(request.sessions, sessions);
  assert.notEqual(request.readErrors, readErrors);
  assert.deepEqual(request.readErrors, readErrors);
  assert.equal(request.viewerPubkey, "33".repeat(32));
});

test("a response is decoded, not passed through", async () => {
  const page = await invokePulseDeclaredWork(
    { project: PROJECT, channelIds: channelIds() },
    { invoke: stubInvoke(fixture()) },
  );
  assert.equal(Object.isFrozen(page), true);
  assert.equal(page.sessions.length, 2);
  assert.equal(page.sessions[0].assignments[0].status, "reported");
});

test("a malformed response fails loudly rather than returning no declared work", async () => {
  const broken = fixture();
  broken.sessions[0].assignments[0].status = "done";
  const message = await failure(
    invokePulseDeclaredWork(
      { project: PROJECT, channelIds: channelIds() },
      { invoke: stubInvoke(broken) },
    ),
  );
  // The distinction that matters: this is not "no work", it is "no answer".
  assert.notEqual(message, null);
  assert.match(String(message), /status/);
});

test("a transport failure propagates rather than becoming an empty page", async () => {
  const message = await failure(
    invokePulseDeclaredWork(
      { project: PROJECT, channelIds: channelIds() },
      {
        invoke: async () => {
          throw new Error("the bridge is not available");
        },
      },
    ),
  );
  assert.equal(message, "the bridge is not available");
});
