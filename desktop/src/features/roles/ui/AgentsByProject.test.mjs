import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { AgentsByProject } from "./AgentsByProject.tsx";

function seat(overrides = {}) {
  return {
    key: "channel-1:generation-1",
    channelId: "channel-1",
    generationId: "generation-1",
    label: "Build the card",
    agentName: "Builder",
    agentPubkey: "a".repeat(64),
    projectId: "p1",
    projectName: "Beekeeper",
    role: "builder",
    status: "running",
    ageSeconds: 300,
    packSha: "9f2e1d0c".padEnd(40, "a"),
    ...overrides,
  };
}

test("the section is about sessions, and an empty project says so in a sentence", () => {
  const html = renderToStaticMarkup(
    React.createElement(AgentsByProject, {
      byProject: [
        { projectId: "p1", projectName: "Beekeeper", seats: [seat()] },
        { projectId: "p2", projectName: "Quiet", seats: [] },
      ],
      unplaced: [],
    }),
  );

  assert.match(html, />Sessions by project</);
  assert.doesNotMatch(html, />Agents by project</);
  assert.match(html, /data-testid="project-agents-p1"/);
  assert.match(html, /data-testid="project-agents-p2"/);
  assert.match(html, /No agents in open sessions\./);
  assert.match(html, /1 agent</);
  assert.match(html, /0 agents</);
  // The seats nothing claims keep their own block, empty state included.
  assert.match(html, /data-testid="project-agents-unplaced"/);
  assert.match(html, />Unplaced</);
});
