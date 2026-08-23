import assert from "node:assert/strict";
import test from "node:test";

const { projectContainsRepositoryRouteId, projectMatchesRouteId } =
  await import("./projectRoutes.ts");

const OWNER =
  "6cbdf4451d3989c10c20d13240c665a9e11e3959a95488382193481692b68df2";

function repository(dtag) {
  return { dtag, owner: OWNER, repoAddress: `30617:${OWNER}:${dtag}` };
}

/** A container named "Tank Loop" holding a repo imported as "tankloop". */
function project(dtag, repoDtags) {
  return {
    dtag,
    owner: OWNER,
    projectAddress: `30621:${OWNER}:${dtag}`,
    repositoryAddresses: repoDtags.map((d) => `30617:${OWNER}:${d}`),
    repositories: repoDtags.map(repository),
  };
}

const tankLoop = project("tank-loop", ["tankloop"]);

test("canonical repo coordinate resolves to the containing project", () => {
  assert.equal(
    projectMatchesRouteId(tankLoop, `30617:${OWNER}:tankloop`),
    true,
  );
});

test("canonical project coordinate still resolves exactly", () => {
  assert.equal(
    projectMatchesRouteId(tankLoop, `30621:${OWNER}:tank-loop`),
    true,
  );
  assert.equal(projectMatchesRouteId(tankLoop, `30621:${OWNER}:other`), false);
});

test("a legacy <owner>:<repo-dtag> link does not match the project's own dtag", () => {
  // The regression: Repository.id is `<owner>:<dtag>`, and the repo's dtag
  // ("tankloop") differs from its container's ("tank-loop"), so the exact
  // matcher correctly declines — which is what stranded the repo page.
  assert.equal(projectMatchesRouteId(tankLoop, `${OWNER}:tankloop`), false);
});

test("the fallback resolves a legacy repo-dtag link to its container", () => {
  assert.equal(
    projectContainsRepositoryRouteId(tankLoop, `${OWNER}:tankloop`),
    true,
  );
});

test("the fallback is scoped to owner and never fires on a canonical address", () => {
  const other = "a".repeat(64);
  assert.equal(
    projectContainsRepositoryRouteId(tankLoop, `${other}:tankloop`),
    false,
  );
  assert.equal(
    projectContainsRepositoryRouteId(tankLoop, `30617:${OWNER}:tankloop`),
    false,
  );
});

test("a project owning the dtag outranks one merely containing it", () => {
  // Two-pass ordering in useProjectQuery: exact first, fallback second.
  const owns = project("tankloop", ["something-else"]);
  const contains = tankLoop;
  const projects = [contains, owns];
  const routeId = `${OWNER}:tankloop`;
  const resolved =
    projects.find((p) => projectMatchesRouteId(p, routeId)) ??
    projects.find((p) => projectContainsRepositoryRouteId(p, routeId));
  assert.equal(resolved, owns);
});
