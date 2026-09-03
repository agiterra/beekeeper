import assert from "node:assert/strict";
import test from "node:test";

import { selectLaunchRepoRef } from "./codingSessionLaunchRepoRef.ts";

const OWNER = "6c".repeat(32);

function repo(dtag, name = dtag) {
  return { dtag, name, repoAddress: `30617:${OWNER}:${dtag}` };
}

test("L20.1: a checkout matching one project repo names that repo", () => {
  const repoRef = selectLaunchRepoRef({
    repos: [repo("other"), repo("beekeeper")],
    localRepos: [{ name: "beekeeper", path: "/Users/brian/beekeeper" }],
  });
  assert.equal(repoRef, `30617:${OWNER}:beekeeper`);
});

test("L20.1: no checkout match but exactly one project repo names it anyway", () => {
  const repoRef = selectLaunchRepoRef({
    repos: [repo("beekeeper")],
    localRepos: [],
  });
  assert.equal(repoRef, `30617:${OWNER}:beekeeper`);
});

test("L20.1: no checkout match and two or more repos names none — never guesses", () => {
  const repoRef = selectLaunchRepoRef({
    repos: [repo("beekeeper"), repo("other")],
    localRepos: [{ name: "third", path: "/tmp/third" }],
  });
  assert.equal(repoRef, null);
});

test("L20.1: a project with no repositories at all names none", () => {
  assert.equal(selectLaunchRepoRef({ repos: [], localRepos: [] }), null);
});

test("L20.1: a checkout match wins even when the project has several repos", () => {
  const repoRef = selectLaunchRepoRef({
    repos: [repo("first"), repo("second"), repo("third")],
    localRepos: [{ name: "second", path: "/Users/brian/second" }],
  });
  assert.equal(repoRef, `30617:${OWNER}:second`);
});
