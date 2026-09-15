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
  associateConfirmText,
  assignmentScopeText,
  relationshipText,
  sessionInstructionsText,
} from "./projectAgentsCopy.ts";

const SHA = "f0132d13".padEnd(40, "0");

const PROJECT_REF = `30621:${"6".repeat(64)}:tank-loop`;

function row(overrides = {}) {
  return {
    pubkey: "2".repeat(64),
    name: "Bob",
    avatarUrl: null,
    section: "borrowed",
    isProjectAgent: false,
    claimAuthority: null,
    carriedFromAnotherComputer: false,
    state: "working",
    location: { kind: "here" },
    primaryRole: "builder",
    seatedRoles: ["builder"],
    managedHere: true,
    associationMissing: false,
    otherProject: null,
    mayAssociate: true,
    relationship: {
      kind: "seated",
      role: "builder",
      sessionName: "Project team setup",
      assignerName: "Loom",
    },
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
        status: "running",
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

function projectAgent(overrides = {}) {
  return row({
    pubkey: "4".repeat(64),
    name: "Builder",
    section: "project",
    isProjectAgent: true,
    state: "available",
    mayAssociate: false,
    relationship: null,
    sessions: [],
    assignments: [],
    lastSeenSeconds: null,
    ...overrides,
  });
}

const EMPTY = {
  projectAgents: [],
  unverified: [],
  borrowed: [],
  available: [],
  previous: [],
  readiness: [],
};
const ALLOWED = { kind: "allowed" };

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
        associateAccess: ALLOWED,
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
  const client = new QueryClient();
  return renderToStaticMarkup(
    React.createElement(
      QueryClientProvider,
      { client },
      React.createElement(RouterProvider, { router }),
    ),
  );
}

function between(html, startTestId, endTestId) {
  const start = html.indexOf(`data-testid="${startTestId}"`);
  const end = endTestId
    ? html.indexOf(`data-testid="${endTestId}"`)
    : html.length;
  assert.ok(start >= 0, `${startTestId} rendered`);
  return html.slice(start, end < 0 ? html.length : end);
}

test("the three sections render under their headings, and empty sections are omitted", async () => {
  const html = await render({
    model: {
      ...EMPTY,
      projectAgents: [
        projectAgent(),
        projectAgent({
          pubkey: "7".repeat(64),
          name: "Andy's Runner",
          primaryRole: "runner",
          state: "elsewhere",
          managedHere: false,
          location: {
            kind: "elsewhere",
            ownerPubkey: "a".repeat(64),
            ownerName: "Andy",
          },
        }),
      ],
      borrowed: [row()],
    },
  });

  assert.match(html, />Project agents <span/);
  assert.match(html, />Borrowed participants <span/);
  assert.doesNotMatch(html, /data-testid="project-agents-previous"/);
  assert.doesNotMatch(html, /Working here|Installed, waiting/);

  const members = between(
    html,
    "project-agents-members",
    "project-agents-borrowed",
  );
  assert.match(members, /Project agent/);
  assert.match(members, /Available/);
  assert.match(members, /On another computer/);
  assert.match(members, /Owned by Andy · can&#x27;t run on this computer/);
  assert.doesNotMatch(members, /Not a Tank Loop agent/);

  const borrowed = between(html, "project-agents-borrowed");
  assert.match(borrowed, />Borrowed</);
  assert.match(borrowed, />Working</);
  assert.match(
    borrowed,
    /Not a Tank Loop agent — seated here without project association\. New hires use this project&#x27;s agents only\./,
  );
  assert.match(borrowed, /Builder in Project team setup · assigned by Loom/);
  assert.match(borrowed, /claude-primary · sonnet · running \(3h\)/);
  assert.match(borrowed, /Instructions builder @ f0132d13/);
  assert.match(borrowed, /host 99999999…9999/);
  assert.match(borrowed, /settled · approve-with-notes · 2 reports/);
  assert.match(html, /href="\/projects\/p1\/packs"/);
  // A managed identity can be renamed in place.
  assert.match(html, /Rename/);
});

test("state is said in words for every state", async () => {
  const states = {
    working: "Working",
    idle: "Idle",
    disconnected: "Disconnected",
    available: "Available",
    "not-associated": "Not associated yet",
    elsewhere: "On another computer",
    carried: "Associated from another computer",
    "not-running": "Not running",
    historical: "Historical",
  };
  for (const [state, word] of Object.entries(states)) {
    const html = await render({
      model: { ...EMPTY, projectAgents: [projectAgent({ state })] },
    });
    assert.match(
      html,
      new RegExp(`data-testid="project-agent-state"[^>]*>.*?${word}</span>`),
      state,
    );
  }
});

test("a previous participant keeps its borrowed label beside Previously here", async () => {
  const html = await render({
    model: {
      ...EMPTY,
      previous: [
        row({
          name: "Ira",
          section: "previous",
          state: "historical",
          otherProject: { ref: "x", name: "Attic" },
          mayAssociate: false,
        }),
      ],
    },
  });
  const previous = between(html, "project-agents-previous");
  assert.match(previous, />Previously here</);
  assert.match(previous, />Borrowed</);
  assert.match(previous, /Not a Tank Loop agent — belongs to Attic\./);
  assert.doesNotMatch(previous, /Associate with/);
});

test("installed but unassociated: the warning is on the project row, with Associate", async () => {
  const html = await render({
    model: {
      ...EMPTY,
      projectAgents: [
        projectAgent({
          associationMissing: true,
          state: "not-associated",
          mayAssociate: true,
        }),
      ],
    },
  });
  assert.match(
    html,
    /Installed for this project but not associated yet — the lead can&#x27;t hire it\. Reopen setup or associate it\./,
  );
  assert.match(html, /Associate with Tank Loop/);
});

test("Associate is enabled for a project writer and disabled with its reason otherwise", async () => {
  const model = { ...EMPTY, borrowed: [row()] };
  const allowed = await render({ model });
  const button = allowed.match(
    /<button[^>]*data-testid="project-agent-associate-button"[^>]*>/,
  )[0];
  assert.doesNotMatch(button, / disabled=""/);

  const denied = await render({
    model,
    associateAccess: {
      kind: "denied",
      reason:
        "Only Tank Loop's owners and collaborators can associate agents with it.",
    },
  });
  const deniedButton = denied.match(
    /<button[^>]*data-testid="project-agent-associate-button"[^>]*>/,
  )[0];
  assert.match(deniedButton, / disabled=""/);
  assert.match(
    denied,
    /data-testid="project-agent-associate-denied"[^>]*>Only Tank Loop&#x27;s owners and collaborators/,
  );

  const notOffered = await render({
    model: { ...EMPTY, borrowed: [row({ mayAssociate: false })] },
  });
  assert.doesNotMatch(notOffered, /project-agent-associate-button/);
});

test("an identity not managed here offers no rename and says where it is", async () => {
  const html = await render({
    model: {
      ...EMPTY,
      borrowed: [
        row({
          managedHere: false,
          mayAssociate: false,
          location: { kind: "unknown" },
        }),
      ],
    },
  });
  assert.doesNotMatch(html, /Rename/);
  assert.doesNotMatch(html, /On this computer/);
  assert.match(html, /Not on this computer/);
});

test("rows wrap and truncate so a narrow window stays usable", async () => {
  const html = await render({
    model: { ...EMPTY, borrowed: [row()] },
  });
  const rowTag = html.match(/<li[^>]*data-testid="project-agent-row"[^>]*>/)[0];
  assert.match(rowTag, /min-w-0/);
  assert.match(html, /class="flex min-w-0 flex-wrap items-start gap-2"/);
  assert.match(
    html,
    /class="min-w-0 truncate text-sm font-medium text-foreground" data-testid="project-agent-name" title="Bob · 2{64}"/,
  );
  assert.match(
    html,
    /class="truncate font-mono text-2xs text-muted-foreground" data-testid="project-agent-pubkey" title="2{64}"/,
  );
  // rem tokens only: no arbitrary pixel text.
  assert.doesNotMatch(html, /text-\[\d+px\]/);
});

test("empty and loading are different sentences", async () => {
  const empty = EMPTY;
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
    model: { ...EMPTY, borrowed: [row()] },
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

test("copy: relationship sentences, association confirmation and instruction lines", () => {
  assert.equal(
    associateConfirmText("Bob", "Tank Loop", "builder"),
    "Bob becomes a permanent Tank Loop Builder agent. Its history stays attributed to Bob. This does not change project access.",
  );
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
