import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  projectTeamSetupBlocker,
  projectTeamSetupError,
  projectTeamSetupFailure,
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

const SETUP_ERROR_CODES = [
  "existing_authoring",
  "existing_draft",
  "filesystem",
  "invalid_authoring",
  "invalid_draft",
  "invalid_input",
  "invalid_publication",
  "invalid_setup_actor",
  "invalid_setup_launch",
  "invalid_snapshot",
  "publication_unavailable",
  "scope_changed",
  "setup_launch_unavailable",
  "source_changed",
];

test("every native setup code maps to one plain sentence and keeps the raw detail", () => {
  const summaries = new Map();
  for (const code of SETUP_ERROR_CODES) {
    const raw = `native ${code} message: /Users/x/drafts/${"a".repeat(64)}`;
    const failure = projectTeamSetupFailure({ code, message: raw });
    assert.equal(failure.code, code);
    assert.equal(failure.detail, raw, `${code} must keep the native message`);
    assert.notEqual(failure.summary, raw);
    assert.doesNotMatch(failure.summary, /[a-f0-9]{16}|\/Users|_/, code);
    assert.match(failure.summary, /\.$/, code);
    summaries.set(code, failure.summary);
  }
  assert.match(summaries.get("source_changed"), /shared roles changed/);
  assert.match(summaries.get("source_changed"), /check the draft again/);
  for (const code of ["publication_unavailable", "setup_launch_unavailable"]) {
    // Also raised for local install, option-read and provider failures, so
    // the copy must not blame the relay.
    assert.doesNotMatch(summaries.get(code), /relay/i);
    assert.match(summaries.get(code), /didn't complete/);
    assert.match(summaries.get(code), /same saved request when one exists/);
  }
  assert.match(summaries.get("scope_changed"), /community or project changed/);
  assert.match(
    summaries.get("filesystem"),
    /local file couldn't be read or written/,
  );
});

test("a native refusal wrapped by invokeTauri still maps by its payload code", async () => {
  const { TauriInvokeError } = await import("@/shared/api/tauri");
  const payload = { code: "scope_changed", message: "Relay changed." };
  const failure = projectTeamSetupFailure(
    new TauriInvokeError(payload.message, payload),
  );
  assert.equal(failure.code, "scope_changed");
  assert.match(failure.summary, /community or project changed/);
  assert.equal(failure.detail, "Relay changed.");
});

test("uncoded or unknown failures show their own message with nothing hidden", () => {
  assert.deepEqual(projectTeamSetupFailure(new Error("Response lost")), {
    summary: "Response lost",
    detail: null,
    code: null,
  });
  assert.deepEqual(
    projectTeamSetupFailure({ code: "brand_new_code", message: "Try later" }),
    { summary: "Try later", detail: null, code: "brand_new_code" },
  );
  assert.doesNotMatch(
    projectTeamSetupFailure(undefined).summary,
    /Object|undefined|null/,
  );
});

test("a recovered draft shows one current stage and keeps paths under Technical details", () => {
  const html = renderToStaticMarkup(
    React.createElement(ProjectTeamSetupDraftView, { draft }),
  );
  assert.match(html, /Author project roles/);
  assert.doesNotMatch(html, /Draft ready/);
  assert.match(html, /aria-current="step"/);
  assert.match(html, /has not been checked yet/);
  assert.match(html, /Draft edits stay local/);
  assert.match(html, /Check draft/);
  assert.doesNotMatch(html, />Start authoring session<|<button[^>]*>Publish/);
  const details = html.slice(
    html.indexOf('data-testid="project-team-setup-technical-details"'),
  );
  assert.ok(details.includes(input.projectDirectory));
  assert.ok(details.includes(draft.rolesDirectory));
  const beforeDetails = html.slice(
    0,
    html.indexOf('data-testid="project-team-setup-technical-details"'),
  );
  assert.ok(!beforeDetails.includes(input.projectDirectory));
  assert.ok(!beforeDetails.includes(draft.rolesDirectory));
});
