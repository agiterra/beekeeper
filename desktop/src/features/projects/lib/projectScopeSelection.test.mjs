import assert from "node:assert/strict";
import { test } from "node:test";
import { resolveProjectScope } from "./projectScopeSelection.ts";

const local = { id: "local:general", dtag: "general" };
const general = { id: "owner:general", dtag: "general" };
const other = { id: "owner:other", dtag: "other" };

test("General selection follows publication without changing the selected project", () => {
  assert.equal(resolveProjectScope(local.id, [local, other]), local.id);
  assert.equal(resolveProjectScope(local.id, [general, other]), general.id);
  assert.equal(resolveProjectScope(general.id, [general, other]), general.id);
});

test("an unknown or deleted project never becomes an unrelated creation target", () => {
  assert.equal(resolveProjectScope("deleted", [general, other]), "all");
  assert.equal(resolveProjectScope(local.id, [other]), "all");
  assert.equal(resolveProjectScope("all", [general, other]), "all");
  assert.equal(resolveProjectScope(other.id, [general, other]), other.id);
});

test("unresolved duplicate Generals do not select an arbitrary owner", () => {
  assert.equal(
    resolveProjectScope(local.id, [
      general,
      { ...general, id: "other:general" },
    ]),
    "all",
  );
});
