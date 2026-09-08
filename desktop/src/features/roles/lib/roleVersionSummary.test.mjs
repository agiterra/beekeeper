import assert from "node:assert/strict";
import test from "node:test";

import {
  roleReportSentence,
  rolesUncertaintySentence,
  summarizeRoleReports,
  summarizeRolesUncertainty,
} from "./roleVersionSummary.ts";

const SHA = "a".repeat(40);
const OTHER_SHA = "b".repeat(40);

let counter = 0;
function row(overrides = {}) {
  counter += 1;
  return {
    channelId: `channel-${counter}`,
    generationId: `generation-${counter}`,
    label: "Report",
    claimedByPubkey: "c".repeat(64),
    reportedAt: 1_700_000_000_000,
    coordinate: {
      repo: "30617:owner:packs",
      sha: SHA,
      role: "builder",
      path: "personas/roles/builder",
    },
    reason: null,
    relation: "current",
    behind: null,
    ahead: null,
    note: null,
    status: "running",
    ageSeconds: 60,
    adoption: "none",
    provenance: "commissioned",
    provenanceReason: null,
    founderPubkey: null,
    role: null,
    ...overrides,
  };
}

function snapshotsWith(reported) {
  return {
    resolvedAt: null,
    resolved: [],
    reported,
    comparison: null,
    provenanceNotes: [],
  };
}

// ── Grouping by the report's own role claim ──────────────────────────────

test("groups by coordinate.role when a full coordinate exists", () => {
  const summary = summarizeRoleReports(snapshotsWith([row()]));
  assert.deepEqual([...summary.keys()], ["builder"]);
});

test("falls back to the row's own role when there is no coordinate", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ coordinate: null, relation: "incomplete", role: "lead" }),
    ]),
  );
  assert.deepEqual([...summary.keys()], ["lead"]);
  const lead = summary.get("lead");
  assert.equal(lead.total, 1);
  assert.equal(lead.unknown, 1);
  assert.equal(lead.versions.length, 1);
  assert.equal(lead.versions[0].sha, null);
});

test("a report with neither a coordinate nor a role groups under the empty key", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ coordinate: null, relation: "incomplete", role: null }),
    ]),
  );
  assert.deepEqual([...summary.keys()], [""]);
  assert.equal(summary.get("").total, 1);
});

test("never infers a role from the label", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({
        coordinate: null,
        relation: "incomplete",
        role: null,
        label: "lead",
      }),
    ]),
  );
  assert.deepEqual([...summary.keys()], [""]);
});

// ── Version grouping: keyed by (sha, relation) ────────────────────────────

test("two reports with the same sha and relation merge into one version", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([row({ ageSeconds: 120 }), row({ ageSeconds: 10 })]),
  );
  const builder = summary.get("builder");
  assert.equal(builder.total, 2);
  assert.equal(builder.versions.length, 1);
  assert.equal(builder.versions[0].count, 2);
  // The newest of the group (smallest ageSeconds) is `latest`.
  assert.equal(builder.versions[0].latestAgeSeconds, 10);
});

test("same sha but different relation is a distinct version", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ relation: "current", ageSeconds: 5 }),
      row({ relation: "earlier", behind: 3, ageSeconds: 50 }),
    ]),
  );
  const builder = summary.get("builder");
  assert.equal(builder.versions.length, 2);
});

test("different sha is a distinct version even with the same relation", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ ageSeconds: 5 }),
      row({
        coordinate: {
          repo: "30617:owner:packs",
          sha: OTHER_SHA,
          role: "builder",
          path: "personas/roles/builder",
        },
        ageSeconds: 50,
      }),
    ]),
  );
  const builder = summary.get("builder");
  assert.equal(builder.versions.length, 2);
});

test("versions sort newest first by latestAgeSeconds, null last", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({
        coordinate: {
          repo: "30617:owner:packs",
          sha: OTHER_SHA,
          role: "builder",
          path: "personas/roles/builder",
        },
        ageSeconds: null,
      }),
      row({ ageSeconds: 500 }),
      row({ relation: "earlier", behind: 1, ageSeconds: 5 }),
    ]),
  );
  const builder = summary.get("builder");
  assert.equal(builder.versions.length, 3);
  assert.equal(builder.versions[0].latestAgeSeconds, 5);
  assert.equal(builder.versions[1].latestAgeSeconds, 500);
  assert.equal(builder.versions[2].latestAgeSeconds, null);
});

test("provenance counts come from each row's own provenance state", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ provenance: "commissioned" }),
      row({ provenance: "proof-unavailable" }),
      row({ provenance: "disputed" }),
    ]),
  );
  const version = summary.get("builder").versions[0];
  assert.equal(version.count, 3);
  assert.deepEqual(version.provenance, {
    commissioned: 1,
    unavailable: 1,
    disputed: 1,
  });
});

// ── Bucketing (sameAsHere / earlier / newer / other / unknown) ───────────

test("buckets current as sameAsHere, earlier/later as earlier/newer", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ relation: "current" }),
      row({ relation: "earlier", behind: 2 }),
      row({ relation: "later", ahead: 4 }),
    ]),
  );
  const builder = summary.get("builder");
  assert.equal(builder.sameAsHere, 1);
  assert.equal(builder.earlier, 1);
  assert.equal(builder.newer, 1);
  assert.equal(builder.other, 0);
  assert.equal(builder.unknown, 0);
});

test("buckets unrelated/different-source/shipped-differs/unknown-here as other, incomplete as unknown", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ relation: "unrelated" }),
      row({ relation: "different-source" }),
      row({ relation: "shipped-differs" }),
      row({ relation: "unknown-here" }),
      row({ relation: "incomplete", coordinate: null, role: "builder" }),
    ]),
  );
  const builder = summary.get("builder");
  assert.equal(builder.other, 4);
  assert.equal(builder.unknown, 1);
  assert.equal(builder.total, 5);
});

// ── roleReportSentence ─────────────────────────────────────────────────

test("roleReportSentence includes only non-zero parts, in order", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ relation: "current" }),
      row({ relation: "current" }),
      row({ relation: "earlier", behind: 1 }),
    ]),
  ).get("builder");
  assert.equal(
    roleReportSentence(summary),
    "3 reports · 2 on the same version as this computer · 1 on an earlier version",
  );
});

test("roleReportSentence uses singular 'report' for a count of one", () => {
  const summary = summarizeRoleReports(snapshotsWith([row()])).get("builder");
  assert.equal(
    roleReportSentence(summary),
    "1 report · 1 on the same version as this computer",
  );
});

test("roleReportSentence covers newer, other, and unknown parts", () => {
  const summary = summarizeRoleReports(
    snapshotsWith([
      row({ relation: "later", ahead: 2 }),
      row({ relation: "unrelated" }),
      row({ relation: "incomplete", coordinate: null, role: "builder" }),
    ]),
  ).get("builder");
  assert.equal(
    roleReportSentence(summary),
    "3 reports · 1 on a newer version · 1 on another version · 1 with no version reported",
  );
});

// ── summarizeRolesUncertainty / rolesUncertaintySentence ─────────────────

function uncertaintyInput(overrides = {}) {
  return {
    snapshots: snapshotsWith([]),
    resolvedError: null,
    resolvedIsStale: false,
    revisionsError: null,
    provenanceError: null,
    reports: { isLoading: false, error: null, authorityError: null },
    ...overrides,
  };
}

test("no phrases when nothing is uncertain", () => {
  const uncertainty = summarizeRolesUncertainty(uncertaintyInput());
  assert.deepEqual(uncertainty.phrases, []);
  assert.equal(rolesUncertaintySentence(uncertainty), "");
});

test("version check unavailable only when the resolved error is not a stale read", () => {
  const uncertainty = summarizeRolesUncertainty(
    uncertaintyInput({ resolvedError: "boom", resolvedIsStale: false }),
  );
  assert.deepEqual(uncertainty.phrases, ["version check unavailable"]);
});

test("a stale resolved read reports staleness instead of unavailability", () => {
  const uncertainty = summarizeRolesUncertainty(
    uncertaintyInput({ resolvedError: "boom", resolvedIsStale: true }),
  );
  assert.deepEqual(uncertainty.phrases, [
    "version check is from an earlier read",
  ]);
});

test("revision comparison and sender check failures each add their own phrase", () => {
  const uncertainty = summarizeRolesUncertainty(
    uncertaintyInput({
      revisionsError: "git exited 1",
      provenanceError: "relay closed",
    }),
  );
  assert.deepEqual(uncertainty.phrases, [
    "revision comparison unavailable",
    "sender checks unavailable",
  ]);
});

test("unconfirmed and disputed report counts, plus the no-role bucket", () => {
  const snapshots = snapshotsWith([
    row({ provenance: "proof-unavailable" }),
    row({ provenance: "disputed" }),
    row({ provenance: "commissioned" }),
    row({ coordinate: null, relation: "incomplete", role: null }),
  ]);
  const uncertainty = summarizeRolesUncertainty(
    uncertaintyInput({ snapshots }),
  );
  assert.deepEqual(uncertainty.phrases, [
    "1 of 4 reports could not be confirmed",
    "1 reports are disputed",
    "1 reports name no role",
  ]);
});

test("reports query states each add their own phrase, in order", () => {
  const uncertainty = summarizeRolesUncertainty(
    uncertaintyInput({
      reports: { isLoading: true, error: "partial", authorityError: "denied" },
    }),
  );
  assert.deepEqual(uncertainty.phrases, [
    "report list may be incomplete",
    "reports unavailable",
    "still reading reports",
  ]);
});

test("rolesUncertaintySentence joins every applicable phrase with ' · '", () => {
  const uncertainty = summarizeRolesUncertainty(
    uncertaintyInput({
      resolvedError: "boom",
      revisionsError: "git exited 1",
      reports: { isLoading: true, error: null, authorityError: null },
    }),
  );
  assert.equal(
    rolesUncertaintySentence(uncertainty),
    "version check unavailable · revision comparison unavailable · still reading reports",
  );
});

// ── Bounded by distinct versions, not row count ───────────────────────────

test("summarizeRoleReports is bounded by distinct versions and fast over 1,000 rows", () => {
  const shas = Array.from({ length: 5 }, (_, i) => `${i}`.padStart(40, "0"));
  const relations = [
    "current",
    "earlier",
    "later",
    "unrelated",
    "unknown-here",
  ];
  const rows = Array.from({ length: 1_000 }, (_, i) => {
    const sha = shas[i % shas.length];
    const relation = relations[Math.floor(i / shas.length) % relations.length];
    return row({
      coordinate: {
        repo: "30617:owner:packs",
        sha,
        role: "builder",
        path: "personas/roles/builder",
      },
      relation,
      behind: relation === "earlier" ? 1 : null,
      ahead: relation === "later" ? 1 : null,
      ageSeconds: i,
    });
  });

  const start = performance.now();
  const summary = summarizeRoleReports(snapshotsWith(rows)).get("builder");
  const elapsedMs = performance.now() - start;

  assert.equal(summary.total, 1_000);
  // Distinct (sha, relation) pairs: 5 shas × 5 relations = 25, however many
  // reports share one — never 1,000.
  assert.equal(summary.versions.length, 25);
  assert.ok(
    elapsedMs < 100,
    `summarizeRoleReports took ${elapsedMs}ms for 1,000 rows`,
  );
});

test("a sessions shelf that could not read everything is said in the uncertainty line", () => {
  assert.deepEqual(
    summarizeRolesUncertainty(
      uncertaintyInput({ sessions: { kind: "partial" } }),
    ).phrases,
    ["some sessions could not be read"],
  );
  assert.deepEqual(
    summarizeRolesUncertainty(
      uncertaintyInput({ sessions: { kind: "unavailable" } }),
    ).phrases,
    ["sessions unavailable"],
  );
  assert.deepEqual(
    summarizeRolesUncertainty(uncertaintyInput({ sessions: { kind: "ready" } }))
      .phrases,
    [],
  );
});
