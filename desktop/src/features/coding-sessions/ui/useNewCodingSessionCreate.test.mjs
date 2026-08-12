import assert from "node:assert/strict";
import test from "node:test";

import { loadOrProvisionCodingSessionProvider } from "./useNewCodingSessionCreate.ts";

const provisioned = {
  provisioned: true,
  running: true,
  providerPubkey: "a".repeat(64),
  instanceId: "a".repeat(16),
};

test("an existing coding-session provider is reused", async () => {
  let provisionCalls = 0;
  let provisioningNotices = 0;

  const status = await loadOrProvisionCodingSessionProvider({
    getStatus: async () => provisioned,
    onProvisioning: () => {
      provisioningNotices += 1;
    },
    provision: async () => {
      provisionCalls += 1;
      return provisioned;
    },
  });

  assert.equal(status, provisioned);
  assert.equal(provisionCalls, 0);
  assert.equal(provisioningNotices, 0);
});

test("a fresh create screen provisions this computer before target selection", async () => {
  let provisionCalls = 0;
  let provisioningNotices = 0;

  const status = await loadOrProvisionCodingSessionProvider({
    getStatus: async () => ({ provisioned: false, running: false }),
    onProvisioning: () => {
      provisioningNotices += 1;
    },
    provision: async () => {
      provisionCalls += 1;
      return provisioned;
    },
  });

  assert.equal(status, provisioned);
  assert.equal(provisionCalls, 1);
  assert.equal(provisioningNotices, 1);
});
