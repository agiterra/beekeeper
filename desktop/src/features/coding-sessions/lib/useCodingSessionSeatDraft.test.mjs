/**
 * The seat draft is shared by founding a session and joining one, so the
 * wiring — not just the pure default — is pinned here: choosing an agent
 * fills the role, typing pins it, and clearing the agent forgets both.
 */
import assert from "node:assert/strict";
import { after, before, test } from "node:test";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    Element: dom.window.Element,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
});

after(() => dom.window.close());

const ADA = "aa11bb22".repeat(8);
const BEN = "bb22cc33".repeat(8);
const AGENTS = [
  { pubkey: ADA, name: "Ada", homeRole: "builder" },
  { pubkey: BEN, name: "Ben" },
];

async function mountDraft() {
  const React = (await import("react")).default;
  const { act, cleanup, render } = await import("@testing-library/react");
  const { useCodingSessionSeatDraft } = await import(
    "./useCodingSessionSeatDraft.ts"
  );
  let latest = null;
  function Probe() {
    latest = useCodingSessionSeatDraft(AGENTS);
    return null;
  }
  await act(async () => {
    render(React.createElement(Probe));
  });
  return {
    cleanup,
    act,
    read: () => latest,
  };
}

test("choosing an agent fills the role from its home role, and clearing forgets it", async () => {
  const { act, cleanup, read } = await mountDraft();
  try {
    assert.equal(read().role, "");
    assert.equal(read().seat, null);
    assert.equal(read().error, null);

    await act(async () => read().onActorChange(ADA));
    assert.equal(read().role, "builder");
    assert.deepEqual(read().seat, { actor: ADA, role: "builder" });
    assert.equal(read().label, "Ada");

    // An agent whose home role this build cannot see fills nothing — and a
    // seat with no role is refused before anything is signed.
    await act(async () => read().onActorChange(BEN));
    assert.equal(read().role, "");
    assert.equal(read().seat, null);
    assert.match(read().error, /Give this seat a role/);

    await act(async () => read().onActorChange(null));
    assert.equal(read().role, "");
    assert.equal(read().error, null);
  } finally {
    cleanup();
  }
});

test("an agent's primary role fixes the seat role; a typed role applies only to an agent without one", async () => {
  const { act, cleanup, read } = await mountDraft();
  try {
    await act(async () => read().onActorChange(ADA));
    await act(async () => read().onRoleChange("verifier"));
    // A builder is never relabelled by typing: a new seat takes its primary
    // role, and the host would refuse any other (SEAT_ROLE_NOT_PRIMARY).
    assert.equal(read().role, "builder");
    assert.equal(read().roleLocked, true);
    assert.deepEqual(read().seat, { actor: ADA, role: "builder" });

    // An agent with no primary role keeps the role the person typed.
    await act(async () => read().onActorChange(BEN));
    assert.equal(read().roleLocked, false);
    assert.equal(read().role, "verifier");
    assert.deepEqual(read().seat, { actor: BEN, role: "verifier" });

    await act(async () => read().onActorChange(null));
    await act(async () => read().onActorChange(ADA));
    assert.equal(read().role, "builder");
  } finally {
    cleanup();
  }
});

const TANK_LOOP = `30621:${"e".repeat(64)}:tank-loop`;
const BEEKEEPER = `30621:${"e".repeat(64)}:beekeeper`;
const TANK_BUILDER = "cc33dd44".repeat(8);
const BOB = "dd44ee55".repeat(8);
const STRAY = "ee55ff66".repeat(8);
const SCOPED_AGENTS = [
  {
    pubkey: TANK_BUILDER,
    name: "Builder",
    homeRole: "builder",
    projectRef: TANK_LOOP,
  },
  { pubkey: BOB, name: "Bob", homeRole: "builder", projectRef: BEEKEEPER },
  { pubkey: STRAY, name: "Stray", homeRole: "builder", projectRef: null },
];

function execution(projectRef, statusEventId = "1".repeat(64)) {
  return { activeGeneration: { projectRef, statusEventId } };
}

test("the seat scope is the project the join signs: an execution's, else none once one has reported, else unknown", async () => {
  const { resolveCodingSessionSeatProjectScope } = await import(
    "./useCodingSessionSeatDraft.ts"
  );
  assert.deepEqual(
    resolveCodingSessionSeatProjectScope({
      executions: [execution(null), execution(TANK_LOOP)],
    }),
    { kind: "project", projectRef: TANK_LOOP },
  );
  assert.deepEqual(
    resolveCodingSessionSeatProjectScope({ executions: [execution(null)] }),
    { kind: "none" },
  );
  // No execution has reported its metadata: a null project is not an answer.
  assert.deepEqual(
    resolveCodingSessionSeatProjectScope({
      executions: [execution(null, null)],
    }),
    { kind: "unknown" },
  );
  assert.deepEqual(resolveCodingSessionSeatProjectScope({ executions: [] }), {
    kind: "unknown",
  });
});

test("two projects' builders: each project's seat picker offers only its own", async () => {
  const { partitionCodingSessionSeatAgents, codingSessionSeatScopeSentence } =
    await import("./useCodingSessionSeatDraft.ts");
  const tank = partitionCodingSessionSeatAgents({
    agents: SCOPED_AGENTS,
    scope: { kind: "project", projectRef: TANK_LOOP },
  });
  assert.deepEqual(
    tank.eligible.map((agent) => agent.name),
    ["Builder"],
  );
  assert.equal(tank.excludedCount, 2);
  assert.deepEqual(
    partitionCodingSessionSeatAgents({
      agents: SCOPED_AGENTS,
      scope: { kind: "project", projectRef: BEEKEEPER },
    }).eligible.map((agent) => agent.name),
    ["Bob"],
  );
  // A projectless session: only agents in no project.
  const none = partitionCodingSessionSeatAgents({
    agents: SCOPED_AGENTS,
    scope: { kind: "none" },
  });
  assert.deepEqual(
    none.eligible.map((agent) => agent.name),
    ["Stray"],
  );
  assert.equal(
    codingSessionSeatScopeSentence({
      scope: { kind: "none" },
      excludedCount: none.excludedCount,
    }),
    "2 agents on this computer belong to a project, so they can't be seated in this session, which belongs to none.",
  );
  // Unknown: the join signs no project, so only agents in no project —
  // never every agent — disclosed as unknown.
  const unknown = partitionCodingSessionSeatAgents({
    agents: SCOPED_AGENTS,
    scope: { kind: "unknown" },
  });
  assert.deepEqual(
    unknown.eligible.map((agent) => agent.name),
    ["Stray"],
  );
  assert.match(
    codingSessionSeatScopeSentence({
      scope: { kind: "unknown" },
      excludedCount: 0,
    }),
    /project is not known yet/,
  );
  // Renaming changes no eligibility.
  assert.deepEqual(
    partitionCodingSessionSeatAgents({
      agents: SCOPED_AGENTS.map((agent) => ({ ...agent, name: "Renamed" })),
      scope: { kind: "project", projectRef: TANK_LOOP },
    }).eligible.map((agent) => agent.pubkey),
    [TANK_BUILDER],
  );
});

test("a pick that stops being eligible when the scope resolves reads as no seat", async () => {
  const React = (await import("react")).default;
  const { act, cleanup, render } = await import("@testing-library/react");
  const { useCodingSessionSeatDraft } = await import(
    "./useCodingSessionSeatDraft.ts"
  );
  let latest = null;
  function Probe({ scope }) {
    latest = useCodingSessionSeatDraft(SCOPED_AGENTS, scope);
    return null;
  }
  const beekeeper = { kind: "project", projectRef: BEEKEEPER };
  const tank = { kind: "project", projectRef: TANK_LOOP };
  let view = null;
  await act(async () => {
    view = render(React.createElement(Probe, { scope: beekeeper }));
  });
  try {
    await act(async () => latest.onActorChange(BOB));
    assert.deepEqual(latest.seat, { actor: BOB, role: "builder" });
    await act(async () => {
      view.rerender(React.createElement(Probe, { scope: tank }));
    });
    assert.equal(latest.actor, null);
    assert.equal(latest.seat, null);
    assert.equal(latest.error, null);
    assert.deepEqual(
      latest.agents.map((agent) => agent.pubkey),
      [TANK_BUILDER],
    );
    await act(async () => latest.onActorChange(TANK_BUILDER));
    assert.deepEqual(latest.seat, { actor: TANK_BUILDER, role: "builder" });
  } finally {
    cleanup();
  }
});
