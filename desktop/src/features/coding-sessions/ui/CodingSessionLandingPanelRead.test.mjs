/**
 * SV-24: Landed's `main` read never waits on an unrelated failure. When the
 * land rule captured no announcement and has stopped asking, Landing reads
 * the announcement itself — genesis or not.
 */
import assert from "node:assert/strict";
import test from "node:test";

import { codingSessionLandingReadsRepositoryDirectly } from "./CodingSessionLandingPanelRead.ts";

const REPO_REF = `30617:${"ab".repeat(32)}:buzz`;
const ANNOUNCEMENT = {
  ownerPubkey: "ab".repeat(32),
  protectionTags: [["clone", "https://example.invalid/buzz.git"]],
  ruleRecords: [],
  projectOwnerPubkeys: [],
};

function extension(rule, overrides = {}) {
  return { rule, repository: null, repoRef: REPO_REF, ...overrides };
}

test("a failed mission-evidence read still lets Landed read main", () => {
  assert.equal(
    codingSessionLandingReadsRepositoryDirectly(
      extension({
        state: "not-read",
        reason: "This mission's evidence was not read: boom",
      }),
    ),
    true,
  );
});

test("a rule that read without reaching its repository seam reads directly", () => {
  assert.equal(
    codingSessionLandingReadsRepositoryDirectly(extension({ state: "failed" })),
    true,
  );
  assert.equal(
    codingSessionLandingReadsRepositoryDirectly(
      extension({
        state: "read",
        land: { state: "ready", headSha: null },
        newestVerdict: null,
      }),
    ),
    true,
  );
});

test("no genesis (rule never asked) reads directly", () => {
  assert.equal(
    codingSessionLandingReadsRepositoryDirectly(
      extension({
        state: "not-read",
        reason:
          "This session has no genesis, so there is no mission verdict or land rule to read.",
      }),
    ),
    true,
  );
});

test("a rule still asking, a captured announcement, or no repository: no direct read", () => {
  assert.equal(
    codingSessionLandingReadsRepositoryDirectly(extension({ state: "asking" })),
    false,
  );
  assert.equal(
    codingSessionLandingReadsRepositoryDirectly(
      extension({ state: "failed" }, { repository: ANNOUNCEMENT }),
    ),
    false,
  );
  assert.equal(
    codingSessionLandingReadsRepositoryDirectly(
      extension({ state: "failed" }, { repoRef: null }),
    ),
    false,
  );
  assert.equal(codingSessionLandingReadsRepositoryDirectly(null), false);
});
