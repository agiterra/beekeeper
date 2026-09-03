import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  readProjectPulseDigest,
  rememberProjectPulseDigest,
  resetProjectPulseState,
} from "@/features/project-pulse/lib/projectPulseCache";
import { foldProjectPulseDigest } from "@/features/project-pulse/lib/pulseFold";
import { PROJECT_CHILD_TYPE_RANK } from "@/features/projects-container/lib/projectChildren";
import { parseProjectPageTab } from "@/features/projects-container/ui/ProjectPageTabs";

const PROJECT =
  "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo";

/**
 * Pulse is a tab of the project page, not a sidebar row. The sidebar row
 * model is the contract: if a `pulse` type ever returns there, the row has
 * been re-added somewhere the design put it deliberately out of.
 */
test("the sidebar row model carries no Pulse row", () => {
  assert.equal("pulse" in PROJECT_CHILD_TYPE_RANK, false);
});

test("the child-type rank map stays gapless after the renumber", () => {
  const ranks = Object.values(PROJECT_CHILD_TYPE_RANK).sort((a, b) => a - b);
  assert.deepEqual(
    ranks,
    ranks.map((_, index) => index),
  );
});

/**
 * The tab strip, the flag gate and the redirect live in files that pull the
 * Tauri-backed screen tree and cannot be imported here, so the wiring is
 * asserted on their source — enough to catch the tab silently disappearing.
 */
function desktopSource(...segments) {
  const here = path.dirname(fileURLToPath(import.meta.url));
  return readFileSync(path.join(here, "..", "..", "..", ...segments), "utf8");
}

test("the project page hosts Pulse as a tab behind the flag and a real project", () => {
  const screen = desktopSource(
    "features",
    "projects-container",
    "ui",
    "ProjectContainerScreen.tsx",
  );
  assert.match(screen, /useFeatureEnabled\("project-pulse"\)/);
  assert.match(screen, /showPulse=\{pulseEnabled && !isFallback\}/);
  assert.match(
    screen,
    /<ProjectPulseScreen embedded projectId=\{project\.id\} \/>/,
  );
  // The overview card still offers "Open Pulse"; it switches tabs now.
  assert.match(screen, /search: \{ tab: "pulse" \}/);
});

test("the old Pulse route redirects onto the tab so deep links keep working", () => {
  const route = desktopSource("app", "routes", "projects.$projectId.pulse.tsx");
  assert.match(route, /redirect\(/);
  assert.match(route, /to: "\/projects\/\$projectId"/);
  assert.match(route, /search: \{ tab: "pulse" \}/);
});

test("the tab search param is parsed strictly", () => {
  assert.equal(parseProjectPageTab("pulse"), "pulse");
  assert.equal(parseProjectPageTab("overview"), "overview");
  assert.equal(parseProjectPageTab("PULSE"), "overview");
  assert.equal(parseProjectPageTab(undefined), "overview");
  assert.equal(parseProjectPageTab(42), "overview");
});

test("switching communities clears every folded digest", () => {
  resetProjectPulseState();
  const digest = foldProjectPulseDigest({
    project: PROJECT,
    now: 1_000,
    events: [],
  });
  rememberProjectPulseDigest(PROJECT, digest);
  assert.notEqual(
    readProjectPulseDigest(PROJECT),
    null,
    "a complete digest is banked",
  );
  resetProjectPulseState();
  assert.equal(
    readProjectPulseDigest(PROJECT),
    null,
    "one community's claims never paint under another community's project",
  );
});

test("a partial read is never banked as a project's last-good Pulse", () => {
  resetProjectPulseState();
  rememberProjectPulseDigest(
    PROJECT,
    foldProjectPulseDigest({
      project: PROJECT,
      now: 1_000,
      events: [],
      sourceErrors: [{ scope: "entries", message: "relay unavailable" }],
    }),
  );
  assert.equal(readProjectPulseDigest(PROJECT), null);
});

test("switching communities clears every banked mission read", async () => {
  const { readPulseMissionRows, rememberPulseMissionRows } = await import(
    "@/features/project-pulse/lib/projectPulseCache"
  );
  const { decodePulseMissionRows } = await import(
    "@/features/project-pulse/lib/pulseMissionWire"
  );
  const here = path.dirname(fileURLToPath(import.meta.url));
  const rows = decodePulseMissionRows({
    ...JSON.parse(
      readFileSync(
        path.join(here, "pulseMissionResponse.fixture.json"),
        "utf8",
      ),
    ),
    // Only a read that lost nothing is banked; see the next test.
    missionErrors: [],
  });
  resetProjectPulseState();
  rememberPulseMissionRows(PROJECT, rows);
  assert.notEqual(readPulseMissionRows(PROJECT), null);
  resetProjectPulseState();
  assert.equal(
    readPulseMissionRows(PROJECT),
    null,
    "one community's seats never paint under another community's project",
  );
});

test("a mission read that lost a session is never banked as last-good", async () => {
  // Same discipline as the digest: `missionErrors` means this read did not see
  // everything, and freezing it as the last-good answer would keep showing a
  // project as quieter than it is long after the relay recovered.
  const { readPulseMissionRows, rememberPulseMissionRows } = await import(
    "@/features/project-pulse/lib/projectPulseCache"
  );
  const { decodePulseMissionRows } = await import(
    "@/features/project-pulse/lib/pulseMissionWire"
  );
  const here = path.dirname(fileURLToPath(import.meta.url));
  const payload = JSON.parse(
    readFileSync(path.join(here, "pulseMissionResponse.fixture.json"), "utf8"),
  );
  resetProjectPulseState();
  rememberPulseMissionRows(PROJECT, decodePulseMissionRows(payload));
  assert.equal(readPulseMissionRows(PROJECT), null);

  payload.missionErrors = [];
  rememberPulseMissionRows(PROJECT, decodePulseMissionRows(payload));
  assert.notEqual(readPulseMissionRows(PROJECT), null);
});
