import assert from "node:assert/strict";
import test from "node:test";

import {
  applyRequireVerdictOnMain,
  parseProtectionTags,
  requireVerdictOnMain,
} from "./projectRepositoryProtection.ts";

test("parseProtectionTags reads ref pattern and rules off buzz-protect tags", () => {
  const rules = parseProtectionTags([
    ["d", "agiterra-beekeeper"],
    ["buzz-protect", "refs/heads/main", "require-verdict", "no-force-push"],
    ["buzz-protect", "refs/heads/dev", "no-delete"],
    ["maintainers", "a".repeat(64)],
  ]);
  assert.deepEqual(rules, [
    {
      refPattern: "refs/heads/main",
      rules: ["require-verdict", "no-force-push"],
    },
    { refPattern: "refs/heads/dev", rules: ["no-delete"] },
  ]);
});

test("parseProtectionTags reads a rule-less buzz-protect tag as an empty rule set", () => {
  const rules = parseProtectionTags([["buzz-protect", "refs/heads/main"]]);
  assert.deepEqual(rules, [{ refPattern: "refs/heads/main", rules: [] }]);
});

test("parseProtectionTags ignores every non-buzz-protect tag", () => {
  assert.deepEqual(
    parseProtectionTags([
      ["d", "x"],
      ["clone", "https://example.com/x.git"],
    ]),
    [],
  );
});

test("requireVerdictOnMain is true exactly when main's own rule list carries it", () => {
  assert.equal(
    requireVerdictOnMain([
      { refPattern: "refs/heads/main", rules: ["require-verdict"] },
    ]),
    true,
  );
  assert.equal(
    requireVerdictOnMain([
      { refPattern: "refs/heads/main", rules: ["no-delete"] },
    ]),
    false,
  );
  assert.equal(
    requireVerdictOnMain([
      { refPattern: "refs/heads/dev", rules: ["require-verdict"] },
    ]),
    false,
    "a rule on a different ref never counts for main",
  );
  assert.equal(requireVerdictOnMain([]), false);
});

test("applyRequireVerdictOnMain adds a new buzz-protect tag for main when none exists", () => {
  const tags = applyRequireVerdictOnMain(
    [
      ["d", "x"],
      ["clone", "url"],
    ],
    true,
  );
  assert.deepEqual(tags, [
    ["d", "x"],
    ["clone", "url"],
    ["buzz-protect", "refs/heads/main", "require-verdict"],
  ]);
});

test("applyRequireVerdictOnMain adds the rule to an existing tag's rule list, in place", () => {
  const tags = applyRequireVerdictOnMain(
    [["buzz-protect", "refs/heads/main", "no-force-push"]],
    true,
  );
  assert.deepEqual(tags, [
    ["buzz-protect", "refs/heads/main", "no-force-push", "require-verdict"],
  ]);
});

test("applyRequireVerdictOnMain is idempotent when the rule is already present", () => {
  const tags = applyRequireVerdictOnMain(
    [["buzz-protect", "refs/heads/main", "require-verdict"]],
    true,
  );
  assert.deepEqual(tags, [
    ["buzz-protect", "refs/heads/main", "require-verdict"],
  ]);
});

test("applyRequireVerdictOnMain removes the rule but keeps the tag if other rules remain", () => {
  const tags = applyRequireVerdictOnMain(
    [["buzz-protect", "refs/heads/main", "require-verdict", "no-delete"]],
    false,
  );
  assert.deepEqual(tags, [["buzz-protect", "refs/heads/main", "no-delete"]]);
});

test("applyRequireVerdictOnMain drops the tag entirely once it carries no rules", () => {
  const tags = applyRequireVerdictOnMain(
    [
      ["buzz-protect", "refs/heads/main", "require-verdict"],
      ["d", "x"],
    ],
    false,
  );
  assert.deepEqual(tags, [["d", "x"]]);
});

test("applyRequireVerdictOnMain never touches a different ref's own tag", () => {
  const tags = applyRequireVerdictOnMain(
    [["buzz-protect", "refs/heads/dev", "no-delete"]],
    true,
  );
  assert.deepEqual(tags, [
    ["buzz-protect", "refs/heads/dev", "no-delete"],
    ["buzz-protect", "refs/heads/main", "require-verdict"],
  ]);
});

test("applyRequireVerdictOnMain never mutates its input array", () => {
  const input = [["buzz-protect", "refs/heads/main", "no-delete"]];
  const frozenInput = JSON.parse(JSON.stringify(input));
  applyRequireVerdictOnMain(input, true);
  assert.deepEqual(input, frozenInput);
});
