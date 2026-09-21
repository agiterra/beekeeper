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

import { ProjectPageTabs } from "./ProjectPageTabs.tsx";

async function renderTabs(props) {
  const rootRoute = createRootRoute({
    component: () => React.createElement(ProjectPageTabs, props),
  });
  const projectRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectId",
  });
  const packsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectId/packs",
  });
  const agentsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectId/agents",
  });
  const actionsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectId/actions",
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/projects/p1/packs"] }),
    routeTree: rootRoute.addChildren([
      projectRoute,
      packsRoute,
      agentsRoute,
      actionsRoute,
    ]),
  });
  await router.load();
  return renderToStaticMarkup(React.createElement(RouterProvider, { router }));
}

test("the packs tab reads 'Roles' while keeping its id, path and testid", async () => {
  const html = await renderTabs({
    active: "packs",
    projectId: "p1",
    showPulse: false,
  });

  assert.match(html, /data-testid="project-tab-packs"/);
  assert.match(html, /href="\/projects\/p1\/packs"/);
  assert.match(html, /data-testid="project-tab-packs"[^>]*>Roles</);
  // The old label is gone; nothing on this strip says "Packs".
  assert.doesNotMatch(html, />Packs</);
});

test("the active packs tab is still marked as the current page", async () => {
  const html = await renderTabs({
    active: "packs",
    projectId: "p1",
    showPulse: true,
  });

  assert.match(html, /data-state="active" data-testid="project-tab-packs"/);
  assert.match(html, /data-testid="project-tab-overview"/);
  assert.match(html, /data-testid="project-tab-pulse"/);
  assert.match(html, /data-testid="project-tab-agents"[^>]*>Agents</);
});

test("the strip reads Overview, Pulse, To-Do, Agents, Actions, Roles, Files, and Contributors is gone", async () => {
  const html = await renderTabs({
    active: "agents",
    projectId: "p1",
    showPulse: true,
  });

  const order = [...html.matchAll(/data-testid="project-tab-([a-z]+)"/g)].map(
    (match) => match[1],
  );
  assert.deepEqual(order, [
    "overview",
    "pulse",
    "todos",
    "agents",
    "actions",
    "packs",
    "files",
  ]);
  assert.match(html, /href="\/projects\/p1\/files"/);
  assert.match(html, /data-testid="project-tab-files"[^>]*>Files</);
  assert.match(html, /href="\/projects\/p1\/todos"/);
  assert.match(html, /href="\/projects\/p1\/agents"/);
  assert.match(html, /href="\/projects\/p1\/actions"/);
  assert.match(html, /data-testid="project-tab-actions"[^>]*>Actions</);
  assert.match(html, /data-state="active" data-testid="project-tab-agents"/);
  assert.doesNotMatch(html, /Contributors/);
});
