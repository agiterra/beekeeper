import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CODING_SESSION_ASSIGNMENT_INPUT_REFUSALS } from "../lib/codingSessionAssignmentInput.ts";
import { buildCodingSessionMissionTransactionRows } from "../lib/codingSessionMissionTransactionRows.ts";
import { CodingSessionMissionTransactionRow } from "./CodingSessionMissionTransactionRow.tsx";

/**
 * The assignment row is where a person finds out whether the seat that
 * verified something was standing on the commit under test. These pin the
 * sentences — including the one that refuses to let "established" be read as
 * "established before the seat woke".
 */

const FOUNDER = "f".repeat(64);
const VERIFIER = "1".repeat(64);
const COMMIT = "d698c7773".padEnd(40, "a");

function assignmentRow() {
  return buildCodingSessionMissionTransactionRows({
    transactions: [
      {
        sourceEventId: "a".repeat(64),
        type: "assignment",
        authorPubkey: FOUNDER,
        createdAt: 1_800_000_000,
        counterpartyPubkey: VERIFIER,
        parentEventId: null,
        summary: "Verify the landing",
        decision: null,
        requiredAction: null,
        fileCount: null,
        testCount: null,
        unseated: false,
        assigneeRole: "verifier",
        baseSha: COMMIT,
      },
    ],
    resolveActor: (pubkey) =>
      pubkey === FOUNDER
        ? { label: "Keystone", executionKey: "lead" }
        : { label: "Vera", executionKey: "verifier" },
    founderPubkey: FOUNDER,
    density: "live",
  })[0];
}

function render(verificationInput, onRetry) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionMissionTransactionRow, {
      onRetryVerificationInput: onRetry,
      row: assignmentRow(),
      verificationInput,
    }),
  );
}

test("an established input names the commit, the tree, and is not ordered against the wake", () => {
  const markup = render({
    kind: "established",
    commit: COMMIT,
    result: {
      path: "/Users/x/Code/repo-verifier-1",
      branch: "main",
      commit: COMMIT,
      remote: "origin",
      alreadyCurrent: false,
    },
  });
  assert.match(markup, /data-testid="coding-session-verification-input"/);
  assert.match(markup, /Verification input established: d698c77/);
  assert.match(markup, /in \/Users\/x\/Code\/repo-verifier-1/);
  assert.match(
    markup,
    /This is not ordered against the lead&#x27;s wake, so a turn may have started first\./,
  );
});

test("a tree that already held the commit says so, with the same ordering clause", () => {
  const markup = render({
    kind: "established",
    commit: COMMIT,
    result: {
      path: "/Users/x/Code/repo-verifier-1",
      branch: "main",
      commit: COMMIT,
      remote: "origin",
      alreadyCurrent: true,
    },
  });
  assert.match(markup, /Verification input already current: d698c77/);
  assert.match(markup, /not ordered against the lead&#x27;s wake/);
});

test("every refusal code produces its own plain sentence and a Try again", () => {
  const expected = {
    unrecorded_tree: /did not cut that seat&#x27;s worktree/,
    missing_tree: /no longer on disk/,
    dirty_tree:
      /2 uncommitted changes in the seat&#x27;s tree; nothing was discarded/,
    no_remote: /no remote to fetch the commit from/,
    ambiguous_remote: /more than one remote/,
    fetch_failed: /Fetching the commit from the seat&#x27;s remote failed/,
    unknown_commit: /The commit is not on the resolved remote\./,
    checkout_failed: /could not be moved onto the commit/,
    invalid_input: /could not read the request for that verification input/,
  };
  for (const code of CODING_SESSION_ASSIGNMENT_INPUT_REFUSALS) {
    const markup = render(
      {
        kind: "refused",
        commit: COMMIT,
        code,
        message: `host said ${code}`,
        detail: "raw host detail",
        changes: 2,
      },
      () => {},
    );
    assert.match(markup, /Verification input not established:/, code);
    assert.match(markup, expected[code], code);
    assert.match(
      markup,
      /data-testid="coding-session-verification-input-retry"/,
      code,
    );
    // The host's raw words stay under the disclosure, never in the headline.
    assert.match(markup, /<details[^>]*>.*raw host detail/s, code);
    const headline = markup.slice(
      markup.indexOf("verification-input-sentence"),
      markup.indexOf("<details"),
    );
    assert.equal(headline.includes("raw host detail"), false, code);
  }
});

test("a build without the command says so rather than showing nothing", () => {
  const markup = render(
    {
      kind: "unavailable",
      commit: COMMIT,
      message: "Command not found",
      detail: null,
    },
    () => {},
  );
  assert.match(markup, /this build cannot establish it/);
  assert.match(markup, /Command not found/);
});

test("an assignment naming no commit says so, quietly, and offers no retry", () => {
  const markup = render({ kind: "unnamed" }, () => {});
  assert.match(
    markup,
    /No verification input named: this assignment does not name a commit to verify, so nothing was established\./,
  );
  assert.equal(
    markup.includes("coding-session-verification-input-retry"),
    false,
  );
  assert.equal(markup.includes("text-amber-700"), false);
});

test("a recorded establishment reads as a record, not as a live check", () => {
  const markup = render(
    {
      kind: "recorded",
      commit: COMMIT,
      inner: {
        kind: "established",
        commit: COMMIT,
        result: {
          path: "/Users/x/Code/repo-verifier-1",
          branch: "main",
          commit: COMMIT,
          remote: "origin",
          alreadyCurrent: false,
        },
      },
      record: {},
    },
    () => {},
  );
  assert.match(markup, /Verification input established: d698c77/);
  assert.match(
    markup,
    /Recorded earlier on this computer, not checked again now\./,
  );
  assert.match(markup, /recorded: established/);
  // A recorded establishment is as quiet as a live one.
  assert.equal(markup.includes("text-amber-700"), false);
});

test("a record for another commit is shown as no attempt for this one", () => {
  const markup = render(
    { kind: "stale-record", commit: COMMIT, recordedCommit: "ecd4336f4bbbb" },
    () => {},
  );
  assert.match(
    markup,
    /No attempt is recorded for this commit: this computer&#x27;s last recorded attempt was for ecd4336, not d698c77\./,
  );
  assert.match(markup, /data-testid="coding-session-verification-input-retry"/);
  assert.equal(markup.includes("Verification input established"), false);
});

test("a row with nothing to say about an input renders no disclosure at all", () => {
  const markup = render(null);
  assert.equal(markup.includes("coding-session-verification-input"), false);
});
