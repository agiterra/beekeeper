import assert from "node:assert/strict";
import test from "node:test";

import { prepareProjectForTeams } from "./teamReadinessPrepare.ts";

// Ledger 302(i): a project whose roles live in its agents repository (a
// kind:30624 pack source) stages each seat's role at launch. Prepare must not
// install from the checkout's personas/roles — that folder is not where its
// roles are, and scanning it is what read "No role packs were discovered".
test("a project with a pack source prepares the provider and installs nothing from the checkout", async () => {
  const calls = [];
  const finalReadiness = { readyForFirstSession: true };
  const result = await prepareProjectForTeams({
    rolesStagedAtLaunch: true,
    dependencies: {
      installRoles: async () => calls.push("install"),
      startRoles: async () => calls.push("start roles"),
      provisionProvider: async () => calls.push("provision"),
      startProvider: async () => calls.push("start"),
      refreshRuntime: async () => calls.push("refresh runtime"),
      rereadReadiness: async () => {
        calls.push("reread");
        return finalReadiness;
      },
    },
  });

  assert.deepEqual(calls, ["provision", "start", "refresh runtime", "reread"]);
  assert.equal(result.error, null);
  assert.equal(result.readiness, finalReadiness);
  for (const id of ["install_roles", "start_roles"]) {
    const step = result.steps.find((entry) => entry.id === id);
    assert.equal(step.state, "done", id);
    assert.match(step.detail, /agents repository/, id);
  }
});

test("without a pack source Prepare still installs and starts from the checkout", async () => {
  const calls = [];
  await prepareProjectForTeams({
    dependencies: {
      installRoles: async () => calls.push("install"),
      startRoles: async () => calls.push("start roles"),
      provisionProvider: async () => calls.push("provision"),
      startProvider: async () => calls.push("start"),
      refreshRuntime: async () => calls.push("refresh runtime"),
      rereadReadiness: async () => {
        calls.push("reread");
        return null;
      },
    },
  });

  assert.deepEqual(calls.slice(0, 2), ["install", "start roles"]);
});
