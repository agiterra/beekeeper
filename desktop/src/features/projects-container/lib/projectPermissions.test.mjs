import assert from "node:assert/strict";
import { test } from "node:test";

import { viewerIsProjectOwner } from "./projectPermissions.ts";

const CREATOR = "a".repeat(64);
const CO_OWNER = "b".repeat(64);
const COLLABORATOR = "c".repeat(64);
const VIEWER = "d".repeat(64);
const STRANGER = "e".repeat(64);

const project = { owner: CREATOR };
const roster = [
  { pubkey: CO_OWNER, role: "owner" },
  { pubkey: COLLABORATOR, role: "collaborator" },
  { pubkey: VIEWER, role: "viewer" },
];

test("the creator is an owner without appearing on the roster", () => {
  // The relay's kind:39010 projection never emits a p tag for the creator —
  // a membership op targeting them is refused — so the roster below is what
  // a real read returns for this project.
  assert.equal(viewerIsProjectOwner(CREATOR, project, roster), true);
});

test("a roster owner is the same tier as the creator", () => {
  assert.equal(viewerIsProjectOwner(CO_OWNER, project, roster), true);
});

test("write access is not ownership", () => {
  assert.equal(viewerIsProjectOwner(COLLABORATOR, project, roster), false);
  assert.equal(viewerIsProjectOwner(VIEWER, project, roster), false);
  assert.equal(viewerIsProjectOwner(STRANGER, project, roster), false);
});

test("an unresolved identity owns nothing", () => {
  // Called during the first render, before the identity query settles. A
  // truthy answer here would flash a Delete item at everyone.
  assert.equal(viewerIsProjectOwner(null, project, roster), false);
});

test("the ownerless local General placeholder has no owner", () => {
  assert.equal(viewerIsProjectOwner(CREATOR, { owner: "" }, []), false);
  assert.equal(viewerIsProjectOwner(CREATOR, null, []), false);
});

test("pubkey comparison is case-insensitive on both sides", () => {
  // Relay reads are lowercase hex, but a head event's p tag and a locally
  // held identity have both arrived uppercase before.
  assert.equal(
    viewerIsProjectOwner(CREATOR, { owner: CREATOR.toUpperCase() }, []),
    true,
  );
  assert.equal(
    viewerIsProjectOwner(CO_OWNER, project, [
      { pubkey: CO_OWNER.toUpperCase(), role: "owner" },
    ]),
    true,
  );
});

test("an empty roster leaves only the creator", () => {
  assert.equal(viewerIsProjectOwner(CREATOR, project, []), true);
  assert.equal(viewerIsProjectOwner(CO_OWNER, project, []), false);
});
