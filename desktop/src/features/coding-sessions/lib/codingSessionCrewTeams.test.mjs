import assert from "node:assert/strict";
import test from "node:test";

import { readCodingSessionCrewTeams } from "./codingSessionCrewTeams.ts";

test("only teams carrying a crew block are crews", () => {
  const teams = readCodingSessionCrewTeams([
    { id: "t1", name: "Plain", crew: null },
    {
      id: "t2",
      name: "Crew",
      crew: { primary: "p1", seats: [{ personaId: "p1", role: "lead" }] },
    },
    { id: "t3", name: "Broken", crew: { primary: "p9", seats: [] } },
  ]);
  assert.deepEqual(
    teams.map((team) => team.id),
    ["t2"],
  );
});
