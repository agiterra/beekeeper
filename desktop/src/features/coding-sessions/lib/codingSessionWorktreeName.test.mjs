import assert from "node:assert/strict";
import test from "node:test";

import {
  MAX_CODING_SESSION_WORKTREE_SLUG_LENGTH,
  codingSessionLeadWorktreeName,
  codingSessionWorktreeSlug,
} from "./codingSessionWorktreeName.ts";

// These cases mirror `worktree_tests.rs` one for one. The preview and the
// directory that gets created must agree, and the only way they can is if
// both slug functions answer identically for the same input.

test("a session name becomes a hyphenated slug", () => {
  assert.equal(
    codingSessionWorktreeSlug("Improve Coding Session Creation"),
    "improve-coding-session-creation",
  );
});

test("runs of separators collapse to one hyphen", () => {
  assert.equal(
    codingSessionWorktreeSlug("fix   the --- push, timeout!"),
    "fix-the-push-timeout",
  );
});

test("leading and trailing separators are dropped", () => {
  assert.equal(codingSessionWorktreeSlug("  -- hello --  "), "hello");
});

test("a name with nothing to slug yields nothing", () => {
  for (const name of ["", "   ", "!!! ---", "日本語"]) {
    assert.equal(codingSessionWorktreeSlug(name), "");
  }
});

test("truncation never leaves a trailing hyphen", () => {
  const long = "a".repeat(MAX_CODING_SESSION_WORKTREE_SLUG_LENGTH + 10);
  assert.equal(
    codingSessionWorktreeSlug(long).length,
    MAX_CODING_SESSION_WORKTREE_SLUG_LENGTH,
  );

  const onSeparator = `${"b".repeat(
    MAX_CODING_SESSION_WORKTREE_SLUG_LENGTH - 1,
  )} tail`;
  assert.ok(!codingSessionWorktreeSlug(onSeparator).endsWith("-"));
});

test("a slug never starts with a hyphen", () => {
  // A leading hyphen would let a name reach git as an option.
  for (const name of ["--force", "-b other", " - dash"]) {
    assert.ok(!codingSessionWorktreeSlug(name).startsWith("-"));
  }
});

/**
 * Item 87(d): the Team tab ran the lead in the checkout it named — the
 * operator's own — while the seats it hired got worktrees. The lead's tree is
 * named for the session and the seat, so a directory listing says which is
 * which.
 */
test("the lead's worktree is named for the session and the seat", () => {
  assert.equal(codingSessionLeadWorktreeName("UI"), "ui-lead");
  assert.equal(
    codingSessionLeadWorktreeName("Front door integration"),
    "front-door-integration-lead",
  );
  // Nothing addressable survives, so nothing is suggested — `-lead` alone
  // would name a directory after no session at all.
  assert.equal(codingSessionLeadWorktreeName("   "), "");
  assert.equal(codingSessionLeadWorktreeName("…"), "");
});
