import assert from "node:assert/strict";
import { test } from "node:test";

import { codingSessionFileIcon } from "./CodingSessionFilesPanelIcons.ts";

const icon = (name, kind = "file") => codingSessionFileIcon({ name, kind });

test("a file's glyph follows its type, as T3's tree colours by type (SV-24)", () => {
  assert.equal(icon("relay.ts").className, "text-blue-500");
  assert.equal(icon("README.md").className, "text-sky-500");
  assert.equal(icon("Cargo.toml").className, "text-orange-500");
  assert.equal(icon("pnpm-lock.yaml").className, "text-amber-500");
  assert.equal(icon("tsconfig.json").className, "text-yellow-500");
  assert.equal(icon(".gitignore").className, "text-zinc-500");
});

test("an unknown type and a symlink stay muted, never borrowing a type's hue", () => {
  assert.equal(icon("LICENSE").className, "text-muted-foreground");
  assert.equal(icon("relay.ts", "symlink").className, "text-muted-foreground");
});
