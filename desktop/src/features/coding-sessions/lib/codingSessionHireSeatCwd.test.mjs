import assert from "node:assert/strict";
import test from "node:test";

import { CodingSessionCreateRefusedError } from "./codingSessionCrewReceipt.ts";
import { grantSeat } from "./codingSessionHireGrant.ts";
import {
  codingSessionSeatCwdRefusalCode,
  stageHiredSeatWorkdirOrRefuse,
} from "./codingSessionHireSeatCwd.ts";

const PLAN = {
  channelId: "11111111-2222-3333-4444-555555555555",
  commandId: "csl-1d57e229",
  sessionRef: "beb51e9d-8359-4629-bcda-514bb9df5381",
  seatLabel: "Verifier",
  projectRef: "30621:aa:kettle",
  genesisRef: "g".repeat(64),
  providerAuthorityPubkey: "b".repeat(64),
  actor: "a".repeat(64),
};

const REQUEST = {
  channelId: PLAN.channelId,
  requesterPubkey: "c".repeat(64),
  action: { sessionRef: PLAN.sessionRef, role: "verifier" },
};

function deps(overrides = {}) {
  const calls = { disposed: [], staged: [], published: [], grants: [] };
  return {
    calls,
    deps: {
      stageSeatCreateHint: async (input) => {
        calls.staged.push(input);
        return "/repos/kettle-wt-coding-session-verifier-1";
      },
      disposeSeatWorktree: async (input) => {
        calls.disposed.push(input);
        return `removed the worktree for ${input.seatLabel}`;
      },
      signer: async (event) => ({ ...event, id: "e", sig: "s", pubkey: "p" }),
      publisher: {
        publishEvent: async (event) => {
          calls.published.push(event);
          return event;
        },
      },
      awaitSeatReceipt: async () => ({ sessionId: "x", generation: 1 }),
      ensureOperatorGrant: async (input) => {
        calls.grants.push(input);
      },
      newTurnCommandId: () => "csc-1",
      sleep: async () => {},
      monotonicNow: () => 0,
      ...overrides,
    },
  };
}

const INPUT = { agents: [], targetForActor: () => null };

test("a SEAT_CWD code is read off the host's own sentence", () => {
  assert.equal(
    codingSessionSeatCwdRefusalCode("SEAT_CWD_UNRECORDED: no record"),
    "SEAT_CWD_UNRECORDED",
  );
  assert.equal(codingSessionSeatCwdRefusalCode("disk full"), null);
});

test("a hire's hint is staged from the seat's record, by session and label", async () => {
  const { calls, deps: d } = deps();
  const refused = await stageHiredSeatWorkdirOrRefuse(
    REQUEST,
    PLAN,
    "/repos/kettle-wt-coding-session-verifier-1",
    INPUT,
    d,
  );
  assert.equal(refused, null);
  assert.deepEqual(calls.staged, [
    {
      commandId: PLAN.commandId,
      sessionRef: PLAN.sessionRef,
      seatLabel: PLAN.seatLabel,
      projectRef: PLAN.projectRef,
    },
  ]);
  assert.deepEqual(calls.disposed, []);
});

test("a hint the host refuses refuses the hire by its code and removes the cut tree", async () => {
  const { calls, deps: d } = deps({
    stageSeatCreateHint: async () => {
      throw new Error(
        "SEAT_CWD_UNRECORDED: this host has no worktree recorded",
      );
    },
  });
  const refused = await stageHiredSeatWorkdirOrRefuse(
    REQUEST,
    PLAN,
    "/repos/kettle-wt-coding-session-verifier-1",
    INPUT,
    d,
  );
  assert.equal(refused, "SEAT_CWD_UNRECORDED");
  assert.deepEqual(calls.disposed, [
    { sessionRef: PLAN.sessionRef, seatLabel: PLAN.seatLabel },
  ]);
  const said = calls.published.map((event) => event.content).join("\n");
  assert.match(said, /SEAT_CWD_UNRECORDED/);
});

test("a create the provider refused removes the tree cut for it", async () => {
  const { calls, deps: d } = deps({
    awaitSeatReceipt: async () => {
      throw new CodingSessionCreateRefusedError(
        "SEAT_CWD_SHARED",
        "refusing to seat an agent in /repos/kettle",
      );
    },
  });
  const failure = await grantSeat(PLAN, d);
  assert.match(failure, /^SEAT_CWD_SHARED: /);
  assert.match(failure, /removed the worktree for Verifier/);
  assert.deepEqual(calls.disposed, [
    { sessionRef: PLAN.sessionRef, seatLabel: PLAN.seatLabel },
  ]);
  assert.deepEqual(calls.grants, [], "no grant over a refused create");
});

test("a receipt that never came keeps the tree: the seat may exist", async () => {
  const { calls, deps: d } = deps({
    awaitSeatReceipt: async () => {
      throw new Error("The provider did not answer within the wait");
    },
  });
  const failure = await grantSeat(PLAN, d);
  assert.equal(failure, "The provider did not answer within the wait");
  assert.deepEqual(calls.disposed, []);
});
