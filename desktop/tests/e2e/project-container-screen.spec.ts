import { expect, test, type Page } from "@playwright/test";

import { installMockBridge } from "../helpers/bridge";

// The per-project screen (sidebar ↗ arrow) is the project-scoped management
// surface: owner-gated edit/delete, per-row "Move to project" menus, and
// per-section + create buttons — the all-projects manage tab keeps the
// cross-project view.

const FEATURES = JSON.stringify({
  projects: true,
  forum: true,
  workflows: true,
});

async function boot(page: Page) {
  await page.addInitScript((features) => {
    window.localStorage.setItem("buzz-feature-overrides-v1", features);
  }, FEATURES);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
}

async function createProject(
  page: Page,
  name: string,
  options?: { visibility?: "private" },
) {
  await page.getByTestId("project-container-new").click();
  await page.getByTestId("create-project-container-name").fill(name);
  if (options?.visibility === "private") {
    await page.getByTestId("create-project-container-visibility").click();
    await page
      .getByTestId("create-project-container-visibility-option-private")
      .click();
  }
  await page.getByTestId("create-project-container-submit").click();
}

async function openProjectScreen(page: Page, dtag: string) {
  const group = page.getByTestId(`project-group-${dtag}`);
  await expect(group).toBeVisible({ timeout: 10_000 });
  await group.hover();
  await page.getByTestId(`project-open-${dtag}`).click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/?$/);
}

test("owner can edit a project from its screen; General cannot be deleted", async ({
  page,
}) => {
  await boot(page);
  await createProject(page, "Skunkworks");
  await openProjectScreen(page, "skunkworks");

  await page.getByTestId("project-screen-actions").click();
  await page.getByTestId("project-screen-edit").click();
  await expect(page.getByTestId("edit-project-container-name")).toHaveValue(
    "Skunkworks",
  );
  await page.getByTestId("edit-project-container-name").fill("Moonshot");
  await page.getByTestId("edit-project-container-save").click();
  await expect(page.getByRole("heading", { name: "Moonshot" })).toBeVisible({
    timeout: 10_000,
  });

  // The rename republished the container: two kind:30621 heads share the dtag.
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.filter(
            (event) =>
              event.kind === 30621 &&
              event.tags.some(
                (tag) => tag[0] === "d" && tag[1] === "skunkworks",
              ),
          ).length ?? 0,
      ),
    )
    .toBeGreaterThan(1);

  // General is editable but never deletable.
  await page.goBack();
  await openProjectScreen(page, "general");
  await page.getByTestId("project-screen-actions").click();
  await expect(page.getByTestId("project-screen-edit")).toBeVisible();
  await expect(page.getByTestId("project-screen-delete")).toHaveCount(0);
});

test("item rows offer moves; private targets require confirmation", async ({
  page,
}) => {
  await boot(page);
  await createProject(page, "Skunkworks", { visibility: "private" });
  await expect(page.getByTestId("project-lock-skunkworks")).toBeVisible({
    timeout: 10_000,
  });
  await openProjectScreen(page, "general");

  // The seeded mock repos are unclaimed, so they land under General.
  const repoRow = page.getByTestId("project-screen-item-row").first();
  await expect(repoRow).toBeVisible({ timeout: 10_000 });
  await repoRow.hover();
  await repoRow.getByTestId("manage-item-move").click();
  await page.getByRole("menuitem", { name: "Skunkworks" }).click();

  // Moving into a private project pauses on the shared confirm gate.
  await expect(page.getByTestId("manage-move-confirm")).toBeVisible();
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByTestId("manage-move-confirm")).toHaveCount(0);
});

test("section + buttons create items scoped to the project", async ({
  page,
}) => {
  await boot(page);
  await openProjectScreen(page, "general");

  // Repo: the Code section's + menu opens the repo-only dialog — not the
  // legacy create-project dialog (which spawned a whole new project
  // container alongside the repo).
  await page.getByTestId("project-section-create-repo").click();
  await page.getByTestId("project-section-create-repo-new").click();
  await expect(page.getByTestId("create-project-repo-dialog")).toBeVisible();
  await expect(page.getByTestId("create-project-dialog")).toHaveCount(0);
  await page.getByTestId("create-project-repo-name").fill("widget-lib");
  await page.getByTestId("create-project-repo-submit").click();
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.filter(
            (event) =>
              event.kind === 30617 &&
              event.tags.some(
                (tag) => tag[0] === "d" && tag[1] === "widget-lib",
              ) &&
              event.tags.some(
                (tag) =>
                  tag[0] === "project" &&
                  /^30621:[0-9a-f]{64}:general$/.test(tag[1] ?? ""),
              ),
          ).length ?? 0,
      ),
    )
    .toBe(1);
  // Regression: creating a repo inside a project must not publish a sibling
  // kind:30621 project container carrying the repo's dtag.
  expect(
    await page.evaluate(
      () =>
        window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.filter(
          (event) =>
            event.kind === 30621 &&
            event.tags.some((tag) => tag[0] === "d" && tag[1] === "widget-lib"),
        ).length ?? 0,
    ),
  ).toBe(0);

  // Channel: creates into the project and navigates to the new channel.
  await page.getByTestId("project-section-create-channel").click();
  await page.getByTestId("create-channel-name").fill("skunk-chat");
  await page.getByTestId("create-channel-submit").click();
  await expect(page).toHaveURL(/\/channels\//, { timeout: 10_000 });
});

test("the Code section + menu attaches an existing repository", async ({
  page,
}) => {
  // Seed a standalone (unclaimed) repo owned by the mock identity
  // (deadbeef…, the mock-mode default) so the attach picker has a candidate
  // and the move can republish its back-ref.
  await page.addInitScript(() => {
    window.__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = [
      {
        id: "seeded-standalone-repo",
        kind: 30617,
        pubkey: "deadbeef".repeat(8),
        created_at: 1_700_000_000,
        content: "",
        tags: [
          ["d", "drifter"],
          ["name", "drifter"],
        ],
      },
    ];
  });
  await boot(page);
  await createProject(page, "Attic");
  await openProjectScreen(page, "attic");

  await page.getByTestId("project-section-create-repo").click();
  await page.getByTestId("project-section-create-repo-attach").click();
  await expect(page.getByTestId("attach-project-repo-dialog")).toBeVisible();
  await page.getByTestId("attach-project-repo-item-drifter").click();

  // The move republished the repo's kind:30617 with the attic back-ref.
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.filter(
            (event) =>
              event.kind === 30617 &&
              event.tags.some(
                (tag) => tag[0] === "d" && tag[1] === "drifter",
              ) &&
              event.tags.some(
                (tag) =>
                  tag[0] === "project" &&
                  /^30621:[0-9a-f]{64}:attic$/.test(tag[1] ?? ""),
              ),
          ).length ?? 0,
      ),
    )
    .toBeGreaterThan(0);
});

test("the Code section + menu imports a local repository", async ({ page }) => {
  await boot(page);
  await openProjectScreen(page, "general");

  await page.getByTestId("project-section-create-repo").click();
  await page.getByTestId("project-section-create-repo-import").click();
  await expect(page.getByTestId("import-project-repo-dialog")).toBeVisible();

  // The mock native picker returns /tmp/buzz/import/widget-lib (no origin).
  await page.getByTestId("import-project-repo-browse").click();
  await expect(page.getByTestId("import-project-repo-name")).toHaveValue(
    "widget-lib",
  );
  // No foreign origin — the remote-strategy prompt stays hidden.
  await expect(
    page.getByTestId("import-project-repo-remote-origin"),
  ).toHaveCount(0);
  await page.getByTestId("import-project-repo-submit").click();

  // Exactly one relay-hosted announcement: project back-ref, no clone tag.
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.filter(
            (event) =>
              event.kind === 30617 &&
              event.tags.some(
                (tag) => tag[0] === "d" && tag[1] === "widget-lib",
              ) &&
              event.tags.some(
                (tag) =>
                  tag[0] === "project" &&
                  /^30621:[0-9a-f]{64}:general$/.test(tag[1] ?? ""),
              ) &&
              !event.tags.some((tag) => tag[0] === "clone"),
          ).length ?? 0,
      ),
    )
    .toBe(1);
  // The import command received the chosen path and the derived clone URL.
  const imported = await page.evaluate(
    () => window.__BEEKEEPER_E2E_IMPORTED_REPO__,
  );
  expect(imported?.path).toBe("/tmp/buzz/import/widget-lib");
  expect(imported?.dtag).toBe("widget-lib");
  expect(imported?.remoteStrategy).toBe("set-origin");
  expect(imported?.cloneUrl).toMatch(/\/git\/[0-9a-f]{64}\/widget-lib$/);
});

test("importing a checkout with a foreign origin offers the buzz remote", async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.__BEEKEEPER_E2E_IMPORT_FOLDER__ = {
      path: "/tmp/buzz/import/forked-lib",
      name: "forked-lib",
      is_git_repo: true,
      current_branch: "main",
      origin_url: "https://github.com/example/forked-lib.git",
      has_commits: true,
    };
  });
  await boot(page);
  await openProjectScreen(page, "general");

  await page.getByTestId("project-section-create-repo").click();
  await page.getByTestId("project-section-create-repo-import").click();
  await page.getByTestId("import-project-repo-browse").click();

  await expect(
    page.getByTestId("import-project-repo-remote-origin"),
  ).toBeVisible();
  await page.getByTestId("import-project-repo-remote-buzz").check();
  await page.getByTestId("import-project-repo-submit").click();

  await expect
    .poll(async () =>
      page.evaluate(
        () => window.__BEEKEEPER_E2E_IMPORTED_REPO__?.remoteStrategy,
      ),
    )
    .toBe("add-buzz-remote");
});

test("a repo row links an existing local checkout without publishing", async ({
  page,
}) => {
  // Seed a repo owned by someone else, announced fork-style: the clone tag
  // names the external GitHub upstream. Linking must work for any readable
  // repo and always target the derived relay URL, never the upstream.
  await page.addInitScript(() => {
    window.__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = [
      {
        id: "seeded-foreign-repo",
        kind: 30617,
        pubkey: "feedface".repeat(8),
        created_at: 1_700_000_000,
        content: "",
        tags: [
          ["d", "drifter"],
          ["name", "drifter"],
          ["clone", "https://github.com/upstream/drifter.git"],
        ],
      },
    ];
    window.__BEEKEEPER_E2E_IMPORT_FOLDER__ = {
      path: "/tmp/buzz/checkouts/drifter",
      name: "drifter",
      is_git_repo: true,
      current_branch: "main",
      origin_url: "https://github.com/upstream/drifter.git",
      has_commits: true,
    };
  });
  await boot(page);
  await openProjectScreen(page, "general");

  const signedBefore = await page.evaluate(
    () => window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.length ?? 0,
  );

  const repoRow = page
    .getByTestId("project-screen-item-row")
    .filter({ hasText: "drifter" });
  await repoRow.hover();
  await repoRow.getByTestId("manage-item-move").click();
  await page.getByTestId("move-to-project-link-local").click();
  await expect(page.getByTestId("link-project-repo-dialog")).toBeVisible();
  await page.getByTestId("link-project-repo-browse").click();
  await expect(page.getByTestId("link-project-repo-path")).toContainText(
    "/tmp/buzz/checkouts/drifter",
  );
  // The checkout's origin is the GitHub upstream — the remote-strategy
  // prompt appears; keep origin and add a dedicated buzz remote.
  await expect(
    page.getByTestId("link-project-repo-remote-origin"),
  ).toBeVisible();
  await page.getByTestId("link-project-repo-remote-buzz").check();
  await page.getByTestId("link-project-repo-submit").click();

  await expect
    .poll(async () =>
      page.evaluate(() => window.__BEEKEEPER_E2E_LINKED_REPO__?.dtag),
    )
    .toBe("drifter");
  const linked = await page.evaluate(
    () => window.__BEEKEEPER_E2E_LINKED_REPO__,
  );
  expect(linked?.owner).toBe("feedface".repeat(8));
  expect(linked?.path).toBe("/tmp/buzz/checkouts/drifter");
  expect(linked?.remoteStrategy).toBe("add-buzz-remote");
  // Derived relay URL, never the announced GitHub upstream.
  expect(linked?.cloneUrl).toMatch(/\/git\/(feedface){8}\/drifter$/);
  // Linking signs nothing.
  const signedAfter = await page.evaluate(
    () => window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.length ?? 0,
  );
  expect(signedAfter).toBe(signedBefore);
});

test("deleting a project from its screen lands on the projects list", async ({
  page,
}) => {
  await boot(page);
  await createProject(page, "Doomed");
  await openProjectScreen(page, "doomed");

  await page.getByTestId("project-screen-actions").click();
  await page.getByTestId("project-screen-delete").click();
  await page.getByTestId("manage-project-delete-confirm").click();
  await expect(page).toHaveURL(/\/projects\?filter=projects/, {
    timeout: 10_000,
  });
});
