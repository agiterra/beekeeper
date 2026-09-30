import { createHash } from "node:crypto";

import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * LANE-L23 — the "Packs" project-settings row: which git repository (and
 * pinned commit) a project's coding-session seats stage their persona packs
 * from, kind:30624.
 *
 * The founder-only "Set source" form publishes straight from the desktop —
 * `signRelayEvent` + `relayClient.publishEvent`, the same client-side
 * publish path `publishProjectContainer` uses for kind:30621 — so this spec
 * drives the real dialog, the real form, and reads the real signed event
 * back off `window.__BUZZ_E2E_SIGNED_EVENTS__`, exactly like
 * `project-settings-screenshots.spec.ts` does for icon/color.
 */

// These land in the repo's own `test-results/`, like every other spec's
// shots. The absolute path this replaced was a review directory on one
// laptop, so the whole file errored `ENOENT`/`EACCES` for anyone else and
// the evidence it claims to produce existed on exactly one machine.
const SHOTS = "test-results/project-packs";

const hashes = new Map<string, string>();

async function capture(page: import("@playwright/test").Page, name: string) {
  await waitForAnimations(page);
  const buffer = await page.screenshot({
    path: `${SHOTS}/${name}.png`,
    clip: { x: 0, y: 0, width: 900, height: 700 },
  });
  const digest = createHash("sha256").update(buffer).digest("hex");
  for (const [other, otherDigest] of hashes) {
    expect(digest, `${name} captured the same pixels as ${other}`).not.toBe(
      otherDigest,
    );
  }
  hashes.set(name, digest);
}

test("the project owner sets a pack source, and the row reads it back off the real signed event", async ({
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

  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("projects-create-menu").click();
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Waggle");
  await page.getByTestId("create-project-container-submit").click();
  const card = page.getByTestId("manage-project-waggle");
  await expect(card).toBeVisible({ timeout: 10_000 });

  await page.getByTestId("manage-project-actions-waggle").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await expect(page.getByTestId("edit-project-container-name")).toHaveValue(
    "Waggle",
  );
  await waitForAnimations(page);

  await page.getByTestId("project-settings-tab-packs").click();
  await expect(page.getByTestId("project-packs-section")).toBeVisible();

  // 1 — before anything is set: the honest shipped-defaults disclosure, and
  // (this identity created the project, so it is the owner) both founder
  // actions.
  await expect(page.getByTestId("project-packs-source-shipped")).toContainText(
    "no project source is set, so seats stage the packs built into this app.",
  );
  await expect(page.getByTestId("project-packs-create-repo-open")).toHaveText(
    "Create packs repository",
  );
  await expect(page.getByTestId("project-packs-use-existing-open")).toHaveText(
    "Use an existing repository",
  );
  await capture(page, "01-packs-empty");

  // 2 — "Use an existing repository": fill and submit the form.
  await page.getByTestId("project-packs-use-existing-open").click();
  const repoCoord = `30617:${"a".repeat(64)}:agiterra-packs`;
  await page.getByTestId("project-packs-repo-input").fill(repoCoord);
  await page.getByTestId("project-packs-pin-input").fill("refs/heads/main");
  await capture(page, "02-packs-form-filled");
  await page.getByTestId("project-packs-set-source-submit").click();

  // 3 — the row reads back the real signed 30624: repo, ref, default path.
  await expect(page.getByTestId("project-packs-source-row")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByTestId("project-packs-source-row")).toContainText(
    repoCoord,
  );
  await expect(page.getByTestId("project-packs-source-row")).toContainText(
    "refs/heads/main",
  );
  await expect(page.getByTestId("project-packs-source-row")).toContainText(
    "personas/roles",
  );
  await capture(page, "03-packs-source-set");

  const published = await page.evaluate(() => {
    const events = window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [];
    const event = [...events].reverse().find((entry) => entry.kind === 30624);
    return event
      ? {
          tags: event.tags,
          content: JSON.parse(event.content),
        }
      : null;
  });
  expect(published).not.toBeNull();
  expect(published?.content.schema).toBe("buzz-project-pack-source/v1");
  expect(published?.tags).toContainEqual(["repo", repoCoord]);
  expect(published?.tags).toContainEqual(["ref", "refs/heads/main"]);
  // Exactly one of ref/sha on the real signed event.
  expect(published?.tags.some((tag) => tag[0] === "sha")).toBe(false);
});

const SELF = "deadbeef".repeat(8);

test("Create packs repository: the founder-only host action prints every wire fact it produced", async ({
  page,
}) => {
  const projectRef = `30621:${SELF}:beeline`;
  const initResult = {
    repoRef: `30617:${SELF}:beeline-packs`,
    sourceEventId: "f".repeat(64),
    seedCommitSha: "1".repeat(40),
    seedError: null,
    commitIdentityName: "Beeline",
    commitIdentityEmail: `${SELF.slice(0, 8)}@beekeeper.local`,
    pushRecordEventId: "2".repeat(64),
    announcementWithdrawnEventId: null,
    announcementWithdrawalError: null,
  };
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true, forum: true }),
    );
  });
  await installMockBridge(page, {
    projectPacksInitByProject: { [projectRef]: initResult },
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("projects-create-menu").click();
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Beeline");
  await page.getByTestId("create-project-container-submit").click();
  await expect(page.getByTestId("manage-project-beeline")).toBeVisible({
    timeout: 10_000,
  });

  await page.getByTestId("manage-project-actions-beeline").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await page.getByTestId("project-settings-tab-packs").click();
  await expect(page.getByTestId("project-packs-section")).toBeVisible();

  await page.getByTestId("project-packs-create-repo-open").click();
  await expect(
    page.getByTestId("project-packs-create-repo-panel"),
  ).toBeVisible();
  await page.getByTestId("project-packs-create-repo-submit").click();

  // Every wire fact the host command produced, printed — not summarized.
  const resultPanel = page.getByTestId("project-packs-create-repo-result");
  await expect(resultPanel).toBeVisible({ timeout: 10_000 });
  await expect(resultPanel).toContainText(initResult.repoRef);
  await expect(resultPanel).toContainText(initResult.seedCommitSha.slice(0, 8));
  await capture(page, "06-packs-create-repo-result");
});

test("Create packs repository: a chosen repository id reaches the host, and the printed coordinate is the one it made (LANE-L30)", async ({
  page,
}) => {
  // "one packs repository for all of agiterra; every project points at it" —
  // a founder must be able to name the *shared* repository rather than get a
  // fresh `<project-slug>-packs` every time.
  const projectRef = `30621:${SELF}:hive-mind`;
  const customId = "agiterra-shared-packs";
  const initResult = {
    repoRef: `30617:${SELF}:${customId}`,
    sourceEventId: "3".repeat(64),
    seedCommitSha: "4".repeat(40),
    seedError: null,
    commitIdentityName: "Hive Mind",
    commitIdentityEmail: `${SELF.slice(0, 8)}@beekeeper.local`,
    pushRecordEventId: "5".repeat(64),
    announcementWithdrawnEventId: null,
    announcementWithdrawalError: null,
  };
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true, forum: true }),
    );
  });
  await installMockBridge(page, {
    projectPacksInitByProject: { [projectRef]: initResult },
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("projects-create-menu").click();
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Hive Mind");
  await page.getByTestId("create-project-container-submit").click();
  await expect(page.getByTestId("manage-project-hive-mind")).toBeVisible({
    timeout: 10_000,
  });

  await page.getByTestId("manage-project-actions-hive-mind").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await page.getByTestId("project-settings-tab-packs").click();
  await expect(page.getByTestId("project-packs-section")).toBeVisible();

  await page.getByTestId("project-packs-create-repo-open").click();
  const repoIdField = page.getByTestId("project-packs-create-repo-id");
  await expect(repoIdField).toHaveValue("hive-mind-packs");
  // The name field silently followed the default id until now.
  await expect(page.getByTestId("project-packs-create-repo-name")).toHaveValue(
    "hive-mind-packs",
  );

  await repoIdField.fill(customId);
  // Untouched, the name keeps following the id the viewer is now typing.
  await expect(page.getByTestId("project-packs-create-repo-name")).toHaveValue(
    customId,
  );
  await capture(page, "07-packs-create-repo-custom-id");
  await page.getByTestId("project-packs-create-repo-submit").click();

  const resultPanel = page.getByTestId("project-packs-create-repo-result");
  await expect(resultPanel).toBeVisible({ timeout: 10_000 });
  await expect(
    page.getByTestId("project-packs-create-repo-coordinate"),
  ).toContainText(initResult.repoRef);
  await capture(page, "08-packs-create-repo-custom-id-result");

  // The chosen id — not a project-derived one — is exactly what reached the
  // host, and the name defaulted to it exactly as the field showed.
  const calls = await page.evaluate(
    () => window.__BUZZ_E2E_PROJECT_PACKS_INIT_CALLS__ ?? [],
  );
  const call = calls.at(-1);
  expect(call?.projectRef).toBe(projectRef);
  expect(call?.repoId).toBe(customId);
  expect(call?.name).toBe(customId);
});

test("Create packs repository: a seed failure reports the sentence, not git's raw stderr, with the withdrawal disclosed (LANE-L31, Finding 66)", async ({
  page,
}) => {
  const projectRef = `30621:${SELF}:waggle-farm`;
  const rawGitError =
    "Author identity unknown\n\n*** Please tell me who you are. …\nfatal: unable to auto-detect email address (got 'brian@MacBookPro.(none)')";
  const initResult = {
    repoRef: `30617:${SELF}:waggle-farm-packs`,
    sourceEventId: null,
    seedCommitSha: null,
    seedError: rawGitError,
    commitIdentityName: "Waggle Farm",
    commitIdentityEmail: `${SELF.slice(0, 8)}@beekeeper.local`,
    pushRecordEventId: null,
    announcementWithdrawnEventId: "6".repeat(64),
    announcementWithdrawalError: null,
  };
  await page.addInitScript(() => {
    window.localStorage.setItem(
      "buzz-feature-overrides-v1",
      JSON.stringify({ projects: true, forum: true }),
    );
  });
  await installMockBridge(page, {
    projectPacksInitByProject: { [projectRef]: initResult },
  });
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("projects-create-menu").click();
  await page.getByTestId("projects-create-menu-project").click();
  await page.getByTestId("create-project-container-name").fill("Waggle Farm");
  await page.getByTestId("create-project-container-submit").click();
  await expect(page.getByTestId("manage-project-waggle-farm")).toBeVisible({
    timeout: 10_000,
  });

  await page.getByTestId("manage-project-actions-waggle-farm").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await page.getByTestId("project-settings-tab-packs").click();
  await expect(page.getByTestId("project-packs-section")).toBeVisible();

  await page.getByTestId("project-packs-create-repo-open").click();
  await page.getByTestId("project-packs-create-repo-submit").click();

  // The product sentence: what failed, the identity it would have used, and
  // that the announcement was withdrawn — never the bridge's returned
  // `seedError` printed directly.
  const outcome = page.getByTestId("project-packs-create-repo-seed-outcome");
  await expect(outcome).toBeVisible({ timeout: 10_000 });
  await expect(outcome).toContainText("seeding failed as Waggle Farm");
  await expect(outcome).toContainText("withdrawn");
  await expect(outcome).not.toContainText("Author identity unknown");
  await capture(page, "09-packs-create-repo-seed-failed");

  // The raw text is not gone — it is one click away, behind a disclosure.
  // `<details>` keeps its content in the DOM even collapsed, so the check
  // is visibility of the raw-text node, not `textContent` (which would see
  // through the collapse either way).
  const details = page.getByTestId(
    "project-packs-create-repo-seed-error-details",
  );
  const rawText = details.locator("pre");
  await expect(details).toBeVisible();
  await expect(rawText).toBeHidden();
  await details.locator("summary").click();
  await expect(rawText).toBeVisible();
  await expect(rawText).toContainText("Author identity unknown");
  await capture(page, "10-packs-create-repo-seed-failed-details-open");
});
