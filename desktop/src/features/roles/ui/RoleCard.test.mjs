import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";

import { RoleCard } from "./RoleCard.tsx";

const PROJECT_SHA = "9f2e1d0c".padEnd(40, "a");
const EARLIER_SHA = "dd11ee22".padEnd(40, "b");

function role(overrides = {}) {
  return {
    role: "builder",
    hasPack: true,
    displayName: "Builder",
    description: "Builds one bounded change and reports.",
    summary: "Reads the spec, writes the change, runs the gate.",
    version: "1.3.0",
    origin: "project",
    packDir: "/packs/personas/roles/builder",
    packRef: { repo: "30617:owner:packs", sha: PROJECT_SHA },
    skills: [],
    refusal: null,
    agents: [],
    seats: [],
    ...overrides,
  };
}

function agent(overrides = {}) {
  return {
    pubkey: "a".repeat(64),
    name: "Ada",
    hasRolePack: true,
    packRefusedSharedHome: false,
    status: "running",
    avatarUrl: null,
    runtime: "goose",
    model: "sonnet",
    ...overrides,
  };
}

function seat(overrides = {}) {
  return {
    key: "channel-1/generation-1",
    channelId: "channel-1",
    generationId: "generation-1",
    label: "Build the card",
    agentName: "Ada",
    agentPubkey: "a".repeat(64),
    projectId: "p1",
    projectName: "Beekeeper",
    role: "builder",
    status: "running",
    ageSeconds: 300,
    packSha: PROJECT_SHA,
    ...overrides,
  };
}

function reportedRow(overrides = {}) {
  return {
    channelId: "project-transport",
    generationId: "generation-1",
    label: "Build the card",
    claimedByPubkey: "f".repeat(64),
    reportedAt: 1_700_000_000_000,
    coordinate: {
      repo: "30617:owner:packs",
      sha: PROJECT_SHA,
      role: "builder",
      path: "personas/roles/builder",
    },
    reason: null,
    relation: "current",
    behind: null,
    ahead: null,
    note: null,
    status: "running",
    ageSeconds: 10,
    adoption: "none",
    provenance: "commissioned",
    provenanceReason: null,
    founderPubkey: null,
    role: "builder",
    ...overrides,
  };
}

function version(overrides = {}) {
  return {
    sha: PROJECT_SHA,
    relation: "current",
    behind: null,
    ahead: null,
    count: 2,
    latestAgeSeconds: 10,
    provenance: { commissioned: 2, unavailable: 0, disputed: 0 },
    ...overrides,
    latest: reportedRow(overrides.latest ?? {}),
  };
}

function summary(overrides = {}) {
  return {
    role: "builder",
    total: 2,
    sameAsHere: 2,
    earlier: 0,
    newer: 0,
    other: 0,
    unknown: 0,
    versions: [version()],
    ...overrides,
  };
}

function render(props) {
  return renderToStaticMarkup(React.createElement(RoleCard, props));
}

// ── The activity dot: colour only for a state a session reported ─────────

test("the leading dot is green only while a session in this role is running", () => {
  const running = render({ role: role({ seats: [seat()] }) });
  assert.match(running, /data-testid="role-activity-builder"/);
  assert.match(running, /data-activity="running"/);
  assert.match(running, /bg-emerald-500/);
  assert.match(running, /aria-label="A session in this role is running"/);

  const idle = render({ role: role({ seats: [seat({ status: "idle" })] }) });
  assert.match(idle, /data-activity="idle"/);
  assert.match(idle, /aria-label="This role has open sessions, none running"/);
  assert.doesNotMatch(idle, /bg-emerald-500/);

  const none = render({ role: role() });
  assert.match(none, /data-activity="none"/);
  assert.match(none, /aria-label="No open sessions in this role"/);
  assert.match(none, /bg-muted-foreground\/45/);
});

// ── The version chip: short on the face, whole sentence in the tooltip ───

test("the version chip names the version and commit, with the sentence in its title", () => {
  const html = render({ role: role() });

  assert.match(html, /data-testid="role-available-builder"/);
  assert.match(html, /data-availability="available"/);
  assert.match(html, />v1\.3\.0 · 9f2e1d0c</);
  // The whole sentence, plus the full 40-hex commit, stay in the tooltip.
  assert.match(
    html,
    /title="Available here: v1\.3\.0 \(9f2e1d0c\) from the project&#x27;s repository · 9f2e1d0ca{32}"/,
  );
});

test("built-in defaults and this-computer-only roles keep their own chip text", () => {
  const shipped = render({
    role: role({
      origin: "shipped",
      version: "0.4.2",
      packRef: { repo: "app:shipped", sha: "0.4.2" },
    }),
  });
  assert.match(shipped, />v0\.4\.2</);
  assert.match(shipped, /title="Available here: v0\.4\.2 from this app/);

  const local = render({
    role: role({ origin: "installed", version: null, packRef: null }),
  });
  assert.match(local, />version not recorded</);
});

test("a role with no instructions here says so in amber, with the reason disclosed", () => {
  const missing = render({
    role: role({
      hasPack: false,
      version: null,
      origin: null,
      packRef: null,
      summary: "",
    }),
  });
  assert.match(missing, /data-availability="unavailable"/);
  assert.match(missing, />Not available here</);
  assert.match(missing, /text-amber-600 dark:text-amber-400/);
  // The reason is the chip's tooltip, and the whole sentence is one click away.
  assert.match(missing, /title="No instructions for this role are on this/);
  assert.match(missing, /data-testid="role-unavailable-builder"/);
  assert.match(missing, />Why not available</);
  assert.match(missing, /Not available here — No instructions for this role/);

  const refused = render({
    role: role({ refusal: "This project's repository is not synced here." }),
  });
  assert.match(refused, /data-availability="unavailable"/);
  assert.match(
    refused,
    /title="This project&#x27;s repository is not synced here\."/,
  );
});

// ── Agents: a face, a name, and a state somebody reported ────────────────

test("an agent row carries its avatar, its name and its status word", () => {
  const html = render({ role: role({ agents: [agent()] }) });

  assert.match(html, /data-testid="role-agents-builder"/);
  assert.match(html, /data-testid="role-agent-chip"/);
  assert.match(html, /data-agent-scope="local"/);
  assert.match(html, /data-agent-pack="present"/);
  // UserAvatar size "sm".
  assert.match(html, /h-6 w-6/);
  assert.match(html, />Ada</);
  // Colour never travels alone: the word is in the title beside the dot.
  assert.match(html, /title="running"/);
  assert.match(html, /bg-emerald-500/);

  const deployed = render({
    role: role({ agents: [agent({ status: "deployed" })] }),
  });
  assert.match(deployed, /bg-sky-500/);
  assert.match(deployed, /title="deployed"/);

  const stopped = render({
    role: role({ agents: [agent({ status: "not_deployed" })] }),
  });
  assert.match(stopped, /title="not deployed"/);
  assert.doesNotMatch(stopped, /bg-emerald-500|bg-sky-500/);
});

test("an agent whose run state nobody reported gets no dot and says so", () => {
  const html = render({
    role: role({ agents: [agent({ status: undefined })] }),
  });

  assert.match(html, /title="status not reported here"/);
  // No dot on the chip at all — a grey one would read as "stopped", which
  // nobody claimed. The card's own activity dot is the only one left.
  const dots = (html) =>
    (html.match(/inline-block size-1\.5 shrink-0 rounded-full/g) ?? []).length;
  assert.equal(dots(html), 1);
  assert.equal(dots(render({ role: role({ agents: [agent()] }) })), 2);
});

test("an agent this computer does not manage is marked shared and claims nothing about it", () => {
  const html = render({
    role: role({ agents: [agent({ isManagedHere: false })] }),
  });

  assert.match(html, /data-agent-scope="shared"/);
  assert.match(html, /data-agent-pack="unknown"/);
  assert.match(html, />shared</);
  assert.match(
    html,
    /title="Seen in this project&#x27;s sessions or channels; not set up on this computer\./,
  );
  assert.match(
    html,
    /title="Whether this agent has this role&#x27;s instructions on its own machine is not known here\."/,
  );
  // Never a run state, and never the pack answer for a machine this is not.
  assert.doesNotMatch(html, /bg-emerald-500|bg-sky-500/);
  assert.doesNotMatch(html, /data-agent-pack="missing"/);
  // The dashed outline every non-present pack state carries.
  assert.match(html, /border-dashed/);
});

test("a pack state the role itself refuses keeps its dashed chip and its reason", () => {
  const html = render({
    role: role({
      hasPack: false,
      agents: [agent({ hasRolePack: false })],
    }),
  });
  assert.match(html, /data-agent-pack="missing"/);
  assert.match(html, /border-dashed/);
});

test("a role with reports but no agents says only that", () => {
  const html = render({
    role: role(),
    reports: summary(),
  });
  assert.match(html, /data-testid="role-agents-builder-empty"/);
  assert.match(html, />No agents yet\.</);
});

// ── Reports: one line on the face, the versions behind a disclosure ──────

test("the reports line abbreviates the counts and keeps the long sentence in its title", () => {
  const html = render({
    role: role({ agents: [agent()] }),
    reports: summary({
      total: 3,
      sameAsHere: 2,
      earlier: 1,
      versions: [
        version(),
        version({
          sha: EARLIER_SHA,
          relation: "earlier",
          behind: 1,
          count: 1,
          latestAgeSeconds: 120,
          provenance: { commissioned: 0, unavailable: 1, disputed: 0 },
        }),
      ],
    }),
  });

  assert.match(html, /data-testid="role-reports-builder"/);
  assert.match(html, /data-reports="some"/);
  assert.match(html, />3 reports · 2 same version · 1 earlier</);
  assert.match(
    html,
    /title="3 reports · 2 on the same version as this computer · 1 on an earlier version"/,
  );
  // The per-version detail is one click away, counted in the summary.
  assert.match(html, /data-testid="role-versions-builder"/);
  assert.match(html, />Versions \(2\)</);
  assert.match(
    html,
    /9f2e1d0c · same version as here · 2 reports · latest just now · sender confirmed/,
  );
  assert.match(
    html,
    /dd11ee22 · earlier, 1 behind · 1 report · latest 2m ago · sender unconfirmed/,
  );
  // The long labels are the tooltip, never the card's own words.
  assert.match(html, /title="Same revision as this machine · Reported by the/);
  assert.doesNotMatch(html, />Reported by the assigned provider</);
  // Never says instructions ran.
  assert.doesNotMatch(html, /ran the|executed|instructions ran/i);
});

test("a report that named this role without a version says exactly that", () => {
  const html = render({
    role: role({ agents: [agent()] }),
    reports: summary({
      total: 1,
      sameAsHere: 0,
      unknown: 1,
      versions: [
        version({
          sha: null,
          relation: "incomplete",
          count: 1,
          latestAgeSeconds: null,
          provenance: { commissioned: 0, unavailable: 1, disputed: 0 },
        }),
      ],
    }),
  });

  assert.match(html, />1 report · 1 no version reported</);
  assert.match(
    html,
    /version not reported · 1 report · latest time not reported · sender unconfirmed/,
  );
  assert.match(html, /data-reports="some"/);
  // A missing version is never rendered as a sha or as "same version".
  assert.doesNotMatch(html, /same version as here/);
});

test("a disputed report leads the line, in the word and in the colour", () => {
  const html = render({
    role: role({ agents: [agent()] }),
    reports: summary({
      total: 1,
      sameAsHere: 1,
      versions: [
        version({
          count: 1,
          provenance: { commissioned: 0, unavailable: 0, disputed: 1 },
        }),
      ],
    }),
  });

  assert.match(html, /text-destructive/);
  assert.match(html, />disputed</);
});

test("a role with agents but no reports says so in one short line", () => {
  const html = render({ role: role({ agents: [agent()] }), reports: null });

  assert.match(html, /data-reports="none"/);
  assert.match(html, />No reports yet\.</);
  assert.doesNotMatch(html, /data-testid="role-versions-builder"/);

  const empty = render({
    role: role({ agents: [agent()] }),
    reports: summary({ total: 0, sameAsHere: 0, versions: [] }),
  });
  assert.match(empty, /data-reports="none"/);
  assert.match(empty, />No reports yet\.</);
});

test("the card never grows past five version lines, and counts the rest", () => {
  const versions = Array.from({ length: 8 }, (_, index) =>
    version({
      sha: `${index}`.repeat(8).padEnd(40, "c"),
      relation: "unrelated",
      count: 1,
    }),
  );
  const html = render({
    role: role({ agents: [agent()] }),
    reports: summary({ total: 8, sameAsHere: 0, other: 8, versions }),
  });

  assert.equal(
    (html.match(/data-testid="role-report-version"/g) ?? []).length,
    5,
  );
  assert.match(html, />Versions \(8\)</);
  assert.match(html, /and 3 more versions in Report history/);
});

// ── Sessions: drawn only when there are some, project column kept ────────

test("sessions are drawn only when the role has one, with a status dot and the project", () => {
  const withSeat = render({
    role: role({ seats: [seat({ status: "waiting_for_input" })] }),
  });
  assert.match(withSeat, /data-testid="role-seats-builder"/);
  assert.match(withSeat, /data-seat-status="waiting_for_input"/);
  assert.match(withSeat, /bg-amber-500/);
  assert.match(withSeat, /title="waiting_for_input"/);
  // Root's project-scope fix is pending, so the row keeps saying which project.
  assert.match(withSeat, /data-seat-column="project"[^>]*>Beekeeper</);

  const none = render({ role: role({ agents: [agent()] }) });
  assert.doesNotMatch(none, /data-testid="role-seats-builder"/);
  assert.doesNotMatch(none, /No open sessions\./);
});

// ── The quiet card, and the foot disclosure ──────────────────────────────

test("a role nothing is using says it once, not three times", () => {
  const html = render({ role: role(), reports: null });

  assert.match(html, /data-testid="role-quiet-builder"/);
  assert.match(html, />No agents or sessions observed for this role\.</);
  // Still discloses that there are no reports, for anything reading the state.
  assert.match(html, /data-reports="none"/);
  // And none of the three sentences it replaces.
  assert.doesNotMatch(html, />No agents yet\.</);
  assert.doesNotMatch(html, />No reports yet\.</);
  assert.doesNotMatch(html, /No open sessions\./);
});

test("the long description and the skills live behind one foot disclosure", () => {
  const empty = render({ role: role() });
  assert.match(
    empty,
    /<details class="mt-auto" data-testid="role-about-builder">/,
  );
  assert.match(empty, />About this role · Skills \(0\)</);
  assert.match(empty, /data-testid="role-summary-builder"/);
  assert.match(empty, />No skills listed\.</);
  // Collapsed: no `open` attribute on the disclosure.
  assert.doesNotMatch(empty, /data-testid="role-about-builder" open/);
  // The summary paragraph is off the card's face.
  assert.doesNotMatch(
    empty,
    /class="line-clamp-3[^"]*"[^>]*data-testid="role-summary-builder"/,
  );

  const filled = render({
    role: role({
      skills: [
        { name: "review", description: "Reads a diff", shared: false },
        { name: "shell", description: "", shared: true },
      ],
    }),
  });
  assert.match(filled, />About this role · Skills \(2\)</);
  // The id existing assertions use stays on the inner list container.
  assert.match(filled, /<ul[^>]*data-testid="role-skills-builder"/);
  assert.equal((filled.match(/data-testid="role-skill"/g) ?? []).length, 2);
  assert.match(filled, /\(shared\)/);
});

test("the title row carries the name, the slug and no bare version or origin badge", () => {
  const html = render({ role: role() });

  assert.match(html, />Builder</);
  assert.match(html, /data-testid="role-slug-builder"/);
  assert.match(html, /data-testid="role-description-builder"/);
  assert.doesNotMatch(html, /data-testid="role-origin-builder"/);
  assert.doesNotMatch(html, /data-testid="role-version-builder"/);
});
