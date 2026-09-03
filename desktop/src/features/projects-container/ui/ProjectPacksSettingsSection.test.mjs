import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import {
  ProjectPacksSettingsSection,
  projectPackSourceQueryKey,
} from "./ProjectPacksSettingsSection.tsx";
import { projectRosterQueryKey } from "../lib/projectMembers.ts";

const OWNER = "a".repeat(64);
const OTHER = "b".repeat(64);
const PROJECT = {
  id: "proj-1",
  dtag: "agiterra",
  owner: OWNER,
  name: "agiterra",
  description: "",
  createdAt: 1_800_000_000,
  address: `30621:${OWNER}:agiterra`,
  repoAddrs: [`30617:${OWNER}:agiterra-packs`],
  agentAddrs: [],
  channelIds: [],
  visibility: "public",
  members: [],
  icon: null,
  color: null,
};

const SOURCE = {
  eventId: "e".repeat(64),
  author: OWNER,
  createdAt: 1_800_000_000,
  repo: `30617:${OWNER}:agiterra-packs`,
  ref: "refs/heads/main",
  sha: null,
  path: "personas/roles",
  note: null,
};

function render({ self = OWNER, source = null } = {}) {
  const client = new QueryClient();
  client.setQueryData(["identity"], { pubkey: self });
  client.setQueryData(projectRosterQueryKey(PROJECT.address), []);
  client.setQueryData(projectPackSourceQueryKey(PROJECT.address), source);
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(ProjectPacksSettingsSection, { project: PROJECT }),
    ),
  );
}

test("no source set: discloses the shipped-defaults fallback, not a blank row", () => {
  const markup = render();
  assert.match(markup, /data-testid="project-packs-source-shipped"/);
  assert.match(
    markup,
    /no project source is set, so seats stage the packs built into this app\./,
  );
});

test("a real source shows repository, pin, path, and who set it", () => {
  const markup = render({ source: SOURCE });
  assert.match(markup, /data-testid="project-packs-source-row"/);
  assert.match(markup, new RegExp(SOURCE.repo.replace(/[/:.]/g, "\\$&")));
  assert.match(markup, /refs\/heads\/main/);
  assert.match(markup, /personas\/roles/);
});

test("a sha-pinned source shows the short sha, not the ref", () => {
  const shaSource = {
    ...SOURCE,
    ref: null,
    sha: "c".repeat(40),
  };
  const markup = render({ source: shaSource });
  assert.match(markup, /sha cccccccc/);
  assert.doesNotMatch(markup, /refs\/heads/);
});

test("the project owner sees both founder actions", () => {
  const markup = render({ self: OWNER });
  assert.match(markup, /data-testid="project-packs-create-repo-open"/);
  assert.match(markup, /data-testid="project-packs-use-existing-open"/);
  assert.doesNotMatch(markup, /data-testid="project-packs-readonly-note"/);
});

test("a stranger sees the read-only sentence, and neither founder action", () => {
  const markup = render({ self: OTHER });
  assert.match(markup, /data-testid="project-packs-readonly-note"/);
  assert.match(
    markup,
    /Only this project&#x27;s repository founders or its owner can set the pack source\./,
  );
  assert.doesNotMatch(markup, /data-testid="project-packs-create-repo-open"/);
  assert.doesNotMatch(markup, /data-testid="project-packs-use-existing-open"/);
});
