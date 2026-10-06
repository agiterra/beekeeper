import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { test } from "node:test";

import {
  agentMaySeatInProject,
  agentProjectRelation,
  normalizeProjectCoordinate,
  projectAgentDigest,
  readPublishedAgentAssociation,
  sameProjectRef,
} from "./projectAgentAssociation.ts";

const vectors = JSON.parse(
  readFileSync(
    new URL(
      "../../../../crates/beekeeper-core/testdata/project_agent_association/vectors.json",
      import.meta.url,
    ),
    "utf8",
  ),
);

const OWNER = "ab".repeat(32);
const TANK = `30621:${OWNER}:tank-loop`;
const OTHER = `30621:${OWNER}:other`;

test("digest matches the buzz-core vectors byte for byte", () => {
  assert.ok(vectors.length >= 4);
  for (const vector of vectors) {
    assert.equal(
      projectAgentDigest(vector.coordinate),
      vector.digest,
      vector.coordinate,
    );
  }
});

test("normalization lowercases the owner and refuses other kinds", () => {
  assert.equal(
    normalizeProjectCoordinate(`30621:${OWNER.toUpperCase()}:x`),
    `30621:${OWNER}:x`,
  );
  assert.equal(normalizeProjectCoordinate(`30178:${OWNER}:x`), null);
  assert.equal(normalizeProjectCoordinate(null), null);
  assert.ok(sameProjectRef(TANK, `30621:${OWNER.toUpperCase()}:tank-loop`));
  assert.equal(sameProjectRef(null, null), false);
});

test("relation: a matching role is never association", () => {
  assert.equal(agentProjectRelation({ projectRef: TANK }, TANK), "project");
  assert.equal(
    agentProjectRelation({ projectRef: OTHER }, TANK),
    "other-project",
  );
  assert.equal(
    agentProjectRelation({ projectRef: null }, TANK),
    "unassociated",
  );
  assert.equal(agentProjectRelation({}, TANK), "unassociated");
  assert.equal(agentMaySeatInProject({ projectRef: null }, TANK), false);
});

test("relation: a projectless session takes only unassociated agents", () => {
  assert.equal(agentMaySeatInProject({ projectRef: null }, null), true);
  assert.equal(agentMaySeatInProject({ projectRef: TANK }, null), false);
  assert.equal(agentMaySeatInProject({ projectRef: null }, "   "), true);
});

test("relation: a malformed session project matches no agent", () => {
  assert.equal(agentMaySeatInProject({ projectRef: null }, "30621:x:y"), false);
  assert.equal(
    agentMaySeatInProject({ projectRef: TANK }, "30621:x:tank-loop"),
    false,
  );
});

test("reads a published association claim and refuses malformed events", () => {
  const pubkey = "cd".repeat(32);
  const digest = projectAgentDigest(TANK);
  const read = readPublishedAgentAssociation({
    kind: 30177,
    pubkey: OWNER,
    created_at: 10,
    tags: [["d", pubkey]],
    content: JSON.stringify({
      name: "Builder",
      home_role: "builder",
      project_digest: digest,
      parallelism: 1,
    }),
  });
  assert.deepEqual(read, {
    pubkey,
    ownerPubkey: OWNER,
    name: "Builder",
    homeRole: "builder",
    projectDigest: digest,
    createdAt: 10,
  });
  assert.equal(
    readPublishedAgentAssociation({
      kind: 30177,
      pubkey: OWNER,
      created_at: 1,
      tags: [],
      content: "{}",
    }),
    null,
  );
  assert.equal(
    readPublishedAgentAssociation({
      kind: 30177,
      pubkey: OWNER,
      created_at: 1,
      tags: [["d", pubkey]],
      content: "not json",
    }),
    null,
  );
});
