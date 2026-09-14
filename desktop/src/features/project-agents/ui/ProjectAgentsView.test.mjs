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
import {
  assignmentScopeText,
  relationshipText,
  sessionInstructionsText,
} from "./projectAgentsCopy.ts";

const SHA = "f0132d13".padEnd(40, "0");

function row(overrides = {}) {
  return {
    pubkey: "2".repeat(64),
    name: "Bob",
    avatarUrl: null,
    managedHere: true,
    section: "working",
    relationship: {
      kind: "seated",
      role: "builder",
      sessionName: "Project team setup",
      assignerName: "Loom",
    },
    roles: ["builder"],
    installations: [],
    sessions: [
      {
        key: "chan-1/gen-bob",
        sessionRef: "c".repeat(64),
        sessionName: "Project team setup",
        sessionClosed: false,
        openTarget: {
          channelId: "chan-1",
          generationId: "gen-lead",
          founded: false,
        },
        role: "builder",
        provider: "claude-primary",
        runtime: "claude",
        model: "sonnet",
        status: "idle",
        ageSeconds: 3 * 3_600,
        packRef: {
          repo: "30617:x:packs",
          sha: SHA,
          role: "builder",
          path: "personas/roles/builder",
        },
        packDiffersFromInstalled: false,
        providerAuthorityPubkey: "9".repeat(64),
      },
    ],
    assignments: [
      {
        key: "e".repeat(64),
        sessionRef: "c".repeat(64),
        sessionName: "Project team setup",
        sessionClosed: false,
        openTarget: {
          channelId: "chan-1",
          generationId: "gen-lead",
          founded: false,
        },
        role: "builder",
        assignerPubkey: "1".repeat(64),
        assignerName: "Loom",
        objective: "Set up local development",
        brief: "Serialized setup on main.",
        acceptanceSteps: ["Run the gate"],
        status: "settled",
        reportCount: 2,
        latestDecision: "approve-with-notes",
        createdAt: 1,
      },
    ],
    lastSeenSeconds: 3 * 3_600,
    ...overrides,
  };
}

const READY_SCOPE = {
  kind: "ready",
  scannedSessions: 3,
  visibleSessions: 3,
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
        isLoading: false,
        notices: [],
        onOpenSession: () => {},
        projectId: "p1",
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
  const client = new QueryClient();
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(RouterProvider, { router }),
    ),
  );
}

test("the three sections render with their agents, and empty sections are omitted", async () => {
  const html = await render({
    model: {
      working: [row()],
      installed: [
        row({
          pubkey: "4".repeat(64),
          name: "Sage",
          section: "installed",
          relationship: { kind: "installed", role: "verifier" },
          sessions: [],
          assignments: [],
          installations: [
            {
              role: "verifier",
              packRef: {
                repo: "30617:x:packs",
                sha: SHA,
                role: "verifier",
                path: "personas/roles/verifier",
              },
            },
          ],
          lastSeenSeconds: null,
        }),
      ],
      previous: [],
    },
  });

  assert.match(html, /data-testid="project-agents-working"/);
  assert.match(html, /data-testid="project-agents-installed"/);
  assert.doesNotMatch(html, /data-testid="project-agents-previous"/);
  assert.match(html, /Builder in Project team setup · assigned by Loom/);
  assert.match(html, /Installed as Verifier · instructions f0132d13/);
  // The installation line is the whole sentence; it is not said twice.
  assert.equal(html.split("Installed as Verifier").length - 1, 1);
  assert.match(html, /claude-primary · sonnet · idle \(3h\)/);
  assert.match(html, /Instructions builder @ f0132d13/);
  assert.match(html, /Set up local development/);
  assert.match(html, /settled · approve-with-notes · 2 reports/);
  assert.match(html, /href="\/projects\/p1\/packs"/);
  // A managed identity can be renamed in place; the rename control is there.
  assert.match(html, /Rename/);
});

test("an identity not managed here offers no rename", async () => {
  const html = await render({
    model: {
      working: [row({ managedHere: false })],
      installed: [],
      previous: [],
    },
  });
  assert.doesNotMatch(html, /Rename/);
  assert.doesNotMatch(html, /On this computer/);
});

test("empty and loading are different sentences", async () => {
  const empty = { working: [], installed: [], previous: [] };
  assert.match(
    await render({ model: empty }),
    /data-testid="project-agents-empty"/,
  );
  assert.match(
    await render({ model: empty, isLoading: true }),
    /data-testid="project-agents-loading"/,
  );
});

test("an assignment read that covered only part of the sessions says so, with a way to read more", async () => {
  const html = await render({
    model: { working: [row()], installed: [], previous: [] },
    assignments: {
      ...READY_SCOPE,
      scannedSessions: 8,
      visibleSessions: 12,
      hasMore: true,
    },
  });
  assert.match(html, /Assignments read from the newest 8 of 12 sessions\./);
  assert.match(html, /data-testid="project-agents-read-more"/);
});

test("copy: relationship sentences and instruction lines", () => {
  assert.equal(
    relationshipText({
      kind: "seated",
      role: "project-setup",
      sessionName: "Setup",
      assignerName: null,
    }),
    "Project Setup in Setup",
  );
  assert.equal(
    relationshipText({
      kind: "assigned",
      role: "verifier",
      sessionName: "Audit",
      assignerName: "Brian",
    }),
    "Assigned as Verifier in Audit · by Brian",
  );
  assert.equal(
    sessionInstructionsText({ packRef: null, packDiffersFromInstalled: false }),
    "Instructions revision not reported",
  );
  assert.match(
    sessionInstructionsText({
      packRef: { repo: "r", sha: SHA, role: "builder", path: "p" },
      packDiffersFromInstalled: true,
    }),
    /differs from the revision installed here$/,
  );
  assert.equal(
    assignmentScopeText({
      ...READY_SCOPE,
      kind: "unreadable",
      message: "boom",
    }),
    "Assignments could not be read: boom",
  );
  assert.equal(assignmentScopeText(READY_SCOPE), null);
});
