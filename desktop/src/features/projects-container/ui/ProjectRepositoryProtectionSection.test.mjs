import assert from "node:assert/strict";
import test from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { ProjectRepositoryProtectionSection } from "./ProjectRepositoryProtectionSection.tsx";
import { projectRepositoriesQueryKey } from "../lib/projectRepositoryProtection.ts";

const OWNER = "a".repeat(64);
const OTHER = "b".repeat(64);

const PROJECT_CONTAINER = {
  id: "proj-1",
  dtag: "agiterra",
  owner: OWNER,
  name: "agiterra",
  description: "",
  createdAt: 1_800_000_000,
  address: `30621:${OWNER}:agiterra`,
  repoAddrs: [`30617:${OWNER}:agiterra-beekeeper`],
  agentAddrs: [],
  channelIds: [],
  visibility: "public",
  members: [],
  icon: null,
  color: null,
};

function repository(overrides = {}) {
  return {
    id: `30617:${OWNER}:agiterra-beekeeper`,
    dtag: "agiterra-beekeeper",
    name: "agiterra-beekeeper",
    description: "",
    cloneUrls: [],
    webUrl: null,
    owner: OWNER,
    contributors: [],
    createdAt: 1_800_000_000,
    status: "active",
    defaultBranch: "main",
    repoAddress: `30617:${OWNER}:agiterra-beekeeper`,
    projectRef: null,
    eventTags: [],
    eventContent: "",
    ...overrides,
  };
}

function render({ self = OWNER, repo = repository() } = {}) {
  const client = new QueryClient();
  client.setQueryData(["identity"], { pubkey: self });
  client.setQueryData(
    projectRepositoriesQueryKey(PROJECT_CONTAINER.repoAddrs),
    [repo],
  );
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(ProjectRepositoryProtectionSection, {
        project: PROJECT_CONTAINER,
      }),
    ),
  );
}

test("no linked repositories: the honest empty state", () => {
  const client = new QueryClient();
  client.setQueryData(["identity"], { pubkey: OWNER });
  const emptyProject = { ...PROJECT_CONTAINER, repoAddrs: [] };
  const markup = renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(ProjectRepositoryProtectionSection, {
        project: emptyProject,
      }),
    ),
  );
  assert.match(markup, /data-testid="project-repository-protection-empty"/);
});

test("no rules set: says so plainly", () => {
  const markup = render();
  assert.match(markup, /data-testid="project-repository-protection-none"/);
  assert.match(markup, /No protection rules are set on this repository\./);
});

test("real buzz-protect rules are listed, ref pattern and all", () => {
  const markup = render({
    repo: repository({
      eventTags: [
        ["buzz-protect", "refs/heads/main", "require-verdict", "no-force-push"],
        ["buzz-protect", "refs/heads/dev", "no-delete"],
      ],
    }),
  });
  assert.match(markup, /data-testid="project-repository-protection-rules"/);
  assert.match(markup, /refs\/heads\/main/);
  assert.match(markup, /require-verdict, no-force-push/);
  assert.match(markup, /refs\/heads\/dev/);
  assert.match(markup, /no-delete/);
});

test("the announcement's signer sees an enabled switch and no read-only note", () => {
  const markup = render({ self: OWNER });
  assert.match(
    markup,
    /data-testid="project-repository-require-verdict-switch"/,
  );
  assert.doesNotMatch(
    markup,
    /data-testid="project-repository-protection-readonly-note"/,
  );
});

test("a non-signer sees the switch disabled and the sentence naming who can", () => {
  const markup = render({ self: OTHER });
  assert.match(
    markup,
    /data-testid="project-repository-protection-readonly-note"/,
  );
  assert.match(markup, /can set this\./);
  // The switch itself is present (read-only), but disabled.
  const switchMarkup = markup.match(
    /<button[^>]*data-testid="project-repository-require-verdict-switch"[^>]*>/,
  )?.[0];
  assert.ok(switchMarkup, "switch control must still render, disabled");
  assert.match(switchMarkup, /disabled=""/);
});

test("the switch reflects require-verdict on main specifically, not any rule on any ref", () => {
  const withMainRule = render({
    repo: repository({
      eventTags: [["buzz-protect", "refs/heads/main", "require-verdict"]],
    }),
  });
  const withDevRule = render({
    repo: repository({
      eventTags: [["buzz-protect", "refs/heads/dev", "require-verdict"]],
    }),
  });
  const ariaChecked = (markup) => {
    const button = markup.match(
      /<button[^>]*data-testid="project-repository-require-verdict-switch"[^>]*>/,
    )?.[0];
    return button?.match(/aria-checked="(true|false)"/)?.[1];
  };
  assert.equal(ariaChecked(withMainRule), "true");
  assert.equal(ariaChecked(withDevRule), "false");
});
