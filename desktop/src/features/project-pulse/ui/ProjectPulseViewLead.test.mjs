import assert from "node:assert/strict";
import { test } from "node:test";

import { splitProjectPulseLeadSession } from "./ProjectPulseViewLead.ts";

const session = (sessionKey, sessionRef = null) => ({ sessionKey, sessionRef });

const groups = () => ({
  providerReachable: [session("a"), session("b", "30620:pk:b-ref")],
  openUnverified: [session("c")],
  closed: [session("d")],
});

test("no key moves nothing", () => {
  const input = groups();
  const split = splitProjectPulseLeadSession(input, null);
  assert.equal(split.lead, null);
  assert.equal(split.leadGroup, null);
  assert.equal(split.listed, input);
});

test("the lead leaves its group, keeps its classification, and is listed once", () => {
  const split = splitProjectPulseLeadSession(groups(), "c");
  assert.equal(split.lead?.sessionKey, "c");
  assert.equal(split.leadGroup, "open_unverified");
  assert.deepEqual(split.listed.openUnverified, []);
  assert.equal(split.listed.providerReachable.length, 2);
  assert.equal(split.listed.closed.length, 1);
});

test("a session ref matches as well as a session key", () => {
  const split = splitProjectPulseLeadSession(groups(), "30620:pk:b-ref");
  assert.equal(split.lead?.sessionKey, "b");
  assert.equal(split.leadGroup, "provider_reachable");
  assert.deepEqual(
    split.listed.providerReachable.map((entry) => entry.sessionKey),
    ["a"],
  );
});

test("a key not visible under the filter moves nothing", () => {
  const input = groups();
  const split = splitProjectPulseLeadSession(input, "missing");
  assert.equal(split.lead, null);
  assert.equal(split.listed, input);
});
