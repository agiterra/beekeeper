import assert from "node:assert/strict";
import { test } from "node:test";

import {
  grantConsent,
  isSessionConsented,
  revokeConsent,
} from "./sessionConsent.ts";

test("nothing is consented by default", () => {
  const consent = { consented: [] };
  assert.ok(!isSessionConsented(consent, "A"));
});

test("grant makes a single session interactable; others stay off", () => {
  const consent = grantConsent({ consented: [] }, "A");
  assert.ok(isSessionConsented(consent, "A"));
  assert.ok(!isSessionConsented(consent, "B"));
});

test("grant is idempotent", () => {
  const once = grantConsent({ consented: [] }, "A");
  assert.equal(grantConsent(once, "A"), once);
});

test("revoke removes consent; revoking an unconsented session is a no-op", () => {
  const granted = { consented: ["A", "B"] };
  const revoked = revokeConsent(granted, "A");
  assert.ok(!isSessionConsented(revoked, "A"));
  assert.ok(isSessionConsented(revoked, "B"));
  assert.equal(revokeConsent(granted, "Z"), granted);
});
