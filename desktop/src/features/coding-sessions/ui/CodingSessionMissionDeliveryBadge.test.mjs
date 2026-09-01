import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL,
  codingSessionSeatAuthorityCopy,
  codingSessionTeamWakeDeliveryCopy,
} from "../lib/codingSessionMissionContracts.ts";
import {
  CodingSessionMissionDeliveryBadge,
  CodingSessionMissionUnseatedBadge,
  CodingSessionSeatAuthorityBadge,
} from "./CodingSessionMissionDeliveryBadge.tsx";

function delivery(kind, overrides = {}) {
  return {
    sourceEventId: "a".repeat(64),
    operationType: "report",
    sourceActorPubkey: "b".repeat(64),
    leadTargetKey: "lead-target",
    kind,
    owningCommandId: "cmd-1",
    duplicateRefusedCommandIds: [],
    failures: [],
    reArmCount: 0,
    observedAtMs: 1,
    detail: codingSessionTeamWakeDeliveryCopy[kind].detail,
    ...overrides,
  };
}

test("U-T2: all eight delivery kinds render an icon and a word", () => {
  for (const kind of Object.keys(codingSessionTeamWakeDeliveryCopy)) {
    const copy = codingSessionTeamWakeDeliveryCopy[kind];
    const markup = renderToStaticMarkup(
      React.createElement(CodingSessionMissionDeliveryBadge, {
        delivery: delivery(kind),
      }),
    );
    assert.match(markup, /data-testid="coding-session-delivery-badge"/, kind);
    assert.match(markup, new RegExp(`data-kind="${kind}"`), kind);
    assert.match(markup, /<svg[^>]*aria-hidden/, `${kind} needs an icon`);
    assert.ok(
      markup.includes(`>${copy.badge ?? copy.detail}<`),
      `${kind} must carry a word, not only a colour`,
    );
    assert.ok(
      markup.includes(`title="${copy.detail}"`),
      `${kind} title must equal the frozen detail`,
    );
  }
  assert.equal(Object.keys(codingSessionTeamWakeDeliveryCopy).length, 8);
});

test("U-T2: the §2a residual detail renders verbatim", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionMissionDeliveryBadge, {
      delivery: delivery("failed", {
        detail: CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL,
      }),
    }),
  );
  assert.ok(
    markup.includes(`title="${CODING_SESSION_TEAM_WAKE_RESIDUAL_DETAIL}"`),
  );
  assert.match(markup, />failed</);
});

test("U-T2: seat authority badges say ungranted and unknown, and nothing for granted", () => {
  const authority = (kind) => ({
    executionKey: "builder",
    actorPubkey: "b".repeat(64),
    role: "builder",
    kind,
    grantEventId: null,
    detail: codingSessionSeatAuthorityCopy[kind].detail,
    remedy:
      kind === "created-ungranted"
        ? "bee sessions seat-repair --channel c --session-ref s --actor a"
        : null,
  });
  const ungranted = renderToStaticMarkup(
    React.createElement(CodingSessionSeatAuthorityBadge, {
      authority: authority("created-ungranted"),
    }),
  );
  assert.match(ungranted, />ungranted</);
  assert.match(
    ungranted,
    /title="Seat created, not granted — bee sessions seat-repair --channel c --session-ref s --actor a"/,
  );
  const unknown = renderToStaticMarkup(
    React.createElement(CodingSessionSeatAuthorityBadge, {
      authority: authority("unknown"),
    }),
  );
  assert.match(unknown, />unknown</);
  assert.match(unknown, /title="Seat authority unknown"/);
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionSeatAuthorityBadge, {
        authority: authority("granted"),
      }),
    ),
    "",
  );
});

test("U-F3: the unseated badge states the fold's fact, not a seat's history", () => {
  const withRole = renderToStaticMarkup(
    React.createElement(CodingSessionMissionUnseatedBadge, { role: "builder" }),
  );
  assert.match(withRole, />unseated</);
  assert.match(withRole, /title="Report author holds no seat for builder"/);
  assert.match(withRole, /<svg[^>]*aria-hidden/);
  // `unseatedReports` (§1d) covers a seat never created and a seat held for a
  // different role; neither is "created, not granted", which is §1c vocabulary
  // for a create receipt this badge has no evidence of.
  assert.doesNotMatch(withRole, /Seat created, not granted/);

  const withoutRole = renderToStaticMarkup(
    React.createElement(CodingSessionMissionUnseatedBadge, {}),
  );
  assert.match(withoutRole, />unseated</);
  assert.match(
    withoutRole,
    /title="Report author holds no seat for the assigned role"/,
  );
  assert.doesNotMatch(withoutRole, /Seat created, not granted/);
});
