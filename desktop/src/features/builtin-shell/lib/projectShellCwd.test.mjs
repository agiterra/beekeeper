import assert from "node:assert/strict";
import { test } from "node:test";

import { matchProjectCwd } from "./projectShellCwd.ts";

const local = [
  { name: "tankloop", path: "/Users/andy/Code/tankloop" },
  { name: "buzz", path: "/Users/andy/.buzz/REPOS/buzz" },
];

test("first project repo with a local checkout wins, in repo order", () => {
  const cwd = matchProjectCwd(
    [
      { dtag: "missing", name: "missing" },
      { dtag: "buzz", name: "Buzz" },
      { dtag: "tankloop", name: "TankLoop" },
    ],
    local,
  );
  assert.equal(cwd, "/Users/andy/.buzz/REPOS/buzz");
});

test("falls back from dtag to display name and to undefined", () => {
  assert.equal(
    matchProjectCwd([{ dtag: "tl", name: "tankloop" }], local),
    "/Users/andy/Code/tankloop",
  );
  assert.equal(
    matchProjectCwd([{ dtag: "none", name: "nowhere" }], local),
    undefined,
  );
  assert.equal(matchProjectCwd([], local), undefined);
});
