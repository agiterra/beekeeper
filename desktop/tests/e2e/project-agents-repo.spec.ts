import { expect, test, type Page } from "@playwright/test";

import type { RelayEvent } from "@/shared/api/types";
import {
  KIND_AGENTS_REPO_DRAFT_OP,
  KIND_PROJECT,
  KIND_PROJECT_PACK_SOURCE,
} from "@/shared/constants/kinds";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge } from "../helpers/bridge";

/**
 * The Files tab (spec § 4.12): the agents repository read from `main`,
 * edited as shared drafts (NIP-AD, kind 44249), committed from the app.
 *
 * The host commands are answered by `e2eBridgeAgentsRepo.ts` from a seed;
 * the drafts are real signed events published through the mock relay's
 * project store and read back by `#a`, so the fold, the head-conflict rule
 * and the disclosure strip run on the real path. The commit's host result
 * is scripted — a stale-base refusal first, then a landing — so the dialog
 * is seen printing both verbatim.
 */

const GENERAL_CHANNEL_ID = "9a1657ac-f7aa-5db0-b632-d8bbeb6dfb50";
const PROJECT_OWNER = "a1".repeat(32);
const PROJECT_ADDRESS = `${KIND_PROJECT}:${PROJECT_OWNER}:general`;
const AGENTS_REPO = `30617:${PROJECT_OWNER}:general-beekeeper-agents`;
const TIP = "5c2bf83980041e5a38a0dbbe881788b7f032d3e5";
const ROADMAP_BLOB = "29aadf90f27a03d826fb8d9db2852b3a46c4b89b";
const LEAD_BLOB = "a2dcee2c4f048401fdcc1e054abfcd72e40a7c98";
const ROADMAP_TEXT = "# Roadmap\n\nOverworld first.\n";

function generalProject(): RelayEvent {
  return {
    id: "project-general-files".padEnd(64, "0"),
    pubkey: PROJECT_OWNER,
    created_at: Math.floor(Date.now() / 1000) - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", "general"],
      ["name", "General"],
      ["channel", GENERAL_CHANNEL_ID],
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

/** The project's kind:30624: an agents repository at the root, following main. */
function agentsRepoSource(): RelayEvent {
  return {
    id: "pack-source-general-files".padEnd(64, "0"),
    pubkey: PROJECT_OWNER,
    created_at: Math.floor(Date.now() / 1000) - 3_600,
    kind: KIND_PROJECT_PACK_SOURCE,
    tags: [
      ["d", PROJECT_ADDRESS],
      ["repo", AGENTS_REPO],
      ["ref", "refs/heads/main"],
      ["path", "."],
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

const SEED = {
  listing: {
    repo: AGENTS_REPO,
    branch: "main",
    commit: TIP,
    syncedAt: "2026-09-21T15:00:00Z",
    entries: [
      {
        path: "README.md",
        blob: "4bf6574b69b0183e6350d27e9287c200e023fd1e",
        size: 12,
        kind: "readme",
      },
      {
        path: "team.yml",
        blob: "7b9a85bbe3dbcc64eadd04d4759783cd555a2b8d",
        size: 40,
        kind: "manifest",
      },
      { path: "roles/lead.md", blob: LEAD_BLOB, size: 60, kind: "role" },
      {
        path: "plans/roadmap.md",
        blob: ROADMAP_BLOB,
        size: ROADMAP_TEXT.length,
        kind: "plan",
      },
      {
        path: "plans/.gitkeep",
        blob: "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391",
        size: 0,
        kind: "gitkeep",
      },
    ],
  },
  files: {
    "README.md": "# Agents\n",
    "team.yml": "schema: beekeeper-team/v1\nlead: lead\n",
    "roles/lead.md":
      "---\ndescription: lead\n---\n![[beekeeper/lead@^1.0.0]]\n",
    "plans/roadmap.md": ROADMAP_TEXT,
  },
  commitResults: [
    {
      pushed: "no",
      tipBefore: TIP,
      refusals: [
        {
          path: "plans/roadmap.md",
          code: "stale-base",
          message:
            "main changed plans/roadmap.md after this draft by e5ebc6cd was based on it; reload the file and re-apply the draft — nothing was pushed",
        },
      ],
    },
    {
      pushed: "yes",
      tipBefore: TIP,
      commit: "7c079ad1d37334ae313d4b8facc44203d660d208",
      tree: "122f8c7c2105aa531fbc11e02f5398c9ca78aa04",
      paths: [{ path: "plans/roadmap.md", status: "M" }],
      actions: "checked (0 actions)",
    },
  ],
};

async function openFilesTab(page: Page) {
  await page.addInitScript(
    (features) => {
      window.localStorage.setItem("buzz-feature-overrides-v1", features);
    },
    JSON.stringify({ projects: true }),
  );
  await page.addInitScript(
    (events) => {
      (
        window as unknown as { __BUZZ_E2E_EXTRA_PROJECT_EVENTS__: unknown }
      ).__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events;
    },
    [generalProject(), agentsRepoSource()],
  );
  await page.addInitScript((seed) => {
    (
      window as unknown as { __BUZZ_E2E_AGENTS_REPO__: unknown }
    ).__BUZZ_E2E_AGENTS_REPO__ = seed;
    (
      window as unknown as { __BUZZ_E2E_SIGNED_EVENTS__: unknown[] }
    ).__BUZZ_E2E_SIGNED_EVENTS__ = [];
  }, SEED);
  await installMockBridge(page, {});
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  const general = page.getByTestId("project-group-general");
  await expect(general).toBeVisible({ timeout: 15_000 });
  await general.hover();
  await page.getByTestId("project-open-general").click();
  await expect(page.getByTestId("project-page-tabs")).toBeVisible();
  await page.getByTestId("project-tab-files").click();
  await expect(page.getByTestId("agents-repo-screen")).toBeVisible();
}

test("a plan is read from main, drafted, previewed, badged, and refused when a newer head appears", async ({
  page,
}) => {
  await openFilesTab(page);

  // The tree lists main's files grouped with plans first, and says which tip.
  const tree = page.getByTestId("agents-repo-tree");
  await expect(tree).toContainText("Plans");
  await expect(page.getByTestId("agents-repo-tip")).toContainText(
    TIP.slice(0, 8),
  );
  await expect(tree.getByTestId("agents-repo-file-plans/.gitkeep")).toHaveCount(
    0,
  );

  // Open the plan: main's text is previewed as Markdown.
  await page.getByTestId("agents-repo-file-plans/roadmap.md").click();
  await expect(page.getByTestId("agents-repo-preview")).toContainText(
    "Overworld first.",
  );
  await expect(page.getByTestId("agents-repo-disclosures")).toHaveCount(0);

  // Edit and save: a kind 44249 with base = main's blob and prev = null.
  await page.getByTestId("agents-repo-editor-tab-edit").click();
  await page.getByTestId("agents-repo-edit").click();
  await page
    .getByTestId("agents-repo-textarea")
    .fill("# Roadmap\n\nOverworld first, then the battle screen.\n");
  await page.getByTestId("agents-repo-note").fill("battle screen next");
  await page.getByTestId("agents-repo-save").click();
  await expect(page.getByTestId("agents-repo-disclosure-draft")).toContainText(
    "Draft by",
  );
  await expect(
    page.getByTestId("agents-repo-draft-badge-plans/roadmap.md"),
  ).toBeVisible();
  const signed = await page.evaluate(
    () =>
      (
        window as unknown as {
          __BUZZ_E2E_SIGNED_EVENTS__: {
            kind: number;
            content: string;
            tags: string[][];
          }[];
        }
      ).__BUZZ_E2E_SIGNED_EVENTS__,
  );
  const draft = signed.find(
    (event) => event.kind === KIND_AGENTS_REPO_DRAFT_OP,
  );
  expect(draft).toBeTruthy();
  const content = JSON.parse(draft?.content ?? "{}") as Record<string, unknown>;
  expect(content.op).toBe("file.put");
  expect(content.base).toBe(ROADMAP_BLOB);
  expect(content.baseCommit).toBe(TIP);
  expect(content.prev).toBeNull();
  expect(content.message).toBe("battle screen next");
  expect(draft?.tags).toContainEqual(["ad-repo", AGENTS_REPO]);
  expect(draft?.tags).toContainEqual(["ad-path", "plans/roadmap.md"]);

  // The drafts panel lists it and the diff tab shows the change.
  await expect(
    page.getByTestId("agents-repo-draft-row-plans/roadmap.md"),
  ).toContainText("edit by");
  await page.getByTestId("agents-repo-editor-tab-diff").click();
  await expect(page.getByTestId("agents-repo-diff")).toContainText(
    "battle screen",
  );

  // Someone else's newer head arrives on the same path while this person
  // edits: the save is refused before signing, naming them, and the text
  // stays in the editor.
  await page.getByTestId("agents-repo-editor-tab-edit").click();
  await page.getByTestId("agents-repo-edit").click();
  await page.getByTestId("agents-repo-textarea").fill("# Roadmap\n\nmine\n");
  const mine = draft?.content ?? "";
  const otherId = "b".repeat(64);
  await page.evaluate(
    (event) => {
      const seed = window.__BUZZ_E2E_SEED_MOCK_PROJECT_EVENT__;
      if (!seed) throw new Error("mock project-event seam is missing");
      seed(event as never);
    },
    {
      id: otherId,
      pubkey: "2".repeat(64),
      created_at: Math.floor(Date.now() / 1000) + 30,
      kind: KIND_AGENTS_REPO_DRAFT_OP,
      tags: [
        ["a", PROJECT_ADDRESS],
        ["ad-v", "ad1-1"],
        ["ad-op", "file.put"],
        ["ad-repo", AGENTS_REPO],
        ["ad-path", "plans/roadmap.md"],
      ],
      content: JSON.stringify({
        ...JSON.parse(mine),
        text: "# Roadmap\n\nsomeone else's\n",
        prev: null,
        message: null,
      }),
      sig: "mocksig".repeat(20).slice(0, 128),
    },
  );
  await page.getByTestId("agents-repo-refresh").click();
  await expect(page.getByTestId("agents-repo-newer-head")).toContainText(
    "saved a newer draft",
    { timeout: 10_000 },
  );
  await page.getByTestId("agents-repo-save").click();
  await expect(page.getByTestId("agents-repo-editor-error")).toContainText(
    "saved a newer draft",
  );
  await expect(page.getByTestId("agents-repo-textarea")).toHaveValue(
    "# Roadmap\n\nmine\n",
  );
  const signedAfter = await page.evaluate(
    () =>
      (
        window as unknown as { __BUZZ_E2E_SIGNED_EVENTS__: { kind: number }[] }
      ).__BUZZ_E2E_SIGNED_EVENTS__.filter((e) => e.kind === 44249).length,
  );
  expect(signedAfter).toBe(1);
});

test("New plan asks for a name in a dialog and opens the editor on a path that is not on main", async ({
  page,
}) => {
  await openFilesTab(page);
  await page.getByTestId("agents-repo-new-plan").click();
  const dialog = page.getByTestId("agents-repo-new-plan-dialog");
  await expect(dialog).toBeVisible();

  // The name is slugged and the destination path shown before anything is saved.
  await page.getByTestId("agents-repo-new-plan-name").fill("Battle Screen");
  await expect(page.getByTestId("agents-repo-new-plan-preview")).toHaveText(
    "plans/battle-screen.md",
  );
  await page.getByTestId("agents-repo-new-plan-create").click();
  await expect(dialog).toHaveCount(0);

  // The editor opens on the new path straight into editing (nothing to
  // preview yet), says it is not on main, and saves a draft whose base is null.
  const editor = page.getByTestId("agents-repo-editor");
  await expect(editor).toContainText("plans/battle-screen.md");
  await expect(editor).toContainText("not on main");
  await expect(page.getByTestId("agents-repo-textarea")).toBeEnabled();
  await page
    .getByTestId("agents-repo-textarea")
    .fill("# Battle screen\n\nTurn order first.\n");
  await page.getByTestId("agents-repo-save").click();
  await expect(
    page.getByTestId("agents-repo-draft-badge-plans/battle-screen.md"),
  ).toBeVisible();
  const draft = await page.evaluate(() => {
    const events = (
      window as unknown as {
        __BUZZ_E2E_SIGNED_EVENTS__: { kind: number; content: string }[];
      }
    ).__BUZZ_E2E_SIGNED_EVENTS__;
    const found = events.find((e) => e.kind === 44249);
    return found ? JSON.parse(found.content) : null;
  });
  expect(draft).toMatchObject({
    op: "file.put",
    path: "plans/battle-screen.md",
    base: null,
    prev: null,
  });

  // A reserved name is refused inside the dialog, not silently.
  await page.getByTestId("agents-repo-new-plan").click();
  await page.getByTestId("agents-repo-new-plan-name").fill("archive");
  await page.getByTestId("agents-repo-new-plan-create").click();
  await expect(page.getByTestId("agents-repo-new-plan-preview")).toContainText(
    "reserved",
  );
});

test("the commit dialog prints a stale-base refusal verbatim, then a landing, and marks the drafts committed", async ({
  page,
}) => {
  await openFilesTab(page);
  await page.getByTestId("agents-repo-file-plans/roadmap.md").click();
  await page.getByTestId("agents-repo-editor-tab-edit").click();
  await page.getByTestId("agents-repo-edit").click();
  await page.getByTestId("agents-repo-textarea").fill("# Roadmap\n\nv2\n");
  await page.getByTestId("agents-repo-save").click();
  await expect(
    page.getByTestId("agents-repo-draft-badge-plans/roadmap.md"),
  ).toBeVisible();

  await page.getByTestId("agents-repo-commit-open").click();
  await expect(page.getByTestId("agents-repo-commit-dialog")).toBeVisible();
  await waitForAnimations(page);
  await expect(
    page.getByTestId("agents-repo-commit-pick-plans/roadmap.md"),
  ).toBeChecked();
  await page
    .getByTestId("agents-repo-commit-message")
    .fill("docs(agents): roadmap v2");
  await page.getByTestId("agents-repo-commit-confirm").click();

  // First scripted answer: nothing pushed, the path named with the author.
  await expect(page.getByTestId("agents-repo-commit-result-no")).toContainText(
    "Nothing was pushed.",
  );
  await expect(
    page.getByTestId("agents-repo-refusal-stale-base"),
  ).toContainText("plans/roadmap.md");
  await expect(
    page.getByTestId("agents-repo-refusal-stale-base"),
  ).toContainText("e5ebc6cd");
  await page.getByTestId("agents-repo-commit-close").click();
  await expect(
    page.getByTestId("agents-repo-draft-badge-plans/roadmap.md"),
  ).toBeVisible();

  // Second: the landing. The record closes the draft, so the badge goes.
  await page.getByTestId("agents-repo-commit-open").click();
  await waitForAnimations(page);
  await page
    .getByTestId("agents-repo-commit-message")
    .fill("docs(agents): roadmap v2");
  await page.getByTestId("agents-repo-commit-confirm").click();
  await expect(page.getByTestId("agents-repo-commit-result-yes")).toContainText(
    "1 file committed as 7c079ad1",
  );
  await expect(page.getByTestId("agents-repo-commit-result-yes")).toContainText(
    "M plans/roadmap.md",
  );
  const calls = await page.evaluate(
    () =>
      (
        window as unknown as {
          __BUZZ_E2E_AGENTS_REPO_COMMIT_CALLS__: {
            request: Record<string, unknown>;
          }[];
        }
      ).__BUZZ_E2E_AGENTS_REPO_COMMIT_CALLS__,
  );
  expect(calls).toHaveLength(2);
  expect(calls[1]?.request.expectedTip).toBe(TIP);
  expect(calls[1]?.request.message).toBe("docs(agents): roadmap v2");
  const record = await page.evaluate(() =>
    (
      window as unknown as {
        __BUZZ_E2E_SIGNED_EVENTS__: { kind: number; content: string }[];
      }
    ).__BUZZ_E2E_SIGNED_EVENTS__
      .filter((e) => e.kind === 44249)
      .map((e) => JSON.parse(e.content) as Record<string, unknown>)
      .find((c) => c.op === "commit.record"),
  );
  expect(record?.commit).toBe("7c079ad1d37334ae313d4b8facc44203d660d208");
  expect(record?.paths).toEqual(["plans/roadmap.md"]);
  await page.getByTestId("agents-repo-commit-close").click();
  await expect(
    page.getByTestId("agents-repo-draft-badge-plans/roadmap.md"),
  ).toHaveCount(0);
  await expect(page.getByTestId("agents-repo-drafts-panel")).toContainText(
    "No open drafts.",
  );
});

test("a viewer of a private project reads only, and the commit button is replaced by the reason", async ({
  page,
}) => {
  await page.addInitScript(
    (features) => {
      window.localStorage.setItem("buzz-feature-overrides-v1", features);
    },
    JSON.stringify({ projects: true }),
  );
  const viewerPubkey =
    "e5ebc6cdb579be112e336cc319b5989b4bb6af11786ea90dbe52b5f08d741b34";
  const privateProject: RelayEvent = {
    ...generalProject(),
    tags: [
      ["d", "general"],
      ["name", "General"],
      ["channel", GENERAL_CHANNEL_ID],
      ["buzz-access", "private"],
      ["p", viewerPubkey, "", "viewer"],
    ],
  };
  await page.addInitScript(
    (events) => {
      (
        window as unknown as { __BUZZ_E2E_EXTRA_PROJECT_EVENTS__: unknown }
      ).__BUZZ_E2E_EXTRA_PROJECT_EVENTS__ = events;
    },
    [privateProject, agentsRepoSource()],
  );
  await page.addInitScript((seed) => {
    (
      window as unknown as { __BUZZ_E2E_AGENTS_REPO__: unknown }
    ).__BUZZ_E2E_AGENTS_REPO__ = seed;
  }, SEED);
  await page.addInitScript(
    (identity) => {
      window.localStorage.setItem(
        "buzz:e2e-identity-override.v1",
        JSON.stringify(identity),
      );
    },
    {
      privateKey:
        "3dbaebadb5dfd777ff25149ee230d907a15a9e1294b40b830661e65bb42f6c03",
      pubkey: viewerPubkey,
      username: "tyler",
    },
  );
  await installMockBridge(page, {});
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  const general = page.getByTestId("project-group-general");
  await expect(general).toBeVisible({ timeout: 15_000 });
  await general.hover();
  await page.getByTestId("project-open-general").click();
  await page.getByTestId("project-tab-files").click();
  await page.getByTestId("agents-repo-file-plans/roadmap.md").click();
  await page.getByTestId("agents-repo-editor-tab-edit").click();
  await expect(page.getByTestId("agents-repo-read-only")).toContainText(
    "viewer of this project",
  );
  await expect(page.getByTestId("agents-repo-edit")).toHaveCount(0);
  await expect(page.getByTestId("agents-repo-new-plan")).toHaveCount(0);
});
