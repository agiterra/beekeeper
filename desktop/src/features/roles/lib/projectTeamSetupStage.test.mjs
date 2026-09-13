import assert from "node:assert/strict";
import test from "node:test";
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

function adopted(act) {
  return {
    snapshot: "saved",
    publication: { status: "adopted" },
    activation: act,
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
  assert.match(path[5].next, /Create the project session channel/);
});

test("only an installed, started lead is done; every other activation combination is not", () => {
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
  assert.match(
    projectTeamSetupStage(adopted(activation("installed", "started"))).next,
    /^The project lead was started\./,
  );
});
