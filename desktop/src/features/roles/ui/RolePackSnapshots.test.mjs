import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { RolePackSnapshots } from "./RolePackSnapshots.tsx";

const coordinate = {
  repo: "30617:owner:packs",
  sha: "a".repeat(40),
  role: "builder",
  path: "personas/roles/builder",
};
const MEMBER_PUBKEY = "f".repeat(64);

function reportedRow(overrides = {}) {
  return {
    channelId: "project-transport",
    generationId: "generation-4",
    label: "Build the snapshot",
    claimedByPubkey: MEMBER_PUBKEY,
    reportedAt: 1_700_000_001_000,
    coordinate,
    reason: null,
    relation: "current",
    behind: null,
    ahead: null,
    status: "running",
    ageSeconds: 300,
    adoption: "none",
    provenance: "unverified-channel-metadata",
    ...overrides,
  };
}

test("states the checkout commit and when git answered, and marks every metadata claim unverified", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: 1_700_000_000_000,
        resolved: [{ role: "builder", coordinate, reason: null }],
        reported: [reportedRow({ relation: "current" })],
        comparison: {
          currentSha: coordinate.sha,
          comparedAt: 1_700_000_000_900,
          reason: null,
        },
      },
      resolvedError: "native resolver timed out",
      resolvedIsStale: true,
      revisionsError: null,
      reports: {
        isLoading: false,
        error: "one visible channel did not answer",
        authorityError: null,
      },
    }),
  );

  assert.match(html, /Checked at/);
  assert.match(html, /On aaaaaaaa · git answered at/);
  assert.match(
    html,
    /last refresh failed; rows below are from the earlier check/,
  );
  assert.match(
    html,
    /Metadata claims may be incomplete: one visible channel did not answer/,
  );
  assert.match(html, /Unverified metadata/);
  assert.match(html, /from <span[^>]+>ffffffff…ffff<\/span>/);
  assert.match(html, /Same revision as this machine/);
  assert.match(html, /reported 5m ago/);
  assert.match(html, /Unverified channel metadata/);
  assert.match(html, /not verified that a commissioned provider authored/);
  // A current row never carries the adoption sentence.
  assert.doesNotMatch(html, /Keeps this revision until its next launch/);
  assert.doesNotMatch(html, /Reported by sessions/);
  assert.doesNotMatch(html, /universally adopted|every machine is current/);
});

test("discloses a failed revision comparison instead of guessing a relation", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: 1_700_000_000_000,
        resolved: [{ role: "builder", coordinate, reason: null }],
        reported: [
          reportedRow({
            relation: "unknown-here",
            status: "running",
            adoption: "keeps-until-next-generation",
          }),
        ],
        comparison: null,
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: "git exited 1",
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  assert.match(html, /Revision comparison unavailable: git exited 1/);
  assert.match(html, /Revision unknown to this machine/);
  assert.match(html, /Keeps this revision until its next launch or resume/);
  // Never render a row as current while the comparison failed.
  assert.doesNotMatch(html, /Same revision as this machine/);
});

test("shows the earlier/later distance and the terminal status without an adoption sentence", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [
          reportedRow({
            generationId: "generation-earlier",
            relation: "earlier",
            behind: 4,
            status: "completed",
            adoption: "none",
          }),
        ],
        comparison: null,
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: { isLoading: false, error: null, authorityError: null },
    }),
  );

  assert.match(html, /Earlier revision · 4 behind this machine/);
  assert.match(html, />completed</);
  assert.doesNotMatch(html, /Keeps this revision/);
});

test("does not turn a failed session discovery into a zero-report claim", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: null,
        resolved: [],
        reported: [],
        comparison: null,
      },
      resolvedError: null,
      resolvedIsStale: false,
      revisionsError: null,
      reports: {
        isLoading: false,
        error: "could not read this project's channels",
        authorityError: null,
      },
    }),
  );

  assert.match(html, /Metadata claims may be incomplete/);
  assert.match(html, /No complete metadata-claim list is available/);
  assert.doesNotMatch(
    html,
    /No role-version metadata claims are visible for this project/,
  );
});
