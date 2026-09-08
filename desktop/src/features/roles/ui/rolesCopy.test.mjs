import assert from "node:assert/strict";
import { test } from "node:test";

import {
  AGENT_NO_ROLE_PACK_LABEL,
  AGENT_NO_ROLE_PACK_REMEDY,
  AGENT_SHARED_HOME_LABEL,
  AGENT_SHARED_HOME_REMEDY,
} from "@/features/agents/ui/AgentHomeRoleBadges";
import {
  ADOPTION_KEEPS_UNTIL_NEXT_GENERATION,
  AGE_UNKNOWN,
  AGENT_CHIP_NO_PACK_TITLE,
  AGENT_CHIP_SHARED_HOME_TITLE,
  AGENTS_BY_PROJECT_TITLE,
  checkoutAnsweredSentence,
  formatAge,
  INSTALL_ROLES_BUTTON_LABEL,
  PACKS_SOURCE_MIXED,
  packsSourceSentence,
  PROJECT_PACKS_MISSING,
  PROJECT_SEATS_EMPTY,
  projectSeatCount,
  PROVENANCE_LABEL_COMMISSIONED,
  PROVENANCE_LABEL_DISPUTED,
  PROVENANCE_LABEL_PROOF_UNAVAILABLE,
  PROVENANCE_MIXED_SHORT,
  provenanceLabelText,
  provenanceNotesSentence,
  provenanceShortText,
  reportCountText,
  reportedAgoText,
  reportHistorySummary,
  reportLatestText,
  revisionComparisonUnavailableSentence,
  revisionRelationShortText,
  revisionRelationText,
  ROLE_AGENTS_EMPTY,
  ROLE_NO_PACK,
  ROLE_PACK_SNAPSHOTS_SUBTITLE,
  ROLE_REPORTS_EMPTY,
  ROLE_REPORTS_TITLE,
  ROLE_SEATS_EMPTY,
  ROLE_SEATS_TITLE,
  ROLE_SKILLS_EMPTY,
  ROLE_VERSION_UNRECORDED,
  roleAvailabilitySentence,
  roleReportProvenanceText,
  roleReportsMoreText,
  roleReportVersionLine,
  ROLES_EMPTY,
  ROLES_RECHECK_BUSY_LABEL,
  ROLES_RECHECK_LABEL,
  ROLES_SUBTITLE,
  ROLES_TECHNICAL_DETAILS_TITLE,
  ROLES_TITLE,
  ROLES_UNCERTAINTY_NONE,
  rolesErrorSentence,
  roleSkillsSummary,
  SEAT_NO_AGENT,
  SEAT_UNMANAGED_AGENT,
  seatStatusText,
  SHA_UNKNOWN,
  shaText,
  SHARED_SKILL_MARK,
  UNPLACED_TITLE,
} from "@/features/roles/ui/rolesCopy";

/** Words the usability spec bans from the header, the source line and the cards. */
const JARGON =
  /\b(seat|seats|pack|packs|rung|rungs|coordinate|coordinates|genesis|commissioned|provider)\b/i;

test("the empty states are the usability spec's exact words", () => {
  assert.equal(ROLES_TITLE, "Roles");
  assert.equal(ROLE_SKILLS_EMPTY, "No skills listed.");
  assert.equal(ROLE_AGENTS_EMPTY, "No agent has this role yet.");
  // A session is what an agent opened; the card no longer calls it a seat.
  assert.equal(ROLE_SEATS_TITLE, "Sessions");
  assert.equal(ROLE_SEATS_EMPTY, "No open sessions.");
  assert.equal(PROJECT_SEATS_EMPTY, "No agents in open sessions.");
  assert.equal(AGENTS_BY_PROJECT_TITLE, "Sessions by project");
  assert.equal(ROLE_REPORTS_TITLE, "Reported by agents");
  assert.equal(
    ROLE_REPORTS_EMPTY,
    "No agent has reported running this role yet.",
  );
  assert.equal(UNPLACED_TITLE, "Unplaced");
  assert.equal(SHA_UNKNOWN, "version unknown");
  assert.equal(SHARED_SKILL_MARK, "(shared)");
  assert.equal(
    ROLES_EMPTY,
    "No roles are available for this project, and no agent or open session names one.",
  );
  assert.equal(PROJECT_PACKS_MISSING, "This project is not readable here.");
  assert.equal(INSTALL_ROLES_BUTTON_LABEL, "Install roles");
  assert.equal(ROLES_RECHECK_LABEL, "Check again");
  assert.equal(ROLES_RECHECK_BUSY_LABEL, "Checking…");
  assert.equal(ROLES_TECHNICAL_DETAILS_TITLE, "Technical details");
});

test("the header sentence says what a role is and what this page answers", () => {
  assert.match(ROLES_SUBTITLE, /^Each role is a set of instructions/);
  assert.match(ROLES_SUBTITLE, /which agents can take it/);
  assert.match(
    ROLES_SUBTITLE,
    /available on this computer and reported by running agents\.$/,
  );
  assert.doesNotMatch(ROLES_SUBTITLE, JARGON);
});

test("the primary copy carries none of the protocol vocabulary", () => {
  for (const sentence of [
    ROLES_SUBTITLE,
    ROLES_EMPTY,
    ROLE_NO_PACK,
    ROLE_AGENTS_EMPTY,
    ROLE_SEATS_EMPTY,
    ROLE_SEATS_TITLE,
    ROLE_REPORTS_EMPTY,
    ROLE_REPORTS_TITLE,
    PROJECT_SEATS_EMPTY,
    AGENTS_BY_PROJECT_TITLE,
    ROLES_UNCERTAINTY_NONE,
    PACKS_SOURCE_MIXED,
    packsSourceSentence("Beekeeper", 0, {
      origins: [],
      location: null,
      sha: null,
      shasDiffer: false,
    }).text,
  ]) {
    assert.doesNotMatch(sentence, JARGON, sentence);
  }
});

test("the agent chip tooltips are the Agents tab's own badge copy, unchanged", () => {
  assert.equal(
    AGENT_CHIP_NO_PACK_TITLE,
    `${AGENT_NO_ROLE_PACK_LABEL} — ${AGENT_NO_ROLE_PACK_REMEDY}`,
  );
  assert.equal(
    AGENT_CHIP_SHARED_HOME_TITLE,
    `${AGENT_SHARED_HOME_LABEL} — ${AGENT_SHARED_HOME_REMEDY}`,
  );
  assert.match(AGENT_CHIP_NO_PACK_TITLE, /^Role pack not installed here — /);
});

test("a role with no instructions here says why it is listed at all", () => {
  assert.match(ROLE_NO_PACK, /^No instructions for this role/);
  assert.match(ROLE_NO_PACK, /an agent or an open session names it/);
  assert.equal(SEAT_NO_AGENT, "no agent");
  assert.equal(SEAT_UNMANAGED_AGENT, "unmanaged agent");
});

test("formatAge is compact and discloses an unknown age", () => {
  assert.equal(formatAge(null), AGE_UNKNOWN);
  assert.equal(formatAge(0), "just now");
  assert.equal(formatAge(59), "just now");
  assert.equal(formatAge(60), "1m");
  assert.equal(formatAge(3_599), "59m");
  assert.equal(formatAge(3_600), "1h");
  assert.equal(formatAge(86_399), "23h");
  assert.equal(formatAge(86_400), "1d");
  assert.equal(formatAge(3 * 86_400 + 5), "3d");
});

test("seat status is the catalog word verbatim, then the age", () => {
  assert.equal(seatStatusText("running", 300), "running (5m)");
  assert.equal(
    seatStatusText("waiting_for_input", null),
    "waiting_for_input (age unknown)",
  );
});

test("shaText shortens a commit the way the seat pack line does, and never invents one", () => {
  assert.equal(shaText("a".repeat(40)), "aaaaaaaa");
  assert.equal(shaText("0.4.2-block"), "0.4.2-block");
  assert.equal(shaText(null), "version unknown");
});

test("projectSeatCount pluralizes", () => {
  assert.equal(projectSeatCount(0), "0 agents");
  assert.equal(projectSeatCount(1), "1 agent");
  assert.equal(projectSeatCount(2), "2 agents");
});

test("rolesErrorSentence attributes the backend's words", () => {
  assert.equal(
    rolesErrorSentence("no checkout recorded"),
    "Role instructions could not be read: no checkout recorded",
  );
});

test("the source line names one place in plain words, and refuses to average", () => {
  const sha = "c".repeat(40);
  assert.deepEqual(
    packsSourceSentence(
      "Beekeeper",
      2,
      {
        origins: ["project"],
        location: "30617:owner:packs",
        sha,
        shasDiffer: false,
      },
      { checkedAgeSeconds: 300, shippedVersion: null },
    ),
    {
      text: `Instructions come from the project's repository at ${sha.slice(0, 8)} · checked 5m ago`,
      shaTitle: sha,
    },
  );
  // No local check time recorded is disclosed, never rendered as "just now".
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 1, {
      origins: ["project"],
      location: "30617:owner:packs",
      sha,
      shasDiffer: false,
    }),
    {
      text: `Instructions come from the project's repository at ${sha.slice(0, 8)} · last check time not recorded`,
      shaTitle: sha,
    },
  );
  assert.deepEqual(
    packsSourceSentence(
      "Beekeeper",
      1,
      {
        origins: ["shipped"],
        location: "/app/shipped",
        sha: "0.4.2-block",
        shasDiffer: false,
      },
      { checkedAgeSeconds: 10, shippedVersion: "0.4.2" },
    ),
    {
      text: "Instructions come from this app's built-in defaults (v0.4.2).",
      shaTitle: null,
    },
  );
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 1, {
      origins: ["installed"],
      location: "/packs",
      sha: null,
      shasDiffer: false,
    }),
    {
      text: "Instructions come from this computer only (not shared with the project).",
      shaTitle: null,
    },
  );
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 1, {
      origins: ["checkout"],
      location: "/checkout/personas",
      sha: null,
      shasDiffer: false,
    }),
    {
      text: "Instructions come from this computer only (not shared with the project).",
      shaTitle: null,
    },
  );
  // Two rungs: the line points at Technical details rather than picking one.
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 2, {
      origins: ["project", "shipped"],
      location: null,
      sha: null,
      shasDiffer: true,
    }),
    { text: PACKS_SOURCE_MIXED, shaTitle: null },
  );
  // One rung, different commits per role: still never one sha in the tooltip.
  assert.deepEqual(
    packsSourceSentence(
      "Beekeeper",
      2,
      {
        origins: ["project"],
        location: "30617:owner:packs",
        sha: null,
        shasDiffer: true,
      },
      { checkedAgeSeconds: 0, shippedVersion: null },
    ),
    {
      text: "Instructions come from the project's repository, at a different commit for each role — see Technical details. · checked just now",
      shaTitle: null,
    },
  );
  assert.deepEqual(
    packsSourceSentence("Beekeeper", 0, {
      origins: [],
      location: null,
      sha: null,
      shasDiffer: false,
    }),
    {
      text: "No role instructions are available for Beekeeper.",
      shaTitle: null,
    },
  );
});

test("the availability line uses the card's own version, origin and refusal", () => {
  const sha = "9".repeat(40);
  assert.deepEqual(
    roleAvailabilitySentence({
      hasPack: true,
      version: "1.3.0",
      origin: "project",
      packRef: { repo: "30617:owner:packs", sha },
      refusal: null,
    }),
    {
      availability: "available",
      text: `Available here: v1.3.0 (${sha.slice(0, 8)}) from the project's repository`,
      title: sha,
    },
  );
  assert.deepEqual(
    roleAvailabilitySentence({
      hasPack: true,
      version: "0.4.2",
      origin: "shipped",
      packRef: { repo: "app:shipped", sha: "0.4.2" },
      refusal: null,
    }),
    {
      availability: "available",
      text: "Available here: v0.4.2 from this app's built-in defaults",
      title: null,
    },
  );
  assert.deepEqual(
    roleAvailabilitySentence({
      hasPack: true,
      version: null,
      origin: "installed",
      packRef: null,
      refusal: null,
    }),
    {
      availability: "available",
      text: `Available here: ${ROLE_VERSION_UNRECORDED} from this computer only`,
      title: null,
    },
  );
  // A role with no instructions here carries the reason, not a blank version.
  assert.deepEqual(
    roleAvailabilitySentence({
      hasPack: false,
      version: null,
      origin: null,
      packRef: null,
      refusal: null,
    }),
    {
      availability: "unavailable",
      text: `Not available here — ${ROLE_NO_PACK}`,
      title: null,
    },
  );
  // A refused role quotes the backend's own sentence.
  assert.deepEqual(
    roleAvailabilitySentence({
      hasPack: true,
      version: "1.0.0",
      origin: "project",
      packRef: { repo: "30617:owner:packs", sha },
      refusal: "This project's packs repository is not synced here.",
    }),
    {
      availability: "unavailable",
      text: "Not available here — This project's packs repository is not synced here.",
      title: null,
    },
  );
});

test("revisionRelationText is the design's final wording, verbatim", () => {
  assert.equal(
    revisionRelationText("current", null, null),
    "Same revision as this machine",
  );
  assert.equal(
    revisionRelationText("earlier", 3, null),
    "Earlier revision · 3 behind this machine",
  );
  assert.equal(
    revisionRelationText("later", null, 7),
    "Newer than this machine's copy · 7 ahead — this machine has not refreshed",
  );
  assert.equal(
    revisionRelationText("unrelated", null, null),
    "Different history from this machine's copy",
  );
  assert.equal(
    revisionRelationText("unknown-here", null, null),
    "Revision unknown to this machine",
  );
  assert.equal(
    revisionRelationText("different-source", null, null),
    "Different pack source",
  );
  assert.equal(
    revisionRelationText("shipped-differs", null, null),
    "Shipped defaults from a different app version",
  );
  assert.equal(
    revisionRelationText("incomplete", null, null),
    "Version claim incomplete",
  );
});

test("the short relation keeps every distance the long one discloses", () => {
  assert.equal(
    revisionRelationShortText("current", null, null),
    "same version as here",
  );
  assert.equal(
    revisionRelationShortText("earlier", 1, null),
    "earlier, 1 behind",
  );
  assert.equal(
    revisionRelationShortText("earlier", null, null),
    "earlier, distance unknown",
  );
  assert.equal(
    revisionRelationShortText("later", null, 2),
    "newer than here, 2 ahead",
  );
  assert.equal(
    revisionRelationShortText("later", null, null),
    "newer than here, distance unknown",
  );
  assert.equal(
    revisionRelationShortText("unrelated", null, null),
    "different history",
  );
  assert.equal(
    revisionRelationShortText("unknown-here", null, null),
    "version unknown here",
  );
  assert.equal(
    revisionRelationShortText("different-source", null, null),
    "different source",
  );
  assert.equal(
    revisionRelationShortText("shipped-differs", null, null),
    "built-in defaults, other app version",
  );
  assert.equal(
    revisionRelationShortText("incomplete", null, null),
    "version not reported",
  );
});

test("the short provenance words name the sender check, never what ran", () => {
  assert.equal(provenanceShortText("commissioned"), "sender confirmed");
  assert.equal(provenanceShortText("proof-unavailable"), "sender unconfirmed");
  assert.equal(provenanceShortText("disputed"), "disputed");
  for (const state of ["commissioned", "proof-unavailable", "disputed"]) {
    assert.doesNotMatch(provenanceShortText(state), /ran|executed|adopted/i);
  }
});

test("a version group with two different sender checks says they differ", () => {
  assert.deepEqual(
    roleReportProvenanceText({ commissioned: 2, unavailable: 0, disputed: 0 }),
    { text: "sender confirmed", title: "Reported by the assigned provider: 2" },
  );
  assert.deepEqual(
    roleReportProvenanceText({ commissioned: 1, unavailable: 2, disputed: 0 }),
    {
      text: PROVENANCE_MIXED_SHORT,
      title:
        "Reported by the assigned provider: 1 · Unverified · proof unavailable: 2",
    },
  );
  assert.equal(
    roleReportProvenanceText({ commissioned: 0, unavailable: 0, disputed: 0 })
      .text,
    "sender checks not recorded",
  );
});

test("a reported-version line reads as one plain row, with the long labels in its title", () => {
  const sha = "9f2e1d0c".padEnd(40, "a");
  assert.deepEqual(
    roleReportVersionLine({
      sha,
      relation: "current",
      behind: null,
      ahead: null,
      count: 2,
      latestAgeSeconds: 10,
      provenance: { commissioned: 2, unavailable: 0, disputed: 0 },
    }),
    {
      text: "9f2e1d0c · same version as here · 2 reports · latest just now · sender confirmed",
      title: `Same revision as this machine · Reported by the assigned provider: 2 · ${sha}`,
    },
  );
  assert.deepEqual(
    roleReportVersionLine({
      sha: "dd11ee22".padEnd(40, "b"),
      relation: "earlier",
      behind: 1,
      ahead: null,
      count: 1,
      latestAgeSeconds: 120,
      provenance: { commissioned: 0, unavailable: 1, disputed: 0 },
    }).text,
    "dd11ee22 · earlier, 1 behind · 1 report · latest 2m ago · sender unconfirmed",
  );
  // A report that named this role but no version leads with that fact.
  assert.deepEqual(
    roleReportVersionLine({
      sha: null,
      relation: "incomplete",
      behind: null,
      ahead: null,
      count: 3,
      latestAgeSeconds: null,
      provenance: { commissioned: 0, unavailable: 3, disputed: 0 },
    }).text,
    "version not reported · 3 reports · latest time not reported · sender unconfirmed",
  );
});

test("the card's report helpers count and pluralize honestly", () => {
  assert.equal(reportCountText(1), "1 report");
  assert.equal(reportCountText(4), "4 reports");
  assert.equal(reportLatestText(null), "latest time not reported");
  assert.equal(reportLatestText(0), "latest just now");
  assert.equal(reportLatestText(7_200), "latest 2h ago");
  assert.equal(roleReportsMoreText(1), "and 1 more version in Report history");
  assert.equal(roleReportsMoreText(3), "and 3 more versions in Report history");
  assert.equal(roleSkillsSummary(0), "Skills (0)");
  assert.equal(roleSkillsSummary(2), "Skills (2)");
  assert.equal(reportHistorySummary(41), "Report history (41)");
});

test("reportedAgoText discloses an unreported time rather than zeroing it", () => {
  assert.equal(reportedAgoText(null), "time not reported");
  assert.equal(reportedAgoText(300), "reported 5m ago");
});

test("checkoutAnsweredSentence names the commit and when git answered", () => {
  assert.equal(
    checkoutAnsweredSentence("a".repeat(40), "1/1/2026, 12:00:00 AM"),
    "On aaaaaaaa · git answered at 1/1/2026, 12:00:00 AM.",
  );
});

test("revisionComparisonUnavailableSentence attributes the failure", () => {
  assert.equal(
    revisionComparisonUnavailableSentence("git exited 1"),
    "Revision comparison unavailable: git exited 1",
  );
});

test("the adoption sentence is the design's final wording, verbatim", () => {
  assert.equal(
    ADOPTION_KEEPS_UNTIL_NEXT_GENERATION,
    "Keeps this revision until its next launch or resume.",
  );
});

test("the revision-snapshots subtitle names what a commissioned label does and does not prove", () => {
  assert.equal(
    ROLE_PACK_SNAPSHOTS_SUBTITLE,
    "Versions found on this machine and pack revisions reported in this project’s channels. Beekeeper checks who sent each report. Confirming its source does not prove which role instructions were used.",
  );
  // `commissioned` is never worded as verified execution or adoption.
  assert.doesNotMatch(
    ROLE_PACK_SNAPSHOTS_SUBTITLE,
    /verified execution|verified adoption/,
  );
});

test("the three provenance labels are the design's final wording, verbatim", () => {
  assert.equal(
    PROVENANCE_LABEL_COMMISSIONED,
    "Reported by the assigned provider",
  );
  assert.equal(
    PROVENANCE_LABEL_PROOF_UNAVAILABLE,
    "Unverified · proof unavailable",
  );
  assert.equal(PROVENANCE_LABEL_DISPUTED, "Disputed");
  // Never worded as verified execution or adoption (constraint 2).
  for (const label of [
    PROVENANCE_LABEL_COMMISSIONED,
    PROVENANCE_LABEL_PROOF_UNAVAILABLE,
    PROVENANCE_LABEL_DISPUTED,
  ]) {
    assert.doesNotMatch(label, /verified execution|verified adoption/i);
  }
});

test("provenanceLabelText appends the reason only when one is present", () => {
  assert.equal(
    provenanceLabelText("commissioned", null),
    "Reported by the assigned provider",
  );
  assert.equal(
    provenanceLabelText(
      "proof-unavailable",
      "operator authority not projected",
    ),
    "Unverified · proof unavailable · operator authority not projected",
  );
  assert.equal(
    provenanceLabelText(
      "disputed",
      "the 44223 signer is not the accepted provider",
    ),
    "Disputed · the 44223 signer is not the accepted provider",
  );
});

test("provenanceNotesSentence prefixes the fold's own notes with 'Proof reads:'", () => {
  assert.equal(
    provenanceNotesSentence([
      "Lifecycle receipts could not be read: timed out.",
    ]),
    "Proof reads: Lifecycle receipts could not be read: timed out.",
  );
  assert.equal(
    provenanceNotesSentence(["First sentence.", "Second sentence."]),
    "Proof reads: First sentence. Second sentence.",
  );
});
