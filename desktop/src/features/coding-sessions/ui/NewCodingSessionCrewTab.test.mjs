import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CODING_SESSION_CREW_LAUNCH_SCOPE_NOTE,
  codingSessionCrewProviderNote,
  codingSessionCrewReadinessRoles,
  CodingSessionCrewLaunchSteps,
  CodingSessionCrewRoster,
  CodingSessionCrewSkillNotice,
} from "./NewCodingSessionCrewTab.tsx";

test("readiness roles come from the team definition before local seats resolve", () => {
  assert.deepEqual(
    codingSessionCrewReadinessRoles({
      id: "portable-team",
      name: "Portable team",
      crew: {
        primary: "missing-lead",
        seats: [
          { personaId: "missing-lead", role: " Lead " },
          { personaId: "missing-builder", role: "builder" },
          { personaId: "other-lead", role: "LEAD" },
        ],
      },
    }),
    ["builder", "lead"],
  );
});

const SEATS = [
  {
    personaId: "p-lead",
    role: "lead",
    actor: "a".repeat(64),
    actorLabel: "Fable",
    model: "claude-opus-5",
    vendor: null,
  },
  {
    personaId: "p-verify",
    role: "verifier",
    actor: "b".repeat(64),
    actorLabel: "Quinn",
    model: "qwen3-coder",
    vendor: null,
  },
];

test("the roster says where each vendor came from, and names the one it cannot", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionCrewRoster, {
      primaryPersonaId: "p-lead",
      seats: SEATS,
    }),
  );
  assert.match(html, /anthropic \(from the model id\)/);
  assert.match(html, /vendor not declared/);
  assert.match(html, /first turn/);
  assert.match(html, /Quinn/);
});

test("the step list marks the failed step and leaves the untouched ones untouched", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionCrewLaunchSteps, {
      steps: [
        {
          id: "genesis",
          label: "Found the session",
          state: "done",
          detail: null,
        },
        {
          id: "create:1",
          label: "Seat Codey as builder",
          state: "failed",
          detail: "ACTOR_UNAVAILABLE",
        },
        {
          id: "first-turn",
          label: "Send the goal and the roster",
          state: "pending",
          detail: null,
        },
      ],
    }),
  );
  assert.match(html, /data-state="failed"/);
  assert.match(html, /ACTOR_UNAVAILABLE/);
  assert.match(html, /data-state="pending"/);
  assert.match(html, /data-testid="crew-step-create:1"/);
});

test("the roster shows a declaration its model contradicts, not the declaration", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionCrewRoster, {
      primaryPersonaId: "p-lead",
      seats: [{ ...SEATS[0], vendor: "local" }],
    }),
  );
  assert.match(html, /declared local, but claude-opus-5 is anthropic/);
  // The model is inside that phrase already; printing it twice reads as two
  // seats' worth of model.
  assert.doesNotMatch(html, /· claude-opus-5/);
});

test("seats with no role pack are named, and a full crew says nothing", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionCrewSkillNotice, {
      labels: ["Codey", "Quinn"],
    }),
  );
  assert.match(html, /Codey, Quinn carry no role skills/);
  assert.match(html, /.agents\/skills/);
  assert.equal(
    renderToStaticMarkup(
      React.createElement(CodingSessionCrewSkillNotice, { labels: [] }),
    ),
    "",
  );
});

// ── The pack disclosure, before anything is signed ───────────────────────────
//
// `CodingSessionCrewSkillNotice` only ever had labels after a launch: the
// launch's own `seatsWithoutRolePack` is filled in by the staging call. So the
// operator learned a seat carries no craft only once the seats existed — while
// every other pre-submit surface in the batch discloses it before signing
// (SESSION_STATE item 76, poke finding F5). The roster knows it up front,
// because the agents it resolves each carry `hasRolePack`.

test("the roster says which seats carry no role skills before the launch", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionCrewRoster, {
      primaryPersonaId: "p-lead",
      seats: [
        { ...SEATS[0], hasRolePack: true },
        { ...SEATS[1], hasRolePack: false },
      ],
    }),
  );
  assert.match(html, /data-testid="crew-seat-no-role-pack-p-verify"/);
  assert.match(
    html,
    /carries no role skills: this computer has no role pack behind it\./,
  );
  assert.doesNotMatch(html, /data-testid="crew-seat-no-role-pack-p-lead"/);
});

test("a roster whose agents were never asked about a pack claims nothing", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionCrewRoster, {
      primaryPersonaId: "p-lead",
      seats: SEATS,
    }),
  );
  assert.doesNotMatch(html, /carries no role skills/);
});

/**
 * D14 — a launch seats the lead only, so the roster is an offer, not a
 * manifest. A row that reads exactly like the seated one is the same lie the
 * launch used to tell in events: four rows, one agent.
 */
test("the roster marks the lead as the seat that is created and the rest as hireable", () => {
  const html = renderToStaticMarkup(
    React.createElement(CodingSessionCrewRoster, {
      primaryPersonaId: "p-lead",
      seats: SEATS,
    }),
  );
  assert.match(html, /data-seat-state="seated"/);
  assert.match(html, /data-seat-state="hireable"/);
  assert.match(html, /the lead may hire/);
});

test("the tab says a launch seats the lead only", () => {
  assert.match(
    CODING_SESSION_CREW_LAUNCH_SCOPE_NOTE,
    /Launching seats the lead only/,
  );
  assert.match(CODING_SESSION_CREW_LAUNCH_SCOPE_NOTE, /bee sessions hire/);
});

// The team tab has no model picker yet (item 87c), so the sentence under the
// team select is the only thing that says which model the lead runs on. On
// claude-primary that model id is literally `default`, and ", on default"
// reads as a model somebody chose.
test("the team tab never says the lead runs on `default`", () => {
  const note = codingSessionCrewProviderNote({
    providerLabel: "Claude Code",
    model: "default",
    allowedModels: ["default", "sonnet"],
  });
  assert.match(note, /Runtime default \(not named on the record\)/);
  assert.doesNotMatch(note, /on default/);
  assert.match(note, /Claude Code/);
});

test("a concrete model is named exactly as it goes on the wire", () => {
  const note = codingSessionCrewProviderNote({
    providerLabel: "Claude Code",
    model: "claude-fable-5[1m]",
    allowedModels: ["default", "claude-fable-5[1m]"],
  });
  assert.match(note, /on claude-fable-5\[1m\]/);
});

test("no model resolved yet says nothing about one", () => {
  const note = codingSessionCrewProviderNote({
    providerLabel: null,
    model: null,
    allowedModels: [],
  });
  assert.match(note, /this computer&#x27;s provider|this computer's provider/);
  assert.doesNotMatch(note, /, on /);
});
