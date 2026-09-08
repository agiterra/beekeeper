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
  const contributorsRoute = createRoute({
    getParentRoute: () => rootRoute,
    path: "/projects/$projectId/contributors",
  });
  const router = createRouter({
    history: createMemoryHistory({ initialEntries: ["/projects/p1/packs"] }),
    routeTree: rootRoute.addChildren([
      projectRoute,
      packsRoute,
      contributorsRoute,
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
  assert.match(
    html,
    /data-testid="project-tab-contributors"[^>]*>Contributors</,
  );
});
