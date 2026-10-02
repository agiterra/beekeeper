import { execFile } from "node:child_process";
import { promisify } from "node:util";

import { expect, test, type Page } from "@playwright/test";

import { installRelayBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { waitForAnimations } from "../helpers/animations";

const exec = promisify(execFile);

// Live gate for project artifacts (NIP-AD documents, NIP-AR pins) against a
// REAL relay. The app watches as `bee` — a *second* client, signing as a
// collaborator — drafts a document, commits it, and pins it, and the sidebar
// must grow the row with no reload. Then the filter box hides it and brings
// it back.
//
// What only this can prove, and a synthetic spec cannot:
//   - a pin is **shared**: alice pins, tyler's sidebar shows it;
//   - the sidebar actually *paints* a pinned-artifact row. It did not: the
//     row was built, numbered and counted but left out of the JSX, so the
//     filter read "1 pinned artifact is hidden" and ticking the box back on
//     drew nothing (ledger 309(m)). The paint list and the numbering list are
//     now one array, and this spec is what notices if they part again;
//   - hiding is **disclosed**, with the count, not silent.
//
// Each run leaves one project behind in the community it ran against (the
// same way the to-do gate does). Clear them with:
//
//   bee projects delete <slug> --cascade --yes      # as tyler
//
// which detaches, and does not delete, the two repositories it made.
//
// Requires: BUZZ_E2E_PROJECT_ARTIFACTS=1, BUZZ_E2E_CLI_BIN (a built `bee`),
// BUZZ_E2E_RELAY_URL pointing at a running relay, and the git credential
// helper (`just install-git-credentials`) — `bee packs init` pushes the
// agents repository's seed commit over NIP-98 git.
const enabled = process.env.BUZZ_E2E_PROJECT_ARTIFACTS === "1";

function required(name: string, value: string | undefined): string {
  if (!value) throw new Error(`${name} is required for the live gate`);
  return value;
}

async function runCli(args: string[], privateKey: string): Promise<string> {
  const binary = required("BUZZ_E2E_CLI_BIN", process.env.BUZZ_E2E_CLI_BIN);
  const relayUrl = required(
    "BUZZ_E2E_RELAY_URL",
    process.env.BUZZ_E2E_RELAY_URL,
  );
  const { stdout } = await exec(binary, args, {
    cwd: "..",
    env: {
      ...process.env,
      BUZZ_AUTH_TAG: "",
      BUZZ_PRIVATE_KEY: privateKey,
      BUZZ_RELAY_URL: relayUrl,
      // `packs init` and `agents-repo commit` spawn `git push`, and
      // git-credential-nostr resolves `NOSTR_PRIVATE_KEY` — never
      // `BUZZ_PRIVATE_KEY`, on purpose (`git-credential-nostr::resolve_key`).
      // Without this the CLI acts as one identity while its git push signs as
      // whoever owns this machine's key file, and the relay answers
      // "repository not found" for a repository under someone else's pubkey.
      NOSTR_PRIVATE_KEY: privateKey,
    },
  });
  return stdout;
}

type Seed = { coordinate: string; dtag: string };

/** A project with an agents repository, and alice able to write to it. */
async function seedProject(): Promise<Seed> {
  const dtag = `artifacts-live-${process.pid}-${Date.now().toString(36)}`;
  const tyler = TEST_IDENTITIES.tyler;
  const coordinate = `30621:${tyler.pubkey}:${dtag}`;
  await runCli(
    ["repos", "create", "--id", dtag, "--name", dtag],
    tyler.privateKey,
  );
  await runCli(
    ["projects", "create", dtag, "--repo", dtag, "--name", `Artifacts ${dtag}`],
    tyler.privateKey,
  );
  await runCli(
    [
      "projects",
      "add-member",
      dtag,
      "--pubkey",
      TEST_IDENTITIES.alice.pubkey,
      "--role",
      "collaborator",
    ],
    tyler.privateKey,
  );
  // The documents live in the agents repository, so the project needs one.
  // `projects create` attempts this too; doing it here means a credential
  // problem fails the gate by name instead of surfacing as "no kind:30624".
  await runCli(["packs", "init", "--project", coordinate], tyler.privateKey);
  return { coordinate, dtag };
}

async function openProject(page: Page, seed: Seed) {
  // The preview server has no history fallback and the app's router does not
  // read browser history, so arrive the way a person does.
  await page.goto("/");
  await page
    .getByTestId("app-sidebar")
    .waitFor({ state: "visible", timeout: 60_000 });
  await page
    .getByTestId(`project-open-${seed.dtag}`)
    .click({ timeout: 60_000 });
}

test.describe("project artifacts (live relay)", () => {
  test.skip(!enabled, "set BUZZ_E2E_PROJECT_ARTIFACTS=1 to run the live gate");
  test.setTimeout(240_000);

  test("a collaborator's pinned document reaches the sidebar, and the filter discloses hiding it", async ({
    page,
  }) => {
    const seed = await seedProject();
    const alice = TEST_IDENTITIES.alice.privateKey;
    const docPath = "docs/notes/api-shape.md";
    await installRelayBridge(page, "tyler");
    await openProject(page, seed);
    const group = page.getByTestId(`project-group-${seed.dtag}`);
    const artifactRows = group.locator(
      '[data-testid^="project-artifact-row-"]',
    );
    await expect(artifactRows).toHaveCount(0);

    await test.step("a collaborator drafts a document and lands it on main", async () => {
      // `docs edit` reads the new text from stdin or --file; this runner
      // cannot feed a pipe, so the text is a fixture beside this spec.
      await runCli(
        [
          "docs",
          "edit",
          "notes/api-shape.md",
          "--project",
          seed.coordinate,
          "--file",
          "desktop/tests/e2e/fixtures/artifacts-note.md",
          "--message",
          "the shape",
        ],
        alice,
      );
      await runCli(
        [
          "agents-repo",
          "commit",
          "--all",
          "--project",
          seed.coordinate,
          "--message",
          "docs(artifacts): a note",
        ],
        alice,
      );
      const listed = JSON.parse(
        await runCli(
          ["--format", "compact", "docs", "list", "--project", seed.coordinate],
          alice,
        ),
      ) as { documents: { path: string; class: string }[] };
      expect(listed.documents.map((entry) => entry.path)).toContain(docPath);
    });

    await test.step("the collaborator's pin appears in the creator's sidebar live", async () => {
      await runCli(
        ["pins", "pin", "--project", seed.coordinate, docPath],
        alice,
      );
      await expect(artifactRows).toHaveCount(1, { timeout: 30_000 });
      await expect(artifactRows).toContainText("api-shape");
    });

    await test.step("hiding the pins says how many, and ticking the box draws them again", async () => {
      await page.getByTestId(`project-session-filter-${seed.dtag}`).click();
      const box = page.getByTestId(
        "project-session-filter-show-pinned-artifacts",
      );
      await expect(box).toBeVisible({ timeout: 30_000 });
      await box.click();
      // Hidden, and *said* so with the count — the rule this filter states.
      const note = page.getByTestId(
        "project-session-filter-hidden-artifacts-note",
      );
      await expect(note).toContainText("1 pinned artifact is hidden");
      await expect(note).toContainText("Show pinned artifacts");
      await expect(artifactRows).toHaveCount(0);
      // And back. This is the assertion the bug failed: the count was right
      // and the row never returned.
      await box.click();
      await expect(artifactRows).toHaveCount(1, { timeout: 30_000 });
      await expect(note).toHaveCount(0);
      await page.keyboard.press("Escape");
    });

    await test.step("the pinned row opens the document alone: no tabs, no tree, no repository controls", async () => {
      // Scoped to this run's group on purpose: the dev community keeps the
      // projects earlier runs made, and each one pins the same path.
      await group
        .getByTestId(`project-artifact-row-${docPath}`)
        .click({ timeout: 30_000 });
      // What stays: the document and its history.
      await expect(page.getByTestId("agents-repo-editor")).toBeVisible({
        timeout: 30_000,
      });
      await expect(page.getByTestId("agents-repo-editor")).toContainText(
        "api-shape.md",
      );
      await expect(page.getByTestId("agents-repo-drafts-panel")).toBeVisible();
      // What goes: the project's tabs, the file tree, and every control that
      // is about the repository rather than this document.
      await expect(page.getByTestId("project-page-tabs")).toHaveCount(0);
      await expect(page.getByTestId("agents-repo-tree")).toHaveCount(0);
      await expect(page.getByTestId("agents-repo-refresh")).toHaveCount(0);
      await expect(page.getByTestId("agents-repo-tip")).toHaveCount(0);
      await expect(page.getByTestId("agents-repo-new-document")).toHaveCount(0);
      await expect(page).toHaveURL(/[?&]view=file(&|$)/);
      // And a way back, so the view is not a dead end.
      await page.getByTestId("agents-repo-focused-all").click();
      await expect(page.getByTestId("project-page-tabs")).toBeVisible({
        timeout: 30_000,
      });
      await expect(page.getByTestId("agents-repo-tree")).toBeVisible();
      await expect(page).not.toHaveURL(/view=file/);
    });

    await test.step("a folder pins as its own row, and unpinning drops both", async () => {
      await runCli(
        ["pins", "pin", "--project", seed.coordinate, "docs/notes", "--folder"],
        alice,
      );
      await expect(artifactRows).toHaveCount(2, { timeout: 30_000 });
      // A folder's row keeps the whole tab: the folder *is* a tree row, so
      // the one-file view would hide the thing that was pinned.
      await group
        .getByTestId("project-artifact-row-docs/notes")
        .click({ timeout: 30_000 });
      await expect(page.getByTestId("agents-repo-tree")).toBeVisible({
        timeout: 30_000,
      });
      await expect(page.getByTestId("project-page-tabs")).toBeVisible();
      await expect(page).not.toHaveURL(/view=file/);
      await runCli(
        ["pins", "unpin", "--project", seed.coordinate, docPath],
        alice,
      );
      await runCli(
        ["pins", "unpin", "--project", seed.coordinate, "docs/notes"],
        alice,
      );
      await expect(artifactRows).toHaveCount(0, { timeout: 30_000 });
    });

    await waitForAnimations(page);
    await page.screenshot({
      path: "test-results/project-artifacts/live-sidebar.png",
      clip: { x: 0, y: 0, width: 320, height: 720 },
    });
  });
});
