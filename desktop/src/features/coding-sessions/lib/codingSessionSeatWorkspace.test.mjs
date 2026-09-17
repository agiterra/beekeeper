import assert from "node:assert/strict";
import test from "node:test";

import { hideRolesForSeat } from "./codingSessionSeatWorkspace.ts";

const AGENT = "ab".repeat(32);
const PROJECT = `30621:${"cd".repeat(32)}:tank-loop`;

function preview(overrides = {}) {
  return {
    packStaged: true,
    origin: "project",
    role: "builder",
    packDir: "/packs/staged/x/y",
    personaId: "builder",
    packRef: null,
    refusal: null,
    reason: null,
    rolesVisible: false,
    ...overrides,
  };
}

test("the directory is hidden by default and shown only when the manifest says so", async () => {
  const calls = [];
  const deps = {
    previewSeat: async (input) => {
      calls.push(input);
      return preview({ rolesVisible: input.role === "project-setup" });
    },
    fetchPackSource: async () => ({
      repo: `30617:${"cd".repeat(32)}:tank-loop`,
      gitRef: "refs/heads/main",
      sha: null,
      path: "beekeeper",
    }),
  };
  const builder = await hideRolesForSeat(
    { agentPubkey: AGENT, role: "builder", projectRef: PROJECT },
    deps,
  );
  assert.deepEqual(builder, { hideRoles: true, reason: null });
  const author = await hideRolesForSeat(
    { agentPubkey: AGENT, role: "project-setup", projectRef: PROJECT },
    deps,
  );
  assert.deepEqual(author, { hideRoles: false, reason: null });
  // The preview was asked the way a staging is: the same pack source.
  assert.equal(calls.length, 2);
  assert.equal(calls[0].packSource.path, "beekeeper");
  assert.equal(calls[0].agentPubkey, AGENT);
});

test("every failure keeps the default and says why", async () => {
  const noPreview = await hideRolesForSeat(
    { agentPubkey: AGENT, role: "builder", projectRef: PROJECT },
    {},
  );
  assert.equal(noPreview.hideRoles, true);
  assert.match(noPreview.reason, /cannot preview/);

  const noRole = await hideRolesForSeat(
    { agentPubkey: AGENT, role: null, projectRef: PROJECT },
    { previewSeat: async () => preview({ rolesVisible: true }) },
  );
  assert.equal(noRole.hideRoles, true);
  assert.match(noRole.reason, /no role/);

  const refused = await hideRolesForSeat(
    { agentPubkey: AGENT, role: "builder", projectRef: PROJECT },
    {
      previewSeat: async () =>
        preview({ rolesVisible: true, refusal: "This project stages…" }),
    },
  );
  assert.equal(refused.hideRoles, true);
  assert.match(refused.reason, /would be refused/);

  const threw = await hideRolesForSeat(
    { agentPubkey: AGENT, role: "builder", projectRef: PROJECT },
    {
      previewSeat: async () => {
        throw new Error("ipc down");
      },
    },
  );
  assert.equal(threw.hideRoles, true);
  assert.match(threw.reason, /ipc down/);

  // A host too old to say `rolesVisible` hides: absence is not permission.
  const older = await hideRolesForSeat(
    { agentPubkey: AGENT, role: "builder", projectRef: PROJECT },
    {
      previewSeat: async () => {
        const p = preview();
        delete p.rolesVisible;
        return p;
      },
    },
  );
  assert.deepEqual(older, { hideRoles: true, reason: null });
});
