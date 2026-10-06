// Contract tests for the pre-push floor's pass stamp (scripts/pre-push-floor-stamp.mjs).
//
// The stamp is the only thing standing between "the wrapper already ran the
// floor" and "the hook silently trusts a stale or unrelated pass" — so every
// case here is a way that trust could be misplaced: a stamp from a different
// commit, a stamp for a different set of changed files, or a stamp old enough
// that the floor's own answer might no longer hold.
//
// Run: node --test scripts/pre-push-floor-stamp.test.mjs

import assert from "node:assert/strict";
import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import test from "node:test";

import {
  consumeStamp,
  readValidStamp,
  scopeHash,
  stampPath,
  writeStamp,
} from "./pre-push-floor-stamp.mjs";

function withTempGitDir(fn) {
  const dir = mkdtempSync(join(tmpdir(), "pre-push-floor-stamp-test-"));
  try {
    fn(dir);
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
}

test("scopeHash is order-independent and stable", () => {
  const a = scopeHash(["scripts/foo.mjs", "crates/beekeeper-core/src/lib.rs"]);
  const b = scopeHash(["crates/beekeeper-core/src/lib.rs", "scripts/foo.mjs"]);
  assert.equal(a, b);
});

test("scopeHash differs for a different file set", () => {
  const a = scopeHash(["scripts/foo.mjs"]);
  const b = scopeHash(["scripts/foo.mjs", "scripts/bar.mjs"]);
  assert.notEqual(a, b);
});

test("a fresh, matching stamp is valid", () => {
  withTempGitDir((gitDir) => {
    const hash = scopeHash(["crates/beekeeper-core/src/lib.rs"]);
    const now = 1_000_000;
    writeStamp({ gitDir, sha: "abc123", scopeHash: hash, now, ttlSeconds: 600 });
    const result = readValidStamp({ gitDir, sha: "abc123", scopeHash: hash, now: now + 5_000 });
    assert.equal(result.valid, true);
    assert.equal(result.ageSeconds, 5);
  });
});

test("no stamp on disk reads as missing", () => {
  withTempGitDir((gitDir) => {
    const hash = scopeHash(["scripts/foo.mjs"]);
    const result = readValidStamp({ gitDir, sha: "abc123", scopeHash: hash });
    assert.deepEqual(result, { valid: false, reason: "missing" });
  });
});

test("an expired stamp is refused even with a matching sha and scope", () => {
  withTempGitDir((gitDir) => {
    const hash = scopeHash(["scripts/foo.mjs"]);
    const now = 1_000_000;
    writeStamp({ gitDir, sha: "abc123", scopeHash: hash, now, ttlSeconds: 60 });
    const result = readValidStamp({ gitDir, sha: "abc123", scopeHash: hash, now: now + 61_000 });
    assert.deepEqual(result, { valid: false, reason: "expired" });
  });
});

test("exactly at the ttl boundary is still valid; one ms over is expired", () => {
  withTempGitDir((gitDir) => {
    const hash = scopeHash(["scripts/foo.mjs"]);
    const now = 1_000_000;
    writeStamp({ gitDir, sha: "abc123", scopeHash: hash, now, ttlSeconds: 60 });
    assert.equal(readValidStamp({ gitDir, sha: "abc123", scopeHash: hash, now: now + 60_000 }).valid, true);
    assert.deepEqual(readValidStamp({ gitDir, sha: "abc123", scopeHash: hash, now: now + 60_001 }), {
      valid: false,
      reason: "expired",
    });
  });
});

test("a stamp for a different sha is never honoured, fresh or not", () => {
  withTempGitDir((gitDir) => {
    const hash = scopeHash(["scripts/foo.mjs"]);
    const now = 1_000_000;
    writeStamp({ gitDir, sha: "abc123", scopeHash: hash, now, ttlSeconds: 600 });
    const result = readValidStamp({ gitDir, sha: "def456", scopeHash: hash, now: now + 1_000 });
    assert.deepEqual(result, { valid: false, reason: "sha-mismatch" });
  });
});

test("a stamp for a different scope is never honoured, even for the same sha", () => {
  withTempGitDir((gitDir) => {
    const hashA = scopeHash(["scripts/foo.mjs"]);
    const hashB = scopeHash(["scripts/foo.mjs", "crates/beekeeper-core/src/lib.rs"]);
    const now = 1_000_000;
    writeStamp({ gitDir, sha: "abc123", scopeHash: hashA, now, ttlSeconds: 600 });
    const result = readValidStamp({ gitDir, sha: "abc123", scopeHash: hashB, now: now + 1_000 });
    assert.deepEqual(result, { valid: false, reason: "scope-mismatch" });
  });
});

test("sha-mismatch is reported even when the stamp is also expired", () => {
  withTempGitDir((gitDir) => {
    const hash = scopeHash(["scripts/foo.mjs"]);
    const now = 1_000_000;
    writeStamp({ gitDir, sha: "abc123", scopeHash: hash, now, ttlSeconds: 1 });
    const result = readValidStamp({ gitDir, sha: "def456", scopeHash: hash, now: now + 1_000_000 });
    assert.deepEqual(result, { valid: false, reason: "sha-mismatch" });
  });
});

test("malformed JSON on disk reads as malformed, not a crash", () => {
  withTempGitDir((gitDir) => {
    writeFileSync(stampPath(gitDir), "not json", "utf8");
    const hash = scopeHash(["scripts/foo.mjs"]);
    const result = readValidStamp({ gitDir, sha: "abc123", scopeHash: hash });
    assert.deepEqual(result, { valid: false, reason: "malformed" });
  });
});

test("a stamp missing a required field reads as malformed", () => {
  withTempGitDir((gitDir) => {
    writeFileSync(stampPath(gitDir), JSON.stringify({ sha: "abc123" }), "utf8");
    const hash = scopeHash(["scripts/foo.mjs"]);
    const result = readValidStamp({ gitDir, sha: "abc123", scopeHash: hash });
    assert.deepEqual(result, { valid: false, reason: "malformed" });
  });
});

test("consumeStamp removes a stamp so it cannot answer a later push", () => {
  withTempGitDir((gitDir) => {
    const hash = scopeHash(["scripts/foo.mjs"]);
    writeStamp({ gitDir, sha: "abc123", scopeHash: hash, now: 1_000_000, ttlSeconds: 600 });
    consumeStamp(gitDir);
    const result = readValidStamp({ gitDir, sha: "abc123", scopeHash: hash, now: 1_000_500 });
    assert.deepEqual(result, { valid: false, reason: "missing" });
  });
});

test("consumeStamp on an already-absent stamp does not throw", () => {
  withTempGitDir((gitDir) => {
    assert.doesNotThrow(() => consumeStamp(gitDir));
  });
});
