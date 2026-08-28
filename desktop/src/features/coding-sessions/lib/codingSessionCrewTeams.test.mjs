import assert from "node:assert/strict";
import test from "node:test";

import {
  readCodingSessionCrewTeams,
  resolveCodingSessionCrewSeats,
} from "./codingSessionCrewTeams.ts";

test("only teams carrying a crew block are crews", () => {
  const teams = readCodingSessionCrewTeams([
    { id: "t1", name: "Plain", crew: null },
    {
      id: "t2",
      name: "Crew",
      crew: { primary: "p1", seats: [{ personaId: "p1", role: "lead" }] },
    },
    { id: "t3", name: "Broken", crew: { primary: "p9", seats: [] } },
  ]);
  assert.deepEqual(
    teams.map((team) => team.id),
    ["t2"],
  );
});

const CREW = {
  primary: "p-lead",
  seats: [
    { personaId: "p-lead", role: "lead" },
    {
      personaId: "p-build",
      role: "builder",
      model: "gpt-5.6-sol",
      vendor: "openai",
    },
  ],
};

test("a seat binds to the agent filling its persona, and the seat's model wins", () => {
  const resolved = resolveCodingSessionCrewSeats({
    crew: CREW,
    agents: [
      {
        pubkey: "A".repeat(64),
        name: "Fable",
        personaId: "p-lead",
        model: "claude-opus-5",
      },
      {
        pubkey: "b".repeat(64),
        name: "Codey",
        personaId: "p-build",
        model: "gpt-4",
      },
    ],
  });
  assert.equal(resolved.error, null);
  assert.deepEqual(resolved.seats, [
    {
      personaId: "p-lead",
      role: "lead",
      actor: "a".repeat(64),
      actorLabel: "Fable",
      model: "claude-opus-5",
      vendor: null,
    },
    {
      personaId: "p-build",
      role: "builder",
      actor: "b".repeat(64),
      actorLabel: "Codey",
      model: "gpt-5.6-sol",
      vendor: "openai",
    },
  ]);
});

test("a seat nobody fills stops the resolution by name", () => {
  const resolved = resolveCodingSessionCrewSeats({
    crew: CREW,
    agents: [
      {
        pubkey: "a".repeat(64),
        name: "Fable",
        personaId: "p-lead",
        model: null,
      },
    ],
  });
  assert.equal(resolved.seats, null);
  assert.match(resolved.error, /builder seat/);
  assert.match(resolved.error, /p-build/);
});

test("two seats on one persona take two agents, never the same one twice", () => {
  const resolved = resolveCodingSessionCrewSeats({
    crew: {
      primary: "p-build",
      seats: [
        { personaId: "p-build", role: "builder" },
        { personaId: "p-build", role: "verifier" },
      ],
    },
    agents: [
      {
        pubkey: "a".repeat(64),
        name: "One",
        personaId: "p-build",
        model: null,
      },
      {
        pubkey: "b".repeat(64),
        name: "Two",
        personaId: "p-build",
        model: null,
      },
    ],
  });
  assert.equal(resolved.error, null);
  assert.deepEqual(
    resolved.seats.map((seat) => seat.actor),
    ["a".repeat(64), "b".repeat(64)],
  );
});

test("a seat carries the model the launch will publish, fallback included", () => {
  // The launch publishes the resolved seat's model verbatim, so this is the
  // only place a fallback may be applied: a check that reads one model while
  // the create carries another is a check that passed the wrong crew.
  const resolved = resolveCodingSessionCrewSeats({
    crew: {
      primary: "p-lead",
      seats: [
        { personaId: "p-lead", role: "lead" },
        { personaId: "p-build", role: "builder", model: "gpt-5.6-sol" },
      ],
    },
    agents: [
      {
        pubkey: "a".repeat(64),
        name: "Fable",
        personaId: "p-lead",
        model: null,
      },
      {
        pubkey: "b".repeat(64),
        name: "Codey",
        personaId: "p-build",
        model: "gpt-4",
      },
    ],
    fallbackModel: "claude-opus-5",
  });
  assert.equal(resolved.error, null);
  assert.deepEqual(
    resolved.seats.map((seat) => seat.model),
    ["claude-opus-5", "gpt-5.6-sol"],
  );
});

test("with no model anywhere a seat carries none, rather than inventing one", () => {
  const resolved = resolveCodingSessionCrewSeats({
    crew: { primary: "p-lead", seats: [{ personaId: "p-lead", role: "lead" }] },
    agents: [
      {
        pubkey: "a".repeat(64),
        name: "Fable",
        personaId: "p-lead",
        model: null,
      },
    ],
    fallbackModel: null,
  });
  assert.equal(resolved.seats[0].model, null);
});

// The roster has to disclose a packless seat *before* the launch, so the seat
// resolution has to carry the agent's answer through rather than dropping it
// (SESSION_STATE item 76, poke finding F5).
test("a resolved seat carries whether this computer holds the agent's role pack", () => {
  const resolution = resolveCodingSessionCrewSeats({
    crew: {
      primary: "p-lead",
      seats: [
        { personaId: "p-lead", role: "lead" },
        { personaId: "p-verify", role: "verifier" },
        { personaId: "p-quiet", role: "runner" },
      ],
    },
    agents: [
      {
        pubkey: "a".repeat(64),
        name: "Fable",
        personaId: "p-lead",
        model: "opus",
        hasRolePack: true,
      },
      {
        pubkey: "b".repeat(64),
        name: "Quinn",
        personaId: "p-verify",
        model: "qwen",
        hasRolePack: false,
      },
      // Never asked: absence must survive the mapping as absence.
      {
        pubkey: "c".repeat(64),
        name: "Ada",
        personaId: "p-quiet",
        model: null,
      },
    ],
  });
  assert.equal(resolution.error, null);
  assert.deepEqual(
    resolution.seats.map((seat) => seat.hasRolePack),
    [true, false, undefined],
  );
});

test("a seat's runtime travels with it, so the family rule can read it", () => {
  // The vendor of a seat on a provider-locked runtime is decided by that
  // runtime, not by its model alias — but only if the driver survives the
  // resolution the family check actually reads (SESSION_STATE item 77, F7).
  const resolved = resolveCodingSessionCrewSeats({
    crew: {
      primary: "persona-lead",
      seats: [
        { personaId: "persona-lead", role: "lead", driver: "claude-agent-acp" },
        { personaId: "persona-runner", role: "runner" },
      ],
    },
    agents: [
      {
        pubkey: "AA",
        name: "Keystone",
        personaId: "persona-lead",
        model: null,
      },
      {
        pubkey: "BB",
        name: "Runner",
        personaId: "persona-runner",
        model: null,
      },
    ],
    fallbackModel: "sonnet",
  });
  assert.equal(resolved.error, null);
  assert.equal(resolved.seats[0].driver, "claude-agent-acp");
  // A seat that pins no runtime claims none: absence is not a claim.
  assert.equal("driver" in resolved.seats[1], false);
});
