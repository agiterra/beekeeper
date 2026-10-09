import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const SHOTS = "test-results/projects-sidebar";

// A project group is two flat lists — channels, then sessions — with no
// sub-headers. Repositories and tooling are the project page's business.
test("project groups list channels and sessions, not repos", async ({
  page,
}) => {
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
  const children = group.getByTestId("project-children-general");
  await expect(children).toBeVisible();

  // The seeded mock repo is unclaimed and lands in General — on the project
  // page, not in the sidebar. No sub-headers either: the old collapsible
  // sections are gone, so there is nothing to toggle.
  await expect(group.getByTestId("project-code-row")).toHaveCount(0);
  await expect(group.getByTestId("project-pulse-row")).toHaveCount(0);
  await expect(group.getByRole("region")).toHaveCount(0);
  await expect(group.getByRole("button", { name: "Channels" })).toHaveCount(0);
  await expect(children.getByTestId("project-channels-general")).toBeVisible();
  const channelRow = children.getByTestId("channel-general");
  await expect(channelRow).toBeVisible();
  // No sessions in the fixture: no session list, and no filter to apply.
  await expect(children.getByTestId("project-sessions-general")).toHaveCount(0);
  await expect(group.getByTestId("project-session-filter-general")).toHaveCount(
    0,
  );

  // Collapsing the project hides the whole child list.
  await group.getByTestId("project-group-toggle-general").click();
  await expect(group.getByTestId("project-children-general")).toHaveCount(0);
  await group.getByTestId("project-group-toggle-general").click();
  await expect(group.getByTestId("project-children-general")).toBeVisible();
  await expect(children.getByTestId("channel-general")).toBeVisible();

  // The unclaimed repository (`design-system` in the fixture; `buzz` is its
  // own project) is still one click away on the project page's Code card.
  await group.getByTestId("project-open-general").click();
  await expect(page).toHaveURL(/\/projects\/[^/?]+\/?$/);
  await expect(
    page
      .getByTestId("project-screen-item-row")
      .filter({ hasText: "design-system" }),
  ).toBeVisible({ timeout: 10_000 });
  await page.goBack();
  await expect(children.getByTestId("channel-general")).toBeVisible({
    timeout: 10_000,
  });

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
          window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.filter(
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
  const beekeeperRepo = page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first();
  await expect(beekeeperRepo).toBeVisible({ timeout: 10_000 });

  // Scope to General (the migration swept the mock repos into it). The
  // dropdown lives in the toolbar, so it works on every tab.
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "General" }).click();
  await expect(beekeeperRepo).toBeVisible();

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

  // The project page's Workflows section picks the new workflow up under
  // General; its row navigates to the workflow detail route. The sidebar
  // never lists workflows — it is channels and sessions only. (No page
  // reload — the mock relay state is per-page-load.)
  const group = page.getByTestId("project-group-general");
  await expect(group).toBeVisible({ timeout: 10_000 });
  await expect(group.getByTestId("project-workflow-row")).toHaveCount(0);
  const workflowRow = page.getByRole("button", { name: "Nightly deploy" });
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
        window.__BEEKEEPER_E2E_SIGNED_EVENTS__?.find(
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

  // Settings dialog pre-fills visibility. Members live on their own tab,
  // backed by the same roster manager as the project page's Members card —
  // the head event's p tags are never edited from the dialog.
  await page.getByTestId("manage-project-actions-skunkworks").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await expect(page.getByTestId("edit-project-container-name")).toHaveValue(
    "Skunkworks",
  );
  await expect(
    page.getByTestId("edit-project-container-visibility"),
  ).toHaveText(/Private/);
  await expect(page.getByTestId("project-settings-tab-members")).toBeVisible();
  await expect(
    page.getByTestId("edit-project-container-members-recipient-field"),
  ).toHaveCount(0);
});

// ── Project ordering ─────────────────────────────────────────────────────────

const OWNER = "deadbeef".repeat(8);

/** Two containers whose alphabetical order is the reverse of their age. */
function seedOrderingProjects(page: import("@playwright/test").Page) {
  return page.addInitScript(
    ({ owner }) => {
      window.localStorage.setItem(
        "buzz-feature-overrides-v1",
        JSON.stringify({ projects: true }),
      );
      window.__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = [
        {
          id: "seeded-project-zulu",
          kind: 30621,
          pubkey: owner,
          // Oldest head, last alphabetically.
          created_at: 1_700_000_000,
          content: "",
          tags: [
            ["d", "zulu"],
            ["name", "Zulu"],
          ],
        },
        {
          id: "seeded-project-alpha",
          kind: 30621,
          pubkey: owner,
          // Newest head, first alphabetically. Under the old creation-time
          // sort this sat below Zulu — and every sub-item added to a project
          // republished its head, moving it the same way.
          created_at: 1_800_000_000,
          content: "",
          tags: [
            ["d", "alpha"],
            ["name", "Alpha"],
          ],
        },
      ];
    },
    { owner: OWNER },
  );
}

/**
 * The rendered sidebar project order, by dtag. The prefix also matches each
 * group's collapse toggle (`project-group-toggle-<dtag>`), so those are
 * dropped.
 */
function projectOrder(page: import("@playwright/test").Page) {
  return page.evaluate(() =>
    [...document.querySelectorAll("[data-testid^='project-group-']")]
      .map((el) =>
        el.getAttribute("data-testid")?.replace("project-group-", ""),
      )
      .filter(
        (dtag): dtag is string => Boolean(dtag) && !dtag.startsWith("toggle-"),
      ),
  );
}

test("project order is alphabetical and ignores the head timestamp", async ({
  page,
}) => {
  await seedOrderingProjects(page);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await expect(page.getByTestId("project-group-alpha")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByTestId("project-group-zulu")).toBeVisible();

  // General pinned first, then A→Z — not oldest-head-first, which would put
  // Zulu ahead of Alpha.
  // General pinned first, then A→Z over every project including the seeded
  // mock `buzz` container — not oldest-head-first, which would put Zulu first.
  await expect
    .poll(() => projectOrder(page))
    .toEqual(["general", "alpha", "buzz", "zulu"]);

  // General is the fallback bucket, not a peer project: no reorder grip.
  await expect(page.getByTestId("project-drag-handle-general")).toHaveCount(0);
  await expect(page.getByTestId("project-drag-handle-alpha")).toBeVisible();
});

test("dragging a project reorders the sidebar and persists the order", async ({
  page,
}) => {
  await seedOrderingProjects(page);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await expect(page.getByTestId("project-group-zulu")).toBeVisible({
    timeout: 10_000,
  });
  await expect
    .poll(() => projectOrder(page))
    .toEqual(["general", "alpha", "buzz", "zulu"]);

  const handle = page.getByTestId("project-drag-handle-zulu");
  await page.getByTestId("project-group-zulu").hover();
  const handleBox = await handle.boundingBox();
  const targetBox = await page.getByTestId("project-group-alpha").boundingBox();
  expect(handleBox && targetBox).toBeTruthy();
  if (!handleBox || !targetBox) throw new Error("missing drag geometry");

  const startX = handleBox.x + handleBox.width / 2;
  const startY = handleBox.y + handleBox.height / 2;
  const targetY = targetBox.y + 8;

  // dnd-kit's PointerSensor needs a 6px activation distance before it picks
  // the drag up, so nudge first and then move in small steps.
  await page.mouse.move(startX, startY);
  await page.mouse.down();
  await page.mouse.move(startX, startY - 3, { steps: 3 });
  await page.mouse.move(startX, targetY, { steps: 20 });
  await page.mouse.up();

  await expect
    .poll(() => projectOrder(page))
    .toEqual(["general", "zulu", "alpha", "buzz"]);

  // The chosen order is written through to the relay-scoped local blob (the
  // encrypted kind:30078 publish behind it is debounced 2s and mocked here).
  await expect
    .poll(() =>
      page.evaluate(() => {
        const key = Object.keys(window.localStorage).find((candidate) =>
          candidate.startsWith("buzz-project-order.v1:"),
        );
        if (!key) return null;
        const raw = window.localStorage.getItem(key);
        return raw
          ? ((JSON.parse(raw) as { order: string[] }).order?.slice(0, 2) ??
              null)
          : null;
      }),
    )
    .toEqual([`${OWNER}:zulu`, `${OWNER}:alpha`]);
});

test("startup waits for project placement and paints the saved order immediately", async ({
  page,
}) => {
  await seedOrderingProjects(page);
  await page.addInitScript(
    ({ owner }) => {
      const key = `buzz-project-order.v1:${owner}:${encodeURIComponent("ws://localhost:3000")}`;
      localStorage.setItem(
        key,
        JSON.stringify({
          version: 1,
          order: [`${owner}:zulu`, `${owner}:alpha`],
        }),
      );
      const frames: string[][] = [];
      Object.assign(window, { __sidebarStartupFrames: frames });
      new MutationObserver(() => {
        const groups = [
          ...document.querySelectorAll('[data-testid^="project-group-"]'),
        ]
          .map((node) =>
            (node.getAttribute("data-testid") ?? "").replace(
              "project-group-",
              "",
            ),
          )
          .filter((name) => !name.startsWith("toggle-"));
        if (groups.length) frames.push(groups);
      }).observe(document, { subtree: true, childList: true });
    },
    { owner: OWNER },
  );
  await installMockBridge(page, { projectSnapshotReadDelayMs: 1500 });
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("sidebar-loading")).toBeVisible();
  await expect(page.getByTestId("project-group-general")).toHaveCount(0);
  await expect(page.getByTestId("project-group-zulu")).toBeVisible();
  await expect
    .poll(() => projectOrder(page))
    .toEqual(["general", "zulu", "alpha", "buzz"]);
  const frames = await page.evaluate(
    () =>
      (window as Window & { __sidebarStartupFrames: string[][] })
        .__sidebarStartupFrames,
  );
  expect(frames.length).toBeGreaterThan(0);
  for (const frame of frames)
    expect(frame).toEqual(["general", "zulu", "alpha", "buzz"]);
});
