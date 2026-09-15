import assert from "node:assert/strict";
import test from "node:test";
import { projectRosterReadiness } from "./projectRosterReadiness.ts";
import {
  PROJECT_TEAM_SETUP_STEPS,
  projectTeamSetupStage,
} from "./projectTeamSetupStage.ts";

const LAUNCH_STATUSES = [
  "prepared",
  "awaiting_receipt",
  "ambiguous",
  "created",
  "initial_turn_failed",
  "failed",
];
const PUBLICATION_STATUSES = [
  "checking",
  "candidate_prepared",
  "push_unknown",
  "pushed",
  "source_unknown",
  "adopted",
  "superseded",
  "conflict",
  "refused",
];
const INSTALLATION_STATUSES = [
  "not_installed",
  "installed",
  "unknown",
  "refused",
];
const LEAD_STATUSES = [
  "needs_channel",
  "ready",
  "starting",
  "started",
  "unknown",
  "refused",
];

function activation(
  installation,
  lead,
  { source = true, sessionRef = null } = {},
) {
  return {
    source: source ? { repoRef: "r", commit: "c", packPath: "p" } : null,
    installation: { status: installation },
    lead: { status: lead, sessionRef },
  };
}

const READY_ROSTER = { status: "ready", blocked: [], leadName: "Loom" };

function adopted(act, roster = READY_ROSTER) {
  return {
    snapshot: "saved",
    publication: { status: "adopted" },
    activation: act,
    roster,
    projectName: "Tank Loop",
  };
}

test("the happy path walks the five steps in order", () => {
  const path = [
    { authoringLaunch: null, snapshot: "none" },
    { authoringLaunch: { status: "created" }, snapshot: "none" },
    { validation: { valid: true }, snapshot: "none", authoringLaunch: null },
    { snapshot: "saved", publication: null },
    adopted(activation("not_installed", "needs_channel")),
    adopted(activation("installed", "needs_channel")),
    adopted(activation("installed", "ready")),
    adopted(activation("installed", "started")),
  ].map(projectTeamSetupStage);
  assert.deepEqual(
    path.map((stage) => stage.id),
    [
      "author",
      "check_and_save",
      "check_and_save",
      "publish",
      "install",
      "start_lead",
      "start_lead",
      "lead_started",
    ],
  );
  assert.deepEqual(
    [...new Set(path.map((stage) => stage.step))],
    PROJECT_TEAM_SETUP_STEPS,
  );
  assert.equal(path.at(-1).state, "done");
  assert.ok(path.slice(0, -1).every((stage) => stage.state !== "done"));
  assert.match(path[1].next, /check the draft/);
  assert.match(path[2].next, /Save the checked version/);
  assert.match(path[5].next, /session channel is created first/);
});

test("only an installed, started lead with an associated roster is done; every other activation combination is not", () => {
  for (const installation of INSTALLATION_STATUSES) {
    for (const lead of LEAD_STATUSES) {
      for (const source of [true, false]) {
        for (const sessionRef of [null, "session"]) {
          const stage = projectTeamSetupStage(
            adopted(activation(installation, lead, { source, sessionRef })),
          );
          const done = installation === "installed" && lead === "started";
          assert.equal(
            stage.state === "done",
            done,
            `${installation}/${lead}/source=${source} → ${stage.id}`,
          );
          assert.ok(stage.title.length > 0 && stage.next.length > 0);
          if (installation === "unknown" && source)
            assert.equal(stage.state === "uncertain" || done, true);
          if (installation === "refused" && source && !done)
            assert.equal(stage.state, "blocked");
          if (!source && !done) assert.equal(stage.id, "source_unresolved");
        }
      }
    }
  }
  const unknownLead = projectTeamSetupStage(
    adopted(activation("installed", "unknown", { sessionRef: "s" })),
  );
  assert.equal(unknownLead.state, "uncertain");
  assert.match(unknownLead.next, /Retry the lead handoff/);
  assert.match(
    projectTeamSetupStage(adopted(activation("installed", "unknown"))).next,
    /Retry session-channel setup/,
  );
  assert.equal(
    projectTeamSetupStage(adopted(activation("installed", "refused"))).state,
    "blocked",
  );
  assert.equal(
    projectTeamSetupStage(adopted(activation("installed", "starting"))).state,
    "uncertain",
  );
});

test("no publication status other than adopted reaches install, and unknown push or source is uncertain", () => {
  for (const status of PUBLICATION_STATUSES) {
    const stage = projectTeamSetupStage({
      snapshot: "saved",
      publication: { status },
    });
    assert.notEqual(stage.state, "done", status);
    if (status === "adopted") {
      assert.equal(stage.step, "install");
      assert.equal(stage.state, "checking");
      continue;
    }
    assert.equal(stage.step, "publish", status);
    if (status === "push_unknown" || status === "source_unknown")
      assert.equal(stage.state, "uncertain");
    if (["conflict", "refused", "superseded"].includes(status))
      assert.equal(stage.state, "blocked");
    if (["checking", "candidate_prepared", "pushed"].includes(status)) {
      assert.equal(stage.state, "working");
      assert.match(stage.next, /Retry publication/);
    }
  }
  assert.equal(
    projectTeamSetupStage({
      snapshot: "saved",
      publication: { status: "adopted" },
      activation: null,
    }).state,
    "blocked",
  );
});

test("a later fact outranks an earlier one, and blocked publication never reads as publishable", () => {
  // An adopted publication wins over an unread or failed authoring launch.
  const stage = projectTeamSetupStage({
    authoringLaunch: { status: "failed" },
    validation: { valid: false },
    snapshot: "saved",
    publication: { status: "adopted" },
    activation: activation("installed", "started"),
    roster: READY_ROSTER,
  });
  assert.equal(stage.id, "lead_started");
  for (const blocked of [
    "missing_destination",
    "missing_base",
    "unavailable",
  ]) {
    const result = projectTeamSetupStage({
      snapshot: "saved",
      publication: null,
      publicationBlocked: blocked,
    });
    assert.equal(result.id, "publish_blocked");
    assert.equal(result.state, "blocked");
  }
  assert.equal(
    projectTeamSetupStage({ snapshot: "saved", publication: undefined }).state,
    "checking",
  );
  assert.equal(
    projectTeamSetupStage({ snapshot: "recorded", publication: null }).id,
    "publish",
  );
  assert.equal(
    projectTeamSetupStage({ snapshot: "checking", authoringLaunch: null })
      .state,
    "checking",
  );
});

test("authoring and validation states: uncertain or refused authoring is never treated as authored", () => {
  const byStatus = Object.fromEntries(
    LAUNCH_STATUSES.map((status) => [
      status,
      projectTeamSetupStage({ authoringLaunch: { status }, snapshot: "none" }),
    ]),
  );
  assert.equal(byStatus.created.step, "check_and_save");
  for (const status of ["prepared", "ambiguous"]) {
    assert.equal(byStatus[status].step, "author");
    assert.equal(byStatus[status].state, "uncertain");
  }
  assert.equal(byStatus.awaiting_receipt.state, "working");
  for (const status of ["failed", "initial_turn_failed"]) {
    assert.equal(byStatus[status].step, "author");
    assert.equal(byStatus[status].state, "blocked");
  }
  assert.equal(
    projectTeamSetupStage({ snapshot: "none" }).state,
    "checking",
    "an unread launch is checking, not 'no launch'",
  );
  assert.equal(
    projectTeamSetupStage({ authoringLaunch: "unreadable", snapshot: "none" })
      .state,
    "uncertain",
  );
  const invalid = projectTeamSetupStage({
    authoringLaunch: { status: "created" },
    validation: { valid: false },
    snapshot: "none",
  });
  assert.equal(invalid.id, "draft_invalid");
  assert.equal(invalid.state, "blocked");
});

test("a failed re-check of the saved version is its own blocked stage until the draft is checked again", () => {
  const failed = projectTeamSetupStage({
    authoringLaunch: { status: "created" },
    snapshot: "unverified",
  });
  assert.equal(failed.id, "saved_version_unverified");
  assert.equal(failed.state, "blocked");
  assert.equal(failed.title, "The saved version couldn't be re-checked");
  assert.equal(
    projectTeamSetupStage({
      snapshot: "unverified",
      validation: { valid: true },
    }).id,
    "check_and_save",
  );
  assert.equal(
    projectTeamSetupStage(adopted(activation("installed", "started"))).next,
    "Setup complete. Loom leads Tank Loop. Give Loom a task in its session; it lists the team with `bee projects agents` and hires only these agents.",
  );
});

const PROJECT = `30621:${"a".repeat(64)}:tank-loop`;
const OTHER_PROJECT = `30621:${"a".repeat(64)}:beekeeper`;
const LEAD = "1".repeat(64);
const BUILDER = "2".repeat(64);
const RUNNER = "3".repeat(64);
const VERIFIER = "4".repeat(64);

function managed(pubkey, name, projectRef) {
  return { pubkey, name, homeRole: "x", projectRef, status: "stopped" };
}

function installed() {
  return [
    { role: "lead", agentPubkey: LEAD },
    { role: "builder", agentPubkey: BUILDER },
    { role: "runner", agentPubkey: RUNNER },
    { role: "verifier", agentPubkey: VERIFIER },
  ];
}

function roster(agents) {
  return projectRosterReadiness({
    projectRef: PROJECT,
    installedRoles: installed(),
    agents,
    leadPubkey: LEAD,
  });
}

test("roster readiness: associated, not associated, another project and missing are told apart", () => {
  const readiness = roster([
    // Owner hex case does not change the project.
    managed(LEAD, "Loom", `30621:${"A".repeat(64)}:tank-loop`),
    managed(BUILDER, "Builder", null),
    managed(RUNNER, "Runner", OTHER_PROJECT),
  ]);
  assert.deepEqual(
    readiness.entries.map((entry) => [
      entry.name,
      entry.association,
      entry.isLead,
    ]),
    [
      ["Loom", "associated", true],
      ["Builder", "not-associated", false],
      ["Runner", "other-project", false],
      [null, "missing", false],
    ],
  );
  assert.equal(readiness.status, "blocked");
  assert.equal(readiness.leadName, "Loom");
  assert.deepEqual(
    readiness.blocked.map((entry) => entry.agentPubkey),
    [BUILDER, RUNNER, VERIFIER],
  );
  const everyone = roster([
    managed(LEAD, "Loom", PROJECT),
    managed(BUILDER, "Builder", PROJECT),
    managed(RUNNER, "Runner", PROJECT),
    managed(VERIFIER, "Verifier", PROJECT),
  ]);
  assert.equal(everyone.status, "ready");
  assert.deepEqual(everyone.blocked, []);
  assert.equal(roster(undefined).status, "checking");
  assert.equal(roster(null).status, "checking");
  assert.equal(roster("unreadable").status, "unreadable");
  assert.ok(
    roster("unreadable").entries.every(
      (entry) => entry.association === "unknown",
    ),
  );
  assert.equal(
    projectRosterReadiness({
      projectRef: PROJECT,
      installedRoles: [],
      agents: [],
      leadPubkey: null,
    }).status,
    "empty",
  );
});

test("a not-associated roster blocks install and start lead, and a started lead is never complete", () => {
  const notAssociated = roster([
    managed(LEAD, "Loom", PROJECT),
    managed(BUILDER, "Bob", null),
    managed(RUNNER, "Gordan", null),
    managed(VERIFIER, "Verifier", PROJECT),
  ]);
  for (const lead of ["needs_channel", "ready"]) {
    const result = projectTeamSetupStage(
      adopted(activation("installed", lead), notAssociated),
    );
    assert.equal(result.id, "roster_blocked");
    assert.equal(result.step, "install");
    assert.equal(result.state, "blocked");
    assert.equal(
      result.next,
      "The lead can't hire Bob and Gordan yet: they aren't associated with this project on this computer. Retry installation (safe, keeps identities) or associate them on the project's Agents tab.",
    );
  }
  const started = projectTeamSetupStage(
    adopted(activation("installed", "started"), notAssociated),
  );
  assert.equal(started.id, "roster_blocked");
  assert.equal(started.step, "start_lead");
  assert.notEqual(started.state, "done");
  assert.match(
    started.next,
    /^Lead started, but it can't hire Bob and Gordan:/,
  );
  assert.doesNotMatch(started.next, /complete/i);
});

test("another project's agent is named as belonging elsewhere and not borrowed; a missing record says so", () => {
  const otherProject = projectTeamSetupStage(
    adopted(
      activation("installed", "ready"),
      roster([
        managed(LEAD, "Loom", PROJECT),
        managed(BUILDER, "Bob", OTHER_PROJECT),
        managed(RUNNER, "Runner", PROJECT),
        managed(VERIFIER, "Verifier", PROJECT),
      ]),
    ),
  );
  assert.equal(otherProject.state, "blocked");
  assert.equal(
    otherProject.next,
    "The lead can't hire Bob yet: it isn't associated with this project on this computer. Bob belongs to another project and isn't borrowed. Reinstalling can't move another project's agent here; review this project's agents on its Agents tab.",
  );
  const missing = projectTeamSetupStage(
    adopted(
      activation("installed", "started"),
      roster([
        managed(LEAD, "Loom", PROJECT),
        managed(BUILDER, "Bob", null),
        managed(RUNNER, "Runner", PROJECT),
      ]),
    ),
  );
  assert.equal(missing.state, "blocked");
  assert.equal(
    missing.next,
    "Lead started, but it can't hire Bob and the verifier agent: they aren't associated with this project on this computer. The verifier agent has no agent record on this computer. Retry installation (safe, keeps identities) or associate Bob on the project's Agents tab.",
  );
  const leadOnly = projectTeamSetupStage(
    adopted(
      activation("installed", "ready"),
      roster([
        managed(LEAD, "Loom", null),
        managed(BUILDER, "Builder", PROJECT),
        managed(RUNNER, "Runner", PROJECT),
        managed(VERIFIER, "Verifier", PROJECT),
      ]),
    ),
  );
  assert.equal(
    leadOnly.next,
    "Loom, the project lead, isn't associated with this project on this computer. Retry installation (safe, keeps identities) or associate it on the project's Agents tab.",
  );
});

test("an unread, unreadable or empty roster is checking or uncertain, never ready or done", () => {
  for (const lead of ["needs_channel", "ready", "started"]) {
    const unread = projectTeamSetupStage({
      ...adopted(activation("installed", lead)),
      roster: undefined,
    });
    assert.equal(unread.id, "roster_checking");
    assert.equal(unread.state, "checking");
    const failed = projectTeamSetupStage(
      adopted(activation("installed", lead), roster("unreadable")),
    );
    assert.equal(failed.state, "uncertain");
    assert.match(failed.next, /couldn't read its agents/);
    const empty = projectTeamSetupStage(
      adopted(activation("installed", lead), {
        status: "empty",
        blocked: [],
        leadName: null,
      }),
    );
    assert.equal(empty.state, "uncertain");
  }
  // A lead failure keeps its own stage: the roster gate does not hide it.
  assert.equal(
    projectTeamSetupStage(
      adopted(activation("installed", "refused"), roster("unreadable")),
    ).id,
    "lead_refused",
  );
});
