import assert from "node:assert/strict";
import { test } from "node:test";

import { resolveActiveProjectId } from "./useActiveProjectTint.ts";

const OWNER = "a".repeat(64);

function makeProject(dtag) {
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
    visibility: "public",
    members: [],
    icon: null,
    color: "#3b82f6",
  };
}

const PROJECTS = [makeProject("skunkworks"), makeProject("general")];

test("a /projects route names the project by id or dtag", () => {
  assert.equal(
    resolveActiveProjectId(
      `/projects/${encodeURIComponent(`${OWNER}:skunkworks`)}`,
      null,
      PROJECTS,
    ),
    `${OWNER}:skunkworks`,
  );
  assert.equal(
    resolveActiveProjectId("/projects/skunkworks", null, PROJECTS),
    `${OWNER}:skunkworks`,
  );
  // Sub-routes inherit the project from the same segment.
  assert.equal(
    resolveActiveProjectId("/projects/skunkworks/pulse", null, PROJECTS),
    `${OWNER}:skunkworks`,
  );
});

test("a channel's projectRef names the project when off the /projects routes", () => {
  assert.equal(
    resolveActiveProjectId("/channels/abc", `30621:${OWNER}:general`, PROJECTS),
    `${OWNER}:general`,
  );
  // A projectRef of a non-project kind or an unknown project is no tint.
  assert.equal(
    resolveActiveProjectId("/channels/abc", `30617:${OWNER}:repo`, PROJECTS),
    null,
  );
  assert.equal(
    resolveActiveProjectId("/channels/abc", `30621:${OWNER}:gone`, PROJECTS),
    null,
  );
});

test("surfaces belonging to no project resolve to none", () => {
  assert.equal(resolveActiveProjectId("/", null, PROJECTS), null);
  assert.equal(resolveActiveProjectId("/settings", undefined, PROJECTS), null);
  // The /projects list itself (no id segment) is not one project.
  assert.equal(resolveActiveProjectId("/projects", null, PROJECTS), null);
  // An unknown /projects id must not fall through to the channel ref.
  assert.equal(
    resolveActiveProjectId(
      "/projects/unknown",
      `30621:${OWNER}:general`,
      PROJECTS,
    ),
    null,
  );
});
