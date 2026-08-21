import assert from "node:assert/strict";
import { test } from "node:test";

import { attachableProjectRepos } from "./attachableRepos.ts";
import { LOCAL_GENERAL_ID, makeLocalGeneral } from "./projectContainerModel.ts";

const OWNER = "a".repeat(64);

function makeProject(dtag, overrides = {}) {
  return {
    id: `${OWNER}:${dtag}`,
    dtag,
    owner: OWNER,
    name: dtag,
    description: "",
    createdAt: 1,
    address: `30621:${OWNER}:${dtag}`,
    repoAddrs: [],
    agentAddrs: [],
    channelIds: [],
    ...overrides,
  };
}

function makeRepo(dtag) {
  return {
    id: dtag,
    dtag,
    name: dtag,
    owner: OWNER,
    repoAddress: `30617:${OWNER}:${dtag}`,
  };
}

test("candidates are the repos of other projects, keyed to their source", () => {
  const skunkworks = makeProject("skunkworks");
  const attic = makeProject("attic");
  const atticRepo = makeRepo("attic-repo");
  const ownRepo = makeRepo("own-repo");
  const reposByProject = new Map([
    [skunkworks.id, [ownRepo]],
    [attic.id, [atticRepo]],
  ]);

  const result = attachableProjectRepos(
    [skunkworks, attic],
    reposByProject,
    [],
    skunkworks,
  );

  assert.deepEqual(
    result.candidates.map((repo) => repo.dtag),
    ["attic-repo"],
  );
  assert.equal(result.fromByAddress.get(atticRepo.repoAddress), attic);
});

test("unclaimed repos are candidates for a real project but not for General", () => {
  const skunkworks = makeProject("skunkworks");
  const general = makeProject("general");
  const drifter = makeRepo("drifter");

  const forProject = attachableProjectRepos(
    [skunkworks, general],
    new Map(),
    [drifter],
    skunkworks,
  );
  assert.deepEqual(
    forProject.candidates.map((repo) => repo.dtag),
    ["drifter"],
  );
  assert.equal(forProject.fromByAddress.get(drifter.repoAddress), null);

  const forGeneral = attachableProjectRepos(
    [skunkworks, general],
    new Map(),
    [drifter],
    general,
  );
  assert.deepEqual(forGeneral.candidates, []);

  const forLocalGeneral = attachableProjectRepos(
    [skunkworks],
    new Map(),
    [drifter],
    makeLocalGeneral(),
  );
  assert.equal(makeLocalGeneral().id, LOCAL_GENERAL_ID);
  assert.deepEqual(forLocalGeneral.candidates, []);
});
