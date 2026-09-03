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

// ── lane L26: any founder may set a rule ────────────────────────────────

test("a co-founder gets a live switch, not a read-only note", () => {
  // Finding 33 R2: the panel used to disable the control for everyone but the
  // announcement's signer, and say so — an honest disclosure of a limitation
  // that no longer exists. A NIP-34 maintainer founds the repository and may
  // set its rules with a record of their own.
  const markup = render({
    self: OTHER,
    repo: repository({ maintainers: [OTHER] }),
  });
  assert.doesNotMatch(
    markup,
    /data-testid="project-repository-protection-readonly-note"/,
  );
  const button = markup.match(
    /<button[^>]*data-testid="project-repository-require-verdict-switch"[^>]*>/,
  )?.[0];
  assert.ok(button, "switch control must render");
  assert.doesNotMatch(button, /disabled=""/);
});

test("a stranger still sees the switch disabled and a founder-shaped sentence", () => {
  const markup = render({ self: OTHER, repo: repository({ maintainers: [] }) });
  assert.match(
    markup,
    /data-testid="project-repository-protection-readonly-note"/,
  );
  assert.match(markup, /Only a founder of this repository can set this\./);
});

test("each listed rule names the record that carries it and who signed it", () => {
  const markup = render({
    repo: repository({
      eventTags: [["buzz-protect", "refs/heads/main", "require-verdict"]],
    }),
  });
  assert.match(
    markup,
    /data-testid="project-repository-protection-source-refs\/heads\/main"/,
  );
  assert.match(markup, /on the announcement/);
  // Finding 31: with no rule record read, the panel shows exactly the rules
  // the announcement carries — the "signed before the kind existed" case —
  // and discloses that records were not read rather than implying none exist.
  assert.match(markup, /Rule records were not read here/);
});

test("a repository with several founders says any of them can change the rules", () => {
  const markup = render({ repo: repository({ maintainers: [OTHER] }) });
  // The apostrophe is HTML-escaped in static markup.
  assert.match(markup, /Any of this repository&#x27;s 2 founders/);
  assert.doesNotMatch(markup, /only that key can rewrite/);
});
