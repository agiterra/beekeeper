/**
 * A lane message is transcript text like any other, so a privacy marker in it
 * owes the reader a pill — not ninety characters of hash it cannot reveal.
 */
import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { TooltipProvider } from "@/shared/ui/tooltip";
import { UmbrellaConversationRow } from "./CodingSessionUmbrellaConversationRow.tsx";

const DIGEST =
  "01de6a4ef05f5c4052a052531f89bff67836c5ba404d05e4ead0b883e561a88c";
const MARKER = `[elided private context: 31 bytes, sha256:${DIGEST}]`;

function render(content) {
  return renderToStaticMarkup(
    React.createElement(
      TooltipProvider,
      null,
      React.createElement(UmbrellaConversationRow, {
        currentUserPubkey: null,
        message: {
          authorPubkey: "a3945536".padEnd(64, "0"),
          content,
          timestampMs: 1_756_500_000_000,
        },
        operatorProfiles: undefined,
      }),
    ),
  );
}

test("a marker in a lane message renders as a pill", () => {
  const markup = render(`log at ${MARKER} now`);
  assert.match(markup, /data-redaction-pill=""/);
  assert.match(markup, />hidden</);
  // The digest still rides along as a data attribute — that is the copy
  // affordance. What must be gone is the marker as *readable text*.
  assert.doesNotMatch(markup, /elided private context/);
});

test("an ordinary lane message is unchanged", () => {
  const markup = render("started the dev server");
  assert.match(markup, /started the dev server/);
  assert.doesNotMatch(markup, /data-redaction-pill/);
});
