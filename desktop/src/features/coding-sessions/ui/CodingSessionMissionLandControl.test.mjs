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
  assert.match(html, /newest verdict is not-refuted by the founder/);
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

// ── finding 33: the founder line is on the screen in every state ─────────

test("L18: the control names the founders whether it offers the push or refuses", () => {
  // `founderPush` rather than `admitted`: since the 2026-09-03 ruling the
  // admitted fixture is a **seat's** push behind a verifier's verdict, so its
  // viewer is deliberately not a founder. The founder's own case is its own
  // fixture, and `data-founder` must tell the two apart.
  for (const key of ["founderPush", "admitted", "refused", "ungoverned"]) {
    const html = render(key);
    assert.match(
      html,
      /data-testid="mission-land-founders"/,
      `${key} shows the founder line`,
    );
    assert.match(
      html,
      new RegExp(
        `data-founder="${FIXTURE[key].viewerIsFounder ? "viewer" : "other"}"`,
      ),
      `${key} states the viewer's real standing`,
    );
    assert.match(html, /Founders: the founder\./, `${key} names them`);
    assert.match(
      html,
      /only that key can rewrite them/,
      `${key} discloses that rules are signer-only in v1`,
    );
  }
});

test("L18: a viewer who founds nothing is marked as such, not merely refused", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionLandControl, {
      land: codingSessionMissionLandModel({
        result: decodeCodingSessionLandResult({
          ...FIXTURE.refused,
          founders: [
            FIXTURE.refused.newestVerdict.authorPubkey,
            "3d".repeat(32),
          ],
          viewerIsFounder: false,
        }),
        resolveWho: (pubkey) => pubkey.slice(0, 8),
      }),
    }),
  );
  assert.match(html, /data-founder="other"/);
  assert.match(html, /You are not one of them/);
});

test("L20.2: a repository inferred from the project's only repository discloses it, even ready", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionLandControl, {
      land: codingSessionMissionLandModel({
        result: decodeCodingSessionLandResult(FIXTURE.admitted),
        repositoryInferred: true,
        resolveWho: () => "the founder",
      }),
    }),
  );
  assert.match(html, /data-land-state="ready"/);
  assert.match(
    html,
    /data-testid="mission-land-repository-source"/,
    "the inferred disclosure renders even in the ready state",
  );
  assert.match(
    html,
    /inferred from the project.{1,6}s only repository/,
    "L20 spec's own words",
  );
});

test("L20.2: no repoRef and no inference discloses nothing extra", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionLandControl, {
      land: codingSessionMissionLandModel({
        result: decodeCodingSessionLandResult(FIXTURE.unknown),
        repositoryUnknownReason: "no-repo-ref",
        resolveWho: () => "the founder",
      }),
    }),
  );
  assert.ok(
    !/data-testid="mission-land-repository-source"/.test(html),
    "an explicit no-repository state carries no inference disclosure",
  );
});

test("L20.2: a project with two or more repositories names none, and says how many", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionLandControl, {
      land: codingSessionMissionLandModel({
        result: decodeCodingSessionLandResult(FIXTURE.unknown),
        repositoryUnknownReason: "multiple-repos",
        projectRepoCount: 3,
        resolveWho: () => "the founder",
      }),
    }),
  );
  assert.match(html, /data-land-state="unknown"/);
  assert.match(
    html,
    /its project has 3 repositories, so nothing here can say which one gates a push/,
  );
});

// ── finding 37: an absent Land control says why ───────────────────────────

test("L20.3: no identity resolved yet says nothing to land yet, not silence", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: 1_788_400_000_000,
      land: null,
      landUnavailableReason: "no-identity",
      state: {
        kind: "running",
        sourceEventId: "aa".repeat(32),
        phase: "assigned",
        detail: "Assigned.",
        canonicalChain: [],
      },
    }),
  );
  assert.match(html, /data-testid="mission-land-unavailable"/);
  assert.match(html, /data-land-unavailable-reason="no-identity"/);
  assert.match(html, /Nothing to land yet\./);
  assert.ok(!/Land could not be read/.test(html));
});

test("L20.3: a boundary that threw says the read failed, not silence", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionMissionStatePanel, {
      nowMs: 1_788_400_000_000,
      land: null,
      landUnavailableReason: "boundary-failed",
      state: {
        kind: "running",
        sourceEventId: "aa".repeat(32),
        phase: "assigned",
        detail: "Assigned.",
        canonicalChain: [],
      },
    }),
  );
  assert.match(html, /data-testid="mission-land-unavailable"/);
  assert.match(html, /data-land-unavailable-reason="boundary-failed"/);
  assert.match(html, /Land could not be read\./);
  assert.ok(!/Nothing to land yet/.test(html));
});

test("L20.3: land present renders the control, never the unavailable line — even with a stale reason", () => {
  const html = render("admitted");
  assert.ok(!/mission-land-unavailable/.test(html));
});

test("L21 arm (A): a founder's push says why it is ready, and claims no ruling", () => {
  assert.match(render("founderPush"), /data-land-state="ready"/);
  // The confirm step's sentence is where the arm is named. The mission's only
  // ruling is `changes-requested`, so saying "approved" over it would be the
  // comfortable guess this project treats as a bug of the same severity as a
  // crash.
  const sentence = model("founderPush").approvalSentence;
  assert.match(sentence, /You are a founder of this repository/);
  assert.match(sentence, /Nothing here has ruled on this commit/);
  assert.ok(!/Approved by/.test(sentence));
});

test("L21 arm (C): a verifier's clearance is named alongside the approval", () => {
  assert.match(render("admitted"), /data-land-state="ready"/);
  const sentence = model("admitted").approvalSentence;
  assert.match(sentence, /Approved by/);
  assert.match(sentence, /did not refute it \(refutation/);
});
