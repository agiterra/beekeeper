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

// A terminal route carries no channel, so nothing on the URL can name the
// project. The shell session's own `projectRef` is the only link, and it is
// what the sidebar already files the terminal under.
test("a /shell route resolves through the session's projectRef", () => {
  assert.equal(
    resolveActiveProjectId(
      "/shell/session-123",
      null,
      PROJECTS,
      `30621:${OWNER}:skunkworks`,
    ),
    `${OWNER}:skunkworks`,
  );
});

test("a /shell route with no projectRef belongs to no project", () => {
  assert.equal(
    resolveActiveProjectId("/shell/session-123", null, PROJECTS),
    null,
  );
  assert.equal(
    resolveActiveProjectId("/shell/session-123", null, PROJECTS, null),
    null,
  );
});

test("a shell claiming an unknown project resolves to none, not a guess", () => {
  assert.equal(
    resolveActiveProjectId(
      "/shell/session-123",
      null,
      PROJECTS,
      `30621:${OWNER}:vanished`,
    ),
    null,
  );
});

// The channel back-reference must not be consulted for a terminal: a shell
// route has no active channel, and reading a stale one would file the terminal
// under whatever channel was last selected.
test("a shell route ignores the channel back-reference", () => {
  assert.equal(
    resolveActiveProjectId(
      "/shell/session-123",
      `30621:${OWNER}:general`,
      PROJECTS,
      null,
    ),
    null,
  );
});
