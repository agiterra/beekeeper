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

function render(props) {
  return renderToStaticMarkup(React.createElement(AgentsByProject, props));
}

test("only the projects that hold a session are drawn", () => {
  const html = render({
    byProject: [
      { projectId: "p1", projectName: "Beekeeper", seats: [seat()] },
      { projectId: "p2", projectName: "Quiet", seats: [] },
    ],
    unplaced: [],
  });

  assert.match(html, />Sessions by project</);
  assert.match(html, /data-testid="project-agents-p1"/);
  assert.match(html, /1 session</);
  // A project with no sessions is not news, and the old per-project empty
  // sentence said the same thing once per project.
  assert.doesNotMatch(html, /data-testid="project-agents-p2"/);
  assert.doesNotMatch(html, /No agents in open sessions\./);
  // Unplaced only appears when something is actually unplaced.
  assert.doesNotMatch(html, /data-testid="project-agents-unplaced"/);
});

test("the seats nothing claims keep their own block when there are any", () => {
  const html = render({
    byProject: [],
    unplaced: [seat({ projectId: null, projectName: null })],
  });

  assert.match(html, /data-testid="project-agents-unplaced"/);
  assert.match(html, />Unplaced</);
});

test("when nothing anywhere holds a session, absence is stated once", () => {
  const html = render({
    byProject: [
      { projectId: "p1", projectName: "Beekeeper", seats: [] },
      { projectId: "p2", projectName: "Quiet", seats: [] },
    ],
    unplaced: [],
  });

  assert.equal(
    (html.match(/data-testid="project-agents-empty"/g) ?? []).length,
    1,
  );
  assert.match(html, />No open sessions\.</);
  assert.doesNotMatch(html, /data-testid="project-agents-p1"/);
});

test("each row leads with its own status dot, and the colour carries the word", () => {
  const html = render({
    byProject: [
      {
        projectId: "p1",
        projectName: "Beekeeper",
        seats: [
          seat(),
          seat({ key: "c2:g2", status: "waiting_for_input", ageSeconds: 10 }),
          seat({ key: "c3:g3", status: "disconnected", ageSeconds: 10 }),
        ],
      },
    ],
    unplaced: [],
  });

  assert.match(html, /bg-emerald-500[^>]*title="running"/);
  assert.match(html, /bg-amber-500[^>]*title="waiting_for_input"/);
  assert.match(html, /bg-muted-foreground\/45[^>]*title="disconnected"/);
  // The row itself still says the word, so the dot is never the only signal.
  assert.match(html, /data-seat-column="status"[^>]*>running \(5m\)</);
});
