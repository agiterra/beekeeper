import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const SHOTS = "test-results/projects-sidebar";

// Project children stay directly reachable while their labels make the
// difference between a durable session, a conversation channel, and project
// tooling explicit.
test("project groups separate channels from tools", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true, forum: true }),
    );
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const group = page.getByTestId("project-group-general");
  await expect(group).toBeVisible({ timeout: 10_000 });
  // The seeded mock repo is unclaimed, so it lands in General's project group.
  const children = group.getByTestId("project-children-general");
  await expect(children).toBeVisible();
  const repoRow = group.getByTestId("project-code-row").first();
  await expect(repoRow).toBeVisible();

  await expect(
    group.getByRole("region", { name: "Active Sessions" }),
  ).toHaveCount(0);
  await expect(group.getByRole("region", { name: "Channels" })).toBeVisible();
  await expect(
    group.getByRole("region", { name: "Repos & Tools" }),
  ).toBeVisible();

  // Group order keeps channels ahead of repositories without presenting them
  // as the same kind of child.
  const channelRow = children.getByTestId("channel-general");
  await expect(channelRow).toBeVisible();
  const channelBox = await channelRow.boundingBox();
  const repoBox = await repoRow.boundingBox();
  expect(channelBox && repoBox && channelBox.y < repoBox.y).toBe(true);

  // Shelves collapse independently: hiding channels must not hide project
  // tools, and the persisted choice survives the project's own collapse.
  await group.getByRole("button", { name: "Channels" }).click();
  await expect(children.getByTestId("channel-general")).toHaveCount(0);
  await expect(repoRow).toBeVisible();

  // Collapsing the project hides the whole child list.
  await group.getByTestId("project-group-toggle-general").click();
  await expect(group.getByTestId("project-children-general")).toHaveCount(0);
  await expect(group.getByTestId("project-code-row")).toHaveCount(0);
  await group.getByTestId("project-group-toggle-general").click();
  await expect(group.getByTestId("project-children-general")).toBeVisible();
  await expect(children.getByTestId("channel-general")).toHaveCount(0);

  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/sidebar-projects.png`,
    clip: { x: 0, y: 0, width: 320, height: 720 },
  });
});

// Clicking the sidebar "Projects" heading opens the projects screen on the
// management tab, where projects can be created and items organized.
test("projects heading opens the management screen", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true, forum: true }),
    );
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId("open-projects-view").click();
  await expect(page).toHaveURL(/\/projects\?filter=projects/);
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });

  // The General card lists the seeded mock repos with move menus.
  const generalCard = page.getByTestId("manage-project-general");
  await expect(generalCard).toBeVisible();
  await expect(
    generalCard.getByTestId("manage-item-row").first(),
  ).toBeVisible();
  // Channels must belong to a project: there is no global-channels card and
  // unclaimed channels (e.g. the seeded #general) organize under General.
  await expect(page.getByTestId("manage-global-channels")).toHaveCount(0);
  await expect(
    generalCard.getByTestId("manage-item-row").filter({ hasText: "general" }),
  ).toBeVisible();

  // Creating a project publishes a kind:30621 container event. Under "All
  // Projects" the + menu offers only project creation — other kinds need a
  // concrete target project selected in the filter dropdown.
  await page.getByTestId("projects-create-menu").click();
  await expect(
    page.getByRole("menuitem", { name: "Project", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Issue" })).toHaveCount(0);
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Skunkworks");
  await page.getByTestId("create-project-container-submit").click();
  await expect
    .poll(async () =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_SIGNED_EVENTS__?.filter(
            (event) => event.kind === 30621,
          ).length ?? 0,
      ),
    )
    .toBeGreaterThan(0);

  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/projects-manage.png`,
    fullPage: false,
  });
});

// The Repositories/PRs/Issues tabs can be scoped to a single project via the
// "All Projects / <project> …" dropdown.
test("list tabs filter by project", async ({ page }) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true }),
    );
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId("open-projects-view").click();
  await page.getByRole("button", { name: "Repositories", exact: true }).click();

  // Default: all repos visible under "All Projects". (The Repositories tab
  // renders RepositoryCards — repository-card/-row testids — since the
  // multi-repo rework; the fork spec predated that rename.)
  const buzzRepo = page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first();
  await expect(buzzRepo).toBeVisible({ timeout: 10_000 });

  // Scope to General (the migration swept the mock repos into it). The
  // dropdown lives in the toolbar, so it works on every tab.
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "General" }).click();
  await expect(buzzRepo).toBeVisible();

  // With a project selected the + menu unlocks the full create list — the
  // entries target the selected project. (Hover-open: the scope dropdown's
  // closing focus-restore would blur-close a click-opened menu.)
  await page.getByTestId("projects-create-menu").hover();
  await expect(
    page.getByRole("menuitem", { name: "Issue", exact: true }),
  ).toBeVisible();
  await expect(
    page.getByRole("menuitem", { name: "Channel", exact: true }),
  ).toBeVisible();

  // Create an empty project and scope to it — no work items match. (Scope to
  // the tab strip: the sidebar heading shares the "Projects" accessible name.)
  await page
    .locator("fieldset")
    .filter({ has: page.getByText("Project owner filter") })
    .getByRole("button", { name: "Projects", exact: true })
    .click();
  await page.getByTestId("projects-create-menu").click();
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Empty Box");
  await page.getByTestId("create-project-container-submit").click();
  await expect(page.getByTestId("manage-project-empty-box")).toBeVisible();

  // The repository list is scope-independent since the multi-repo rework
  // (every repo shows on the Repositories tab regardless of the dropdown),
  // so assert the scoping on a work-item tab: still under General, the PR
  // list has rows; scoped to the empty project it drains to the empty state.
  await page
    .getByRole("button", { name: "Pull Requests", exact: true })
    .click();
  const prCards = page.getByRole("button", { name: "Review PR" });
  await expect(prCards.first()).toBeVisible({ timeout: 10_000 });
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "Empty Box" }).click();
  await expect(prCards).toHaveCount(0);
  await expect(page.getByText("No pull requests yet.")).toBeVisible();
});

// With the Projects experiment on, the global Workflows menu item disappears
// and workflows surface inside the project owning their trigger channel
// (mock channels are unclaimed, so they land under General).
test("workflows move under projects when the experiment is on", async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true, workflows: true }),
    );
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  // The global menu item is gone; the projects heading is present.
  await expect(page.getByTestId("open-projects-view")).toBeVisible();
  await expect(page.getByTestId("open-workflows-view")).toHaveCount(0);

  // Item creation moved off the sidebar + menu onto the project screen —
  // the menu no longer offers "New workflow".
  await page.getByTestId("project-group-general").hover();
  await page.getByTestId("project-create-general").click();
  await expect(
    page.getByRole("menuitem", { name: "New workflow" }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");

  // Create a workflow from the project screen's Workflows section + button
  // (picks a trigger channel from the project's/General's channels).
  await page.getByTestId("project-group-general").hover();
  await page.getByTestId("project-open-general").click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/?$/);
  await page.getByTestId("project-section-create-workflow").click();
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  await dialog.getByLabel("Workflow name").fill("Nightly deploy");
  await dialog.getByRole("button", { name: "Add step" }).click();
  await dialog.getByRole("button", { name: "Create" }).click();
  await expect(dialog).not.toBeVisible();

  // The sidebar picks the new workflow up under General; its row navigates
  // to the workflow detail route. (No page reload — the mock relay state is
  // per-page-load.)
  const group = page.getByTestId("project-group-general");
  await expect(group).toBeVisible({ timeout: 10_000 });
  const workflowRow = group
    .getByTestId("project-workflow-row")
    .filter({ hasText: "Nightly deploy" });
  await expect(workflowRow).toBeVisible({ timeout: 10_000 });
  await workflowRow.click();
  await expect(page).toHaveURL(/\/workflows\/[^/]+$/);
});

// Creating a private project publishes buzz-access/p tags, shows a lock badge
// everywhere the project is listed, and the Edit dialog pre-fills the
// visibility and invited-member state from the published event.
test("private projects publish access tags and show a lock badge", async ({
  page,
}) => {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true }),
    );
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  const alicePubkey = TEST_IDENTITIES.alice.pubkey;

  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });

  // Create a private project, inviting Alice by pasting her raw hex pubkey
  // (exercises PersonaShareRecipients' allowDirectPubkeyEntry path).
  await page.getByTestId("projects-create-menu").click();
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Skunkworks");
  await page.getByTestId("create-project-container-visibility").click();
  await page
    .getByTestId("create-project-container-visibility-option-private")
    .click();
  const memberSearch = page.getByTestId(
    "create-project-container-members-recipient-search",
  );
  const aliceOption = page.getByTestId(
    `create-project-container-members-recipient-option-${alicePubkey}`,
  );
  // The picker popover can close mid-interaction (Radix's outside-click
  // dismissal racing the preceding visibility-dropdown close) and the mock
  // search directory re-ranks/re-fetches for a bit after typing settles —
  // retry the whole "focus, fill, find the row" sequence rather than relying
  // on one long actionability wait, which can lose either race.
  await expect
    .poll(
      async () => {
        try {
          await memberSearch.click({ timeout: 2_000 });
          await memberSearch.fill(alicePubkey, { timeout: 2_000 });
          await aliceOption.click({ timeout: 2_000 });
          return true;
        } catch {
          return false;
        }
      },
      { timeout: 20_000 },
    )
    .toBe(true);
  // Selecting a member re-opens the picker popover (so more people can be
  // added) — close it before it can intercept the submit click.
  await page.keyboard.press("Escape");
  await page.getByTestId("create-project-container-submit").click();

  const findPublishedEvent = () =>
    page.evaluate(
      () =>
        window.__BUZZ_E2E_SIGNED_EVENTS__?.find(
          (event) =>
            event.kind === 30621 &&
            event.tags.some((tag) => tag[0] === "d" && tag[1] === "skunkworks"),
        ) ?? null,
    );

  await expect.poll(findPublishedEvent).not.toBeNull();
  const published = await findPublishedEvent();
  expect(
    published?.tags.some(
      (tag) => tag[0] === "buzz-access" && tag[1] === "private",
    ),
  ).toBe(true);
  // Initial invitees join as role-carrying collaborators (arity-4 p tag).
  expect(
    published?.tags.some(
      (tag) =>
        tag[0] === "p" && tag[1] === alicePubkey && tag[3] === "collaborator",
    ),
  ).toBe(true);

  // Lock badge on the manage-panel card.
  await expect(
    page.getByTestId("manage-project-lock-skunkworks"),
  ).toBeVisible();

  // Lock badge in the sidebar group too.
  await expect(page.getByTestId("project-lock-skunkworks")).toBeVisible({
    timeout: 10_000,
  });

  // Edit dialog pre-fills visibility. Members are no longer edited here —
  // the project page's Members card is the single management surface.
  await page.getByTestId("manage-project-actions-skunkworks").click();
  await page.getByRole("menuitem", { name: "Edit project" }).click();
  await expect(page.getByTestId("edit-project-container-name")).toHaveValue(
    "Skunkworks",
  );
  await expect(
    page.getByTestId("edit-project-container-visibility"),
  ).toHaveText(/Private/);
  await expect(
    page.getByTestId("edit-project-container-members-recipient-field"),
  ).toHaveCount(0);
});
