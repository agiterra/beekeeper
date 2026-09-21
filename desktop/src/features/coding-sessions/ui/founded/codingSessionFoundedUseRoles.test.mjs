import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionUseRolesOffWarning,
  resolveCodingSessionUseRoles,
} from "./codingSessionFoundedUseRoles.ts";

// Ledger 207(2), the live defect: a brand-new project whose agents
// repository was seeded with eight roles opened with Use roles OFF.
test("a project with a role source opens with roles on", () => {
  assert.equal(
    resolveCodingSessionUseRoles({ choice: null, packSourcePresent: true }),
    true,
  );
});

test("a project with no role source opens with roles off", () => {
  assert.equal(
    resolveCodingSessionUseRoles({ choice: null, packSourcePresent: false }),
    false,
  );
});

test("an unread project opens off rather than guessing", () => {
  assert.equal(
    resolveCodingSessionUseRoles({ choice: null, packSourcePresent: null }),
    false,
  );
  assert.equal(
    resolveCodingSessionUseRoles({
      choice: null,
      packSourcePresent: undefined,
    }),
    false,
  );
});

test("the person's own choice wins over the project's fact, both ways", () => {
  assert.equal(
    resolveCodingSessionUseRoles({ choice: false, packSourcePresent: true }),
    false,
  );
  assert.equal(
    resolveCodingSessionUseRoles({ choice: true, packSourcePresent: false }),
    true,
  );
});

test("unticking on a project with roles says what it costs", () => {
  const warning = codingSessionUseRolesOffWarning({
    useRoles: false,
    packSourcePresent: true,
  });
  assert.ok(warning);
  assert.match(warning, /no role instructions/);
  assert.match(warning, /readiness/);
  assert.match(warning, /delegation/);
});

test("no warning where roles-off is simply the truth", () => {
  assert.equal(
    codingSessionUseRolesOffWarning({
      useRoles: false,
      packSourcePresent: false,
    }),
    null,
  );
  assert.equal(
    codingSessionUseRolesOffWarning({
      useRoles: true,
      packSourcePresent: true,
    }),
    null,
  );
});
