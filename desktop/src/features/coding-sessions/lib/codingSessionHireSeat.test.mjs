/**
 * From a signed hire to the seated create the host publishes.
 *
 * The create is the hire's whole answer — its receipts are the hire's receipts
 * — so what it carries is pinned here: the identity as `actor`, the hired role,
 * the brief as the seat's first turn, and the umbrella's own title and
 * genesis. A seat that arrived with no brief would be an agent asked to guess.
 */
import assert from "node:assert/strict";
import test from "node:test";

import {
  CODING_SESSION_HIRE_BRIEF_PREFIX,
  buildCodingSessionHireSeatPlan,
  codingSessionHireSeatOrdinal,
  codingSessionHireWorktreeName,
  isCodingSessionHireAuthorized,
  listCodingSessionHireLiveSeats,
  selectUnansweredCodingSessionHires,
} from "./codingSessionHireSeat.ts";
import { buildCodingSessionCreateEvent } from "./codingSessionLifecycleCommand.ts";

const SESSION_REF = "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10";
const GENESIS_REF = "a".repeat(64);
const CHANNEL_ID = "3d2a7b18-9b7a-4a41-9a86-6a52a1c0b7e1";
const FOUNDER = "f".repeat(64);
const LEAD = "1".repeat(64);
const ADA = "d".repeat(64);
const PROVIDER = "9".repeat(64);

function plan(overrides = {}) {
  return buildCodingSessionHireSeatPlan({
    commandId: "csl-seat-1",
    channelId: CHANNEL_ID,
    sessionRef: SESSION_REF,
    genesisRef: GENESIS_REF,
    projectRef: null,
    title: "Agent Teams",
    brief: "Take the badge lane. Red test first.",
    role: "builder",
    identity: { pubkey: ADA, name: "Ada", homeRole: "builder" },
    providerInstanceRef: "claude-primary",
    providerAuthorityPubkey: PROVIDER,
    model: "sonnet",
    seatOrdinal: 2,
    ...overrides,
  });
}

test("the create built from a hire carries actor, role, initialTurn and title", () => {
  const seat = plan();
  assert.equal(seat.actor, ADA);
  assert.equal(seat.role, "builder");
  assert.equal(
    seat.initialTurn,
    "[From the lead] Take the badge lane. Red test first.",
  );
  assert.equal(seat.title, "Agent Teams");
  assert.equal(seat.sessionRef, SESSION_REF);
  assert.equal(seat.genesisRef, GENESIS_REF);
  assert.equal(seat.providerInstanceRef, "claude-primary");
  assert.equal(seat.model, "sonnet");
  assert.equal(seat.seatLabel, "Ada");
});

test("the prefix is the contract's, and it is not doubled", () => {
  assert.equal(CODING_SESSION_HIRE_BRIEF_PREFIX, "[From the lead] ");
  const seat = plan({ brief: "[From the lead] already prefixed" });
  assert.equal(seat.initialTurn, "[From the lead] already prefixed");
});

test("the plan is exactly what the 44221 create builder accepts", () => {
  const seat = plan();
  const built = buildCodingSessionCreateEvent({
    channelId: seat.channelId,
    commandId: seat.commandId,
    projectRef: seat.projectRef,
    repoRef: null,
    sessionRef: seat.sessionRef,
    genesisRef: seat.genesisRef,
    actor: seat.actor,
    role: seat.role,
    providerInstanceRef: seat.providerInstanceRef,
    providerAuthorityPubkey: seat.providerAuthorityPubkey,
    model: seat.model,
    title: seat.title,
    initialTurn: seat.initialTurn,
  });
  const action = JSON.parse(built.content).action;
  assert.equal(action.type, "session.create");
  assert.equal(action.actor, ADA);
  assert.equal(action.role, "builder");
  assert.equal(
    action.initialTurn,
    "[From the lead] Take the badge lane. Red test first.",
  );
  assert.equal(action.title, "Agent Teams");
});

test("each seat gets its own tree, named after the session, the role and its ordinal", () => {
  assert.equal(plan().worktreeName, "agent-teams-builder-2");
  assert.equal(
    codingSessionHireWorktreeName({
      title: "Agent Teams",
      role: "builder",
      ordinal: 1,
    }),
    "agent-teams-builder-1",
  );
  // Nothing addressable in the title still names the role and the ordinal.
  assert.equal(
    codingSessionHireWorktreeName({ title: "   ", role: "runner", ordinal: 3 }),
    "runner-3",
  );
});

test("the ordinal counts the seats of that role already in the umbrella", () => {
  assert.equal(codingSessionHireSeatOrdinal([], "builder"), 1);
  assert.equal(
    codingSessionHireSeatOrdinal(
      [
        { actor: ADA, role: "builder" },
        { actor: LEAD, role: "lead" },
      ],
      "builder",
    ),
    2,
  );
});

test("live seats are the seated executions that have not ended", () => {
  const umbrella = {
    executions: [
      {
        activeGeneration: { agentRef: ADA, role: "builder", status: "running" },
      },
      {
        activeGeneration: {
          agentRef: LEAD,
          role: "lead",
          status: "waiting_for_input",
        },
      },
      {
        activeGeneration: {
          agentRef: "e".repeat(64),
          role: "runner",
          status: "stopped",
        },
      },
      { activeGeneration: { agentRef: null, role: null, status: "running" } },
    ],
  };
  assert.deepEqual(listCodingSessionHireLiveSeats(umbrella), [
    { actor: ADA, role: "builder" },
    { actor: LEAD, role: "lead" },
  ]);
});

test("only the founder or a granted operator may hire", () => {
  const authority = { founderPubkey: FOUNDER, grantedOperators: [LEAD] };
  assert.equal(isCodingSessionHireAuthorized(FOUNDER, authority), true);
  assert.equal(isCodingSessionHireAuthorized(LEAD, authority), true);
  assert.equal(isCodingSessionHireAuthorized(ADA, authority), false);
  // An umbrella with no resolved founder authorises nobody: an unresolved
  // anchor must never read as an open door.
  assert.equal(
    isCodingSessionHireAuthorized(FOUNDER, {
      founderPubkey: null,
      grantedOperators: [],
    }),
    false,
  );
});

test("a hire is answered once, however many times it is observed", () => {
  const requests = [
    { commandId: "csl-1", eventId: "e1" },
    { commandId: "csl-2", eventId: "e2" },
    { commandId: "csl-1", eventId: "e1" },
  ];
  const answered = new Set(["csl-2"]);
  assert.deepEqual(
    selectUnansweredCodingSessionHires(requests, answered).map(
      (r) => r.commandId,
    ),
    ["csl-1"],
  );
});
