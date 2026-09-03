import { createHash } from "node:crypto";

import { expect, test } from "@playwright/test";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * LANE-L23 addendum (2026-09-03) — "Project settings → Repository →
 * Protection": `buzz-protect` rules listed off a repository's own kind:30617
 * announcement, who may set them (the announcement's signer — never a
 * maintainer or the project owner), and a "Require verdict on main" switch
 * enabled only for that signer.
 *
 * Both repositories and the project linking them are seeded directly via
 * `__BUZZ_E2E_EXTRA_PROJECT_EVENTS__` (raw store injection, no signature
 * needed — the same mechanism `project-container-screen.spec.ts` uses for a
 * standalone repo) so the test can put one repo under the viewer's own key
 * and one under someone else's without switching identities mid-spec.
 */

const SHOTS =
  "/Users/brian/Projects/beekeeper/review-2026-09-01/batch3/l23c-shots";
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

const SELF = "deadbeef".repeat(8);
const STRANGER = "c0ffee00".repeat(8);
const OWN_REPO_ADDR = `30617:${SELF}:protect-own`;
const STRANGER_REPO_ADDR = `30617:${STRANGER}:protect-theirs`;

test("Repository → Protection: lists real buzz-protect rules, and gates the switch to the announcement's own signer", async ({
  page,
}) => {
  await page.addInitScript(
    ({ self, stranger, ownRepoAddr, strangerRepoAddr }) => {
      window.localStorage.setItem(
        "buzz-feature-overrides-v1",
        JSON.stringify({ projects: true, forum: true }),
      );
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
        {
          id: "seeded-protect-container",
          kind: 30621,
          pubkey: self,
          created_at: 1_800_000_000,
          content: "",
          tags: [
            ["d", "protectproj"],
            ["name", "Protect Co"],
            ["a", ownRepoAddr],
            ["a", strangerRepoAddr],
          ],
        },
        {
          id: "seeded-protect-own-repo",
          kind: 30617,
          pubkey: self,
          created_at: 1_800_000_000,
          content: "",
          tags: [
            ["d", "protect-own"],
            ["name", "protect-own"],
            // No require-verdict on main yet — the toggle below turns it on.
            ["buzz-protect", "refs/heads/dev", "no-delete"],
          ],
        },
        {
          id: "seeded-protect-stranger-repo",
          kind: 30617,
          pubkey: stranger,
          created_at: 1_800_000_000,
          content: "",
          tags: [
            ["d", "protect-theirs"],
            ["name", "protect-theirs"],
            ["buzz-protect", "refs/heads/main", "require-verdict"],
          ],
        },
      ];
    },
    {
      self: SELF,
      stranger: STRANGER,
      ownRepoAddr: OWN_REPO_ADDR,
      strangerRepoAddr: STRANGER_REPO_ADDR,
    },
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });
  const card = page.getByTestId("manage-project-protectproj");
  await expect(card).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("manage-project-actions-protectproj").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await page.getByTestId("project-settings-tab-repository").click();
  await expect(
    page.getByTestId("project-repository-protection-section"),
  ).toBeVisible({ timeout: 10_000 });

  const cards = page.getByTestId("project-repository-protection-card");
  await expect(cards).toHaveCount(2);

  const ownCard = cards.filter({ hasText: "protect-own" });
  const strangerCard = cards.filter({ hasText: "protect-theirs" });

  // The stranger's card: rules listed, switch present but disabled and
  // already on, the read-only sentence naming who can change it.
  await expect(
    strangerCard.getByTestId("project-repository-protection-rules"),
  ).toContainText("refs/heads/main");
  await expect(
    strangerCard.getByTestId("project-repository-protection-rules"),
  ).toContainText("require-verdict");
  await expect(
    strangerCard.getByTestId("project-repository-require-verdict-switch"),
  ).toBeDisabled();
  await expect(
    strangerCard.getByTestId("project-repository-protection-readonly-note"),
  ).toBeVisible();

  // The viewer's own card: no rule on main yet, the switch is enabled and
  // unchecked, no read-only note.
  await expect(
    ownCard.getByTestId("project-repository-protection-readonly-note"),
  ).toHaveCount(0);
  const ownSwitch = ownCard.getByTestId(
    "project-repository-require-verdict-switch",
  );
  await expect(ownSwitch).toBeEnabled();
  await expect(ownSwitch).toHaveAttribute("aria-checked", "false");
  await capture(page, "04-protection-before-toggle");

  // Toggle it on — a real signed republish of the own repo's announcement.
  await ownSwitch.click();
  await expect(ownSwitch).toHaveAttribute("aria-checked", "true", {
    timeout: 10_000,
  });
  await expect(
    ownCard.getByTestId("project-repository-protection-published"),
  ).toBeVisible();
  await capture(page, "05-protection-after-toggle");

  const published = await page.evaluate(() => {
    const events = window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [];
    return [...events]
      .reverse()
      .find(
        (event) =>
          event.kind === 30617 &&
          event.tags.some((tag) => tag[0] === "d" && tag[1] === "protect-own"),
      );
  });
  expect(published).toBeTruthy();
  expect(published?.tags).toContainEqual([
    "buzz-protect",
    "refs/heads/main",
    "require-verdict",
  ]);
  // The pre-existing dev-branch rule survives the republish untouched.
  expect(published?.tags).toContainEqual([
    "buzz-protect",
    "refs/heads/dev",
    "no-delete",
  ]);
});
