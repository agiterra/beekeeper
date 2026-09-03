import assert from "node:assert/strict";
import test from "node:test";

import {
  applyRequireVerdictOnMain,
  applyRuleRecordRows,
  parseProtectionTags,
  PROTECTION_RULE_CLEAR,
  repositoryFounders,
  requireVerdictFromDecisions,
  requireVerdictOnMain,
  resolveProtection,
  ruleRecordDTag,
  ruleRecordLayer,
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

// ── lane L26: a founder-signed rule record ──────────────────────────────

const L26_OWNER = "1".repeat(64);
const L26_CO_FOUNDER = "2".repeat(64);
const L26_STRANGER = "3".repeat(64);

function l26Announcement(tags = [], createdAt = 1000) {
  return {
    id: "a".repeat(64),
    pubkey: L26_OWNER,
    created_at: createdAt,
    tags,
  };
}

function l26Record({
  pubkey = L26_CO_FOUNDER,
  rows = [],
  createdAt = 2000,
  dtag = ruleRecordDTag(L26_OWNER, "beekeeper"),
  schema = "buzz-repo-protection/v1",
  id = "b".repeat(64),
} = {}) {
  return {
    id,
    kind: 30625,
    pubkey,
    created_at: createdAt,
    content: JSON.stringify({ schema }),
    tags: [["d", dtag], ...rows.map((row) => ["buzz-protect", ...row])],
  };
}

const L26_FOUNDERS = repositoryFounders({
  owner: L26_OWNER,
  maintainers: [L26_CO_FOUNDER],
});

test("finding 31: rules signed before the rule-record kind existed still govern", () => {
  const decisions = resolveProtection({
    announcement: l26Announcement([
      ["buzz-protect", "refs/heads/main", "require-verdict"],
    ]),
    records: [],
  });
  assert.equal(decisions.length, 1);
  assert.equal(decisions[0].source.record, "announcement");
  assert.equal(decisions[0].source.signedBy, L26_OWNER);
  assert.equal(requireVerdictFromDecisions(decisions), true);
});

test("a co-founder's newer record wins the pattern the announcement set", () => {
  const layer = ruleRecordLayer(
    l26Record({ rows: [["refs/heads/main", "no-delete"]] }),
    L26_OWNER,
    "beekeeper",
    L26_FOUNDERS,
  );
  const decisions = resolveProtection({
    announcement: l26Announcement([
      ["buzz-protect", "refs/heads/main", "require-verdict"],
    ]),
    records: [layer],
  });
  assert.equal(requireVerdictFromDecisions(decisions), false);
  assert.equal(decisions[0].source.record, "rule-record");
  assert.equal(decisions[0].source.signedBy, L26_CO_FOUNDER);
});

test("a founder's clear removes the rule and says so, rather than showing it", () => {
  const layer = ruleRecordLayer(
    l26Record({ rows: [["refs/heads/main", PROTECTION_RULE_CLEAR]] }),
    L26_OWNER,
    "beekeeper",
    L26_FOUNDERS,
  );
  const decisions = resolveProtection({
    announcement: l26Announcement([
      ["buzz-protect", "refs/heads/main", "require-verdict"],
    ]),
    records: [layer],
  });
  assert.equal(decisions[0].cleared, true);
  assert.equal(requireVerdictFromDecisions(decisions), false);
});

test("a record only supersedes the patterns it names", () => {
  const layer = ruleRecordLayer(
    l26Record({ rows: [["refs/heads/main", "no-force-push"]] }),
    L26_OWNER,
    "beekeeper",
    L26_FOUNDERS,
  );
  const decisions = resolveProtection({
    announcement: l26Announcement([
      ["buzz-protect", "refs/heads/main", "require-verdict"],
      ["buzz-protect", "refs/tags/*", "no-delete"],
    ]),
    records: [layer],
  });
  const tags = decisions.find((d) => d.refPattern === "refs/tags/*");
  assert.equal(tags.source.record, "announcement");
  assert.deepEqual([...tags.rules], ["no-delete"]);
});

test("only a founder's record is a layer, and only for this repository", () => {
  assert.equal(
    ruleRecordLayer(
      l26Record({
        pubkey: L26_STRANGER,
        rows: [["refs/heads/main", "no-delete"]],
      }),
      L26_OWNER,
      "beekeeper",
      L26_FOUNDERS,
    ),
    null,
    "a stranger's record governs nothing, exactly as at the relay's gate",
  );
  assert.equal(
    ruleRecordLayer(
      l26Record({ dtag: ruleRecordDTag(L26_OWNER, "other") }),
      L26_OWNER,
      "beekeeper",
      L26_FOUNDERS,
    ),
    null,
    "a record addressing another repository is not a layer for this one",
  );
  assert.equal(
    ruleRecordLayer(
      l26Record({ schema: "something-else/v1" }),
      L26_OWNER,
      "beekeeper",
      L26_FOUNDERS,
    ),
    null,
    "a foreign schema is not a rule record",
  );
});

test("an equal created_at breaks on the event id, not on input order", () => {
  const left = ruleRecordLayer(
    l26Record({ id: "c".repeat(64), rows: [["refs/heads/main", "no-delete"]] }),
    L26_OWNER,
    "beekeeper",
    L26_FOUNDERS,
  );
  const right = ruleRecordLayer(
    l26Record({
      id: "d".repeat(64),
      pubkey: L26_OWNER,
      rows: [["refs/heads/main", "require-verdict"]],
    }),
    L26_OWNER,
    "beekeeper",
    L26_FOUNDERS,
  );
  const forward = resolveProtection({
    announcement: l26Announcement(),
    records: [left, right],
  });
  const backward = resolveProtection({
    announcement: l26Announcement(),
    records: [right, left],
  });
  assert.deepEqual(forward, backward);
  assert.equal(forward[0].source.eventId, "d".repeat(64));
});

test("the toggle's rows keep a founder's other patterns and clear on off", () => {
  const current = [
    ["d", ruleRecordDTag(L26_OWNER, "beekeeper")],
    ["buzz-protect", "refs/tags/*", "no-delete"],
    ["buzz-protect", "refs/heads/main", "require-verdict"],
  ];
  assert.deepEqual(applyRuleRecordRows(current, true), [
    ["buzz-protect", "refs/tags/*", "no-delete"],
    ["buzz-protect", "refs/heads/main", "require-verdict"],
  ]);
  assert.deepEqual(applyRuleRecordRows(current, false), [
    ["buzz-protect", "refs/tags/*", "no-delete"],
    ["buzz-protect", "refs/heads/main", PROTECTION_RULE_CLEAR],
  ]);
});

test("the founder set is the signer plus maintainers plus roster owners", () => {
  assert.deepEqual(
    repositoryFounders(
      { owner: L26_OWNER.toUpperCase(), maintainers: [L26_CO_FOUNDER, "nope"] },
      [L26_STRANGER, L26_OWNER],
    ),
    [L26_OWNER, L26_CO_FOUNDER, L26_STRANGER],
    "lower-cased, deduped, signer first; a malformed key is not a founder",
  );
});
