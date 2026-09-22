#!/usr/bin/env node
// The pre-push floor's pass stamp: lets a wrapper that ran the floor BEFORE
// git opens its connection tell the pre-push hook "this exact push already
// passed" so the hook can return immediately.
//
// Why this exists (ledger 178(n)). `git push` mints its NIP-98 credential at
// ref discovery, before pre-push hooks run, and reuses that one credential
// for the whole push (confirmed empirically in a throwaway clone: the
// credential helper's `get` fires exactly once per `git push`, and the same
// Authorization value is replayed on the retried GET and the receive-pack
// POST — see plans/archive/2026-09-20-pre-push-floor-stamp.md). The relay's
// token window is +-900s. A crate-touching floor can run ~20 minutes, so by
// the time the hook finishes and git uploads the pack, the token it minted at
// the start is long expired and the push fails `HTTP 401` with every check
// green. `scripts/push-with-floor.sh` runs the floor first, writes this
// stamp, and only then calls `git push` — so the credential is seconds old
// when it is used, exactly as a docs-only push already is today.
//
// The stamp is trust-on-exact-match, not a general cache: it names the sha it
// was earned for and the exact scope (the changed-file set) the floor ran
// against, and it expires in minutes. It is NEVER honoured for a different
// sha or a different scope, and pushing without the wrapper still runs the
// floor in full, as it does today.
//
// Every function here is pure except `main()`, so the fresh/expired/
// wrong-sha/wrong-scope cases are tested without a git push:
// `node --test scripts/pre-push-floor-stamp.test.mjs`.

import { createHash } from "node:crypto";
import { readFileSync, writeFileSync, unlinkSync, realpathSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

export const STAMP_FILENAME = "buzz-pre-push-floor-stamp.json";

export const DEFAULT_TTL_SECONDS = 600; // 10 minutes: comfortably inside the
// relay's +-900s token window even after the wrapper's own `git push` setup
// cost, and short enough that a stamp cannot outlive the session that earned
// it.

/** Path to the stamp file inside a given `.git` directory (the per-worktree
 * administrative directory `git rev-parse --git-dir` reports, not the shared
 * common dir — a stamp earned in one worktree must not answer for another). */
export function stampPath(gitDir) {
  return join(gitDir, STAMP_FILENAME);
}

/** Deterministic scope fingerprint over the exact set of changed paths the
 * floor ran against. Order-independent so the caller doesn't have to agree on
 * sort order with whatever produced the list. */
export function scopeHash(changedPaths) {
  const normalized = [...new Set(changedPaths.map((p) => p.trim()).filter(Boolean))].sort();
  return createHash("sha256").update(normalized.join("\n")).digest("hex");
}

/**
 * Write a stamp asserting "the floor passed for `sha` at this `scopeHash`,
 * as of `now`". Overwrites any previous stamp — only one push is ever being
 * prepared at a time per worktree.
 */
export function writeStamp({ gitDir, sha, scopeHash: hash, now = Date.now(), ttlSeconds = DEFAULT_TTL_SECONDS }) {
  if (!gitDir) throw new Error("writeStamp: gitDir is required");
  if (!sha) throw new Error("writeStamp: sha is required");
  if (!hash) throw new Error("writeStamp: scopeHash is required");
  const body = JSON.stringify({ sha, scopeHash: hash, createdAtMs: now, ttlSeconds }, null, 2);
  writeFileSync(stampPath(gitDir), `${body}\n`, "utf8");
}

/** Remove a stamp, if present. A stamp is single-use: the hook consumes it
 * (valid or not) so a later push in the same worktree can never accidentally
 * reuse it for a different commit that happens to collide on scope. */
export function consumeStamp(gitDir) {
  try {
    unlinkSync(stampPath(gitDir));
  } catch {
    // Nothing to remove is not an error.
  }
}

/**
 * Check whether a fresh, matching stamp exists. Returns `{ valid: true }` or
 * `{ valid: false, reason }` naming exactly why not — "missing", "malformed",
 * "expired", "sha-mismatch", or "scope-mismatch" — so a caller can print an
 * honest line instead of silently falling back to running the floor.
 */
export function readValidStamp({ gitDir, sha, scopeHash: hash, now = Date.now() }) {
  let raw;
  try {
    raw = readFileSync(stampPath(gitDir), "utf8");
  } catch {
    return { valid: false, reason: "missing" };
  }

  let parsed;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return { valid: false, reason: "malformed" };
  }
  const { sha: stampedSha, scopeHash: stampedScope, createdAtMs, ttlSeconds } = parsed;
  if (
    typeof stampedSha !== "string" ||
    typeof stampedScope !== "string" ||
    typeof createdAtMs !== "number" ||
    typeof ttlSeconds !== "number"
  ) {
    return { valid: false, reason: "malformed" };
  }

  // Identity checks before the clock: a wrong-sha or wrong-scope stamp is
  // never honoured regardless of age, so an expired-but-also-wrong-sha stamp
  // is reported for the reason that would still bite even inside the window.
  if (stampedSha !== sha) {
    return { valid: false, reason: "sha-mismatch" };
  }
  if (stampedScope !== hash) {
    return { valid: false, reason: "scope-mismatch" };
  }
  const ageMs = now - createdAtMs;
  if (ageMs < 0 || ageMs > ttlSeconds * 1000) {
    return { valid: false, reason: "expired" };
  }
  return { valid: true, ageSeconds: Math.round(ageMs / 1000) };
}

// ---- CLI -------------------------------------------------------------
// `write` and `check` are the only two operations a shell script needs:
//   node pre-push-floor-stamp.mjs write --git-dir <dir> --sha <sha> --scope-file <path> [--ttl <secs>]
//   node pre-push-floor-stamp.mjs check --git-dir <dir> --sha <sha> --scope-file <path>
//   node pre-push-floor-stamp.mjs consume --git-dir <dir>
// `check` prints "valid <ageSeconds>" or "invalid <reason>" on stdout and
// exits 0/1 so bash can branch on the exit code alone.
function parseArgs(argv) {
  const args = {};
  for (let i = 0; i < argv.length; i += 2) {
    const key = argv[i]?.replace(/^--/, "");
    if (!key) continue;
    args[key] = argv[i + 1];
  }
  return args;
}

function readScopeFile(path) {
  if (!path) return [];
  try {
    return readFileSync(path, "utf8").split("\n");
  } catch {
    return [];
  }
}

function main(argv) {
  const [op, ...rest] = argv;
  const args = parseArgs(rest);
  const gitDir = args["git-dir"];
  const sha = args.sha;
  const hash = scopeHash(readScopeFile(args["scope-file"]));
  const ttl = args.ttl ? Number(args.ttl) : DEFAULT_TTL_SECONDS;

  if (op === "write") {
    writeStamp({ gitDir, sha, scopeHash: hash, ttlSeconds: ttl });
    console.log(`wrote stamp for ${sha} (ttl ${ttl}s)`);
    return 0;
  }
  if (op === "check") {
    const result = readValidStamp({ gitDir, sha, scopeHash: hash });
    if (result.valid) {
      console.log(`valid ${result.ageSeconds}`);
      return 0;
    }
    console.log(`invalid ${result.reason}`);
    return 1;
  }
  if (op === "consume") {
    consumeStamp(gitDir);
    return 0;
  }
  console.error(`usage: pre-push-floor-stamp.mjs <write|check|consume> --git-dir <dir> [--sha <sha>] [--scope-file <path>] [--ttl <secs>]`);
  return 2;
}

// `import.meta.url` reports the REALPATH of this file (Node resolves
// symlinks there), while `process.argv[1]` keeps whatever path invoked it —
// on macOS, `/tmp` is itself a symlink to `/private/tmp`, so a naive string
// comparison silently fails under `/tmp` and this CLI never runs, falling
// through to a bare, misleadingly-successful exit. Resolve both sides before
// comparing so a symlinked invocation is still recognised as this file.
function isMain() {
  if (!process.argv[1]) return false;
  try {
    return realpathSync(process.argv[1]) === fileURLToPath(import.meta.url);
  } catch {
    return false;
  }
}

if (isMain()) {
  process.exit(main(process.argv.slice(2)));
}
