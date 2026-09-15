/**
 * The lead's first turn after a Team Start: who it may hire, as this computer
 * would seat them. A founded Team Start launches the lead alone, and before
 * the hire roster its first turn always said nobody else was on this computer
 * — even with the project's Builder and Verifier installed right here.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { launchCodingSessionCrew } from "./codingSessionCrewLaunch.ts";
import {
  codingSessionCrewHireRosterAgents,
  codingSessionCrewLeadFirstTurnText,
} from "./codingSessionCrewLaunchFirstTurn.ts";

const TANK_LOOP = `30621:${"d".repeat(64)}:tank-loop`;
const BEEKEEPER = `30621:${"d".repeat(64)}:beekeeper`;
const LEAD = {
  personaId: "p-lead",
  role: "lead",
  actor: "a".repeat(64),
  actorLabel: "Fable",
  model: "claude-opus-5",
  vendor: null,
};
const BUILDER = {
  pubkey: `2b${"1".repeat(62)}`,
  name: "Builder",
  role: "builder",
};
const VERIFIER = {
  pubkey: `3c${"2".repeat(62)}`,
  name: "Verifier",
  role: "verifier",
};
const GOAL = "Close ledger item 53.";

function turn(roster) {
  return codingSessionCrewLeadFirstTurnText({
    goal: GOAL,
    lead: LEAD,
    hireable: [],
    roster,
  });
}

test("a project Team Start lists the project's agents here as hireable by role", () => {
  const text = turn({
    projectRef: TANK_LOOP,
    projectName: "Tank Loop",
    // The lead is in the roster too; it is never offered to itself.
    agents: [
      VERIFIER,
      { pubkey: LEAD.actor, name: "Fable", role: "lead" },
      BUILDER,
    ],
  });
  assert.ok(text.startsWith(`${GOAL}\n\n`));
  assert.doesNotMatch(text, /Nobody else is/);
  assert.match(text, /\[Tank Loop agents on this computer\]/);
  assert.match(
    text,
    /- builder: Builder \(2b111111…1111\)\n- verifier: Verifier \(3c222222…2222\)/,
  );
  assert.doesNotMatch(text, /- lead: Fable/);
  assert.match(
    text,
    /List this project's agents any time with `bee projects agents`\. Hire by role with `bee sessions hire`; this computer seats only this project's agents\./,
  );
  assert.match(text, /bee sessions hire --channel <uuid>/);
});

test("a project with no other agent here says so, and still names the discovery command", () => {
  const text = turn({
    projectRef: TANK_LOOP,
    projectName: "Tank Loop",
    agents: [{ pubkey: LEAD.actor, name: "Fable", role: "lead" }],
  });
  assert.match(text, /No other Tank Loop agent is on this computer/);
  assert.match(text, /`bee projects agents`/);
});

test("a projectless Team Start lists agents in no project, and says why only those", () => {
  const text = turn({ projectRef: null, projectName: null, agents: [BUILDER] });
  assert.match(text, /\[Agents on this computer in no project\]/);
  assert.match(text, /- builder: Builder \(2b111111…1111\)/);
  assert.match(text, /seats only agents that belong to none/);
  assert.doesNotMatch(text, /bee projects agents/);

  assert.match(
    turn({ projectRef: null, projectName: null, agents: [] }),
    /there is nobody to hire/,
  );
});

test("without a roster, the launch seats are the offer, as before", () => {
  const text = codingSessionCrewLeadFirstTurnText({
    goal: GOAL,
    lead: LEAD,
    hireable: [],
  });
  assert.match(text, /Nobody else is — this team has no other roles/);
});

test("the hire roster is association, never a role name: another project's builder is not offered", () => {
  const agents = [
    { ...BUILDER, homeRole: "builder", projectRef: TANK_LOOP },
    {
      pubkey: `4d${"3".repeat(62)}`,
      name: "Bob",
      homeRole: "builder",
      projectRef: BEEKEEPER,
    },
    {
      pubkey: `5e${"4".repeat(62)}`,
      name: "Stray",
      homeRole: "builder",
      projectRef: null,
    },
    {
      pubkey: `6f${"5".repeat(62)}`,
      name: "Roleless",
      homeRole: null,
      projectRef: TANK_LOOP,
    },
  ];
  assert.deepEqual(
    codingSessionCrewHireRosterAgents({ agents, projectRef: TANK_LOOP }).map(
      (agent) => agent.name,
    ),
    ["Builder"],
  );
  assert.deepEqual(
    codingSessionCrewHireRosterAgents({ agents, projectRef: BEEKEEPER }).map(
      (agent) => agent.name,
    ),
    ["Bob"],
  );
  assert.deepEqual(
    codingSessionCrewHireRosterAgents({ agents, projectRef: null }).map(
      (agent) => agent.name,
    ),
    ["Stray"],
  );
});

test("a launch given a hire roster states it in the lead's first turn, and still seats only the lead", async () => {
  const log = [];
  let sent = null;
  const result = await launchCodingSessionCrew(
    {
      channelId: "chan-1",
      goal: GOAL,
      seats: [LEAD],
      primaryPersonaId: LEAD.personaId,
      provider: { label: "claude-agent-acp", allowedModels: ["claude-opus-5"] },
      projectRef: TANK_LOOP,
      existingUmbrella: { sessionRef: "session-1", genesisRef: "genesis-1" },
      hireRoster: {
        projectRef: TANK_LOOP,
        projectName: "Tank Loop",
        agents: [BUILDER],
      },
    },
    {
      newSessionRef: () => "unused",
      publishGenesis: async () => ({ eventId: "unused" }),
      publishSeatCreate: async ({ seat }) => {
        log.push(`publish:${seat.role}`);
        return { commandId: "cmd-0" };
      },
      awaitSeatReceipt: async () => ({
        driver: "claude-agent-acp",
        instanceId: "inst-1",
        sessionId: "sess-1",
        generation: 1,
      }),
      grantOperator: async () => {},
      grantLeadSeat: async () => {},
      sendFirstTurn: async ({ text }) => {
        sent = text;
      },
    },
  );
  assert.equal(result.ok, true);
  assert.deepEqual(log, ["publish:lead"]);
  assert.doesNotMatch(sent, /Nobody else is/);
  assert.match(sent, /- builder: Builder \(2b111111…1111\)/);
  assert.match(sent, /`bee projects agents`/);
});

test("the hire roster leaves out agents the host will refuse: setup actors and agents with no role pack here", () => {
  const roster = codingSessionCrewHireRosterAgents({
    projectRef: TANK_LOOP,
    agents: [
      {
        pubkey: "1".repeat(64),
        name: "Builder",
        homeRole: "builder",
        projectRef: TANK_LOOP,
      },
      {
        pubkey: "2".repeat(64),
        name: "Project setup",
        homeRole: "project-setup",
        projectRef: TANK_LOOP,
        personaId: "project-team-setup:abc",
      },
      {
        pubkey: "3".repeat(64),
        name: "Runner",
        homeRole: "runner",
        projectRef: TANK_LOOP,
        hasRolePack: false,
      },
    ],
  });
  assert.deepEqual(
    roster.map((agent) => agent.name),
    ["Builder"],
  );
});
