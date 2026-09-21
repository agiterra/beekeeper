import assert from "node:assert/strict";
import test from "node:test";

import { loadProjectWorkCoverage } from "./useProjectWork.ts";
import {
  assertProjectWorkRequestComplete,
  PROJECT_WORK_REQUIRED_KEYS,
} from "@/shared/api/tauriProjectWork.ts";

const SESSION = "11111111-2222-4333-8444-555555555555";
const CHANNEL = "05ef0ecf-745f-5fb8-b7ff-f9cba21e01c2";
const GENESIS = "ab".repeat(32);

function scope() {
  return {
    channelRef: CHANNEL,
    sessionRef: SESSION,
    genesisRef: GENESIS,
    projectRef: `30621:${"3d".repeat(32)}:kettle`,
    founderPubkey: "3d".repeat(32),
    activeSeats: [],
    activeGrants: [],
    repositoryIds: [],
  };
}

function signedEvent(kind) {
  return {
    id: "1e".repeat(32),
    pubkey: "2e".repeat(32),
    created_at: 10,
    kind,
    tags: [["h", CHANNEL]],
    content: "{}",
    sig: "3e".repeat(64),
    // Local-only render keys the signature never covered.
    localKey: "optimistic-1",
    pending: false,
  };
}

test("the invoke payload carries every key the native fold requires", async () => {
  let sent = null;
  await loadProjectWorkCoverage(scope(), {
    fetchEvents: async (filter) =>
      filter.kinds.includes(44244) ? [signedEvent(44244)] : [],
    relaySelf: async () => "9a".repeat(32),
    fold: async (request) => {
      sent = request;
      return {
        schema: "buzz-project-work-response/v1",
        implementation: "buzz-core",
        coverage: {
          schema: "buzz-project-work-coverage/v1",
          sessionRef: SESSION,
          projectRef: scope().projectRef,
          declarations: [],
          excluded: [],
          conflicts: [],
        },
        unreadablePlans: [],
        agentsRepoRead: false,
      };
    },
  });
  // Lane 213 added `channelRef` and `genesisRef` to the Rust struct; every
  // unit test stayed green while the view would have failed at runtime.
  for (const key of PROJECT_WORK_REQUIRED_KEYS) {
    assert.ok(sent[key] !== undefined && sent[key] !== null, `sent ${key}`);
  }
  assert.equal(sent.channelRef, CHANNEL);
  assert.equal(sent.genesisRef, GENESIS);
});

test("team transactions cross with their signatures and nothing else", async () => {
  let sent = null;
  await loadProjectWorkCoverage(scope(), {
    fetchEvents: async (filter) =>
      filter.kinds.includes(44244) ? [signedEvent(44244)] : [],
    relaySelf: async () => null,
    fold: async (request) => {
      sent = request;
      return {
        schema: "buzz-project-work-response/v1",
        implementation: "buzz-core",
        coverage: {
          schema: "buzz-project-work-coverage/v1",
          sessionRef: SESSION,
          projectRef: scope().projectRef,
          declarations: [],
          excluded: [],
          conflicts: [],
        },
        unreadablePlans: [],
        agentsRepoRead: false,
      };
    },
  });
  assert.equal(sent.teamEvents.length, 1);
  assert.deepEqual(Object.keys(sent.teamEvents[0]).sort(), [
    "content",
    "created_at",
    "id",
    "kind",
    "pubkey",
    "sig",
    "tags",
  ]);
});

test("an incomplete request is refused by name, before the invoke", () => {
  for (const key of PROJECT_WORK_REQUIRED_KEYS) {
    const request = Object.fromEntries(
      PROJECT_WORK_REQUIRED_KEYS.map((name) => [name, "value"]),
    );
    delete request[key];
    assert.throws(
      () => assertProjectWorkRequestComplete(request),
      new RegExp(`missing ${key}`),
    );
  }
  const complete = Object.fromEntries(
    PROJECT_WORK_REQUIRED_KEYS.map((name) => [name, "value"]),
  );
  assert.doesNotThrow(() => assertProjectWorkRequestComplete(complete));
});
