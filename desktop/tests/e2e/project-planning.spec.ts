import { mkdirSync } from "node:fs";

import { hexToBytes } from "@noble/hashes/utils.js";
import { expect, test, type Page } from "@playwright/test";
import { finalizeEvent } from "nostr-tools/pure";

import { KIND_PROJECT, KIND_PROJECT_TODO_OP } from "@/shared/constants/kinds";
import type { RelayEvent } from "@/shared/api/types";
import { waitForAnimations } from "../helpers/animations";
import { installMockBridge, TEST_IDENTITIES } from "../helpers/bridge";
import { installPluginRunner, TEST_REGISTRY } from "../helpers/pluginRunner";

/**
 * Project Planning, end to end through the MOCK relay (not a live relay),
 * with the REAL Planning plugin: every read and write the page makes is an
 * operation of the WebAssembly component, run by the real plugin host
 * (`installPluginRunner`) over the records in the mock store.
 *
 * The issues are real NIP-TD ops (kind 44248), signed with the bridge's
 * fixture identities and stored in the mock project store, which matches
 * them by `kinds` + `#a` exactly as the read issues them. Writes made in the
 * UI are signed by the mock identity, published through the mock relay and
 * read back by the next refetch, so what the screen shows after a write is
 * what the store holds — not an optimistic guess.
 *
 * What this cannot prove: persistence across an app restart (the mock store
 * lives in the page) and the real relay's admission rules. Those belong to a
 * live run.
 */

const MOCK_OWNER = "deadbeef".repeat(8);
const DTAG = "planning-demo";
const COORDINATE = `${KIND_PROJECT}:${MOCK_OWNER}:${DTAG}`;
const PROJECT_ID = `${MOCK_OWNER}:${DTAG}`;
const PLANNING_URL = `/#/projects/${PROJECT_ID}/planning`;
const SHOTS = "test-results/planning";

const LIST = "1".repeat(32);
const RELEASE = "2".repeat(32);
const ITEMS = {
  view: "a1".repeat(16),
  crash: "b2".repeat(16),
  doc: "c3".repeat(16),
  ship: "d4".repeat(16),
  linear: "e5".repeat(16),
  retire: "f6".repeat(16),
  notes: "a7".repeat(16),
};

const ALICE = TEST_IDENTITIES.alice;
const BOB = TEST_IDENTITIES.bob;
const SECRET: Record<string, Uint8Array> = {
  [ALICE.pubkey]: hexToBytes(ALICE.privateKey),
  [BOB.pubkey]: hexToBytes(BOB.privateKey),
};

function nowSeconds(): number {
  return Math.floor(Date.now() / 1_000);
}

let clock = nowSeconds() - 3_600;

function op(author: string, content: Record<string, unknown>): RelayEvent {
  const tags = [
    ["a", COORDINATE],
    ["td-v", "td1-1"],
    ["td-op", String(content.op)],
    ["td-list", String(content.listId)],
    ["td-vis", "project"],
  ];
  if (typeof content.itemId === "string")
    tags.push(["td-item", content.itemId]);
  clock += 5;
  const secret = SECRET[author];
  if (!secret) throw new Error("unknown fixture author");
  return finalizeEvent(
    {
      kind: KIND_PROJECT_TODO_OP,
      created_at: clock,
      tags,
      content: JSON.stringify({ schema: "buzz-project-todo/v1", ...content }),
    },
    secret,
  ) as unknown as RelayEvent;
}

function issueOps(input: {
  listId?: string;
  itemId: string;
  text: string;
  rank: string;
  author?: string;
  status?: string;
  priority?: string;
  assignee?: string;
  description?: string;
}): RelayEvent[] {
  const listId = input.listId ?? LIST;
  const author = input.author ?? ALICE.pubkey;
  const base = { listId, itemId: input.itemId };
  const events = [
    op(author, { op: "item.add", ...base, text: input.text, rank: input.rank }),
  ];
  if (input.status === "done" || input.status === "canceled") {
    events.push(op(author, { op: "item.done", ...base, done: true }));
  }
  if (input.status && input.status !== "todo") {
    events.push(
      op(author, { op: "item.status", ...base, status: input.status }),
    );
  }
  if (input.priority) {
    events.push(
      op(BOB.pubkey, {
        op: "item.priority",
        ...base,
        priority: input.priority,
      }),
    );
  }
  if (input.assignee) {
    events.push(
      op(author, { op: "item.assignee", ...base, assignee: input.assignee }),
    );
  }
  if (input.description) {
    events.push(
      op(author, {
        op: "item.description",
        ...base,
        description: input.description,
      }),
    );
  }
  return events;
}

function projectHead(): RelayEvent {
  return {
    id: `project-${DTAG}`.padEnd(64, "0"),
    pubkey: MOCK_OWNER,
    created_at: nowSeconds() - 7_200,
    kind: KIND_PROJECT,
    tags: [
      ["d", DTAG],
      ["name", "Planning Demo"],
      ["description", "Injected head for the Planning spec."],
    ],
    content: "",
    sig: "mocksig".repeat(20).slice(0, 128),
  };
}

function seededIssues(): RelayEvent[] {
  clock = nowSeconds() - 3_600;
  return [
    op(ALICE.pubkey, {
      op: "list.create",
      listId: LIST,
      title: "Launch",
      visibility: "project",
    }),
    op(BOB.pubkey, {
      op: "list.create",
      listId: RELEASE,
      title: "Release",
      visibility: "project",
    }),
    ...issueOps({
      itemId: ITEMS.view,
      text: "List and board read one model",
      rank: "a0",
      status: "in_progress",
      priority: "high",
      assignee: ALICE.pubkey,
    }),
    ...issueOps({
      itemId: ITEMS.crash,
      text: "Triage the crash report from the nightly build",
      rank: "a1",
      author: BOB.pubkey,
      status: "triage",
      priority: "urgent",
    }),
    ...issueOps({
      itemId: ITEMS.doc,
      text: "Document the planning provider contract",
      rank: "a2",
      priority: "medium",
      description:
        "Say what the UI reads and writes, and what a provider must never do:\n- keep a second editable copy\n- hold secrets in the UI",
    }),
    ...issueOps({
      itemId: ITEMS.ship,
      text: "Add status, priority and description ops to NIP-TD",
      rank: "a3",
      status: "done",
    }),
    ...issueOps({
      itemId: ITEMS.linear,
      text: "List what a Linear connection needs from installations",
      rank: "a4",
      author: BOB.pubkey,
      status: "backlog",
      priority: "low",
    }),
    ...issueOps({
      itemId: ITEMS.retire,
      text: "Retire the old quick-add field",
      rank: "a5",
      status: "canceled",
    }),
    ...issueOps({
      listId: RELEASE,
      itemId: ITEMS.notes,
      text: "Write the release notes",
      rank: "a0",
      author: BOB.pubkey,
      assignee: BOB.pubkey,
    }),
  ];
}

async function openPlanning(
  page: Page,
  options: {
    issues?: RelayEvent[];
    hangKinds?: number[];
    rejectKinds?: number[];
    path?: string;
    /** Which plugins the mock relay runs; the first-party registry by default. */
    registry?: string;
  } = {},
) {
  await installPluginRunner(page, options.registry);
  await page.addInitScript(
    ({ events, hangKinds, rejectKinds }) => {
      window.__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = events as never;
      if (hangKinds)
        window.__BEEKEEPER_E2E_HANG_PROJECT_QUERY_KINDS__ = hangKinds;
      if (rejectKinds)
        window.__BEEKEEPER_E2E_REJECT_PROJECT_QUERY_KINDS__ = rejectKinds;
    },
    {
      events: [projectHead(), ...(options.issues ?? seededIssues())] as never,
      hangKinds: options.hangKinds ?? null,
      rejectKinds: options.rejectKinds ?? null,
    },
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
  await page.goto(options.path ?? PLANNING_URL, {
    waitUntil: "domcontentloaded",
  });
  await expect(page.getByTestId("planning-screen")).toBeVisible({
    timeout: 10_000,
  });
}

/**
 * Write access needs the project roster, which the mock answers only after
 * its startup reads drain (about ten seconds here); until then the page says
 * it is checking and draws no write controls.
 */
async function waitForWriteAccess(page: Page) {
  await expect(page.getByTestId("planning-view")).toHaveAttribute(
    "data-access",
    "writable",
    { timeout: 30_000 },
  );
}

async function shot(page: Page, name: string) {
  mkdirSync(SHOTS, { recursive: true });
  await waitForAnimations(page);
  await page.screenshot({ path: `${SHOTS}/${name}.png` });
}

test.use({ viewport: { width: 1440, height: 900 } });

test("the list groups the project's issues by status, and the To-Do tab reads the same records", async ({
  page,
}) => {
  await openPlanning(page);
  await expect(page.getByTestId("project-tab-planning")).toHaveAttribute(
    "aria-current",
    "page",
  );
  const list = page.getByTestId("planning-list");
  await expect(list).toBeVisible();
  await expect(page.getByTestId("planning-count")).toHaveText(/7 of 7 issues/);
  // Groups in workflow order; empty ones folded away in the list.
  const groups = list.locator(
    '[data-testid^="planning-group-"]:not([data-testid^="planning-group-create"])',
  );
  await expect(groups).toHaveText([
    /Triage\s*1/,
    /Backlog\s*1/,
    /Todo\s*2/,
    /In Progress\s*1/,
    /Done\s*1/,
    /Canceled\s*1/,
  ]);
  // Two lists, so each row names its list.
  await expect(page.getByTestId(`planning-row-${ITEMS.notes}`)).toContainText(
    "Release",
  );
  await waitForWriteAccess(page);
  await shot(page, "01-list");

  // The To-Do tab shows the same items: one model, two views.
  await page.getByTestId("project-tab-todos").click();
  await expect(
    page.getByText("Document the planning provider contract"),
  ).toBeVisible();
  await page.getByTestId("project-tab-planning").click();
  await expect(page.getByTestId("planning-list")).toBeVisible();
});

test("quick create from the keyboard writes an issue with its fields, and the detail edits it", async ({
  page,
}) => {
  await openPlanning(page);
  await page.getByTestId("planning-list").click({ position: { x: 5, y: 5 } });
  await waitForWriteAccess(page);
  await page.keyboard.press("c");
  const dialog = page.getByTestId("planning-quick-create");
  await expect(dialog).toBeVisible();
  await expect(dialog).toContainText("In Planning Demo · Launch");
  await page
    .getByTestId("planning-quick-create-title")
    .fill("Board drag writes the column's status");
  await page
    .getByTestId("planning-quick-create-description")
    .fill("Dropping a card on Done must publish item.done then item.status.");
  await page.getByTestId("planning-quick-create-priority").click();
  await page.getByTestId("planning-priority-option-high").click();
  await page.getByTestId("planning-quick-create-status").click();
  await page.getByTestId("planning-status-option-backlog").click();
  await shot(page, "02-quick-create");
  await page.keyboard.press("Meta+Enter");
  await expect(dialog).toBeHidden();

  const row = page
    .getByTestId("planning-list")
    .locator("li", { hasText: "Board drag writes the column's status" });
  await expect(row).toBeVisible({ timeout: 10_000 });
  await expect(page.getByTestId("planning-group-backlog")).toContainText("2");
  await expect(
    row.getByRole("button", { name: /Priority: High/ }),
  ).toBeVisible();

  // Open it with the keyboard and change its status in the detail.
  await row.getByRole("button", { name: /Board drag writes/ }).focus();
  await page.keyboard.press("Enter");
  const detail = page.getByTestId("planning-detail");
  await expect(detail).toBeVisible();
  await expect(page).toHaveURL(/planning\?issue=/);
  await expect(page.getByTestId("planning-detail-description")).toContainText(
    "item.done then item.status",
  );
  await page.getByTestId("planning-detail-status").click();
  await page.getByTestId("planning-status-option-in_progress").click();
  await expect(page.getByTestId("planning-detail-status")).toContainText(
    "In Progress",
    { timeout: 10_000 },
  );
  // History is the ops themselves, in order.
  const activity = page.getByTestId("planning-detail-activity");
  await expect(activity).toContainText("created the issue");
  await expect(activity).toContainText("set priority to High");
  await expect(activity).toContainText("set status to In Progress");
  await expect(page.getByTestId("planning-detail-no-comments")).toBeVisible();

  // Edit the description; ⌘↵ saves.
  await page.getByTestId("planning-detail-edit-description").click();
  await page
    .getByTestId("planning-detail-description-input")
    .fill("Dropping a card publishes the column's value as one op.");
  await page.keyboard.press("Meta+Enter");
  await expect(page.getByTestId("planning-detail-description")).toHaveText(
    "Dropping a card publishes the column's value as one op.",
    { timeout: 10_000 },
  );
  await shot(page, "03-detail");

  // Away and back: the open issue is in the URL and the writes are in the store.
  const url = page.url();
  await page.getByTestId("project-tab-todos").click();
  await page.goto(url, { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("planning-detail-description")).toHaveText(
    "Dropping a card publishes the column's value as one op.",
    { timeout: 10_000 },
  );
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("planning-detail")).toBeHidden();
});

test("the board shows the same issues as columns, and a drop writes the column's status", async ({
  page,
}) => {
  await openPlanning(page);
  await page.getByTestId("planning-layout-board").click();
  const board = page.getByTestId("planning-board");
  await expect(board).toBeVisible();
  await expect(page.getByTestId("planning-count")).toHaveText(/7 of 7 issues/);
  await expect(page.getByTestId("planning-column-todo")).toContainText(
    "Document the planning provider contract",
  );
  await waitForWriteAccess(page);
  // A pointer drag (dnd-kit, as the To-Do tab uses): press on the card's
  // body, travel past the activation distance, release over the column.
  const card = page.getByTestId(`planning-card-${ITEMS.doc}`);
  const from = await card.boundingBox();
  const to = await page.getByTestId("planning-column-done").boundingBox();
  if (!from || !to) throw new Error("board has no layout");
  await page.mouse.move(from.x + from.width - 8, from.y + from.height - 8);
  await page.mouse.down();
  await page.mouse.move(from.x + from.width, from.y + from.height + 20, {
    steps: 5,
  });
  await page.mouse.move(to.x + to.width / 2, to.y + to.height / 2, {
    steps: 10,
  });
  await page.mouse.up();
  await expect(page.getByTestId("planning-column-done")).toContainText(
    "Document the planning provider contract",
    { timeout: 10_000 },
  );
  await page.getByTestId(`planning-card-${ITEMS.doc}`).scrollIntoViewIfNeeded();
  await shot(page, "04-board");

  // The layout is the viewer's own and survives a reload of the page.
  await page.reload({ waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("planning-board")).toBeVisible({
    timeout: 10_000,
  });
});

test("scopes and filters narrow the same records and say so", async ({
  page,
}) => {
  await openPlanning(page);
  await page.getByTestId("planning-scope-triage").click();
  await expect(page.getByTestId("planning-count")).toHaveText(/1 of 7 issues/);
  await page.getByTestId("planning-scope-all").click();
  await page.keyboard.press("/");
  await expect(page.getByTestId("planning-search")).toBeFocused();
  await page.keyboard.type("provider");
  await expect(page.getByTestId("planning-count")).toHaveText(/1 of 7 issues/);
  await page.getByTestId("planning-display-menu").click();
  await page.getByTestId("planning-group-by-priority").click();
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("planning-group-medium")).toBeVisible();
  await shot(page, "05-filtered");
  await page.getByTestId("planning-search").fill("no issue says this");
  await expect(page.getByTestId("planning-no-matches")).toBeVisible();
  await page.getByTestId("planning-clear-filters").click();
  await expect(page.getByTestId("planning-count")).toHaveText(/7 of 7 issues/);
});

test("the provider panel states Beekeeper is in use and Linear is unavailable", async ({
  page,
}) => {
  await openPlanning(page);
  await page.getByTestId("planning-provider").click();
  const panel = page.getByTestId("planning-provider-panel");
  await expect(panel).toBeVisible();
  const native = page.getByTestId("planning-provider-planning");
  await expect(native).toContainText("Beekeeper");
  await expect(native).toContainText("In use");
  await expect(native).toContainText("Plugin planning 1.0.0, sha256");
  await expect(page.getByTestId("planning-provider-linear")).toContainText(
    "Unavailable",
  );
  await expect(
    page.getByTestId("planning-provider-linear-reason"),
  ).toContainText("no Linear plugin exists yet");
  await shot(page, "06-provider");
});

test("a second provider registered the same way is listed and can be viewed through the same page", async ({
  page,
}) => {
  await openPlanning(page, { registry: TEST_REGISTRY });
  await expect(page.getByTestId("planning-count")).toHaveText(/7 of 7 issues/, {
    timeout: 15_000,
  });
  await page.getByTestId("planning-provider").click();
  await expect(page.getByTestId("planning-provider-several")).toContainText(
    "2 Planning providers",
  );
  await page.getByTestId("planning-provider-planning-fixture-choose").click();
  await expect(page.getByTestId("planning-provider")).toContainText("Fixture", {
    timeout: 15_000,
  });
  await expect(
    page
      .getByTestId("planning-list")
      .getByText("Served by the fixture provider"),
  ).toBeVisible({ timeout: 15_000 });
  await expect(page.getByTestId("planning-count")).toHaveText(/1 of 1 issue/);
  await shot(page, "12-replacement-provider");
});

test("a relay that runs no Planning provider says so instead of showing an empty project", async ({
  page,
}) => {
  await page.addInitScript(
    ({ events }) => {
      window.__BEEKEEPER_E2E_EXTRA_PROJECT_EVENTS__ = events as never;
    },
    { events: [projectHead(), ...seededIssues()] as never },
  );
  await installMockBridge(page);
  await page.goto("/", { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("project-group-general")).toBeVisible({
    timeout: 10_000,
  });
  await page.goto(PLANNING_URL, { waitUntil: "domcontentloaded" });
  await expect(page.getByTestId("planning-unavailable")).toContainText(
    "runs no Planning provider",
    { timeout: 15_000 },
  );
  await expect(page.getByTestId("planning-empty")).toHaveCount(0);
  await shot(page, "13-no-provider");
});

test("an empty project offers the first issue; a failed read says so instead of showing nothing", async ({
  page,
}) => {
  await openPlanning(page, { issues: [] });
  await expect(page.getByTestId("planning-empty")).toBeVisible();
  await waitForWriteAccess(page);
  await shot(page, "07-empty");
  await page.getByTestId("planning-empty-create").click();
  await expect(page.getByTestId("planning-quick-create")).toContainText(
    "also creates one named “Issues”",
  );
  await page.getByTestId("planning-quick-create-title").fill("First issue");
  await page.keyboard.press("Enter");
  await expect(
    page.getByTestId("planning-list").getByText("First issue"),
  ).toBeVisible({ timeout: 10_000 });
});

test("a refused read is an error, not an empty project", async ({ page }) => {
  await openPlanning(page, { rejectKinds: [KIND_PROJECT_TODO_OP] });
  await expect(page.getByTestId("planning-notice-error")).toBeVisible({
    timeout: 15_000,
  });
  await expect(page.getByTestId("planning-empty")).toHaveCount(0);
  await shot(page, "08-error");
});

const DOC_URL = `${PLANNING_URL}?issue=${LIST}/${ITEMS.doc}`;
const DOC_TITLE = "Document the planning provider contract";

async function refuseTodoOps(page: Page, ops: string[]) {
  await page.evaluate((refused) => {
    window.__BEEKEEPER_E2E_REJECT_TODO_OPS__ = refused;
  }, ops);
}

test("Escape leaves a title edit without saving it", async ({ page }) => {
  await openPlanning(page, { path: DOC_URL });
  await waitForWriteAccess(page);
  const title = page.getByTestId("planning-detail-title");
  await expect(title).toHaveValue(DOC_TITLE);
  await title.fill("A rename the viewer abandons");
  await page.keyboard.press("Escape");
  await expect(title).toHaveValue(DOC_TITLE);
  await expect(title).not.toBeFocused();
  // The detail stays open: Escape belonged to the field.
  await expect(page.getByTestId("planning-detail")).toBeVisible();
  // Nothing was published: a rename would show in the op history and the row.
  await page.waitForTimeout(1_500);
  await expect(page.getByTestId("planning-detail-activity")).not.toContainText(
    "renamed the issue",
  );
  await expect(page.getByTestId(`planning-row-${ITEMS.doc}`)).toContainText(
    DOC_TITLE,
  );
  await expect(page.getByTestId(`planning-row-${ITEMS.doc}`)).not.toContainText(
    "abandons",
  );

  // Enter still saves.
  await title.fill("Document the planning provider boundary");
  await page.keyboard.press("Enter");
  await expect(page.getByTestId("planning-detail-activity")).toContainText(
    "renamed the issue",
    { timeout: 10_000 },
  );
});

test("a status the relay refuses is reported as a partial change, and the issue reads what item.done saved", async ({
  page,
}) => {
  await openPlanning(page, { path: DOC_URL });
  await waitForWriteAccess(page);
  // A relay that predates item.status refuses it as an unknown op.
  await refuseTodoOps(page, ["item.status"]);
  await page.getByTestId("planning-detail-status").click();
  await page.getByTestId("planning-status-option-canceled").click();
  const toast = page
    .locator("[data-sonner-toast]")
    .filter({ hasText: "Only part of the change was confirmed" });
  await expect(toast).toBeVisible({ timeout: 10_000 });
  await expect(toast).toContainText("the item is marked “done”");
  await expect(toast).toContainText("did not confirm the status “canceled”");
  await expect(toast).toContainText("unknown project todo op");
  // Done, not Canceled and not a pretend rollback: item.done was accepted.
  await expect(page.getByTestId("planning-detail-status")).toContainText(
    "Done",
    { timeout: 10_000 },
  );
  const activity = page.getByTestId("planning-detail-activity");
  await expect(activity).toContainText("marked it done");
  await expect(activity).not.toContainText("set status to Canceled");
  await shot(page, "09-partial-status");
});

test("a field refused at create is reported once; the issue exists once and the draft closes", async ({
  page,
}) => {
  await openPlanning(page);
  await waitForWriteAccess(page);
  await refuseTodoOps(page, ["item.priority"]);
  await page.getByTestId("planning-new-issue").click();
  const dialog = page.getByTestId("planning-quick-create");
  await expect(dialog).toBeVisible();
  await page
    .getByTestId("planning-quick-create-title")
    .fill("Created even though its priority is refused");
  await page.getByTestId("planning-quick-create-priority").click();
  await page.getByTestId("planning-priority-option-high").click();
  await page.keyboard.press("Meta+Enter");
  await expect(dialog).toBeHidden({ timeout: 10_000 });
  const toast = page
    .locator("[data-sonner-toast]")
    .filter({ hasText: "but its priority" });
  await expect(toast).toBeVisible({ timeout: 10_000 });
  await expect(toast).toContainText("was not confirmed");
  const rows = page
    .getByTestId("planning-list")
    .locator("li", { hasText: "Created even though its priority is refused" });
  await expect(rows).toHaveCount(1, { timeout: 10_000 });
  await shot(page, "10-partial-create");
});
