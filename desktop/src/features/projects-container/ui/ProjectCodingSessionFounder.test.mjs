/**
 * When the project founder may found.
 *
 * The bug this pins (review, 2026-09-10): the containers query resolves
 * *after* the repos and channels on a cold start (`/projects/$id/sessions/new`
 * as a deep link). While it loads, `project` is null and `projectRepos` is an
 * empty array, and the checkout read settles at once for that empty list. On
 * the commit where the project appears, `projectRepos` is a new, non-empty
 * array — but the founding effect runs in that same commit, before the
 * re-read, and sees a checkout that still says "settled". Founding then would
 * carry `repoRef: null` and no workdir for a project that has both.
 *
 * So "settled" is an identity, not a flag: the checkout must have answered
 * for the exact repository list the founding will use.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { projectCodingSessionFounderReady } from "./ProjectCodingSessionFounder.tsx";

const EMPTY = [];
const REPOS = [{ repoAddress: "30617:owner:repo" }];

const loaded = {
  projectsLoading: false,
  reposLoading: false,
  channelsLoading: false,
};

test("a checkout settled for the empty list does not make a resolved project ready", () => {
  // The cold-start commit: containers just resolved, the checkout state is
  // still the one written for the empty list.
  assert.equal(
    projectCodingSessionFounderReady({
      ...loaded,
      checkout: { settledFor: EMPTY, path: null, repoRef: null },
      projectRepos: REPOS,
    }),
    false,
  );
  // …and once the read answers for that list, founding may go ahead.
  assert.equal(
    projectCodingSessionFounderReady({
      ...loaded,
      checkout: {
        settledFor: REPOS,
        path: "/src/repo",
        repoRef: "30617:owner:repo",
      },
      projectRepos: REPOS,
    }),
    true,
  );
});

test("an in-flight read is never ready, and an equal-but-different list is not the same list", () => {
  assert.equal(
    projectCodingSessionFounderReady({
      ...loaded,
      checkout: { settledFor: null, path: null, repoRef: null },
      projectRepos: REPOS,
    }),
    false,
  );
  assert.equal(
    projectCodingSessionFounderReady({
      ...loaded,
      checkout: { settledFor: [...REPOS], path: null, repoRef: null },
      projectRepos: REPOS,
    }),
    false,
    "identity, so a re-memoised list forces a re-read rather than a guess",
  );
});

test("a project with no repositories is ready once its (empty) list has been seen", () => {
  assert.equal(
    projectCodingSessionFounderReady({
      ...loaded,
      checkout: { settledFor: EMPTY, path: null, repoRef: null },
      projectRepos: EMPTY,
    }),
    true,
  );
});

test("any query still loading holds the founding, whatever the checkout says", () => {
  const settled = {
    checkout: {
      settledFor: REPOS,
      path: "/src/repo",
      repoRef: "30617:owner:repo",
    },
    projectRepos: REPOS,
  };
  for (const flag of ["projectsLoading", "reposLoading", "channelsLoading"]) {
    assert.equal(
      projectCodingSessionFounderReady({ ...loaded, [flag]: true, ...settled }),
      false,
      flag,
    );
  }
});
