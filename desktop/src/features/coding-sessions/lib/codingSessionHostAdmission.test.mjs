import assert from "node:assert/strict";
import test from "node:test";

import { hostAdmissionLine } from "./codingSessionHostAdmission.ts";

// Ledger 266: three control runs had a host that could not read its own
// project while every surface looked ready.
test("an unreadable admission is disclosed with its reason", () => {
  assert.equal(
    hostAdmissionLine({
      state: "unreadable",
      reason: "admitted but the host cannot read the project yet",
    }),
    "This computer cannot read this project: admitted but the host cannot read the project yet",
  );
});

test("an unauthorized admission is disclosed with its reason", () => {
  assert.equal(
    hostAdmissionLine({ state: "unauthorized", reason: "an owner removed it" }),
    "This computer cannot read this project: an owner removed it",
  );
});

test("public, admitted, and never-asked say nothing", () => {
  assert.equal(hostAdmissionLine({ state: "public" }), null);
  assert.equal(hostAdmissionLine({ state: "admitted", already: false }), null);
  assert.equal(hostAdmissionLine({ state: "admitted", already: true }), null);
  assert.equal(hostAdmissionLine(null), null);
  assert.equal(hostAdmissionLine(undefined), null);
});
