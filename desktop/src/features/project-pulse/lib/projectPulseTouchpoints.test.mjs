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
import {
  PROJECT_CHILD_TYPE_RANK,
  buildProjectChildren,
  projectChildKey,
  projectChildLabel,
} from "@/features/projects-container/lib/projectChildren";

const PROJECT =
  "30621:1111111111111111111111111111111111111111111111111111111111111111:pulse-demo";

function emptyChildInput(overrides = {}) {
  return {
    streamChannels: [],
    forumChannels: [],
    repos: [],
    workflows: [],
    agents: [],
    shellSessions: [],
    ...overrides,
  };
}

test("the Pulse row is opt-in and sits directly below coding sessions", () => {
  assert.equal(buildProjectChildren(emptyChildInput()).length, 0);
  const rows = buildProjectChildren(emptyChildInput({ includePulse: true }));
  assert.deepEqual(rows, [{ type: "pulse" }]);
  assert.equal(
    PROJECT_CHILD_TYPE_RANK.pulse - PROJECT_CHILD_TYPE_RANK["coding-session"],
    1,
  );
  assert.ok(
    PROJECT_CHILD_TYPE_RANK.channel > PROJECT_CHILD_TYPE_RANK.pulse,
    "channels follow Pulse",
  );
});

/**
 * `buildProjectChildren` only emits the Pulse row when its caller opts in, so
 * a green unit test above proves nothing about the sidebar actually showing
 * it. These two files cannot be imported here (they pull the Tauri-backed
 * sidebar tree), so the wiring is asserted on their source — enough to catch
 * the row silently disappearing from the sidebar again.
 */
function sidebarSource(file) {
  const here = path.dirname(fileURLToPath(import.meta.url));
  return readFileSync(
    path.join(here, "..", "..", "projects-container", "ui", file),
    "utf8",
  );
}

test("the sidebar group opts into the Pulse row behind the flag and a real project", () => {
  const group = sidebarSource("ProjectSidebarGroup.tsx");
  assert.match(group, /useFeatureEnabled\("project-pulse"\)/);
  assert.match(group, /!isFallback/);
  assert.match(group, /includePulse: pulseEnabled/);
  assert.match(group, /onOpenPulse=\{onOpenPulse\}/);
});

test("the sidebar sections give the Pulse row somewhere to navigate", () => {
  const sections = sidebarSource("ProjectSidebarSections.tsx");
  assert.match(sections, /onOpenPulse=\{/);
  assert.match(sections, /"\/projects\/\$projectId\/pulse"/);
});

test("the child-type rank map stays gapless after the renumber", () => {
  const ranks = Object.values(PROJECT_CHILD_TYPE_RANK).sort((a, b) => a - b);
  assert.deepEqual(
    ranks,
    ranks.map((_, index) => index),
  );
});

/**
 * Two features called "Pulse" in one sidebar — the social activity feed and
 * this per-project coordination view — is a scan the tooltip cannot fix. The
 * project row carries the feature's own display name.
 */
test("the Pulse row has a stable key and an unambiguous label", () => {
  assert.equal(projectChildKey({ type: "pulse" }), "pulse");
  assert.equal(projectChildLabel({ type: "pulse" }), "Project Pulse");
});

/**
 * §5.6 touchpoint 1 puts Pulse with the live work it describes. Bucketed into
 * `toolRows` it rendered under "Repos & Tools", next to repos and workflows —
 * a collapsible drawer nobody opens to ask "what is happening right now".
 */
test("the sidebar renders Pulse with the live work, not under Repos & Tools", () => {
  const group = sidebarSource("ProjectSidebarGroup.tsx");
  assert.match(group, /const pulseRows = children\.filter/);
  assert.match(group, /row\.type !== "pulse"/);
  assert.match(group, /\{pulseRows\.map\(renderRow\)\}/);
  const toolSection = group.slice(group.indexOf("const toolRows"));
  assert.ok(
    toolSection.indexOf("{pulseRows.map(renderRow)}") <
      toolSection.indexOf('label="Repos & Tools"'),
    "the Pulse row is emitted before the Repos & Tools section",
  );
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
