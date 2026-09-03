import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  codingSessionMissionLandModel,
  decodeCodingSessionLandResult,
} from "@/features/coding-sessions/lib/codingSessionMissionLand.ts";
import { CodingSessionMissionLandControl } from "./CodingSessionMissionLandControl.tsx";
import { CodingSessionMissionStatePanel } from "./CodingSessionMissionStatePanel.tsx";

/** The `coding_session_land` adapter's own output. */
const FIXTURE = JSON.parse(
  readFileSync(
    new URL(
      "../lib/codingSessionLandAdapterResponse.fixture.json",
      import.meta.url,
    ),
    "utf8",
  ),
);

function model(key) {
  return codingSessionMissionLandModel({
    result: decodeCodingSessionLandResult(FIXTURE[key]),
    repositoryUnknownReason: "not-read",
    resolveWho: () => "the founder",
  });
}

function render(key) {
  return renderToStaticMarkup(
    React.createElement(CodingSessionMissionLandControl, { land: model(key) }),
  );
}

test("L8.2: an admitted commit offers the button, and the button names the short sha", () => {
  const html = render("admitted");
  const sha = FIXTURE.admitted.evidence.headSha;
  assert.match(html, /data-land-state="ready"/);
  assert.match(html, new RegExp(`Land ${sha.slice(0, 7)} on main`));
  // The confirm step is not open, so the command is not on screen yet.
  assert.ok(!/git push origin/.test(html));
});

test("L8.2: a refused commit keeps the control and prints §1j's string verbatim", () => {
  const html = render("refused");
  assert.match(html, /data-land-state="refused"/);
  assert.match(
    html,
    /Not ready to land: require-verdict is set and no mission verdict names this commit/,
  );
  // Present, not hidden: a missing control would leave the founder guessing.
  assert.match(html, /data-testid="mission-land-control"/);
  assert.ok(!/mission-land-open/.test(html));
});

test("L8.2: an ungoverned repository says the rule does not govern it, and shows the verdict", () => {
  const html = render("ungoverned");
  assert.match(html, /data-land-state="ungoverned"/);
  assert.match(html, /This repository has no require-verdict rule/);
  assert.match(html, /newest verdict is approve by the founder/);
});

test("L8.2: no repository record reads as unknown, never as no rule", () => {
  const html = render("unknown");
  assert.match(html, /data-land-state="unknown"/);
  assert.match(html, /was not read here/);
  assert.ok(!/no require-verdict rule/.test(html));
});

test("L8.2: an approval for another branch renders the refused control, not a button", () => {
  const html = render("approvedForAnotherRef");
  assert.match(html, /data-land-state="refused"/);
  assert.match(html, /is approved for release, and this is a different ref/);
  assert.ok(!/mission-land-open/.test(html));
  assert.ok(!/git push origin/.test(html));
});

test("L8.2: no state of this control ever renders a git invocation of its own", () => {
  for (const key of [
    "admitted",
    "refused",
    "ungoverned",
    "unknown",
    "approvedForAnotherRef",
  ]) {
    const html = render(key);
    // The only `git` string this surface may carry is the copyable command in
    // the confirm step — and that is text in a <pre>, not an invocation.
    assert.ok(
      !/onSubmit|exec|spawn|invoke\(/.test(html),
      `${key} must not carry an action`,
    );
  }
});

test("L8.3: the state panel prints the fold's completion_not_verified sentence", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      completionNotVerified: true,
      nowMs: 1_788_400_000_000,
      state: {
        kind: "running",
        sourceEventId: "aa".repeat(32),
        phase: "acknowledged",
        detail: "Acknowledged.",
        canonicalChain: [],
      },
    }),
  );
  assert.match(
    html,
    /Completed, but not verified: this session&#x27;s policy requires a verifier&#x27;s ruling and no active verifier has ruled on the approved report\. The completion is not this mission&#x27;s terminal until one does\./,
  );
  // A mission with an excluded completion is not a completed mission.
  assert.ok(!/Mission completed/.test(html));
});

test("L8.3: with no policy record the panel says the fold read none — unknown ≠ false", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: 1_788_400_000_000,
      policyRecordKnown: false,
      state: {
        kind: "running",
        sourceEventId: "aa".repeat(32),
        phase: "assigned",
        detail: "Assigned.",
        canonicalChain: [],
      },
    }),
  );
  assert.match(
    html,
    /No policy record reached this view, so the fold read no verifier requirement\./,
  );
});

test("L8.3: a panel given neither fact renders exactly what it did before this lane", () => {
  const state = {
    kind: "running",
    sourceEventId: "aa".repeat(32),
    phase: "assigned",
    detail: "Assigned.",
    canonicalChain: [],
  };
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: 1_788_400_000_000,
      state,
    }),
  );
  assert.ok(!/mission-completion-not-verified/.test(html));
  assert.ok(!/mission-policy-record-unknown/.test(html));
  assert.ok(!/mission-land-control/.test(html));
});
