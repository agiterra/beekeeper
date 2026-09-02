import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { teamReadinessPrepareScope } from "./teamReadinessModel.ts";
import { TeamReadinessCard } from "../ui/TeamReadinessCard.tsx";

// Live run 2, 10:36: a two-seat launch (lead + one bench builder) over a
// `personas/roles` folder holding six packs.
const PACKS = [
  "lead",
  "builder",
  "verifier",
  "designer",
  "refuter",
  "runner",
].map((role) => ({
  role,
  defaultName: `${role[0].toUpperCase()}${role.slice(1)}`,
  installed: false,
}));

function scan(packs = PACKS) {
  return {
    directory: "/Users/brian/Projects/beekeeper/beekeeper/personas/roles",
    exists: true,
    packs,
    skipped: [],
  };
}

function card(props = {}) {
  return renderToStaticMarkup(
    React.createElement(TeamReadinessCard, {
      readiness: null,
      loading: false,
      readError: null,
      selectedRoles: ["lead", "builder"],
      scan: scan(),
      names: Object.fromEntries(
        PACKS.map((pack) => [pack.role, pack.defaultName]),
      ),
      onNameChange: () => {},
      onBeginPrepare: () => {},
      onConfirmPrepare: () => {},
      onCancelPrepare: () => {},
      scanning: false,
      preparing: false,
      prepareSteps: [],
      prepareError: null,
      prepareWarning: null,
      externalBusy: false,
      runtimeTarget: null,
      ...props,
    }),
  );
}

test("L2.4: a two-seat launch confirms two names and says how many were refreshed", () => {
  const scope = teamReadinessPrepareScope({
    packs: PACKS,
    selectedRoles: ["lead", "builder"],
  });
  assert.deepEqual(
    scope.confirm.map((pack) => pack.role),
    ["builder", "lead"],
  );
  assert.equal(scope.otherCount, 4);
  assert.deepEqual(scope.otherRoles, [
    "designer",
    "refuter",
    "runner",
    "verifier",
  ]);
  assert.equal(
    scope.otherLine,
    "4 other packs were refreshed and are not part of this launch.",
  );
  assert.deepEqual(scope.missingRoles, []);
});

test("L2.4: a lead-only launch confirms one name and says five", () => {
  const scope = teamReadinessPrepareScope({
    packs: PACKS,
    selectedRoles: ["lead"],
  });
  assert.deepEqual(
    scope.confirm.map((pack) => pack.role),
    ["lead"],
  );
  assert.equal(
    scope.otherLine,
    "5 other packs were refreshed and are not part of this launch.",
  );
});

test("L2.4: one other pack reads as one pack, and none says nothing at all", () => {
  const three = teamReadinessPrepareScope({
    packs: PACKS.slice(0, 3),
    selectedRoles: ["lead", "builder"],
  });
  assert.equal(
    three.otherLine,
    "1 other pack was refreshed and is not part of this launch.",
  );
  const exact = teamReadinessPrepareScope({
    packs: PACKS.slice(0, 2),
    selectedRoles: ["lead", "builder"],
  });
  assert.equal(exact.otherCount, 0);
  assert.equal(exact.otherLine, null);
});

test("L2.4: a selected role with no pack is still named, and never silently dropped", () => {
  const scope = teamReadinessPrepareScope({
    packs: PACKS.slice(0, 1),
    selectedRoles: ["lead", "builder"],
  });
  assert.deepEqual(scope.missingRoles, ["builder"]);
  assert.deepEqual(
    scope.confirm.map((pack) => pack.role),
    ["lead"],
  );
});

test("L2.4: the card renders a field per launch seat and no others", () => {
  const html = card();
  assert.match(html, /Confirm the names for this launch/);
  assert.doesNotMatch(html, /Confirm every refreshed role name/);
  assert.match(
    html,
    /4 other packs were refreshed and are not part of this launch\./,
  );
  assert.match(html, /id="team-readiness-name-lead"/);
  assert.match(html, /id="team-readiness-name-builder"/);
  for (const role of ["verifier", "designer", "refuter", "runner"]) {
    assert.doesNotMatch(html, new RegExp(`id="team-readiness-name-${role}"`));
  }
});

test("L2.4: a launch whose pack is missing still names it, and blocks Confirm", () => {
  const html = card({ scan: scan(PACKS.slice(0, 1)) });
  assert.match(html, /Missing selected role pack: builder/);
  assert.match(
    html,
    /data-testid="team-readiness-prepare-confirm"[^>]*disabled/,
  );
});

test("F7: a launch that benches a builder confirms two names, not one", () => {
  // REVIEW-L2 F7: both call sites passed `[lead.role]`, so live run 2's
  // two-seat launch (Keystone leading, Bob benched as builder) put ONE field
  // on screen and refreshed the builder pack silently.
  const scope = teamReadinessPrepareScope({
    packs: PACKS,
    selectedRoles: ["lead", "builder"],
  });
  assert.deepEqual(
    scope.confirm.map((pack) => pack.role),
    ["builder", "lead"],
  );
  const leadOnly = teamReadinessPrepareScope({
    packs: PACKS,
    selectedRoles: ["lead"],
  });
  assert.deepEqual(
    leadOnly.confirm.map((pack) => pack.role),
    ["lead"],
  );
  // The difference the call-site fix makes, stated as a number.
  assert.equal(scope.otherCount, 4);
  assert.equal(leadOnly.otherCount, 5);
});
