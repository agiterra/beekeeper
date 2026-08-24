import assert from "node:assert/strict";
import test from "node:test";

import {
  parseCodingSessionModelFavorites,
  toggleCodingSessionModelFavorite,
} from "./codingSessionModelFavorites.ts";

test("a corrupt preference is nobody's preference, not a broken picker", () => {
  for (const raw of [null, "", "{", "null", '{"a":1}', "[1,2,3]", '["",""]']) {
    assert.deepEqual([...parseCodingSessionModelFavorites(raw)], []);
  }
  assert.deepEqual(
    [...parseCodingSessionModelFavorites('["a","b","a"]')],
    ["a", "b"],
  );
});

test("toggling adds, removes, and stays bounded", () => {
  const one = toggleCodingSessionModelFavorite(new Set(), "k");
  assert.deepEqual([...one], ["k"]);
  assert.deepEqual([...toggleCodingSessionModelFavorite(one, "k")], []);

  const full = new Set(
    Array.from({ length: 64 }, (_, index) => `key-${index}`),
  );
  const overflowed = toggleCodingSessionModelFavorite(full, "one-too-many");
  assert.equal(overflowed.size, 64);
  assert.ok(!overflowed.has("one-too-many"));
  // Removal still works at the ceiling.
  assert.equal(toggleCodingSessionModelFavorite(full, "key-0").size, 63);
});
