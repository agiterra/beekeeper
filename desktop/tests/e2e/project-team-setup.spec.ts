import { createHash } from "node:crypto";
import { waitForAnimations } from "../helpers/animations";
import { expect, test, type Page } from "@playwright/test";
import { installMockBridge } from "../helpers/bridge";
import type { ProjectTeamSetupDraft } from "../../src/features/roles/lib/projectTeamSetup";

const OWNER = "a1".repeat(32);
const PROJECT = `30621:${OWNER}:general`;
type FixtureWindow = Window & {
  __setupCalls: { command: string; args: Record<string, unknown> }[];
  __setupFailGet: boolean;
  __setupValid: boolean;
  __setupSnapshotFail: boolean;
  __TAURI_INTERNALS__: {
    invoke: (
      command: string,
      args?: Record<string, unknown>,
    ) => Promise<unknown>;
  };
};

async function openRoles(page: Page, failGet = false) {
  await page.addInitScript(
    ({ owner }) => {
      window.__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = [
        {
          id: "setup-general".padEnd(64, "0"),
          pubkey: owner,
          created_at: Math.floor(Date.now() / 1000) - 60,
          kind: 30621,
          tags: [
            ["d", "general"],
            ["name", "General"],
            ["channel", "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50"],
          ],
          content: "",
          sig: "0".repeat(128),
        },
      ];
    },
    { owner: OWNER },
  );
  await installMockBridge(page);
  await page.goto("/");
  await expect(page.getByTestId("project-group-general")).toBeVisible();
  await page.getByTestId("project-group-general").hover();
  await page.getByTestId("project-open-general").click();
  await page.getByTestId("project-tab-packs").click();
  await expect(page.getByTestId("project-team-setup-open")).toBeVisible();
  await page.evaluate(
    ({ projectRef, owner, failGet }) => {
      const w = window as unknown as FixtureWindow;
      const original = w.__TAURI_INTERNALS__.invoke.bind(w.__TAURI_INTERNALS__);
      let draft: ProjectTeamSetupDraft | null = null;
      w.__setupCalls = [];
      w.__setupFailGet = failGet;
      w.__setupValid = true;
      w.__setupSnapshotFail = false;
      w.__TAURI_INTERNALS__.invoke = async (command, args = {}) => {
        if (
          !command.startsWith("project_team_setup_") &&
          command !== "pick_coding_session_workdir"
        )
          return original(command, args);
        w.__setupCalls.push({ command, args });
        if (command === "pick_coding_session_workdir")
          return "/projects/tankloop";
        if (command === "project_team_setup_get") {
          if (w.__setupFailGet)
            throw {
              code: "filesystem",
              message: "Saved draft could not be read. Check local access.",
            };
          return draft;
        }
        if (command === "project_team_setup_prepare") {
          draft = {
            setupId: "setup-tankloop",
            projectRef,
            ownerPubkey: owner,
            relayUrl: String(args.expectedRelayUrl),
            status: "draft",
            projectDirectory: String(args.projectDirectory),
            draftDirectory: "/drafts/setup-tankloop",
            rolesDirectory: "/drafts/setup-tankloop/personas/roles",
            intent: String(args.intent),
            createdAt: "2026-09-11T12:00:00Z",
            roles: ["lead", "builder", "reviewer"],
          };
          return draft;
        }
        if (command === "project_team_setup_snapshot") {
          if (w.__setupSnapshotFail)
            throw {
              code: "invalid_draft",
              message: "Draft changed; check it before saving.",
            };
          if (args.snapshotId && args.snapshotId !== "a".repeat(64))
            throw new Error("Unknown saved version");
          if (!args.snapshotId && draft)
            draft = { ...draft, latestSnapshotId: "a".repeat(64) };
          return {
            setupId: "setup-tankloop",
            snapshotId: "a".repeat(64),
            rolesDirectory: "/snapshots/checked-version/personas/roles",
            manifestPath: "/snapshots/checked-version/manifest.json",
            roles: ["lead", "aquarium-specialist"],
          };
        }
        if (command !== "project_team_setup_validate")
          throw new Error(`Unexpected setup command: ${command}`);
        return {
          setupId: "setup-tankloop",
          status: "draft",
          valid: w.__setupValid,
          roles: ["lead", "aquarium-specialist"],
          diagnostics: w.__setupValid
            ? []
            : [
                {
                  level: "error",
                  message: "builder: skill reference does not exist",
                },
              ],
        };
      };
    },
    { projectRef: PROJECT, owner: OWNER, failGet },
  );
  await page.getByTestId("project-team-setup-open").click();
}

async function setupCalls(page: Page) {
  return page.evaluate(() => (window as unknown as FixtureWindow).__setupCalls);
}

test("open is read-only; explicit preparation and validation stay separate from publication; reopening resumes", async ({
  page,
}) => {
  await openRoles(page);
  const dialog = page.getByTestId("project-team-setup-dialog");
  await expect(
    dialog.getByLabel("What should this project accomplish?"),
  ).toBeVisible();
  expect((await setupCalls(page)).map((call) => call.command)).toEqual([
    "project_team_setup_get",
  ]);
  await dialog
    .getByLabel("What should this project accomplish?")
    .fill("Keep Tankloop aquarium maintenance reliable.");
  await dialog.getByRole("button", { name: "Browse", exact: true }).click();
  await expect(dialog.getByLabel("Local project repository")).toHaveValue(
    "/projects/tankloop",
  );
  await dialog
    .getByRole("button", { name: "Prepare draft", exact: true })
    .click();
  await expect(
    dialog.getByRole("heading", { name: "Draft ready" }),
  ).toBeVisible();
  await expect(
    dialog.getByTestId("project-team-setup-validation"),
  ).toContainText("has not been checked yet");
  await expect(
    dialog.getByTestId("project-team-setup-publication"),
  ).toContainText("has not been published or applied");
  expect(
    (await setupCalls(page)).filter(
      (call) => call.command === "project_team_setup_prepare",
    ),
  ).toHaveLength(1);
  await dialog
    .getByRole("button", { name: "Check draft", exact: true })
    .click();
  await expect(
    dialog.getByTestId("project-team-setup-validation"),
  ).toContainText("Pack structure passed");
  await expect(dialog).toContainText("Roles checked");
  await expect(dialog).toContainText("aquarium-specialist");
  await page.evaluate(() => {
    (window as unknown as FixtureWindow).__setupValid = false;
  });
  await dialog
    .getByRole("button", { name: "Check draft", exact: true })
    .click();
  await expect(
    dialog.getByTestId("project-team-setup-validation"),
  ).toContainText("skill reference does not exist");
  await expect(
    dialog.getByRole("button", { name: "Publish", exact: true }),
  ).toHaveCount(0);
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await page.getByTestId("project-team-setup-open").click();
  await expect(
    dialog.getByRole("heading", { name: "Draft ready" }),
  ).toBeVisible();
  await expect(
    dialog.getByTestId("project-team-setup-validation"),
  ).toContainText("has not been checked yet");
  await expect(dialog).toContainText(
    "Keep Tankloop aquarium maintenance reliable.",
  );
  expect(
    (await setupCalls(page)).filter(
      (call) => call.command === "project_team_setup_prepare",
    ),
  ).toHaveLength(1);
});

test("saving and reopening reverify the same separate copy; failed retry clears success", async ({
  page,
}) => {
  await openRoles(page);
  const dialog = page.getByTestId("project-team-setup-dialog");
  await dialog
    .getByLabel("What should this project accomplish?")
    .fill("Maintain Tankloop reliably.");
  await dialog
    .getByLabel("Local project repository")
    .fill("/projects/tankloop");
  await dialog
    .getByRole("button", { name: "Prepare draft", exact: true })
    .click();
  const save = dialog.getByRole("button", {
    name: "Save checked version",
    exact: true,
  });
  await expect(save).toHaveCount(0);
  await dialog
    .getByRole("button", { name: "Check draft", exact: true })
    .click();
  await save.click();
  await expect(dialog.getByRole("status")).toContainText(
    "Later draft edits do not change this copy. It has not been published.",
  );
  await dialog.getByText("Saved version details", { exact: true }).click();
  await expect(dialog.getByRole("status")).toContainText("a".repeat(64));
  await expect(dialog.getByRole("status")).toContainText(
    "Roles: lead, aquarium-specialist",
  );
  await expect(dialog).toContainText("/drafts/setup-tankloop/personas/roles");
  const calls = await setupCalls(page);
  expect(
    calls.find((call) => call.command === "project_team_setup_snapshot")?.args,
  ).toEqual({
    projectRef: PROJECT,
    expectedRelayUrl: calls.find(
      (call) => call.command === "project_team_setup_prepare",
    )?.args.expectedRelayUrl,
    setupId: "setup-tankloop",
  });
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await page.getByTestId("project-team-setup-open").click();
  await expect(dialog.getByRole("status")).toContainText(
    "Checked version saved",
  );
  await expect(
    dialog.getByTestId("project-team-setup-validation"),
  ).toContainText("has not been checked yet");
  await expect(save).toHaveCount(0);
  expect((await setupCalls(page)).at(-1)).toEqual({
    command: "project_team_setup_snapshot",
    args: {
      projectRef: PROJECT,
      expectedRelayUrl: calls.find(
        (call) => call.command === "project_team_setup_prepare",
      )?.args.expectedRelayUrl,
      setupId: "setup-tankloop",
      snapshotId: "a".repeat(64),
    },
  });
  await dialog.getByText("Saved version details", { exact: true }).click();
  await expect(dialog.getByRole("status")).toContainText("a".repeat(64));
  await dialog
    .getByRole("button", { name: "Check draft", exact: true })
    .click();
  await page.evaluate(() => {
    (window as unknown as FixtureWindow).__setupSnapshotFail = true;
  });
  await save.click();
  await expect(dialog.getByRole("alert")).toContainText(
    "Draft changed; check it before saving.",
  );
  await expect(dialog.getByRole("status")).toHaveCount(0);
  await expect(
    dialog.getByTestId("project-team-setup-publication"),
  ).toContainText("has not been published or applied");
  expect((await setupCalls(page)).map((call) => call.command)).toEqual([
    "project_team_setup_get",
    "project_team_setup_prepare",
    "project_team_setup_validate",
    "project_team_setup_snapshot",
    "project_team_setup_get",
    "project_team_setup_snapshot",
    "project_team_setup_validate",
    "project_team_setup_snapshot",
  ]);
});

test("a failed saved-draft read stays actionable and retry never creates a draft", async ({
  page,
}) => {
  await openRoles(page, true);
  const dialog = page.getByTestId("project-team-setup-dialog");
  await expect(dialog.getByRole("alert")).toContainText(
    "Saved draft could not be read",
  );
  await expect(
    dialog.getByRole("button", { name: "Prepare draft", exact: true }),
  ).toBeDisabled();
  await expect(
    dialog.getByRole("heading", { name: "Draft ready" }),
  ).toHaveCount(0);
  await page.evaluate(() => {
    (window as unknown as FixtureWindow).__setupFailGet = false;
  });
  await dialog.getByRole("button", { name: "Check saved draft again" }).click();
  await expect(dialog.getByRole("alert")).toHaveCount(0);
  await expect(
    dialog.getByLabel("What should this project accomplish?"),
  ).toBeVisible();
  expect((await setupCalls(page)).map((call) => call.command)).toEqual([
    "project_team_setup_get",
    "project_team_setup_get",
  ]);
});

for (const layout of [
  { name: "narrow", width: 640, fontSize: "16px" },
  { name: "text zoom 250%", width: 1280, fontSize: "40px" },
]) {
  test(`setup form and draft remain usable at ${layout.name}`, async ({
    page,
  }, testInfo) => {
    await openRoles(page);
    await page.setViewportSize({ width: layout.width, height: 900 });
    await page.evaluate((fontSize) => {
      document.documentElement.style.fontSize = fontSize;
    }, layout.fontSize);
    const dialog = page.getByTestId("project-team-setup-dialog");
    const checkFit = async () => {
      const size = await dialog.evaluate((element) => {
        const box = element.getBoundingClientRect();
        return {
          overflow: element.scrollWidth - element.clientWidth,
          left: box.left,
          right: box.right,
          viewport: window.innerWidth,
        };
      });
      expect(size.overflow).toBeLessThanOrEqual(1);
      expect(size.left).toBeGreaterThanOrEqual(-1);
      expect(size.right).toBeLessThanOrEqual(size.viewport + 1);
    };
    await dialog
      .getByLabel("What should this project accomplish?")
      .fill(
        "Keep Tankloop aquarium maintenance reliable for the entire project team.",
      );
    await dialog
      .getByLabel("Local project repository")
      .fill("/projects/tankloop/repository-with-a-long-readable-folder-name");
    await checkFit();
    await waitForAnimations(page);
    const form = await dialog.screenshot({
      path: testInfo.outputPath("setup-form.png"),
    });
    await dialog
      .getByRole("button", { name: "Prepare draft", exact: true })
      .click();
    await expect(
      dialog.getByRole("heading", { name: "Draft ready" }),
    ).toBeVisible();
    await checkFit();
    await dialog
      .getByRole("button", { name: "Check draft", exact: true })
      .click();
    await expect(dialog).toContainText("Pack structure passed");
    await dialog
      .getByRole("button", { name: "Save checked version", exact: true })
      .click();
    await expect(dialog.getByRole("status")).toContainText(
      "Checked version saved",
    );
    await dialog.getByText("Saved version details", { exact: true }).click();
    await expect(dialog.getByRole("status")).toContainText("a".repeat(64));
    await checkFit();
    await waitForAnimations(page);
    await dialog
      .getByRole("status")
      .screenshot({ path: testInfo.outputPath("saved-version.png") });
    await dialog.evaluate((element) => {
      element.scrollTop = 0;
    });
    await waitForAnimations(page);
    const prepared = await dialog.screenshot({
      path: testInfo.outputPath("setup-draft.png"),
    });
    expect(createHash("sha256").update(prepared).digest("hex")).not.toBe(
      createHash("sha256").update(form).digest("hex"),
    );
  });
}
