import assert from "node:assert/strict";
import { test } from "node:test";

import { buildProjectAgents } from "@/features/project-agents/lib/projectAgentsModel";
import {
  acceptPublishedProjectAgents,
  readsPublishedProjectAgents,
} from "@/features/project-agents/lib/publishedProjectAgents";
import { projectAgentDigest } from "@/shared/lib/projectAgentAssociation";

// Authority verification, private projects, carried digests and hiring
// readiness (docs/history/2026-09-14-project-hiring-review.md).

const OWNER = "6".repeat(64);
const PROJECT = `30621:${OWNER}:tank-loop`;
const OTHER_PROJECT = `30621:${OWNER}:attic`;
const COLLAB = "c".repeat(64);
const LOOM = "1".repeat(64);
const BOB = "2".repeat(64);
const IRA = "3".repeat(64);
const BUILDER = "4".repeat(64);
const REMOTE = "7".repeat(64);
const CREATOR_REMOTE = "8".repeat(64);
const SESSION_REF = "d".repeat(64);
const CLOSED_REF = "f".repeat(64);
const NOW = 1_800_000_000;

function execution(overrides = {}) {
  const { sessionId = "lead", ...session } = overrides;
  return {
    channelId: "chan-1",
    session: {
      generationId: `gen-${sessionId}`,
      label: "Loom execution",
      agentRef: LOOM,
      role: "lead",
      projectRef: PROJECT,
      sessionRef: SESSION_REF,
      provider: "claude-primary",
      runtime: "claude",
      model: "opus",
      status: "idle",
      statusAt: (NOW - 600) * 1_000,
      packRef: null,
      providerAuthorityPubkey: null,
      commandTarget: {
        driver: "acp",
        instanceId: "inst",
        sessionId,
        generation: 1,
      },
      ...session,
    },
  };
}

const UMBRELLAS = [
  {
    channelId: "chan-1",
    generationId: "gen-lead",
    label: "Project team setup",
    sessionRef: SESSION_REF,
    isClosed: false,
  },
  {
    channelId: "chan-1",
    generationId: "gen-old",
    label: "Old audit",
    sessionRef: CLOSED_REF,
    isClosed: true,
  },
];

function local(pubkey, name, homeRole, projectRef = null, extra = {}) {
  return { pubkey, name, avatarUrl: null, homeRole, projectRef, ...extra };
}

function published(pubkey, ownerPubkey, authority, overrides = {}) {
  return {
    pubkey,
    ownerPubkey,
    name: "Remote Runner",
    homeRole: "runner",
    projectDigest: projectAgentDigest(PROJECT),
    createdAt: NOW - 100,
    authority,
    ...overrides,
  };
}

function build(overrides = {}) {
  return buildProjectAgents({
    projectRef: PROJECT,
    executions: [],
    umbrellas: UMBRELLAS,
    declaredSessions: [],
    installations: [],
    localAgents: [local(LOOM, "Loom", "lead", PROJECT)],
    publishedAgents: [],
    otherNames: new Map([[COLLAB, "Casey"]]),
    nowSeconds: NOW,
    ...overrides,
  });
}

function wire(author, agent, createdAt) {
  return {
    id: `${author.slice(0, 2)}${createdAt}`.padEnd(64, "0"),
    kind: 30177,
    pubkey: author,
    created_at: createdAt,
    tags: [["d", agent]],
    content: JSON.stringify({
      name: "Runner",
      home_role: "runner",
      project_digest: projectAgentDigest(PROJECT),
    }),
  };
}

// ── 1. Verification ────────────────────────────────────────────────────────

test("after a failed roster read, only the creator's claims are verified", () => {
  const events = [wire(COLLAB, REMOTE, 100), wire(OWNER, CREATOR_REMOTE, 100)];
  const authorizedAuthors = [OWNER, COLLAB];
  const fallback = acceptPublishedProjectAgents({
    events,
    projectRef: PROJECT,
    authorizedAuthors,
    rosterVerified: false,
  });
  assert.deepEqual(
    fallback.map((entry) => [entry.pubkey, entry.authority]),
    [
      [REMOTE, "unverified"],
      [CREATOR_REMOTE, "verified"],
    ],
  );
  const read = acceptPublishedProjectAgents({
    events,
    projectRef: PROJECT,
    authorizedAuthors,
    rosterVerified: true,
  });
  assert.deepEqual(
    read.map((entry) => entry.authority),
    ["verified", "verified"],
  );
});

test("an unverified claim is listed apart and never counted as a project agent", () => {
  const model = build({
    publishedAgents: [
      published(REMOTE, COLLAB, "unverified"),
      published(CREATOR_REMOTE, OWNER, "verified", { name: "Creator Runner" }),
      // A claim that does not say how it was authorized is not verified.
      published("9".repeat(64), COLLAB, undefined, { name: "Unlabelled" }),
    ],
  });
  assert.deepEqual(
    model.projectAgents.map((row) => row.name),
    ["Creator Runner", "Loom"],
  );
  assert.deepEqual(
    model.unverified.map((row) => [row.name, row.claimAuthority]),
    [
      ["Remote Runner", "unverified"],
      ["Unlabelled", "unverified"],
    ],
  );
  const remote = model.unverified[0];
  assert.equal(remote.section, "unverified");
  assert.equal(remote.isProjectAgent, false);
  assert.equal(remote.state, "elsewhere");
  assert.equal(remote.mayAssociate, false);
  assert.deepEqual(remote.location, {
    kind: "elsewhere",
    ownerPubkey: COLLAB,
    ownerName: "Casey",
  });
  assert.equal(model.projectAgents[0].claimAuthority, "verified");
  assert.equal(model.projectAgents[1].claimAuthority, null, "a local record");
});

// ── 2. Private projects ────────────────────────────────────────────────────

test("a private project reads no publications and lists only this computer's agents", () => {
  assert.equal(readsPublishedProjectAgents({ visibility: "private" }), false);
  assert.equal(readsPublishedProjectAgents({ visibility: "public" }), true);
  assert.equal(readsPublishedProjectAgents(null), false);

  const model = build({
    projectPrivate: true,
    publishedAgents: [published(CREATOR_REMOTE, OWNER, "verified")],
  });
  assert.deepEqual(
    model.projectAgents.map((row) => row.name),
    ["Loom"],
  );
  assert.equal(model.unverified.length, 0);
});

// ── 3. Carried digests ─────────────────────────────────────────────────────

test("a local agent carrying this project's digest is offered for association, not counted", () => {
  const model = build({
    localAgents: [
      local(LOOM, "Loom", "lead", PROJECT),
      local(BOB, "Bob", "builder", null, {
        carriedProjectDigest: projectAgentDigest(PROJECT),
      }),
      // Carrying another project's digest says nothing about this one.
      local(IRA, "Ira", "verifier", null, {
        carriedProjectDigest: projectAgentDigest(OTHER_PROJECT),
      }),
    ],
  });
  assert.deepEqual(
    model.projectAgents.map((row) => row.name),
    ["Loom"],
  );
  assert.deepEqual(
    model.available.map((row) => row.name),
    ["Bob"],
  );
  const bob = model.available[0];
  assert.equal(bob.section, "available");
  assert.equal(bob.isProjectAgent, false);
  assert.equal(bob.carriedFromAnotherComputer, true);
  assert.equal(bob.state, "carried");
  assert.equal(bob.mayAssociate, true);
  assert.deepEqual(bob.location, { kind: "here" });
  assert.equal(
    [...model.borrowed, ...model.previous].some((row) => row.pubkey === IRA),
    false,
  );
});

test("a carried agent with work here stays in its evidence section, and a live seat outranks the carried word", () => {
  const carriedBob = local(BOB, "Bob", "builder", null, {
    carriedProjectDigest: projectAgentDigest(PROJECT).toUpperCase(),
  });
  const localAgents = [local(LOOM, "Loom", "lead", PROJECT), carriedBob];
  const stopped = build({
    localAgents,
    executions: [
      execution({
        sessionId: "bob",
        agentRef: BOB,
        role: "builder",
        status: "stopped",
      }),
    ],
  });
  assert.equal(stopped.borrowed[0].state, "carried");
  assert.equal(stopped.borrowed[0].carriedFromAnotherComputer, true);
  assert.equal(stopped.available.length, 0);

  const running = build({
    localAgents,
    executions: [
      execution({
        sessionId: "bob",
        agentRef: BOB,
        role: "builder",
        status: "running",
      }),
    ],
  });
  assert.equal(running.borrowed[0].state, "working");
});

// ── 4. Hiring readiness ────────────────────────────────────────────────────

const BOB_BUILDER_SEAT = execution({
  sessionId: "bob",
  agentRef: BOB,
  role: "builder",
  status: "running",
});

test("a borrowed builder with no associated builder: readiness names builder and Bob", () => {
  const model = build({
    localAgents: [
      local(LOOM, "Loom", "lead", PROJECT),
      local(BOB, "Bob", "builder"),
    ],
    executions: [execution(), BOB_BUILDER_SEAT],
  });
  assert.deepEqual(model.readiness, [
    { role: "builder", workers: [{ pubkey: BOB, name: "Bob" }] },
  ]);
  // Nothing is chosen for the reader: Bob stays borrowed, unassociated.
  assert.equal(model.borrowed[0].isProjectAgent, false);
});

test("an associated builder here clears the builder line; a role with no evidence has none", () => {
  const model = build({
    localAgents: [
      local(LOOM, "Loom", "lead", PROJECT),
      local(BUILDER, "Builder", "builder", PROJECT),
      local(BOB, "Bob", "builder"),
    ],
    executions: [execution(), BOB_BUILDER_SEAT],
  });
  assert.deepEqual(model.readiness, []);

  const verifierOnly = build({
    localAgents: [local(LOOM, "Loom", "lead", PROJECT)],
    executions: [
      execution(),
      execution({
        sessionId: "ira",
        agentRef: IRA,
        role: "verifier",
        sessionRef: CLOSED_REF,
        status: "completed",
      }),
    ],
  });
  // Closed work counts as evidence; lead has an associated agent; no builder work.
  assert.deepEqual(
    verifierOnly.readiness.map((line) => line.role),
    ["verifier"],
  );
  assert.deepEqual(
    verifierOnly.readiness[0].workers.map((worker) => worker.pubkey),
    [IRA],
  );
});

test("readiness is not claimed from agents on other computers, unread records, or the setup bootstrap", () => {
  // An associated agent on another computer is not hireable here, and is not
  // named as "not associated".
  const elsewhere = build({
    localAgents: [local(LOOM, "Loom", "lead", PROJECT)],
    publishedAgents: [
      published(REMOTE, OWNER, "verified", { homeRole: "builder" }),
    ],
    executions: [
      execution(),
      execution({ sessionId: "remote", agentRef: REMOTE, role: "builder" }),
    ],
  });
  assert.deepEqual(elsewhere.readiness, [{ role: "builder", workers: [] }]);

  // An associated agent of another primary role is not a builder, nor "not associated".
  const otherRole = build({
    localAgents: [local(LOOM, "Loom", "lead", PROJECT)],
    executions: [execution({ role: "builder" })],
  });
  assert.deepEqual(otherRole.readiness, [{ role: "builder", workers: [] }]);

  const unread = build({
    localAgents: [],
    localAgentsRead: false,
    executions: [BOB_BUILDER_SEAT],
  });
  assert.deepEqual(unread.readiness, []);

  const setup = build({
    localAgents: [local(LOOM, "Loom", "lead", PROJECT)],
    executions: [
      execution({
        sessionId: "setup",
        agentRef: "0".repeat(64),
        role: "project-setup",
      }),
    ],
  });
  assert.deepEqual(setup.readiness, []);
});
