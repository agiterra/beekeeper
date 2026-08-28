import assert from "node:assert/strict";
import test from "node:test";

import { resolveAgentCardTitle } from "./agentCardTitle.ts";

test("a card backed by a managed agent is titled with the identity's name", () => {
  // Item 79(a): the installer minted the identity `Keystone` on a reused
  // persona card still titled `Lead`, and the grid read `Lead` — a card
  // naming an agent nobody could find.
  assert.equal(
    resolveAgentCardTitle({
      persona: { displayName: "Lead" },
      agent: { name: "Keystone" },
    }),
    "Keystone",
  );
});

test("a card with no instance yet keeps the persona's own name", () => {
  assert.equal(
    resolveAgentCardTitle({
      persona: { displayName: "Architect" },
      agent: undefined,
    }),
    "Architect",
  );
  // A blank instance name is not a name; it would render an empty card.
  assert.equal(
    resolveAgentCardTitle({
      persona: { displayName: "Architect" },
      agent: { name: "   " },
    }),
    "Architect",
  );
});
