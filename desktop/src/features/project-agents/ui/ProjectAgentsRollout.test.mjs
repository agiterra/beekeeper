import assert from "node:assert/strict";
import test from "node:test";
import * as React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import {
  createMemoryHistory,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
} from "@tanstack/react-router";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { ProjectAgentsView } from "./ProjectAgentsView.tsx";
import { namesText, readinessText } from "./projectAgentsCopy.ts";

// Authority verification, private projects, carried digests and hiring
// readiness as the Agents tab renders them.

const PROJECT_REF = `30621:${"6".repeat(64)}:tank-loop`;
const BOB = "2".repeat(64);

function row(overrides = {}) {
  return {
    pubkey: "4".repeat(64),
    name: "Builder",
    avatarUrl: null,
    section: "project",
    isProjectAgent: true,
    claimAuthority: null,
    carriedFromAnotherComputer: false,
    state: "available",
    location: { kind: "here" },
    primaryRole: "builder",
    seatedRoles: [],
    managedHere: true,
    associationMissing: false,
    otherProject: null,
    mayAssociate: false,
    relationship: null,
    installations: [],
    sessions: [],
    assignments: [],
    lastSeenSeconds: null,
    ...overrides,
  };
}

const EMPTY = {
  projectAgents: [],
  unverified: [],
  borrowed: [],
  available: [],
  previous: [],
  readiness: [],
};

const READY_SCOPE = {
  kind: "ready",
  scannedSessions: 1,
  visibleSessions: 1,
  message: null,
  hasMore: false,
  isFetchingMore: false,
  fetchMore: () => {},
};

async function render(props) {
  const rootRoute = createRootRoute({
    component: () =>
      React.createElement(ProjectAgentsView, {
        assignments: READY_SCOPE,
        associateAccess: { kind: "allowed" },
        isLoading: false,
        notices: [],
        onOpenSession: () => {},
        projectId: "p1",
        projectName: "Tank Loop",
        projectRef: PROJECT_REF,
        ...props,
      }),
  });
  const packsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectId/packs",
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/"] }),
    routeTree: rootRoute.addChildren([packsRoute]),
  });
  await router.load();
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client: new QueryClient() },
      React.createElement(RouterProvider, { router }),
    ),
  );
}

function section(html, testId) {
  const start = html.indexOf(`data-testid="${testId}"`);
  assert.ok(start >= 0, `${testId} rendered`);
  const next = html.indexOf("<section", start);
  return html.slice(start, next < 0 ? html.length : next);
}

test("an unverified published agent is labelled in words and kept out of the project agents count", async () => {
  const html = await render({
    model: {
      ...EMPTY,
      projectAgents: [row()],
      unverified: [
        row({
          pubkey: "7".repeat(64),
          name: "Casey's Runner",
          section: "unverified",
          isProjectAgent: false,
          claimAuthority: "unverified",
          state: "elsewhere",
          managedHere: false,
          primaryRole: "runner",
          location: {
            kind: "elsewhere",
            ownerPubkey: "c".repeat(64),
            ownerName: "Casey",
          },
        }),
      ],
    },
  });
  const members = section(html, "project-agents-members");
  assert.match(members, /Project agents <span[^>]*>1 agent</);
  assert.doesNotMatch(members, /Casey/);

  const unverified = section(html, "project-agents-unverified");
  assert.match(
    unverified,
    />Project authority not verified <span[^>]*>1 agent</,
  );
  assert.match(
    unverified,
    /data-testid="project-agent-badge"[^>]*>Project authority not verified</,
  );
  assert.match(
    unverified,
    /data-testid="project-agent-unverified"[^>]*>Published as a Tank Loop agent by Casey\. Tank Loop&#x27;s members could not be read, so Casey&#x27;s authority to associate agents is not verified\.</,
  );
  assert.doesNotMatch(unverified, /Associate with/);
});

test("a private project says why only this computer's agents are listed", async () => {
  const html = await render({ model: EMPTY, isPrivate: true });
  assert.match(
    section(html, "project-agents-members"),
    /data-testid="project-agents-private"[^>]*>This project is private, so agents on other computers are not published\. Only this computer&#x27;s agents are listed\.</,
  );
  assert.doesNotMatch(html, /Agents on other computers are listed when/);

  const open = await render({ model: EMPTY });
  assert.doesNotMatch(open, /project-agents-private/);
  assert.match(open, /Agents on other computers are listed when/);
});

test("a carried agent reads as associated from another computer, with an explicit Associate", async () => {
  const html = await render({
    model: {
      ...EMPTY,
      available: [
        row({
          pubkey: BOB,
          name: "Bob",
          section: "available",
          isProjectAgent: false,
          carriedFromAnotherComputer: true,
          state: "carried",
          mayAssociate: true,
        }),
      ],
    },
  });
  assert.doesNotMatch(html, /data-testid="project-agents-members"/);
  const available = section(html, "project-agents-available");
  assert.match(available, />Available to associate <span[^>]*>1 agent</);
  assert.match(
    available,
    /data-testid="project-agent-state"[^>]*>.*?Associated from another computer<\/span>/,
  );
  assert.match(
    available,
    /data-testid="project-agent-carried"[^>]*>Published as a Tank Loop agent from another of your computers\. Associate it here to let this computer hire it\.</,
  );
  assert.doesNotMatch(available, /project-agent-not-member/);
  assert.match(available, /data-testid="project-agent-associate-button"/);
  assert.match(available, />Associate with Tank Loop</);
});

test("the readiness notice lists each evidenced role at the top, and nothing when none", async () => {
  const readiness = [
    { role: "builder", workers: [{ pubkey: BOB, name: "Bob" }] },
    { role: "verifier", workers: [] },
  ];
  const html = await render({
    model: { ...EMPTY, borrowed: [row({ section: "borrowed" })], readiness },
  });
  const notice = html.indexOf('data-testid="project-agents-readiness"');
  assert.ok(notice >= 0);
  assert.ok(
    notice < html.indexOf('data-testid="project-agents-borrowed"'),
    "above the sections",
  );
  assert.match(
    html,
    /data-testid="project-agents-readiness-builder"[^>]*>No builder agent belongs to Tank Loop on this computer\. Past builder work here was done by Bob \(not associated\)\. A lead&#x27;s builder hires will be refused until you associate an agent\.</,
  );
  assert.match(
    html,
    /data-testid="project-agents-readiness-verifier"[^>]*>No verifier agent belongs to Tank Loop on this computer\. A lead&#x27;s verifier hires will be refused until you associate an agent\.</,
  );
  // The notice names; it offers no choice.
  const noticeHtml = html.slice(
    notice,
    html.indexOf("</div>", notice) + "</div>".length,
  );
  assert.doesNotMatch(noticeHtml, /<button|<input/);

  const none = await render({ model: EMPTY });
  assert.doesNotMatch(none, /project-agents-readiness/);
  const loading = await render({
    model: { ...EMPTY, readiness },
    isLoading: true,
  });
  assert.doesNotMatch(loading, /project-agents-readiness/);
});

test("copy: names join in prose", () => {
  assert.equal(namesText(["Bob"]), "Bob");
  assert.equal(namesText(["Bob", "Ira"]), "Bob and Ira");
  assert.equal(namesText(["Bob", "Gordan", "Ira"]), "Bob, Gordan and Ira");
  assert.equal(
    readinessText(
      {
        role: "runner",
        workers: [
          { pubkey: "a", name: "Gordan" },
          { pubkey: "b", name: "Ira" },
        ],
      },
      "Beekeeper",
    ),
    "No runner agent belongs to Beekeeper on this computer. Past runner work here was done by Gordan and Ira (not associated). A lead's runner hires will be refused until you associate an agent.",
  );
});
