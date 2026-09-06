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

test("discloses stale local data and marks every metadata claim unverified", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: {
        resolvedAt: 1_700_000_000_000,
        resolved: [{ role: "builder", coordinate, reason: null }],
        reported: [
          {
            channelId: "project-transport",
            generationId: "generation-4",
            label: "Build the snapshot",
            claimedByPubkey: MEMBER_PUBKEY,
            reportedAt: 1_700_000_001_000,
            coordinate,
            reason: null,
            comparison: "claims-match",
            provenance: "unverified-channel-metadata",
          },
        ],
      },
      resolvedError: "native resolver timed out",
      resolvedIsStale: true,
      reports: {
        isLoading: false,
        error: "one visible channel did not answer",
        authorityError: null,
      },
    }),
  );

  assert.match(html, /Checked at/);
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
  assert.match(html, /Claims same version/);
  assert.match(html, /Unverified channel metadata/);
  assert.match(html, /not verified that a commissioned provider authored/);
  assert.doesNotMatch(html, />Same version</);
  assert.doesNotMatch(html, /Reported by sessions/);
  assert.doesNotMatch(html, /universally adopted|every machine is current/);
});

test("does not turn a failed session discovery into a zero-report claim", () => {
  const html = renderToStaticMarkup(
    React.createElement(RolePackSnapshots, {
      snapshots: { resolvedAt: null, resolved: [], reported: [] },
      resolvedError: null,
      resolvedIsStale: false,
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
