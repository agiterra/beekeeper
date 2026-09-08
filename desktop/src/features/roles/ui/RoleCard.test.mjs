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

// ── Reported by agents: the three cases stay distinct ────────────────────

test("a role with reports shows the summary sentence and one line per version", () => {
  const html = render({
    role: role(),
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
  assert.match(
    html,
    /3 reports · 2 on the same version as this computer · 1 on an earlier version/,
  );
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
    role: role(),
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

  assert.match(html, /1 report · 1 with no version reported/);
  assert.match(
    html,
    /version not reported · 1 report · latest time not reported · sender unconfirmed/,
  );
  assert.match(html, /data-reports="some"/);
  // A missing version is never rendered as a sha or as "same version".
  assert.doesNotMatch(html, /same version as here/);
});

test("a role no agent has reported says so instead of showing nothing", () => {
  const html = render({ role: role(), reports: null });

  assert.match(html, /data-reports="none"/);
  assert.match(html, /No agent has reported running this role yet\./);
  const empty = render({
    role: role(),
    reports: summary({ total: 0, sameAsHere: 0, versions: [] }),
  });
  assert.match(empty, /data-reports="none"/);
  assert.match(empty, /No agent has reported running this role yet\./);
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
    role: role(),
    reports: summary({ total: 8, sameAsHere: 0, other: 8, versions }),
  });

  assert.equal(
    (html.match(/data-testid="role-report-version"/g) ?? []).length,
    5,
  );
  assert.match(html, /and 3 more versions in Report history/);
});

// ── Available here ───────────────────────────────────────────────────────

test("the availability line names the version, commit and place for a project role", () => {
  const html = render({ role: role() });

  assert.match(html, /data-testid="role-available-builder"/);
  assert.match(html, /data-availability="available"/);
  assert.match(
    html,
    /Available here: v1\.3\.0 \(9f2e1d0c\) from the project&#x27;s repository/,
  );
  assert.match(html, new RegExp(`title="${PROJECT_SHA}"`));
});

test("built-in defaults and this-computer-only roles say which they are", () => {
  const shipped = render({
    role: role({
      origin: "shipped",
      version: "0.4.2",
      packRef: { repo: "app:shipped", sha: "0.4.2" },
    }),
  });
  assert.match(
    shipped,
    /Available here: v0\.4\.2 from this app&#x27;s built-in defaults/,
  );

  const local = render({
    role: role({ origin: "installed", version: null, packRef: null }),
  });
  assert.match(
    local,
    /Available here: version not recorded from this computer only/,
  );
});

test("a role with no instructions here is unavailable and says why", () => {
  const missing = render({
    role: role({
      hasPack: false,
      version: null,
      origin: null,
      packRef: null,
      description: "",
      summary: "",
    }),
  });
  assert.match(missing, /data-availability="unavailable"/);
  assert.match(
    missing,
    /Not available here — No instructions for this role are on this computer/,
  );

  const refused = render({
    role: role({ refusal: "This project's repository is not synced here." }),
  });
  assert.match(refused, /data-availability="unavailable"/);
  assert.match(
    refused,
    /Not available here — This project&#x27;s repository is not synced here\./,
  );
  // The refusal is carried once, in the availability line.
  assert.doesNotMatch(refused, /data-testid="role-refusal-builder"/);
});

// ── Sessions, skills, header ─────────────────────────────────────────────

test("the sessions and agents sections keep their ids and plain empty sentences", () => {
  const html = render({ role: role() });

  assert.match(html, /data-testid="role-seats-builder"/);
  assert.match(html, /No open sessions\./);
  assert.match(html, /data-testid="role-agents-builder"/);
  assert.match(html, /No agent has this role yet\./);
  assert.match(html, />Sessions</);
  assert.doesNotMatch(html, />Seats</);
});

test("skills are a collapsed disclosure carrying their own count and empty sentence", () => {
  const empty = render({ role: role() });
  assert.match(empty, /<details data-testid="role-skills-builder">/);
  assert.match(empty, /Skills \(0\)/);
  assert.match(empty, /No skills listed\./);
  // Collapsed: no `open` attribute on the disclosure.
  assert.doesNotMatch(empty, /<details data-testid="role-skills-builder" open/);

  const filled = render({
    role: role({
      skills: [
        { name: "review", description: "Reads a diff", shared: false },
        { name: "shell", description: "", shared: true },
      ],
    }),
  });
  assert.match(filled, /Skills \(2\)/);
  assert.equal((filled.match(/data-testid="role-skill"/g) ?? []).length, 2);
  assert.match(filled, /\(shared\)/);
});

test("the header carries the name and slug, and no origin badge or bare version", () => {
  const html = render({ role: role() });

  assert.match(html, />Builder</);
  assert.match(html, /data-testid="role-slug-builder"/);
  assert.doesNotMatch(html, /data-testid="role-origin-builder"/);
  assert.doesNotMatch(html, /data-testid="role-version-builder"/);
});
