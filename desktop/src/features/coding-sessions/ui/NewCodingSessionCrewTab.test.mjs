import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import {
  CodingSessionCrewLaunchSteps,
  CodingSessionCrewRoster,
} from "./NewCodingSessionCrewTab.tsx";

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
