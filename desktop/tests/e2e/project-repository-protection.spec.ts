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

// These land in the repo's own `test-results/`, like every other spec's
// shots. The absolute path this replaced was a review directory on one
// laptop, so the whole file errored `ENOENT`/`EACCES` for anyone else and
// the evidence it claims to produce existed on exactly one machine.
const SHOTS = "test-results/project-repository-protection";
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

test("Repository → Protection: a co-founder sets the rule with a record of their own (kind 30625)", async ({
  page,
}) => {
  // Finding 33 R2, end to end in the app. The viewer is a NIP-34 maintainer of
  // a repository someone else announced: before lane L26 this card's switch was
  // disabled with a note saying only the signer could change it. Now the
  // toggle signs a kind:30625 rule record — an announcement the viewer cannot
  // address is never republished.
  await page.addInitScript(
    ({ self, stranger, repoAddr }) => {
      window.localStorage.setItem(
        "buzz-feature-overrides-v1",
        JSON.stringify({ projects: true, forum: true }),
      );
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
        {
          id: "seeded-cofounder-container",
          kind: 30621,
          pubkey: self,
          created_at: 1_800_000_000,
          content: "",
          tags: [
            ["d", "cofounderproj"],
            ["name", "Co-founder Co"],
            ["a", repoAddr],
          ],
        },
        {
          id: "seeded-cofounder-repo",
          kind: 30617,
          pubkey: stranger,
          created_at: 1_800_000_000,
          content: "",
          tags: [
            ["d", "cofounded"],
            ["name", "cofounded"],
            // The viewer founds this repository without having announced it.
            ["maintainers", self],
          ],
        },
      ];
    },
    { self: SELF, stranger: STRANGER, repoAddr: `30617:${STRANGER}:cofounded` },
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });

  await page.getByTestId("open-projects-view").click();
  await expect(page.getByTestId("projects-manage-panel")).toBeVisible({
    timeout: 10_000,
  });
  await expect(page.getByTestId("manage-project-cofounderproj")).toBeVisible({
    timeout: 10_000,
  });
  await page.getByTestId("manage-project-actions-cofounderproj").click();
  await page.getByRole("menuitem", { name: "Project settings" }).click();
  await page.getByTestId("project-settings-tab-repository").click();
  const card = page
    .getByTestId("project-repository-protection-card")
    .filter({ hasText: "cofounded" });
  await expect(card).toBeVisible({ timeout: 10_000 });

  // Live, not read-only — the whole point.
  await expect(
    card.getByTestId("project-repository-protection-readonly-note"),
  ).toHaveCount(0);
  const toggle = card.getByTestId("project-repository-require-verdict-switch");
  await expect(toggle).toBeEnabled();
  await expect(toggle).toHaveAttribute("aria-checked", "false");
  await capture(page, "08-cofounder-before-toggle");

  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "true", {
    timeout: 10_000,
  });
  await expect(
    card.getByTestId("project-repository-protection-published"),
  ).toContainText("your own rule record");
  // The rule is shown as coming from a rule record, signed by the viewer.
  await expect(
    card.getByTestId("project-repository-protection-source-refs/heads/main"),
  ).toContainText("rule record");
  await capture(page, "09-cofounder-after-toggle");

  const signed = await page.evaluate(() => {
    const events = window.__BUZZ_E2E_SIGNED_EVENTS__ ?? [];
    return {
      records: events.filter((event) => event.kind === 30625),
      announcements: events.filter((event) => event.kind === 30617),
    };
  });
  expect(signed.records).toHaveLength(1);
  expect(signed.records[0]?.tags).toContainEqual([
    "d",
    `${STRANGER}:cofounded`,
  ]);
  expect(signed.records[0]?.tags).toContainEqual([
    "buzz-protect",
    "refs/heads/main",
    "require-verdict",
  ]);
  expect(JSON.parse(signed.records[0]?.content ?? "{}")).toEqual({
    schema: "buzz-repo-protection/v1",
  });
  // The announcement the viewer cannot address is never republished — the bug
  // this kind exists to remove.
  expect(signed.announcements).toHaveLength(0);

  // And turning it back off writes the clear token, not an absent row: an
  // absent row would fall back to whatever the announcement says.
  await toggle.click();
  await expect(toggle).toHaveAttribute("aria-checked", "false", {
    timeout: 10_000,
  });
  const cleared = await page.evaluate(
    () =>
      (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).filter(
        (event) => event.kind === 30625,
      ).length,
  );
  expect(cleared).toBe(2);
  const clearRow = await page.evaluate(() => {
    const events = (window.__BUZZ_E2E_SIGNED_EVENTS__ ?? []).filter(
      (event) => event.kind === 30625,
    );
    return events[events.length - 1]?.tags;
  });
  expect(clearRow).toContainEqual(["buzz-protect", "refs/heads/main", "none"]);
});
