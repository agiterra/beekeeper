import { createHash } from "node:crypto";
import { waitForAnimations } from "../helpers/animations";
import { expect, test, type Page } from "@playwright/test";
import { installMockBridge } from "../helpers/bridge";
import type { ProjectTeamSetupDraft } from "../../src/features/roles/lib/projectTeamSetup";
import type { ProjectTeamSetupLaunch } from "../../src/features/roles/lib/projectTeamSetup";

const OWNER = "a1".repeat(32);
const PROJECT = `30621:${OWNER}:general`;
type FixtureWindow = Window & {
  __setupCalls: { command: string; args: Record<string, unknown> }[];
  __setupFailGet: boolean;
  __setupValid: boolean;
  __setupSnapshotFail: boolean;
  __setupRuntimeFail: boolean;
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
      let reservation: Record<string, unknown> | null = null;
      let launch: ProjectTeamSetupLaunch | null = null;
      let provisioned = false;
      let startCount = 0;
      w.__setupCalls = [];
      w.__setupFailGet = failGet;
      w.__setupValid = true;
      w.__setupSnapshotFail = false;
      w.__setupRuntimeFail = false;
      w.__TAURI_INTERNALS__.invoke = async (command, args = {}) => {
        if (
          !command.startsWith("project_team_setup_") &&
          command !== "pick_coding_session_workdir" &&
          ![
            "coding_session_provider_status",
            "coding_session_provider_runtimes",
            "coding_session_provider_models",
            "provision_coding_session_provider",
          ].includes(command)
        )
          return original(command, args);
        w.__setupCalls.push({ command, args });
        if (
          command === "coding_session_provider_status" ||
          command === "provision_coding_session_provider"
        ) {
          if (command === "provision_coding_session_provider")
            provisioned = true;
          return {
            provisioned,
            running: provisioned,
            ...(provisioned
              ? { providerPubkey: "b".repeat(64), instanceId: "local-provider" }
              : {}),
          };
        }
        if (command === "coding_session_provider_runtimes") {
          if (w.__setupRuntimeFail)
            throw new Error("Runtime discovery unavailable");
          return [
            {
              instanceRef: "claude-primary",
              runtime: "claude",
              driver: "claude",
              label: "Claude",
              authState: "ready",
              defaultModel: "default",
              allowedModels: ["default"],
              capabilities: {
                threadTurnStart: true,
                threadTurnInterrupt: true,
                threadSteer: false,
                context: true,
                diff: true,
                plan: true,
              },
            },
          ];
        }
        if (command === "coding_session_provider_models")
          return {
            instanceRef: "claude-primary",
            defaultModel: "default",
            allowedModels: ["default"],
          };
        if (command === "project_team_setup_get_authoring") return reservation;
        if (command === "project_team_setup_get_launch") return launch;
        if (command === "project_team_setup_reserve_authoring") {
          reservation ??= {
            authoringId: "authoring-1",
            sessionRef: "setup-session-1",
            channelId: args.channelId,
            createCommandId: "csl-fixed-request",
            status: "reserved",
            genesisEventId: "c".repeat(64),
          };
          return reservation;
        }
        if (command === "project_team_setup_start_authoring") {
          startCount += 1;
          if (!reservation) throw new Error("Reserve first");
          launch = {
            setupId: String(args.setupId),
            sessionRef: String(reservation.sessionRef),
            channelId: String(reservation.channelId),
            createCommandId: String(reservation.createCommandId),
            providerPubkey: String(args.providerPubkey),
            providerInstanceRef: String(args.providerInstanceRef),
            runtime: String(args.runtime),
            model: String(args.model),
            actorPubkey: "d".repeat(64),
            packRef: {
              repo: "app:shipped",
              sha: "0.5.16",
              role: "project-setup",
              path: "personas/roles/project-setup",
            },
            status: startCount === 1 ? "ambiguous" : "created",
            ...(startCount > 1
              ? {
                  receiptEventId: "e".repeat(64),
                  target: {
                    driver: "claude",
                    instanceId: "local-provider",
                    sessionId: "native-setup",
                    generation: 1,
                  },
                }
              : {}),
          };
          return launch;
        }
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

async function workbenchCalls(page: Page) {
  const authoringReads = new Set([
    "coding_session_provider_status",
    "coding_session_provider_runtimes",
    "coding_session_provider_models",
    "project_team_setup_get_authoring",
    "project_team_setup_get_launch",
  ]);
  return (await setupCalls(page)).filter(
    (call) => !authoringReads.has(call.command),
  );
}

test("open is read-only; explicit preparation and validation stay separate from publication; reopening resumes", async ({
  page,
}) => {
  await openRoles(page);
  const dialog = page.getByTestId("project-team-setup-dialog");
  await expect(
    dialog.getByLabel("What should this project accomplish?"),
  ).toBeVisible();
  expect((await workbenchCalls(page)).map((call) => call.command)).toEqual([
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
  expect(
    (await setupCalls(page)).findLast(
      (call) => call.command === "project_team_setup_snapshot",
    ),
  ).toEqual({
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
  expect((await workbenchCalls(page)).map((call) => call.command)).toEqual([
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
  expect((await workbenchCalls(page)).map((call) => call.command)).toEqual([
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
    const authoringStart = dialog.getByRole("button", {
      name: "Start authoring session",
      exact: true,
    });
    await expect(authoringStart).toBeEnabled();
    await authoringStart.scrollIntoViewIfNeeded();
    await checkFit();
    await waitForAnimations(page);
    await dialog.screenshot({
      path: testInfo.outputPath("setup-authoring.png"),
    });
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

test("authoring opens read-only, provisions explicitly and retries one durable request", async ({
  page,
}) => {
  await openRoles(page);
  const dialog = page.getByTestId("project-team-setup-dialog");
  await dialog
    .getByLabel("What should this project accomplish?")
    .fill("Build the project's baseline team.");
  await dialog
    .getByLabel("Local project repository")
    .fill("/projects/tankloop");
  await dialog
    .getByRole("button", { name: "Prepare draft", exact: true })
    .click();
  const authoring = dialog.getByRole("region", {
    name: "Project team authoring",
  });
  const start = authoring.getByRole("button", {
    name: "Start authoring session",
    exact: true,
  });
  await expect(start).toBeEnabled();
  expect(
    (await setupCalls(page)).filter((call) =>
      /provision_|reserve_authoring|start_authoring/.test(call.command),
    ),
  ).toEqual([]);
  await dialog
    .getByRole("button", { name: "Check draft", exact: true })
    .click();
  await expect(
    dialog.getByTestId("project-team-setup-validation"),
  ).toContainText("Pack structure passed");
  await start.click();
  await expect(
    dialog.getByTestId("project-team-setup-validation"),
  ).toContainText("has not been checked yet");
  await expect(authoring.getByRole("status")).toContainText(
    "Delivery is uncertain",
  );
  await expect(authoring).not.toContainText("provider confirmed creation");
  await authoring
    .getByRole("button", { name: "Retry saved authoring request", exact: true })
    .click();
  await expect(authoring.getByRole("status")).toContainText(
    "provider confirmed creation",
  );
  await expect(
    authoring.getByRole("button", { name: "Open authoring session" }),
  ).toBeVisible();
  const calls = await setupCalls(page);
  expect(
    calls.filter(
      (call) => call.command === "provision_coding_session_provider",
    ),
  ).toHaveLength(1);
  expect(
    calls.filter(
      (call) => call.command === "project_team_setup_reserve_authoring",
    ),
  ).toHaveLength(1);
  const starts = calls.filter(
    (call) => call.command === "project_team_setup_start_authoring",
  );
  expect(starts).toHaveLength(2);
  expect(starts[0].args).toEqual(starts[1].args);
  expect(JSON.stringify(starts)).not.toContain("/projects/tankloop");
  await page.keyboard.press("Escape");
  await expect(dialog).not.toBeVisible();
  await page.getByTestId("project-team-setup-open").click();
  await expect(authoring.getByRole("status")).toContainText(
    "provider confirmed creation",
  );
  expect(
    (await setupCalls(page)).filter(
      (call) => call.command === "project_team_setup_start_authoring",
    ),
  ).toHaveLength(2);
});

test("runtime discovery failure gives a recoverable setup blocker without provisioning", async ({
  page,
}) => {
  await openRoles(page);
  await page.evaluate(() => {
    (window as unknown as FixtureWindow).__setupRuntimeFail = true;
  });
  const dialog = page.getByTestId("project-team-setup-dialog");
  await dialog
    .getByLabel("What should this project accomplish?")
    .fill("Build the project's baseline team.");
  await dialog
    .getByLabel("Local project repository")
    .fill("/projects/tankloop");
  await dialog
    .getByRole("button", { name: "Prepare draft", exact: true })
    .click();
  const authoring = dialog.getByRole("region", {
    name: "Project team authoring",
  });
  await expect(authoring.getByRole("alert")).toContainText(
    "Runtime discovery unavailable",
  );
  await expect(
    authoring.getByRole("button", { name: "Start authoring session" }),
  ).toBeDisabled();
  expect(
    (await setupCalls(page)).filter((call) =>
      /provision_|reserve_authoring|start_authoring/.test(call.command),
    ),
  ).toEqual([]);
  await page.evaluate(() => {
    (window as unknown as FixtureWindow).__setupRuntimeFail = false;
  });
  await authoring.getByRole("button", { name: "Refresh runtimes" }).click();
  await expect(
    authoring.getByRole("button", { name: "Start authoring session" }),
  ).toBeEnabled();
});
