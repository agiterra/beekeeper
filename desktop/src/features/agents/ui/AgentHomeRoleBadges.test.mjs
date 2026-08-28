/**
 * The two badges an agent's role owes the operator, wherever the agent is
 * shown.
 *
 * They exist for one disclosure: an agent carrying a home role with no pack on
 * this computer must never render as if it carried the role's craft. They also
 * have to stay quiet about an agent nobody asked — `hasRolePack: undefined` is
 * "this build never looked", not "the pack is missing".
 */
import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { AgentHomeRoleBadges } from "./AgentHomeRoleBadges.tsx";

function render(agent) {
  return renderToStaticMarkup(
    React.createElement(AgentHomeRoleBadges, { agent }),
  );
}

test("an agent with a home role and its pack shows the role and no warning", () => {
  const markup = render({ homeRole: "verifier", hasRolePack: true });
  assert.match(markup, /data-testid="agent-home-role"/);
  assert.match(markup, /Home role: Verifier/);
  assert.doesNotMatch(markup, /data-testid="agent-no-role-pack"/);
});

test("a home role with no pack on this computer says so, and says what to do", () => {
  const markup = render({ homeRole: "verifier", hasRolePack: false });
  assert.match(markup, /data-testid="agent-no-role-pack"/);
  assert.match(
    markup,
    /Role pack not installed here — install crew roles from the project&#x27;s personas\/roles/,
  );
});

test("a card-sized badge still states the fact, and drops only the remedy", () => {
  const markup = renderToStaticMarkup(
    React.createElement(AgentHomeRoleBadges, {
      agent: { homeRole: "verifier", hasRolePack: false },
      withRemedy: false,
    }),
  );
  assert.match(markup, /data-testid="agent-no-role-pack"/);
  assert.match(markup, /Role pack not installed here/);
  assert.doesNotMatch(markup, /install crew roles/);
});

test("an agent with no home role claims nothing at all", () => {
  assert.equal(render({ homeRole: null, hasRolePack: false }), "");
  assert.equal(render(undefined), "");
});

test("a backend that never answered about a pack accuses the agent of nothing", () => {
  const markup = render({ homeRole: "verifier" });
  assert.match(markup, /data-testid="agent-home-role"/);
  assert.doesNotMatch(
    markup,
    /data-testid="agent-no-role-pack"/,
    "absence is not a claim: an unanswered pack field is not a missing pack",
  );
});
