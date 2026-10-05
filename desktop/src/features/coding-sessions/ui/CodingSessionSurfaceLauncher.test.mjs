import assert from "node:assert/strict";
import test from "node:test";

import {
  codingSessionSurfaceForShortcut,
  codingSessionSurfaceTargetIsTyping,
} from "./CodingSessionSurfaceLauncher.tsx";

const surface = (id, shortcut, available = true) => ({
  definition: { id, shortcut },
  availability: available
    ? { available: true }
    : { available: false, reason: "Not here." },
});

const SURFACES = [
  surface("agents", "A"),
  surface("diff", "D"),
  surface("browser", "B", false),
];

const key = (overrides) => ({
  altKey: false,
  ctrlKey: false,
  defaultPrevented: false,
  isComposing: false,
  key: "d",
  metaKey: false,
  ...overrides,
});

test("a bare letter opens its available surface, either case", () => {
  assert.equal(codingSessionSurfaceForShortcut(SURFACES, key({}))?.id, "diff");
  assert.equal(
    codingSessionSurfaceForShortcut(SURFACES, key({ key: "A" }))?.id,
    "agents",
  );
});

test("a dimmed surface's letter opens nothing", () => {
  assert.equal(
    codingSessionSurfaceForShortcut(SURFACES, key({ key: "b" })),
    null,
  );
});

test("modifiers, composition and handled events open nothing", () => {
  for (const overrides of [
    { metaKey: true },
    { ctrlKey: true },
    { altKey: true },
    { isComposing: true },
    { defaultPrevented: true },
    { key: "Enter" },
    { key: "x" },
  ]) {
    assert.equal(
      codingSessionSurfaceForShortcut(SURFACES, key(overrides)),
      null,
      JSON.stringify(overrides),
    );
  }
});

test("a focused editable is a typing context, empty or not", () => {
  const target = (matches) => ({
    closest: (selector) => (matches(selector) ? {} : null),
  });
  assert.equal(
    codingSessionSurfaceTargetIsTyping(target((s) => s.includes("textarea"))),
    true,
  );
  assert.equal(codingSessionSurfaceTargetIsTyping(target(() => false)), false);
  assert.equal(codingSessionSurfaceTargetIsTyping(null), false);
});
