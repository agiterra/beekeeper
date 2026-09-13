import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  projectTeamSetupBlocker,
  projectTeamSetupBrief,
  projectTeamSetupError,
} from "./projectTeamSetup.ts";
import { ProjectTeamSetupDraftView } from "../ui/ProjectTeamSetupDraftView.tsx";

const projectRef = `30621:${"a".repeat(64)}:tankloop`;
const input = {
  projectRef,
  relayUrl: "wss://example.test",
  intent: "Build Tankloop",
  projectDirectory: "/projects/tankloop",
};
const draft = {
  ...input,
  setupId: "setup-1",
  status: "draft",
  draftDirectory: "/drafts/setup-1",
  rolesDirectory: "/drafts/setup-1/personas/roles",
  roles: ["lead", "builder", "reviewer"],
  ownerPubkey: "a".repeat(64),
  createdAt: "2026-09-11T12:00:00Z",
};

test("setup needs an actual project scope, community, intent and repository before mutation", () => {
  assert.equal(projectTeamSetupBlocker(input), null);
  assert.match(
    projectTeamSetupBlocker({ ...input, projectRef: "tankloop" }),
    /saved project/,
  );
  assert.match(
    projectTeamSetupBlocker({ ...input, relayUrl: "" }),
    /community/,
  );
  assert.match(projectTeamSetupBlocker({ ...input, intent: "  " }), /Describe/);
  assert.match(
    projectTeamSetupBlocker({ ...input, projectDirectory: "" }),
    /repository folder/,
  );
});

test("intent limit counts UTF-8 bytes rather than JavaScript characters", () => {
  assert.equal(
    projectTeamSetupBlocker({ ...input, intent: "🐟".repeat(4096) }),
    null,
  );
  assert.match(
    projectTeamSetupBlocker({ ...input, intent: "🐟".repeat(4097) }),
    /16 KiB/,
  );
});

test("native refusal retains its actionable explanation", () => {
  assert.equal(
    projectTeamSetupError({
      code: "existing_draft",
      message: "Resume the saved draft before changing its repository.",
    }),
    "Resume the saved draft before changing its repository.",
  );
  assert.equal(
    projectTeamSetupError(new Error("Community changed")),
    "Community changed",
  );
  assert.equal(
    projectTeamSetupError("Repository is bare"),
    "Repository is bare",
  );
  assert.doesNotMatch(projectTeamSetupError(null), /Object|undefined|null/);
});

test("authoring brief binds real project input and the isolated write destination", () => {
  const brief = projectTeamSetupBrief(draft);
  assert.ok(brief.includes(projectRef));
  assert.ok(brief.includes(input.intent));
  assert.ok(brief.includes(input.projectDirectory));
  assert.ok(brief.includes(draft.rolesDirectory));
  assert.match(brief, /has not been published/);
  assert.doesNotMatch(
    brief,
    /hive\.agiterra|beekeeper-project|just ci|BUZZ_PRIVATE_KEY/,
  );
});

test("recovered draft is visible without claiming validation, publication or an implemented launcher", () => {
  const html = renderToStaticMarkup(
    React.createElement(ProjectTeamSetupDraftView, { draft }),
  );
  assert.match(html, /Draft ready/);
  assert.match(html, /has not been checked yet/);
  assert.match(html, /Draft edits stay local/);
  assert.match(html, /Check draft/);
  assert.doesNotMatch(html, /Start authoring session|>Publish</);
  assert.ok(html.includes(input.projectDirectory));
});
