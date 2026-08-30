import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { CodingSessionProvenanceDetails } from "./CodingSessionHeader.tsx";

const PROVIDER_SIGNER = "1958c6c4".repeat(8);

// Walk finding 5 (docs/design/singularity/WALK-2026-08-29.md:214) read the
// shipped popover in full: four rows, no founder and no context, while both
// were on the wire and `bee sessions status` printed them.
const shipped = {
  channelName: "engineering",
  generationLabel: "3 executions",
  providerAuthorityPubkey: PROVIDER_SIGNER,
};

test("the popover names the founder the genesis carries", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionProvenanceDetails, {
      ...shipped,
      founderDetails: "Brian",
    }),
  );

  assert.match(markup, /Shared session details/);
  assert.match(markup, /Founded by/);
  assert.match(markup, />Brian</);
  assert.match(markup, /Verified source/);
});

test("an unresolved genesis says unresolved rather than dropping the row", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionProvenanceDetails, shipped),
  );

  assert.match(markup, /Founded by/);
  assert.match(markup, />unresolved</);
});

test("context is the cell bee sessions status prints, per seat", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionProvenanceDetails, {
      ...shipped,
      contextLoads: [
        {
          key: "lead",
          label: "Keystone · Lead",
          load: { usedTokens: 137498, contextWindow: 1000000, pct: 14 },
        },
        {
          key: "designer",
          label: "Banksy · Designer",
          load: { usedTokens: 137498, contextWindow: null, pct: null },
        },
        { key: "poker", label: "Texas · Poker", load: null },
      ],
    }),
  );

  assert.match(markup, />Context</);
  assert.match(markup, />Keystone · Lead</);
  assert.match(markup, />137498\/1000000 \(14%\)</);
  assert.match(markup, />137498 tokens \(window unknown\)</);
  assert.match(markup, /title="no usage reported"/);
  assert.match(markup, />—</);
  assert.doesNotMatch(markup, />0%</);
});

test("no execution has reported, so no Context section is invented", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionProvenanceDetails, {
      ...shipped,
      contextLoads: [],
    }),
  );
  assert.doesNotMatch(markup, />Context</);
});

/**
 * The routing disclosure: one line per routed seat, in the popover that
 * already answers "where did this come from".
 *
 * The whole ruling turns on a decision that can be explained from the wire,
 * and a decision nobody can read is not one. One line, no new panel.
 */
test("a routed seat shows the router's own sentence, one line", () => {
  const line =
    "routed: builder/standard → claude-primary/sonnet (medium) — cleared the builder gates (coding≥4.2) and the standard tier's medium effort; incumbent, cheaper than codex-primary/gpt-5.6-luna[medium].";
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionProvenanceDetails, {
      ...shipped,
      routedSeats: [{ key: "builder", line }],
    }),
  );
  assert.match(markup, /Routing/);
  assert.match(markup, /data-testid="coding-session-routed-line"/);
  assert.match(markup, /routed: builder\/standard/);
  assert.match(markup, /cheaper than codex-primary/);
});

test("a session nothing routed shows no routing row at all", () => {
  const markup = renderToStaticMarkup(
    React.createElement(CodingSessionProvenanceDetails, {
      ...shipped,
      routedSeats: [],
    }),
  );
  assert.equal(markup.includes("coding-session-provenance-routing"), false);
  assert.equal(markup.includes(">Routing<"), false);
});
