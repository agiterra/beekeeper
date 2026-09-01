import assert from "node:assert/strict";
import test from "node:test";

import {
  __resetHotkeyTargetsForTests,
  activateHotkeyTarget,
  getHotkeyTargets,
  registerHotkeyTargets,
  setActiveHotkeyScope,
} from "./hotkeyTargetRegistry.ts";

function target(key, log) {
  return { key, label: key, activate: () => log.push(key) };
}

test("registered targets keep the order they were published in", () => {
  __resetHotkeyTargetsForTests();
  const log = [];
  registerHotkeyTargets("projects", [
    target("general", log),
    target("beekeeper", log),
  ]);
  assert.deepEqual(
    getHotkeyTargets("projects").map((entry) => entry.key),
    ["general", "beekeeper"],
  );
});

test("a position activates exactly the row at that index", () => {
  __resetHotkeyTargetsForTests();
  const log = [];
  registerHotkeyTargets("projects", [target("a", log), target("b", log)]);
  assert.equal(activateHotkeyTarget("projects", 1), true);
  assert.deepEqual(log, ["b"]);
});

test("a position past the end reports that nothing was there", () => {
  __resetHotkeyTargetsForTests();
  registerHotkeyTargets("projects", [target("a", [])]);
  assert.equal(activateHotkeyTarget("projects", 4), false);
  assert.equal(activateHotkeyTarget(null, 0), false);
});

test("scopes are independent", () => {
  __resetHotkeyTargetsForTests();
  const log = [];
  registerHotkeyTargets("projects", [target("project", log)]);
  registerHotkeyTargets("dms", [target("dm", log)]);
  activateHotkeyTarget("dms", 0);
  assert.deepEqual(log, ["dm"]);
});

test("unregistering clears the scope", () => {
  __resetHotkeyTargetsForTests();
  const dispose = registerHotkeyTargets("dms", [target("dm", [])]);
  dispose();
  assert.deepEqual(getHotkeyTargets("dms"), []);
});

// A remount registers before the outgoing effect cleans up. The stale
// unregister must not delete the list its successor just published, or the
// chords go dead until the next render.
test("a stale unregister leaves a newer registration alone", () => {
  __resetHotkeyTargetsForTests();
  const log = [];
  const disposeOld = registerHotkeyTargets("dms", [target("old", log)]);
  registerHotkeyTargets("dms", [target("new", log)]);
  disposeOld();
  assert.deepEqual(
    getHotkeyTargets("dms").map((entry) => entry.key),
    ["new"],
  );
});

test("the active scope is a single answer, and resets", () => {
  __resetHotkeyTargetsForTests();
  setActiveHotkeyScope("project:abc");
  setActiveHotkeyScope(null);
  assert.deepEqual(getHotkeyTargets("project:abc"), []);
});
