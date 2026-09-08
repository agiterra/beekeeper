import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { RolesSummaryStrip } from "./RolesSummaryStrip.tsx";

function summary(overrides = {}) {
  return {
    roles: 4,
    agents: 3,
    agentsLocal: 3,
    agentsShared: 0,
    openSessions: 2,
    reports: 7,
    unconfirmed: 0,
    disputed: 0,
    ...overrides,
  };
}

function render(overrides = {}) {
  return renderToStaticMarkup(
    React.createElement(RolesSummaryStrip, { summary: summary(overrides) }),
  );
}

test("the four counts are drawn with their own ids and plural-correct labels", () => {
  const html = render();

  assert.match(html, /data-testid="roles-summary"/);
  assert.match(html, /data-testid="roles-summary-roles"[^>]*>.*?>4<.*?>roles</);
  assert.match(
    html,
    /data-testid="roles-summary-agents"[^>]*>.*?>3<.*?>agents</,
  );
  assert.match(
    html,
    /data-testid="roles-summary-sessions"[^>]*>.*?>2<.*?>open sessions</,
  );
  assert.match(
    html,
    /data-testid="roles-summary-reports"[^>]*>.*?>7<.*?>reports</,
  );

  const one = render({
    roles: 1,
    agents: 1,
    agentsLocal: 1,
    openSessions: 1,
    reports: 1,
  });
  assert.match(one, />role</);
  assert.match(one, />agent</);
  assert.match(one, />open session</);
  assert.match(one, />report</);
});

test("counts are tabular and the strip carries no colour of its own", () => {
  const html = render();
  assert.equal((html.match(/tabular-nums/g) ?? []).length, 4);
  assert.doesNotMatch(html, /emerald|amber|sky|destructive/);
});

test("the scope line splits the agent count and never claims a roster", () => {
  const local = render();
  assert.match(local, /data-testid="roles-scope"/);
  assert.match(local, />3 on this computer</);
  assert.match(
    local,
    /title="Local agents are set up on this computer\. Shared agents were seen/,
  );
  assert.match(local, /not a complete roster\./);

  const mixed = render({ agents: 5, agentsLocal: 3, agentsShared: 2 });
  assert.match(mixed, />3 on this computer · 2 shared</);
});

test("unconfirmed and disputed reports appear only when there are some", () => {
  const clean = render();
  assert.doesNotMatch(clean, /data-testid="roles-summary-reports-detail"/);
  assert.doesNotMatch(clean, /unconfirmed|disputed/);

  const unconfirmed = render({ unconfirmed: 2 });
  assert.match(unconfirmed, /data-testid="roles-summary-reports-detail"/);
  assert.match(unconfirmed, />2 unconfirmed</);
  // Unconfirmed is a gap, not a contradiction: no colour, no icon.
  assert.doesNotMatch(unconfirmed, /text-destructive/);
  assert.doesNotMatch(unconfirmed, /<svg/);

  const disputed = render({ unconfirmed: 2, disputed: 1 });
  assert.match(disputed, />2 unconfirmed</);
  // A contradiction gets the one warning colour on the strip — with the word.
  assert.match(disputed, /text-destructive/);
  assert.match(disputed, /1 disputed/);
  assert.match(disputed, /<svg/);
});
