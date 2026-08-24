import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import {
  allowedLineCount,
  countLines,
  evaluateFileSize,
  parseChangedFiles,
  resolveBaseRef,
} from "./check-file-sizes-core.mjs";

function git(repo, ...args) {
  // These fixture repositories inherit both hook configuration and Git's
  // repository-local environment when this test runs from pre-push. Isolate
  // them completely so fixture commits cannot recurse into the real checkout.
  const env = Object.fromEntries(
    Object.entries(process.env).filter(([key]) => !key.startsWith("GIT_")),
  );
  return execFileSync("git", ["-c", "core.hooksPath=/dev/null", ...args], {
    cwd: repo,
    encoding: "utf8",
    env,
  }).trim();
}

test("local base resolution uses the branch merge-base and fails without origin/main", () => {
  const repo = mkdtempSync(path.join(tmpdir(), "file-size-base-"));
  git(repo, "init", "-b", "main");
  git(repo, "config", "user.name", "Test");
  git(repo, "config", "user.email", "test@example.com");
  git(repo, "commit", "--allow-empty", "-m", "base");
  git(repo, "remote", "add", "origin", repo);
  git(repo, "fetch", "origin", "main:refs/remotes/origin/main");
  const base = git(repo, "rev-parse", "HEAD");
  git(repo, "switch", "-c", "feature");
  git(repo, "commit", "--allow-empty", "-m", "first branch commit");
  git(repo, "commit", "--allow-empty", "-m", "second branch commit");

  assert.equal(resolveBaseRef(repo, {}), base);
  git(repo, "update-ref", "-d", "refs/remotes/origin/main");
  assert.throws(
    () => resolveBaseRef(repo, {}),
    /Could not resolve the file-size base from any of/,
  );
});

test("base resolution follows what main tracks, not the name 'origin'", () => {
  // The 2026-08-24 breakage: `origin` was repointed at the relay's own git
  // hosting and GitHub became `upstream`. A hard-coded origin/main stopped
  // resolving on every branch at once — and since pre-push runs this, that
  // blocked every push in the checkout. Whatever main tracks is the base.
  const repo = mkdtempSync(path.join(tmpdir(), "file-size-track-"));
  git(repo, "init", "-b", "main");
  git(repo, "config", "user.name", "Test");
  git(repo, "config", "user.email", "test@example.com");
  git(repo, "commit", "--allow-empty", "-m", "base");
  git(repo, "remote", "add", "upstream", repo);
  git(repo, "fetch", "upstream", "main:refs/remotes/upstream/main");
  git(repo, "branch", "--set-upstream-to=upstream/main", "main");
  const base = git(repo, "rev-parse", "HEAD");
  git(repo, "switch", "-c", "feature");
  git(repo, "commit", "--allow-empty", "-m", "branch commit");

  // No origin/main exists at all here — the old implementation threw.
  assert.equal(resolveBaseRef(repo, {}), base);
});

test("upstream/main is the last resort when main tracks nothing", () => {
  const repo = mkdtempSync(path.join(tmpdir(), "file-size-fallback-"));
  git(repo, "init", "-b", "main");
  git(repo, "config", "user.name", "Test");
  git(repo, "config", "user.email", "test@example.com");
  git(repo, "commit", "--allow-empty", "-m", "base");
  git(repo, "remote", "add", "upstream", repo);
  git(repo, "fetch", "upstream", "main:refs/remotes/upstream/main");
  const base = git(repo, "rev-parse", "HEAD");
  git(repo, "switch", "-c", "feature");
  git(repo, "commit", "--allow-empty", "-m", "branch commit");

  // main has no configured upstream and there is no origin/main; the chain
  // must still find upstream/main rather than give up.
  assert.equal(resolveBaseRef(repo, {}), base);
});

test("an explicit CHECK_FILE_SIZES_BASE still wins over every candidate", () => {
  const repo = mkdtempSync(path.join(tmpdir(), "file-size-explicit-"));
  git(repo, "init", "-b", "main");
  assert.equal(
    resolveBaseRef(repo, { CHECK_FILE_SIZES_BASE: "deadbeef" }),
    "deadbeef",
  );
});

test("counts empty, LF, and CRLF content with the existing semantics", () => {
  assert.equal(countLines(""), 0);
  assert.equal(countLines("one\n"), 2);
  assert.equal(countLines("one\r\ntwo"), 2);
});

test("new files use the configured ceiling", () => {
  assert.equal(allowedLineCount(null, 1000), 1000);
  assert.deepEqual(
    evaluateFileSize({ baseLines: null, candidateLines: 1000, maxLines: 1000 }),
    {
      limit: 1000,
      violates: false,
    },
  );
  assert.equal(
    evaluateFileSize({ baseLines: null, candidateLines: 1001, maxLines: 1000 })
      .violates,
    true,
  );
});

test("a compliant file may not cross the ceiling", () => {
  assert.equal(
    evaluateFileSize({ baseLines: 996, candidateLines: 1000, maxLines: 1000 })
      .violates,
    false,
  );
  assert.equal(
    evaluateFileSize({ baseLines: 996, candidateLines: 1003, maxLines: 1000 })
      .violates,
    true,
  );
});

test("parses modifications, deletions, and renames from Git's NUL format", () => {
  assert.deepEqual(
    parseChangedFiles(
      "M\0desktop/src/a.ts\0D\0desktop/src/b.ts\0R100\0desktop/src/old.ts\0desktop/src/new.ts\0",
    ),
    [
      { status: "M", path: "desktop/src/a.ts" },
      { status: "D", path: "desktop/src/b.ts" },
      {
        status: "R",
        oldPath: "desktop/src/old.ts",
        path: "desktop/src/new.ts",
      },
    ],
  );
});

test("an inherited oversized file may hold or shrink but not grow", () => {
  assert.equal(allowedLineCount(1026, 1000), 1026);
  assert.equal(
    evaluateFileSize({ baseLines: 1026, candidateLines: 1026, maxLines: 1000 })
      .violates,
    false,
  );
  assert.equal(
    evaluateFileSize({ baseLines: 1026, candidateLines: 1001, maxLines: 1000 })
      .violates,
    false,
  );
  assert.equal(
    evaluateFileSize({ baseLines: 1026, candidateLines: 1027, maxLines: 1000 })
      .violates,
    true,
  );
});
