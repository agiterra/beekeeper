import { expect, test } from "@playwright/test";

import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";

const SHOTS = "test-results/project-commit-detail";
const ALIGNMENT_TOLERANCE_PX = 2;

// The projects surface is a preview feature — opt in before the app mounts.
// Must run before installMockBridge so React reads the override on mount.
async function enableProjectsFeature(page: import("@playwright/test").Page) {
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true }),
    );
  });
}

test("top-level project lists align dates and overflow actions", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  await page.addInitScript(() => {
    window.localStorage.setItem("buzz.projects.viewMode", "list");
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  await expect(
    page.getByRole("heading", { level: 1, name: "Projects" }),
  ).toBeVisible();

  async function trailingPositions(
    row: import("@playwright/test").Locator,
    {
      actionName = /More options for/,
      dateTestId = "projects-row-date",
      summaryTestId,
    }: {
      actionName?: RegExp;
      dateTestId?: string;
      summaryTestId?: string;
    } = {},
  ) {
    await waitForAnimations(page);
    const date = row.getByTestId(dateTestId);
    const menu = row.getByRole("button", { name: actionName });
    await expect(date).toBeVisible();
    await expect(menu).toBeVisible();
    const dateBox = await date.boundingBox();
    const menuBox = await menu.boundingBox();
    const rowBox = await row.boundingBox();
    const summaryBox = summaryTestId
      ? await row.getByTestId(summaryTestId).boundingBox()
      : null;
    expect(dateBox).not.toBeNull();
    expect(menuBox).not.toBeNull();
    expect(rowBox).not.toBeNull();
    if (summaryTestId) expect(summaryBox).not.toBeNull();
    return {
      dateX: dateBox?.x ?? 0,
      menuX: menuBox?.x ?? 0,
      rowHeight: rowBox?.height ?? 0,
      summaryX: summaryBox?.x ?? null,
    };
  }

  // The flat top-level project list is gone — the Projects tab is the
  // container-management panel — so the Repositories list is the alignment
  // reference for the PR/issue lists below.
  await page.getByTestId("projects-section-repositories").click();
  await page.getByRole("button", { name: "Filter repositories" }).click();
  await expect(
    page.getByRole("menuitem", { name: "My Repositories" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("repository-row-buzz")).toBeVisible();
  await expect(page.getByTestId("repository-row-relay-tools")).toBeVisible();
  const repositoryRow = page.getByTestId("repository-row-buzz");
  await expect(
    repositoryRow.getByTestId("repositories-row-summary"),
  ).toContainText("commits");
  await expect(
    repositoryRow.getByTestId("repositories-row-branch"),
  ).toContainText("main");
  // Subtitle is the repository location (owner/repo for Buzz-hosted repos).
  await expect(repositoryRow.locator("p")).toHaveText(/\/buzz$/);
  const repositoryPositions = await trailingPositions(repositoryRow, {
    actionName: /More options for/,
    dateTestId: "repositories-row-date",
    summaryTestId: "repositories-row-summary",
  });
  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/05-project-repositories-list.png`,
  });
  await page
    .getByTestId("repository-row-relay-tools")
    .getByRole("button", { name: "More options for relay-tools" })
    .click();
  await expect(
    page.getByRole("menuitem", { name: "Clone & open in Terminal" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  await page
    .getByRole("button", { name: "Pull Requests", exact: true })
    .click();
  await page.getByRole("button", { name: "Filter pull requests" }).click();
  await expect(
    page.getByRole("menuitem", { name: "My Pull Requests" }),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  // Creates need a concrete target project — under "All Projects" the menu
  // offers only "Project". Scope to General (the mock repos' home) first.
  await page.getByTestId("projects-create-menu").hover();
  await expect(
    page.getByRole("menuitem", { name: "Project", exact: true }),
  ).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Issue" })).toHaveCount(0);
  await page.keyboard.press("Escape");
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "General" }).click();
  await page.getByTestId("projects-create-menu").hover();
  await expect(page.getByRole("menuitem", { name: "Project" })).toBeVisible();
  await expect(page.getByRole("menuitem", { name: "Issue" })).toBeVisible();
  await page
    .getByRole("menuitem", { name: "Pull Request", exact: true })
    .click();
  await expect(page.getByTestId("create-pull-request-dialog")).toBeVisible();
  await expect(
    page.getByTestId("create-pull-request-repository"),
  ).toBeVisible();
  await page.keyboard.press("Escape");
  await page.getByTestId("projects-create-menu").hover();
  await page.getByRole("menuitem", { name: "Issue" }).click();
  await expect(page.getByTestId("create-issue-repository")).toBeVisible();
  await page.keyboard.press("Escape");
  const pullRequestRow = page
    .locator('[data-testid^="projects-pr-row-"]')
    .first();
  const pullRequestPositions = await trailingPositions(pullRequestRow);
  await pullRequestRow
    .getByRole("button", { name: /More options for/ })
    .click();
  await expect(
    page.getByRole("menuitem", { name: /Review PR|View (draft|merge|closed)/ }),
  ).toBeVisible();
  await page.keyboard.press("Escape");

  await page.getByRole("button", { name: "Issues", exact: true }).click();
  await page.getByRole("button", { name: "Filter issues" }).click();
  await expect(page.getByRole("menuitem", { name: "My Issues" })).toBeVisible();
  await page.keyboard.press("Escape");
  const issueRow = page.locator('[data-testid^="projects-issue-row-"]').first();
  const issuePositions = await trailingPositions(issueRow);

  expect(
    Math.abs(pullRequestPositions.dateX - repositoryPositions.dateX),
  ).toBeLessThanOrEqual(ALIGNMENT_TOLERANCE_PX);
  expect(
    Math.abs(pullRequestPositions.menuX - repositoryPositions.menuX),
  ).toBeLessThanOrEqual(ALIGNMENT_TOLERANCE_PX);
  expect(
    Math.abs(issuePositions.dateX - repositoryPositions.dateX),
  ).toBeLessThanOrEqual(ALIGNMENT_TOLERANCE_PX);
  expect(
    Math.abs(issuePositions.menuX - repositoryPositions.menuX),
  ).toBeLessThanOrEqual(ALIGNMENT_TOLERANCE_PX);

  await page.setViewportSize({ height: 720, width: 900 });
  await page.getByTestId("projects-section-repositories").click();
  const responsiveRepositoryRow = page
    .locator('[data-testid^="repository-row-"]')
    .first();
  await expect(
    responsiveRepositoryRow.getByTestId("repositories-row-summary"),
  ).toBeHidden();
  await expect(
    responsiveRepositoryRow.getByTestId("repositories-row-people"),
  ).toBeHidden();
  await expect(
    responsiveRepositoryRow.getByTestId("repositories-row-date"),
  ).toBeVisible();
  await expect(
    responsiveRepositoryRow.getByRole("button", { name: /More options for/ }),
  ).toBeVisible();
  expect(
    await responsiveRepositoryRow.evaluate(
      (row) => row.scrollWidth <= row.clientWidth,
    ),
  ).toBe(true);
});

test("creating a repository publishes only the repo announcement into the project", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo creation needs a concrete target project — scope to General first
  // ("Project" creates a container; "Repository" opens the repo-only dialog,
  // which publishes a single 30617 carrying the container back-ref).
  // `exact` matters: the same menu offers "Import local repository"
  // (src/features/projects/ui/ProjectsCreateMenu.tsx:99-104), whose
  // accessible name also contains "repository", so a substring match
  // resolves to two elements.
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "General" }).click();
  await page.getByTestId("projects-create-menu").hover();
  await page.getByRole("menuitem", { name: "Repository", exact: true }).click();
  await page.getByTestId("create-project-repo-name").fill("multi-repo-demo");
  await page
    .getByTestId("create-project-repo-clone-url")
    .fill("https://relay.example.com/git/owner/multi-repo-demo.git");
  await page.getByTestId("create-project-repo-submit").click();

  await expect(page.getByTestId("create-project-repo-dialog")).toBeHidden();
  await page.getByTestId("projects-section-repositories").click();
  await expect(
    page
      .locator(
        '[data-testid="repository-card-multi-repo-demo"], [data-testid="repository-row-multi-repo-demo"]',
      )
      .first(),
  ).toBeVisible();

  const createdEvents = await page.evaluate(
    () =>
      window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.filter((event) =>
        event.tags.some(
          (tag) => tag[0] === "d" && tag[1] === "multi-repo-demo",
        ),
      ) ?? [],
  );
  // Regression: the repo-only flow must not publish a sibling kind:30621
  // project container for the repo.
  expect(createdEvents.map((event) => event.kind)).toEqual([30617]);
  expect(
    createdEvents[0]?.tags.some(
      (tag) =>
        tag[0] === "project" &&
        /^30621:[0-9a-f]{64}:general$/.test(tag[1] ?? ""),
    ),
  ).toBe(true);

  // Still scoped to General — a duplicate name is rejected by the guard.
  await page.getByTestId("projects-create-menu").hover();
  await page.getByRole("menuitem", { name: "Repository", exact: true }).click();
  await page.getByTestId("create-project-repo-name").fill("multi-repo-demo");
  await page.getByTestId("create-project-repo-submit").click();
  await expect(page.getByTestId("create-project-repo-dialog")).toBeVisible();
  await expect(
    page.getByText(/A repository named "multi-repo-demo" already exists/),
  ).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.filter((event) =>
            event.tags.some(
              (tag) => tag[0] === "d" && tag[1] === "multi-repo-demo",
            ),
          ).length ?? 0,
      ),
    )
    .toBe(1);
});

test("unsupported relays keep the repository accessible", async ({ page }) => {
  await enableProjectsFeature(page);
  await page.addInitScript(() => {
    window.__BUZZ_E2E_UNSUPPORTED_PROJECT_ANNOUNCEMENTS__ = true;
    // A real target container: creating into the local General placeholder
    // would first publish General itself (a 30621), which this relay flag
    // would reject before the repo flow under test even runs.
    window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
      {
        id: "workbenchseed".padEnd(64, "0"),
        kind: 30621,
        pubkey: "deadbeef".repeat(8),
        created_at: Math.floor(Date.now() / 1000) - 3600,
        content: "",
        tags: [
          ["d", "workbench"],
          ["name", "Workbench"],
        ],
      },
    ];
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo creation needs a concrete target project — scope to the seeded
  // real container (see the init script above for why not General).
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "Workbench" }).click();
  await page.getByTestId("projects-create-menu").hover();
  await page.getByRole("menuitem", { name: "Repository", exact: true }).click();
  await page.getByTestId("create-project-repo-name").fill("legacy-fallback");
  await page.getByTestId("create-project-repo-submit").click();

  // The repo publish (30617) succeeds; the owner-curated forward ref on the
  // container (30621) is best-effort and its rejection stays silent.
  await expect(page.getByTestId("create-project-repo-dialog")).toBeHidden();
  await waitForAnimations(page);
  await page.getByTestId("projects-section-repositories").click();
  const projectEntry = page
    .locator(
      '[data-testid="repository-card-legacy-fallback"], [data-testid="repository-row-legacy-fallback"]',
    )
    .first();
  await expect(projectEntry).toBeVisible();
  await projectEntry
    .getByRole("button", { name: "View legacy-fallback" })
    .click();
  const repositoryPicker = page.getByTestId("project-repository-picker");
  await expect(repositoryPicker).toBeVisible();
  await expect(repositoryPicker).toContainText("legacy-fallback");
  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/06-single-repository-add.png`,
  });

  const acceptedKinds = await page.evaluate(
    () =>
      window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__
        ?.filter((event) =>
          event.tags.some(
            (tag) => tag[0] === "d" && tag[1] === "legacy-fallback",
          ),
        )
        .map((event) => event.kind) ?? [],
  );
  expect(acceptedKinds).toEqual([30617]);
});

test("repository creation can retry after its publication fails", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  await page.addInitScript(() => {
    // The one-shot 30617 rejection can be armed at boot: nothing else in the
    // boot path publishes a repo announcement (the General sweep is a 30621).
    window.__BUZZ_E2E_REJECT_PROJECT_EVENT_KINDS__ = [30617];
    // A real target container keeps ensureRealProject from publishing a
    // General head mid-flow.
    window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
      {
        id: "workbenchseed".padEnd(64, "0"),
        kind: 30621,
        pubkey: "deadbeef".repeat(8),
        created_at: Math.floor(Date.now() / 1000) - 3600,
        content: "",
        tags: [
          ["d", "workbench"],
          ["name", "Workbench"],
        ],
      },
    ];
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo creation needs a concrete target project — scope to the seeded
  // real container (see the init script above for why not General).
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "Workbench" }).click();
  await page.getByTestId("projects-create-menu").hover();
  await page.getByRole("menuitem", { name: "Repository", exact: true }).click();
  await page.getByTestId("create-project-repo-name").fill("retry-project");
  await page.getByTestId("create-project-repo-submit").click();

  await expect(page.getByTestId("create-project-repo-dialog")).toBeVisible();
  await expect(page.getByText("mock project event rejection")).toBeVisible();

  await page.getByTestId("create-project-repo-submit").click();
  await expect(page.getByTestId("create-project-repo-dialog")).toBeHidden();
  await page.getByTestId("projects-section-repositories").click();
  await expect(
    page
      .locator(
        '[data-testid="repository-card-retry-project"], [data-testid="repository-row-retry-project"]',
      )
      .first(),
  ).toBeVisible();
});

test("repository creation recovers from a lost publish acknowledgement", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  await page.addInitScript(() => {
    // The one-shot 30617 lost-ack can be armed at boot: nothing else in the
    // boot path publishes a repo announcement (the General sweep is a 30621).
    window.__BUZZ_E2E_FAIL_PROJECT_EVENT_ACK_KINDS__ = [30617];
    // A real target container keeps ensureRealProject from publishing a
    // General head mid-flow.
    window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
      {
        id: "workbenchseed".padEnd(64, "0"),
        kind: 30621,
        pubkey: "deadbeef".repeat(8),
        created_at: Math.floor(Date.now() / 1000) - 3600,
        content: "",
        tags: [
          ["d", "workbench"],
          ["name", "Workbench"],
        ],
      },
    ];
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo creation needs a concrete target project — scope to the seeded
  // real container (see the init script above for why not General).
  await page.getByRole("button", { name: "Filter by project" }).click();
  await page.getByRole("menuitem", { name: "Workbench" }).click();
  await page.getByTestId("projects-create-menu").hover();
  await page.getByRole("menuitem", { name: "Repository", exact: true }).click();
  await page.getByTestId("create-project-repo-name").fill("lost-ack-project");
  await page.getByTestId("create-project-repo-submit").click();

  // The relay stored the event but the ACK was lost — the id-keyed recovery
  // query treats the write as a success on the first submit.
  await expect(page.getByTestId("create-project-repo-dialog")).toBeHidden();
  await page.getByTestId("projects-section-repositories").click();
  await expect(
    page
      .locator(
        '[data-testid="repository-card-lost-ack-project"], [data-testid="repository-row-lost-ack-project"]',
      )
      .first(),
  ).toBeVisible();
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.filter((event) =>
            event.tags.some(
              (tag) => tag[0] === "d" && tag[1] === "lost-ack-project",
            ),
          ).length ?? 0,
      ),
    )
    .toBe(1);
});

test("multi-repository projects switch the active repository", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo tiles live on the Repositories tab (the Projects tab is the
  // container-management panel); a tile opens the same repo detail screen.
  await page.getByTestId("projects-section-repositories").click();
  await page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first()
    .click();

  const picker = page.getByTestId("project-repository-picker");
  await expect(picker).toContainText("buzz");
  await picker.click();
  await expect(
    page.getByTestId("project-repository-relay-tools"),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({
    path: `${SHOTS}/04-multi-repository-picker.png`,
  });

  await page.getByTestId("project-repository-relay-tools").click();
  await expect(picker).toContainText("relay-tools");
  await expect(page).toHaveURL(
    new RegExp(`repositoryId=${TEST_IDENTITIES.alice.pubkey}%3Arelay-tools`),
  );

  await picker.click();
  await page.getByTestId("add-project-repository").click();
  await expect(page.getByTestId("create-project-repository")).toBeVisible();
  await expect(page.getByTestId("attach-project-repository")).toBeVisible();
  await page.getByTestId("create-project-repository").click();
  await page.getByTestId("add-project-repository-name").fill("mobile-app");
  await page.getByTestId("add-project-repository-submit").click();
  await expect(page.getByTestId("add-project-repository-dialog")).toBeHidden();
  await expect(picker).toContainText("mobile-app");
  const addedEvents = await page.evaluate(
    () =>
      window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.filter(
        (event) =>
          event.tags.some((tag) => tag[0] === "d" && tag[1] === "mobile-app") ||
          event.tags.some(
            (tag) =>
              tag[0] === "a" &&
              tag[1]?.endsWith(":mobile-app") &&
              event.kind === 30621,
          ),
      ) ?? [],
  );
  expect(addedEvents.map((event) => event.kind)).toEqual([30621, 30617]);
  expect(
    addedEvents.find((event) => event.kind === 30617)?.tags,
  ).toContainEqual(["buzz-channel", "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"]);

  await picker.click();
  await page.getByTestId("add-project-repository").click();
  await page.getByTestId("attach-project-repository").click();
  await expect(
    page.getByTestId("attach-project-repository-dialog"),
  ).toBeVisible();
  await page.getByTestId("attach-existing-repository-design-system").click();
  await expect(
    page.getByTestId("attach-project-repository-dialog"),
  ).toBeHidden();
  await expect(picker).toContainText("design-system");
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.some(
            (event) =>
              event.kind === 30621 &&
              event.tags.some(
                (tag) => tag[0] === "a" && tag[1]?.endsWith(":design-system"),
              ),
          ) ?? false,
      ),
    )
    .toBe(true);
});

test("commit detail opens from the commits feed with a diff", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  await installMockBridge(page);
  // The preview server is a static file server without SPA fallback, so
  // enter at "/" and navigate via the sidebar.
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();

  // Repo tiles live on the Repositories tab (the Projects tab is the
  // container-management panel).
  await page.getByTestId("projects-section-repositories").click();

  // Open the first mock repo (dtag "buzz" from the e2e bridge fixture).
  const projectEntry = page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first();
  await expect(projectEntry).toBeVisible({ timeout: 10_000 });
  await projectEntry.click();

  await page.getByRole("tab", { name: "Commits" }).click();
  const commitRows = page.getByTestId("project-activity-feed-item");
  await expect(commitRows.first()).toBeVisible({ timeout: 10_000 });

  // Commits share the rounded list structure used by issues and pull requests.
  await expect(
    page.getByRole("heading", { name: "Commits", exact: true }),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({
    fullPage: false,
    path: `${SHOTS}/02-commits-feed.png`,
  });

  // Open the newest commit via its subject button.
  await commitRows
    .first()
    .getByRole("button", { name: /Add Trello board workflow details/ })
    .click();

  // Detail header: author line, subject, and hash.
  await expect(page.getByText("Commit from")).toBeVisible();
  await expect(
    page.getByRole("heading", { name: "Add Trello board workflow details" }),
  ).toBeVisible();
  await expect(
    page.getByRole("button", { name: "Copy commit hash" }),
  ).toBeVisible();
  await expect(page.getByTestId("project-workspace-tab-menu")).toHaveCount(0);
  await expect(
    page.getByRole("link", { name: "project guide", exact: true }),
  ).toHaveAttribute("href", "https://example.com/project-guide");
  await expect(
    page.getByRole("button", { name: "Architecture" }),
  ).toBeVisible();
  await expect(page.locator("video")).toHaveAttribute(
    "src",
    "https://example.com/project-demo.mp4",
  );

  // Diff from the mocked get_project_repo_diff renders changed files.
  await expect(page.getByText("2 changed files")).toBeVisible({
    timeout: 10_000,
  });
  await expect(
    page.getByText("CommunityTabs({ selectedCommitHash })"),
  ).toBeVisible();

  await waitForAnimations(page);
  await page.screenshot({
    fullPage: false,
    path: `${SHOTS}/01-commit-detail.png`,
  });

  // Breadcrumb category segment steps back to the commits feed.
  await page
    .getByRole("navigation", { name: "Project breadcrumb" })
    .getByRole("button", { name: "Commits", exact: true })
    .click();
  await expect(commitRows.first()).toBeVisible();
  await expect(page.getByTestId("project-workspace-tab-menu")).toBeVisible();

  // The commits feed itself gets a grayed sub-tab crumb.
  await expect(
    page.getByRole("navigation", { name: "Project breadcrumb" }),
  ).toContainText("Commits");

  // The project-name segment goes to the project home (Overview tab).
  await commitRows
    .first()
    .getByRole("button", { name: /Add Trello board workflow details/ })
    .click();
  await expect(page.getByText("Commit from")).toBeVisible();
  await page
    .getByRole("navigation", { name: "Project breadcrumb" })
    .getByRole("button", { name: "buzz", exact: true })
    .click();
  await expect(page.getByRole("tab", { name: "Overview" })).toHaveAttribute(
    "aria-selected",
    "true",
  );

  // The Projects root segment leaves the project entirely.
  await page
    .getByRole("navigation", { name: "Project breadcrumb" })
    .getByRole("button", { name: "Projects", exact: true })
    .click();
  await expect(projectEntry).toBeVisible();
});

test("pull request and issue feeds share the commit row structure", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();

  // Repo tiles live on the Repositories tab (the Projects tab is the
  // container-management panel).
  await page.getByTestId("projects-section-repositories").click();

  const projectEntry = page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first();
  await expect(projectEntry).toBeVisible({ timeout: 10_000 });
  await projectEntry.click();

  // PR rows use the shared feed row: title button + #id cluster cell.
  await page.getByRole("tab", { name: "Pull Request" }).click();
  const prRows = page.getByTestId("project-pull-request-row");
  await expect(prRows.first()).toBeVisible({ timeout: 10_000 });
  await expect(
    prRows.first().getByRole("button", { name: /^#/ }),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({ fullPage: false, path: `${SHOTS}/03-prs-feed.png` });

  // The #id cell opens the PR detail, same as clicking the title.
  await prRows.first().getByRole("button", { name: /^#/ }).click();
  await expect(
    page.getByRole("navigation", { name: "Project breadcrumb" }),
  ).toContainText("Pull Request");

  // Step back to the feed so the community tabs are available again.
  await page
    .getByRole("navigation", { name: "Project breadcrumb" })
    .getByRole("button", { name: "Pull Request", exact: true })
    .click();
  await expect(prRows.first()).toBeVisible();

  // Issue rows share the same structure.
  await page.getByRole("tab", { name: "Issues" }).click();
  const issueRows = page.getByTestId("project-issue-row");
  await expect(issueRows.first()).toBeVisible({ timeout: 10_000 });
  await expect(
    issueRows.first().getByRole("button", { name: /^#/ }),
  ).toBeVisible();
  await waitForAnimations(page);
  await page.screenshot({
    fullPage: false,
    path: `${SHOTS}/04-issues-feed.png`,
  });
});

test("adding a repository retries and reports an error when the 30617 publication is rejected", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  // Reject the repository-announcement event on every attempt so the mutation
  // exhausts its retry and surfaces a partial-write error.
  await page.addInitScript(() => {
    // Reject kind 30617 twice (initial attempt + one retry).
    window.__BUZZ_E2E_REJECT_PROJECT_EVENT_KINDS__ = [30617, 30617];
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo tiles live on the Repositories tab (the Projects tab is the
  // container-management panel); a tile opens the same repo detail screen.
  await page.getByTestId("projects-section-repositories").click();
  await page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first()
    .click();

  const picker = page.getByTestId("project-repository-picker");
  await picker.click();
  await page.getByTestId("add-project-repository").click();
  await page.getByTestId("create-project-repository").click();
  await page.getByTestId("add-project-repository-name").fill("rejected-repo");
  await page.getByTestId("add-project-repository-submit").click();

  // The project event (30621) is published; the repository event (30617) is
  // rejected on both attempts. The dialog must surface the partial-write error.
  await expect(page.getByTestId("add-project-repository-dialog")).toBeVisible();
  await expect(
    page.getByText(/repository could not be created/i),
  ).toBeVisible();

  // The 30621 must have been published (project was updated).
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.some(
            (event) =>
              event.kind === 30621 &&
              event.tags.some(
                (tag) => tag[0] === "a" && tag[1]?.endsWith(":rejected-repo"),
              ),
          ) ?? false,
      ),
    )
    .toBe(true);

  // The 30617 must NOT have been accepted (both attempts were rejected).
  const acceptedRepo = await page.evaluate(
    () =>
      window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.some(
        (event) =>
          event.kind === 30617 &&
          event.tags.some(
            (tag) => tag[0] === "d" && tag[1] === "rejected-repo",
          ),
      ) ?? false,
  );
  expect(
    acceptedRepo,
    "30617 must not be accepted when the relay rejects both attempts",
  ).toBe(false);
});

test("adding a repository treats a lost 30617 acknowledgement as success", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  // The relay will accept the 30617 but fail to deliver the ACK, then on the
  // retry query the event will be found — the mutation must succeed.
  await page.addInitScript(() => {
    window.__BUZZ_E2E_FAIL_PROJECT_EVENT_ACK_KINDS__ = [30617];
  });
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo tiles live on the Repositories tab (the Projects tab is the
  // container-management panel); a tile opens the same repo detail screen.
  await page.getByTestId("projects-section-repositories").click();
  await page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first()
    .click();

  const picker = page.getByTestId("project-repository-picker");

  await picker.click();
  await page.getByTestId("add-project-repository").click();
  await page.getByTestId("create-project-repository").click();
  await page.getByTestId("add-project-repository-name").fill("lost-ack-repo");
  await page.getByTestId("add-project-repository-submit").click();

  // The dialog should close — the operation recovered from the lost ACK.
  await expect(page.getByTestId("add-project-repository-dialog")).toBeHidden();
  // The repository picker must reflect the newly added repository.
  await expect(picker).toContainText("lost-ack-repo");

  // Both events must have been accepted: the 30621 (project update) and the
  // 30617 (repository — accepted by relay even though ACK was lost).
  await expect
    .poll(() =>
      page.evaluate(
        () =>
          window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.filter((event) =>
            event.tags.some(
              (tag) => tag[0] === "d" && tag[1] === "lost-ack-repo",
            ),
          ).length ?? 0,
      ),
    )
    .toBeGreaterThanOrEqual(1);
});

test("adding a repository blocks when a standalone 30617 already exists at that coordinate", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  // Seed a standalone 30617 (not a project member) owned by the mock identity.
  // The add-repo mutation must block unconditionally when this coordinate exists,
  // even though it is not yet in the "buzz" project's member list.
  const MOCK_OWNER = "deadbeef".repeat(8);
  const STANDALONE_DTAG = "existing-standalone";
  await page.addInitScript(
    ({ owner, dtag }) => {
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
        {
          id: "standalone00".padEnd(64, "0"),
          kind: 30617,
          pubkey: owner,
          created_at: Math.floor(Date.now() / 1000) - 3600,
          content: "A standalone repository that exists outside any project.",
          tags: [
            ["d", dtag],
            ["name", "Existing Standalone"],
            ["clone", "https://git.example.com/standalone.git"],
          ],
        },
      ];
    },
    { owner: MOCK_OWNER, dtag: STANDALONE_DTAG },
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await page.getByTestId("open-projects-view").click();
  // Repo tiles live on the Repositories tab (the Projects tab is the
  // container-management panel); a tile opens the same repo detail screen.
  await page.getByTestId("projects-section-repositories").click();
  await page
    .locator(
      '[data-testid="repository-card-buzz"], [data-testid="repository-row-buzz"]',
    )
    .first()
    .click();

  const picker = page.getByTestId("project-repository-picker");
  await picker.click();
  await page.getByTestId("add-project-repository").click();
  await page.getByTestId("create-project-repository").click();
  // Use the same name — the dtag will match the seeded standalone 30617.
  await page
    .getByTestId("add-project-repository-name")
    .fill("Existing Standalone");
  await page.getByTestId("add-project-repository-submit").click();

  // The dialog must remain open with a clobber error.
  await expect(page.getByTestId("add-project-repository-dialog")).toBeVisible();
  await expect(
    page.getByText(/already exists.*standalone.*another project/i),
  ).toBeVisible();

  // Neither a 30621 (project update) nor a 30617 (new repo) must have been published.
  const publishedForStandalone = await page.evaluate(
    ({ dtag }) =>
      window.__BUZZ_E2E_ACCEPTED_PROJECT_EVENTS__?.some((event) => {
        // The General-container sweep legitimately claims unowned repos by
        // `a`-ref on the General 30621 — only a new 30617 at the coordinate
        // or a non-General project update means the clobber guard failed.
        const isGeneralContainer =
          event.kind === 30621 &&
          event.tags.some((tag) => tag[0] === "d" && tag[1] === "general");
        if (isGeneralContainer) return false;
        return (
          event.tags.some((tag) => tag[0] === "d" && tag[1] === dtag) ||
          event.tags.some(
            (tag) => tag[0] === "a" && tag[1]?.endsWith(`:${dtag}`),
          )
        );
      }) ?? false,
    { dtag: STANDALONE_DTAG },
  );
  expect(
    publishedForStandalone,
    "neither project nor repository event must be published when clobber guard fires",
  ).toBe(false);
});

test("navigating via a 30617 entity-link route opens the correct non-primary repository and renders its PR", async ({
  page,
}) => {
  await enableProjectsFeature(page);
  // Seed a known pull-request for relay-tools (the non-primary member of "buzz")
  // with a deterministic id so the URL can be constructed before navigation.
  const ALICE_PUBKEY =
    "953d3363262e86b770419834c53d2446409db6d918a57f8f339d495d54ab001f";
  const RELAY_TOOLS_ADDRESS = `30617:${ALICE_PUBKEY}:relay-tools`;
  const KNOWN_PR_ID = "entity-link-pr-test".padEnd(64, "0");

  await page.addInitScript(
    ({ repoAddress, prId, alicePubkey }) => {
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
        {
          id: prId,
          kind: 1618, // KIND_GIT_PULL_REQUEST
          pubkey: alicePubkey,
          created_at: Math.floor(Date.now() / 1000) - 60,
          content: "Entity-link test PR from relay-tools",
          tags: [
            ["a", repoAddress],
            ["subject", "Entity-link test PR from relay-tools"],
            ["c", "abc123".padEnd(40, "0")],
            ["h", "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"],
            ["branch-name", "feature/entity-link-test"],
            ["clone", "https://github.com/block/relay-tools.git"],
          ],
        },
      ];
    },
    {
      repoAddress: RELAY_TOOLS_ADDRESS,
      prId: KNOWN_PR_ID,
      alicePubkey: ALICE_PUBKEY,
    },
  );
  await installMockBridge(page);

  // Navigate via the entity-link route using the hash router URL format.
  // Python's http.server (the e2e web server) serves only index.html at `/`;
  // a direct page.goto to `/projects/...` returns 404 because there is no
  // SPA fallback. The app uses createHashHistory(), so the correct URL is
  // `/#/projects/<id>?...` — the server always sees just `/` and the hash
  // fragment is resolved entirely client-side by TanStack Router. Colons are
  // valid in hash-fragment path segments and must NOT be percent-encoded:
  // TanStack Router's param extractor receives the raw decoded segment, and
  // %3A would be passed through literally (as the string "30617%3A…") rather
  // than decoded to "30617:…", causing the project lookup to fail.
  // With project containers, the repo detail lives on the code child route:
  // /projects/<containerId>/code/<repo entity address>. Unclaimed mock repos
  // belong to the local General placeholder container.
  await page.goto(
    `/#/projects/local:general/code/${RELAY_TOOLS_ADDRESS}?pullRequestId=${KNOWN_PR_ID}`,
    { waitUntil: "domcontentloaded" },
  );

  // Repository controls intentionally render only on README and Files. The
  // seeded PR and branch prove that this detail route resolved relay-tools
  // rather than falling back to the project's primary repository.
  await expect(page.getByTestId("project-repository-picker")).toHaveCount(0);
  // Use `first()` to avoid Playwright strict-mode violations: the text appears
  // in both the breadcrumb and the PR title heading once the detail panel opens.
  await expect(
    page.getByText("Entity-link test PR from relay-tools").first(),
  ).toBeVisible({ timeout: 10_000 });
  await expect(
    page.getByText("feature/entity-link-test").first(),
  ).toBeVisible();
});
