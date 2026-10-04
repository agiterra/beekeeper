import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_CONTINUITY_REASONS,
  CODING_SESSION_CONTINUITY_STATUSES,
  CODING_SESSION_CONTINUITY_TITLE,
} from "../lib/codingSessionTranscriptItems.ts";
import { CodingSessionHeaderDetails } from "./CodingSessionHeaderDetails.tsx";
import {
  CodingSessionDetailsContinuity,
  CodingSessionDetailsContinuityProvider,
  codingSessionContinuityLost,
  codingSessionContinuityRows,
} from "./CodingSessionHeaderDetailsContinuity.tsx";

const FRESH = CODING_SESSION_CONTINUITY_STATUSES.get("session_fresh");
const RESUMED = CODING_SESSION_CONTINUITY_STATUSES.get("session_resumed");
const RESTARTED = CODING_SESSION_CONTINUITY_STATUSES.get(
  "session_restarted_without_context",
);
const FIRST = CODING_SESSION_CONTINUITY_REASONS.get("no_prior_execution");
const RELAY = CODING_SESSION_CONTINUITY_REASONS.get("relay_unavailable");

function row(id, text, timestamp = "2026-10-04T10:00:00.000Z") {
  return {
    id,
    type: "lifecycle",
    title: CODING_SESSION_CONTINUITY_TITLE,
    text,
    timestamp,
  };
}

test("continuity rows are read from the transcript, newest first", () => {
  const rows = codingSessionContinuityRows([
    row("a", `${FRESH} — ${FIRST}`),
    { id: "m", type: "message", text: "hi" },
    { id: "s", type: "lifecycle", title: "Status", text: "other" },
    row("b", RESUMED),
  ]);
  assert.deepEqual(
    rows.map((entry) => entry.id),
    ["b", "a"],
  );
});

test("only a start that lost context is flagged", () => {
  assert.equal(codingSessionContinuityLost(RESTARTED), true);
  assert.equal(codingSessionContinuityLost(`${RESTARTED} — ${RELAY}`), true);
  assert.equal(codingSessionContinuityLost(`${FRESH} — ${RELAY}`), true);
  assert.equal(codingSessionContinuityLost(`${FRESH} (some_new_slug)`), true);
  // A first execution had nothing to lose.
  assert.equal(codingSessionContinuityLost(`${FRESH} — ${FIRST}`), false);
  assert.equal(codingSessionContinuityLost(FRESH), false);
  assert.equal(codingSessionContinuityLost(RESUMED), false);
});

test("Details and the transcript share one loss classifier: unknown prose is not called safe", () => {
  // The transcript keeps an unrecognised continuity row in its reading
  // order; Details must flag the same row rather than read it as routine.
  assert.equal(codingSessionContinuityLost("Started in some new way"), true);
});

test("Details lists every start verbatim, the latest first", () => {
  const rows = codingSessionContinuityRows([
    row("a", `${FRESH} — ${FIRST}`),
    row("b", RESUMED),
    row("c", RESTARTED),
  ]);
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionDetailsContinuity, { rows }),
  );
  assert.match(markup, /data-testid="coding-session-details-continuity"/);
  assert.match(markup, /Session continuity/);
  assert.match(
    markup,
    /data-lost="true" data-testid="coding-session-details-continuity-latest">Restarted without prior context/,
  );
  // React escapes the apostrophes in this prose; compare the escaped form.
  const escaped = (text) => text.replaceAll("'", "&#x27;");
  assert.ok(markup.includes(escaped(RESUMED)));
  assert.ok(markup.includes(escaped(FIRST)));
});

test("no continuity published draws no section", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionDetailsContinuity, { rows: [] }),
  );
  assert.equal(markup, "");
});

function details(rows) {
  return renderToStaticMarkup(
    React.createElement(
      CodingSessionDetailsContinuityProvider,
      { value: rows },
      React.createElement(CodingSessionHeaderDetails, {
        channelName: null,
        compact: false,
        generationLabel: "gen 1",
        peopleCount: 0,
        projectName: null,
        providerAuthorityPubkey: null,
      }),
    ),
  );
}

test("a start that lost context marks the Details trigger, so it is not hidden", () => {
  const lost = details(codingSessionContinuityRows([row("c", RESTARTED)]));
  assert.match(lost, /data-testid="coding-session-details-continuity-dot"/);
  assert.match(
    lost,
    /aria-label="Show session details; the latest start lost prior context"/,
  );

  const kept = details(codingSessionContinuityRows([row("b", RESUMED)]));
  assert.doesNotMatch(kept, /coding-session-details-continuity-dot/);
  assert.match(kept, /aria-label="Show session details"/);
});
