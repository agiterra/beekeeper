import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { after, afterEach, before, test } from "node:test";
import { fileURLToPath } from "node:url";

import { JSDOM } from "jsdom";

const dom = new JSDOM("<!doctype html><html><body></body></html>", {
  url: "http://localhost",
});

before(() => {
  Object.assign(globalThis, {
    document: dom.window.document,
    HTMLElement: dom.window.HTMLElement,
    IS_REACT_ACT_ENVIRONMENT: true,
    window: dom.window,
  });
  dom.window.matchMedia = () => ({
    matches: false,
    addEventListener() {},
    removeEventListener() {},
  });
});

afterEach(async () => {
  const { cleanup } = await import("@testing-library/react");
  cleanup();
});

after(() => dom.window.close());

const here = dirname(fileURLToPath(import.meta.url));
const NOW = 1_785_513_037;

function fixture() {
  return JSON.parse(
    readFileSync(
      resolve(here, "..", "lib", "pulseMissionResponse.fixture.json"),
      "utf8",
    ),
  );
}

function session() {
  return {
    sessionKey: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    sessionRef: "5b7e1c2a-90d4-4b0e-a1f3-7c2d8e6f4a10",
    name: "Pulse plumbing",
    goal: null,
    lifecycle: "open",
    coordinationState: "provider_reachable",
    latestObservationAt: NOW - 60,
    observedAgeSeconds: 60,
    generations: [
      {
        targetKey: "coding-session/v1|3:acp6:inst-16:sess-11:1",
        executionKey: "acp6:inst-16:sess-11",
        status: "running",
        statusAt: NOW - 60,
        branch: "main",
        observedCommit: "a".repeat(40),
        dirty: false,
        reachability: "provider_reachable",
        commitConfirmation: "not_checked",
      },
    ],
  };
}

async function renderCard(missionRow) {
  const { createElement } = await import("react");
  const { render } = await import("@testing-library/react");
  const { PulseSessionCard } = await import(
    "@/features/project-pulse/ui/PulseSessionCard"
  );
  return render(
    createElement(
      "ul",
      null,
      createElement(PulseSessionCard, {
        session: session(),
        nowSeconds: NOW,
        missionRow,
      }),
    ),
  );
}

test("a session card without a mission read is unchanged", async () => {
  const screen = await renderCard(undefined);
  assert.ok(screen.getByTestId("pulse-session-card"));
  assert.equal(screen.queryByTestId("pulse-session-mission"), null);
});

test("a session card carries its mission row beside the liveness it already shows", async () => {
  const { decodePulseMissionRows } = await import(
    "@/features/project-pulse/lib/pulseMissionWire"
  );
  const rows = decodePulseMissionRows(fixture());
  const mission = rows.missions[0];
  const screen = await renderCard(mission);
  // The card keeps its own observation row: the mission fold and the digest
  // are two reads of the same work, and replacing one with the other would
  // hide the disagreement rather than show it.
  assert.ok(screen.getByTestId("pulse-session-status"));
  assert.ok(screen.getByTestId("pulse-session-mission"));
  assert.ok(screen.getByTestId(`pulse-mission-row-${mission.sessionKey}`));
  assert.equal(
    screen.getByTestId("pulse-mission-line-waiting").textContent,
    mission.lines[0].text,
  );
});
