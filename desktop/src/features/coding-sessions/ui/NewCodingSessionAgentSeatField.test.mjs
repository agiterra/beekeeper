/**
 * The seat field is where a person learns, before anything is signed, that
 * the role they are typing is not the role the agent *is* — and that the
 * pack behind that role may not exist on this computer at all.
 *
 * Both disclosures are conditional on the backend having answered. A build
 * whose managed agents carry no `homeRole`/`hasRolePack` renders exactly the
 * field it rendered before: absence is not a finding.
 */
import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { NewCodingSessionAgentSeatField } from "./NewCodingSessionAgentSeatField.tsx";

const ACTOR = "aa11bb22".repeat(8);

function render(props = {}) {
  return renderToStaticMarkup(
    React.createElement(NewCodingSessionAgentSeatField, {
      actor: ACTOR,
      agents: [
        { pubkey: ACTOR, name: "Ada", status: "running", homeRole: "builder" },
      ],
      onActorChange() {},
      onRoleChange() {},
      role: "builder",
      ...props,
    }),
  );
}

test("a seat on the agent's own home role reads as unremarkable", () => {
  const markup = render();
  assert.match(markup, /data-testid="new-coding-session-seat-role-notice"/);
  assert.match(markup, /Its home role\./);
  assert.doesNotMatch(markup, /will carry the/);
  assert.doesNotMatch(markup, /data-testid="new-coding-session-seat-pack"/);
});

test("a seat given another role names the pack it will actually carry", () => {
  const markup = render({ role: "lead" });
  // LANE-L23: the seat's role picks the pack, never the actor's home role —
  // "the lead pack", not "the builder pack".
  assert.match(
    markup,
    /Ada is a builder — seating it as lead; it will carry the lead pack\./,
  );
});

test("LANE-L23: with no project known, no pack-status probe is attempted and no preview shows", () => {
  // No `projectRef` passed — every pre-L23 caller's implicit default. The
  // static render never runs effects anyway, but the field must not crash
  // reaching for a project it was never given, and the preview testid must
  // be absent either way.
  const markup = render({ role: "lead" });
  assert.doesNotMatch(markup, /data-testid="new-coding-session-pack-preview"/);
});

test("an agent with no role pack on this computer says so before submit", () => {
  const markup = render({
    agents: [
      {
        pubkey: ACTOR,
        name: "Ada",
        status: "running",
        homeRole: "builder",
        hasRolePack: false,
      },
    ],
  });
  assert.match(markup, /data-testid="new-coding-session-seat-pack"/);
  assert.match(
    markup,
    /Ada has no role pack on this computer, so this seat carries no role skills and runs on its persona prompt alone\./,
  );
});

test("a backend that never answered accuses the agent of nothing", () => {
  const markup = render({
    agents: [{ pubkey: ACTOR, name: "Ada", status: "running" }],
    role: "lead",
  });
  assert.doesNotMatch(markup, /data-testid="new-coding-session-seat-notice"/);
  assert.doesNotMatch(markup, /data-testid="new-coding-session-seat-pack"/);
  assert.doesNotMatch(markup, /home role/);
  assert.doesNotMatch(markup, /no role pack/);
});

test("an unseated field shows no role box and no disclosures at all", () => {
  const markup = render({ actor: null, role: "" });
  assert.match(markup, /No agent — you run this session/);
  assert.doesNotMatch(markup, /data-testid="new-coding-session-seat-role"/);
  assert.doesNotMatch(markup, /data-testid="new-coding-session-seat-pack"/);
});
