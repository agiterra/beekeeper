import assert from "node:assert/strict";
import { test } from "node:test";

import {
  PROJECT_AGENTS_EMPTY_HINT,
  projectAgentCountLabel,
  projectAgentRows,
} from "./projectChildren.ts";

const OWNER = "a".repeat(64);
const ADDRESS = `30621:${OWNER}:kettle-smoke`;

const project = (agentAddrs = []) => ({
  address: ADDRESS,
  agentAddrs,
});

const agent = (n, role) => ({
  pubkey: `${n}`.repeat(64).slice(0, 64),
  name: `${role[0].toUpperCase()}${role.slice(1)} 2`,
  homeRole: role,
  projectRef: ADDRESS,
});

// Ledger 207(3), the live defect: "Agents 0 — No agents in this project"
// under a Members panel listing the project's eight agents.
test("this computer's associated agents are counted and named", () => {
  const rows = projectAgentRows(project(), new Map(), new Map(), [
    agent(1, "lead"),
    agent(2, "builder"),
  ]);
  assert.equal(rows.length, 2);
  assert.deepEqual(
    rows.map((row) => [row.label, row.role, row.source]),
    [
      ["Lead 2", "lead", "local"],
      ["Builder 2", "builder", "local"],
    ],
  );
  assert.equal(projectAgentCountLabel(rows), "2 on this computer");
});

test("an agent associated with another project is not counted", () => {
  const rows = projectAgentRows(project(), new Map(), new Map(), [
    { ...agent(1, "lead"), projectRef: `30621:${OWNER}:other` },
    { ...agent(2, "builder"), projectRef: null },
  ]);
  assert.deepEqual(rows, []);
  assert.equal(projectAgentCountLabel(rows), "0");
});

test("a published curation and its local record are one row, not two", () => {
  const local = agent(1, "lead");
  const addr = `30177:${OWNER}:${local.pubkey}`;
  const rows = projectAgentRows(
    project([addr]),
    new Map(),
    new Map([[local.pubkey, "Lead 2"]]),
    [local],
  );
  assert.equal(rows.length, 1);
  assert.equal(rows[0].source, "published");
  assert.equal(projectAgentCountLabel(rows), "1");
});

test("a mixed card says how many of its rows are local only", () => {
  const published = agent(1, "lead");
  const rows = projectAgentRows(
    project([`30177:${OWNER}:${published.pubkey}`]),
    new Map(),
    new Map([[published.pubkey, "Lead 2"]]),
    [published, agent(2, "builder")],
  );
  assert.equal(rows.length, 2);
  assert.equal(projectAgentCountLabel(rows), "2 · 1 on this computer");
});

test("the empty hint never asserts the project has no agents", () => {
  assert.doesNotMatch(PROJECT_AGENTS_EMPTY_HINT, /No agents in this project/);
  assert.match(PROJECT_AGENTS_EMPTY_HINT, /on this computer/);
});
