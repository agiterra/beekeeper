import assert from "node:assert/strict";
import test from "node:test";

import {
  publishSeatedCodingSessionCreate,
  publishSeatedCodingSessionResume,
} from "./codingSessionSeatedCreate.ts";

const SEAT = {
  actor: "aa11bb22cc33dd44ee55ff66aa77bb88cc99dd00ee11ff22aa33bb44cc55dd66",
  role: "builder",
};

function recorder(overrides = {}) {
  const calls = [];
  return {
    calls,
    deps: {
      ensureMembership: async (input) => {
        calls.push(["ensureMembership", input]);
        if (overrides.membershipError) throw overrides.membershipError;
      },
      stageSeat: async (input) => {
        calls.push(["stageSeat", input]);
        if (overrides.stageError) throw overrides.stageError;
      },
      clearSeat: async (commandId) => {
        calls.push(["clearSeat", commandId]);
      },
    },
  };
}

test("a create with no seat publishes exactly as it does today", async () => {
  const { calls, deps } = recorder();
  let published = 0;
  const result = await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-1",
    seat: null,
    deps,
    publish: async () => {
      published += 1;
      return "ok";
    },
  });
  assert.equal(result, "ok");
  assert.equal(published, 1);
  // No membership write, no custody file, nothing.
  assert.deepEqual(calls, []);
});

test("the create is not published when the membership add fails", async () => {
  const { calls, deps } = recorder({
    membershipError: new Error("Could not add Ada to #crew: not permitted"),
  });
  let published = 0;
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-2",
      seat: SEAT,
      seatLabel: "Ada",
      deps,
      publish: async () => {
        published += 1;
        return "ok";
      },
    }),
    // The reason is named, not swallowed into a generic failure.
    /Could not add Ada to #crew/,
  );
  assert.equal(published, 0, "nothing may be published for a mute seat");
  // And no key material was written for a create that never went out.
  assert.deepEqual(
    calls.map(([name]) => name),
    ["ensureMembership"],
  );
});

test("the create is not published when custody staging fails", async () => {
  const { calls, deps } = recorder({
    stageError: new Error("the OS keyring may be unreachable"),
  });
  let published = 0;
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-3",
      seat: SEAT,
      deps,
      publish: async () => {
        published += 1;
        return "ok";
      },
    }),
    /keyring/,
  );
  assert.equal(published, 0);
  assert.deepEqual(
    calls.map(([name]) => name),
    ["ensureMembership", "stageSeat"],
  );
});

test("custody is staged before the publish, keyed by the exact commandId", async () => {
  const { calls, deps } = recorder();
  await publishSeatedCodingSessionCreate({
    channelId: "channel-1",
    commandId: "csl-4",
    seat: SEAT,
    seatLabel: "Ada",
    deps,
    publish: async () => "ok",
  });
  assert.deepEqual(calls, [
    [
      "ensureMembership",
      { channelId: "channel-1", actorPubkey: SEAT.actor, actorLabel: "Ada" },
    ],
    ["stageSeat", { commandId: "csl-4", agentPubkey: SEAT.actor }],
  ]);
});

test("a failed publish takes the staged seat down with it", async () => {
  const { calls, deps } = recorder();
  await assert.rejects(
    publishSeatedCodingSessionCreate({
      channelId: "channel-1",
      commandId: "csl-5",
      seat: SEAT,
      deps,
      publish: async () => {
        throw new Error("relay refused");
      },
    }),
    /relay refused/,
  );
  assert.deepEqual(calls.at(-1), ["clearSeat", "csl-5"]);
});

test("a resume stages the seat again under the resume's own commandId", async () => {
  // The create's entry was consumed when the first adapter spawned, so a
  // reconnect that stages nothing is refused `ACTOR_UNAVAILABLE` forever.
  const { calls, deps } = recorder();
  let published = 0;
  const result = await publishSeatedCodingSessionResume({
    commandId: "csl-resume-1",
    actorPubkey: SEAT.actor,
    deps,
    publish: async () => {
      published += 1;
      return "resumed";
    },
  });
  assert.equal(result, "resumed");
  assert.equal(published, 1);
  assert.deepEqual(calls, [
    ["stageSeat", { commandId: "csl-resume-1", agentPubkey: SEAT.actor }],
  ]);
});

test("a resume of an unseated execution touches no custody at all", async () => {
  const { calls, deps } = recorder();
  const result = await publishSeatedCodingSessionResume({
    commandId: "csl-resume-2",
    actorPubkey: null,
    deps,
    publish: async () => "resumed",
  });
  assert.equal(result, "resumed");
  assert.deepEqual(calls, []);
});

test("a resume that never went out takes its staged key back", async () => {
  const { calls, deps } = recorder();
  await assert.rejects(
    publishSeatedCodingSessionResume({
      commandId: "csl-resume-3",
      actorPubkey: SEAT.actor,
      deps,
      publish: async () => {
        throw new Error("relay rejected the resume");
      },
    }),
    /relay rejected the resume/,
  );
  assert.deepEqual(calls, [
    ["stageSeat", { commandId: "csl-resume-3", agentPubkey: SEAT.actor }],
    ["clearSeat", "csl-resume-3"],
  ]);
});
