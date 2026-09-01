import assert from "node:assert/strict";
import test from "node:test";

import { DEFAULT_NAV_HOTKEY_BINDINGS } from "./navHotkeyBindings.ts";
import { findNavHotkeyRegistryConflicts } from "./navHotkeyRegistryConflicts.ts";

test("the defaults take nothing away", () => {
  assert.deepEqual(
    findNavHotkeyRegistryConflicts(DEFAULT_NAV_HOTKEY_BINDINGS, true),
    [],
  );
  assert.deepEqual(
    findNavHotkeyRegistryConflicts(DEFAULT_NAV_HOTKEY_BINDINGS, false),
    [],
  );
});

// ⌘F is "Find in channel". A person is allowed to take it — but not by
// accident, and not without being told what it costs.
test("a rebind onto an existing shortcut is reported by name", () => {
  const conflicts = findNavHotkeyRegistryConflicts(
    {
      ...DEFAULT_NAV_HOTKEY_BINDINGS,
      scopeModifier: "meta",
      codes: { ...DEFAULT_NAV_HOTKEY_BINDINGS.codes, dashboard: "KeyF" },
    },
    true,
  );
  const found = conflicts.find((conflict) => conflict.source === "dashboard");
  assert.ok(found, "expected the dashboard rebind to be reported");
  assert.equal(found.chord, "⌘F");
  assert.equal(found.shortcut.id, "find-in-channel");
});

test("the same collision is reported on Windows chord spelling", () => {
  const conflicts = findNavHotkeyRegistryConflicts(
    {
      ...DEFAULT_NAV_HOTKEY_BINDINGS,
      scopeModifier: "ctrl",
      codes: { ...DEFAULT_NAV_HOTKEY_BINDINGS.codes, dashboard: "KeyF" },
    },
    false,
  );
  const found = conflicts.find((conflict) => conflict.source === "dashboard");
  assert.ok(found);
  assert.equal(found.chord, "Ctrl+F");
});

// The positions run 1–9 and stop; 0 is left to zoom reset precisely so this
// family collides with nothing on the default modifiers.
test("the positional family collides with nothing by default", () => {
  const conflicts = findNavHotkeyRegistryConflicts(
    DEFAULT_NAV_HOTKEY_BINDINGS,
    true,
  );
  assert.equal(
    conflicts.filter((conflict) => conflict.source === "positions").length,
    0,
  );
});
