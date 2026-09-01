import assert from "node:assert/strict";
import test from "node:test";

import {
  DEFAULT_NAV_HOTKEY_BINDINGS,
  findNavHotkeyConflicts,
  hasExactModifier,
  isBindableNavHotkeyCode,
  matchNavHotkey,
  navHotkeyCodeLabel,
  navHotkeyDigitLabel,
  navHotkeyIndexForCode,
  navHotkeyModifierGlyph,
  NAV_HOTKEY_DIGIT_CODES,
  NAV_HOTKEY_MAX_POSITIONS,
  parseNavHotkeyPayload,
} from "./navHotkeyBindings.ts";

function chord(overrides = {}) {
  return {
    altKey: false,
    code: "",
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    ...overrides,
  };
}

// ── Positions ────────────────────────────────────────────────────────────────

test("positions stop at 9 — 0 is left to zoom reset", () => {
  assert.equal(NAV_HOTKEY_MAX_POSITIONS, 9);
  assert.ok(!NAV_HOTKEY_DIGIT_CODES.includes("Digit0"));
  assert.equal(navHotkeyDigitLabel(0), "1");
  assert.equal(navHotkeyDigitLabel(8), "9");
  assert.equal(navHotkeyDigitLabel(9), null);
  assert.equal(navHotkeyDigitLabel(-1), null);
});

test("digit codes map back to their position", () => {
  assert.equal(navHotkeyIndexForCode("Digit1"), 0);
  assert.equal(navHotkeyIndexForCode("Digit9"), 8);
  assert.equal(navHotkeyIndexForCode("Digit0"), null);
  assert.equal(navHotkeyIndexForCode("KeyD"), null);
});

// ── Labels ───────────────────────────────────────────────────────────────────

test("labels come from the code, never the key", () => {
  // ⌥D reports key "∂" on macOS; only the code survives the modifier.
  assert.equal(navHotkeyCodeLabel("KeyD"), "D");
  assert.equal(navHotkeyCodeLabel("Digit3"), "3");
  assert.equal(navHotkeyCodeLabel("ArrowUp"), "Up");
});

test("modifier glyphs are symbols on macOS and words elsewhere", () => {
  assert.equal(navHotkeyModifierGlyph("alt", true), "⌥");
  assert.equal(navHotkeyModifierGlyph("meta", true), "⌘");
  assert.equal(navHotkeyModifierGlyph("ctrl", true), "⌃");
  assert.equal(navHotkeyModifierGlyph("alt", false), "Alt+");
  assert.equal(navHotkeyModifierGlyph("ctrl", false), "Ctrl+");
});

// ── Bindable codes ───────────────────────────────────────────────────────────

test("only letters may be bound to a named destination", () => {
  assert.ok(isBindableNavHotkeyCode("KeyQ"));
  assert.ok(!isBindableNavHotkeyCode("Digit4"));
  assert.ok(!isBindableNavHotkeyCode("F5"));
  assert.ok(!isBindableNavHotkeyCode("Comma"));
});

// ── Modifier matching ────────────────────────────────────────────────────────

test("a modifier is exact only when it is the sole one down", () => {
  assert.ok(hasExactModifier(chord({ altKey: true }), "alt"));
  assert.ok(!hasExactModifier(chord({ altKey: true, metaKey: true }), "alt"));
  assert.ok(!hasExactModifier(chord({ altKey: true, shiftKey: true }), "alt"));
  assert.ok(!hasExactModifier(chord({ metaKey: true }), "alt"));
});

// ── Dispatch ─────────────────────────────────────────────────────────────────

test("the default bindings resolve ⌥D / ⌥P / ⌥M", () => {
  for (const [code, action] of [
    ["KeyD", "dashboard"],
    ["KeyP", "projects"],
    ["KeyM", "dms"],
  ]) {
    assert.deepEqual(
      matchNavHotkey(
        chord({ altKey: true, code }),
        DEFAULT_NAV_HOTKEY_BINDINGS,
      ),
      { kind: "action", action },
    );
  }
});

test("⌥<digit> is a project position and ⌘<digit> is a row position", () => {
  assert.deepEqual(
    matchNavHotkey(
      chord({ altKey: true, code: "Digit2" }),
      DEFAULT_NAV_HOTKEY_BINDINGS,
    ),
    { kind: "scope-position", index: 1 },
  );
  assert.deepEqual(
    matchNavHotkey(
      chord({ metaKey: true, code: "Digit2" }),
      DEFAULT_NAV_HOTKEY_BINDINGS,
    ),
    { kind: "item-position", index: 1 },
  );
});

test("a letter on the row modifier matches nothing — only digits are positions", () => {
  assert.equal(
    matchNavHotkey(
      chord({ metaKey: true, code: "KeyD" }),
      DEFAULT_NAV_HOTKEY_BINDINGS,
    ),
    null,
  );
});

test("Shift or a second modifier takes the chord out of scope", () => {
  assert.equal(
    matchNavHotkey(
      chord({ altKey: true, code: "KeyD", shiftKey: true }),
      DEFAULT_NAV_HOTKEY_BINDINGS,
    ),
    null,
  );
  assert.equal(
    matchNavHotkey(
      chord({ altKey: true, code: "KeyD", metaKey: true }),
      DEFAULT_NAV_HOTKEY_BINDINGS,
    ),
    null,
  );
});

test("disabled bindings match nothing at all", () => {
  assert.equal(
    matchNavHotkey(chord({ altKey: true, code: "KeyD" }), {
      ...DEFAULT_NAV_HOTKEY_BINDINGS,
      enabled: false,
    }),
    null,
  );
});

// ── Conflicts ────────────────────────────────────────────────────────────────

test("no conflicts in the defaults", () => {
  assert.deepEqual(findNavHotkeyConflicts(DEFAULT_NAV_HOTKEY_BINDINGS), []);
});

test("one modifier for both families is a conflict", () => {
  assert.deepEqual(
    findNavHotkeyConflicts({
      ...DEFAULT_NAV_HOTKEY_BINDINGS,
      itemModifier: "alt",
    }),
    [{ kind: "same-modifier" }],
  );
});

test("two destinations on one letter is a conflict naming both", () => {
  const conflicts = findNavHotkeyConflicts({
    ...DEFAULT_NAV_HOTKEY_BINDINGS,
    codes: { dashboard: "KeyD", projects: "KeyD", dms: "KeyM" },
  });
  assert.deepEqual(conflicts, [
    {
      kind: "duplicate-code",
      code: "KeyD",
      actions: ["dashboard", "projects"],
    },
  ]);
});

// ── Persistence parsing ──────────────────────────────────────────────────────

test("a stored payload round-trips", () => {
  const stored = {
    version: 1,
    enabled: false,
    scopeModifier: "ctrl",
    itemModifier: "alt",
    codes: { dashboard: "KeyH", projects: "KeyJ", dms: "KeyK" },
  };
  assert.deepEqual(parseNavHotkeyPayload(stored), stored);
});

test("junk fields fall back one at a time, not all at once", () => {
  const parsed = parseNavHotkeyPayload({
    version: 1,
    scopeModifier: "nonsense",
    codes: { dashboard: "KeyH", projects: "Digit4" },
  });
  // The valid rebind survives; the invalid ones take their defaults.
  assert.equal(parsed.codes.dashboard, "KeyH");
  assert.equal(parsed.codes.projects, "KeyP");
  assert.equal(parsed.codes.dms, "KeyM");
  assert.equal(parsed.scopeModifier, "alt");
  assert.equal(parsed.enabled, true);
});

test("a payload that is not v1 is not ours", () => {
  assert.equal(parseNavHotkeyPayload({ version: 2 }), null);
  assert.equal(parseNavHotkeyPayload(null), null);
  assert.equal(parseNavHotkeyPayload([]), null);
  assert.equal(parseNavHotkeyPayload("{}"), null);
});
